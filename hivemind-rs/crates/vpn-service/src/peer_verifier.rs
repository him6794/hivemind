//! Resolve a source's overlay address through the trusted Headscale control plane.
//!
//! A node ID is only a lookup hint. Nodepool must supply the preauth key ID from
//! its own owner-authorized VPN enrollment record, never from a peer's request. A
//! successful lookup is a point-in-time identity observation, not artifact
//! availability, a read lease, or permission to publish an endpoint.

use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::{Client, StatusCode, Url};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_NODE_RESPONSE_BYTES: usize = 64 * 1024;
/// The list covers the whole shared namespace, so it needs a larger bound than a
/// single node. A reply above this bound fails closed instead of being truncated.
const MAX_NODE_LIST_RESPONSE_BYTES: usize = 1024 * 1024;
/// Bounds both hint maps. Eviction only forgets a hint, which can slow a lookup
/// but can never make a wrong node acceptable.
const MAX_NODE_HINTS: usize = 4096;
/// Reuses a "not joined" answer briefly, so repeated queries from one owner do
/// not each list the shared namespace.
const NEGATIVE_HINT_TTL: Duration = Duration::from_secs(10);
const MAX_API_KEY_BYTES: usize = 1024;

#[derive(Debug)]
pub enum PeerVerificationError {
    Configuration(&'static str),
    Http(reqwest::Error),
    Status(StatusCode),
    ResponseTooLarge,
    Json(serde_json::Error),
    InvalidNode(&'static str),
    NodeNotFound,
}

impl fmt::Display for PeerVerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(reason) => write!(
                formatter,
                "invalid Headscale verifier configuration: {reason}"
            ),
            Self::Http(_) => formatter.write_str("Headscale node lookup failed"),
            Self::Status(status) => write!(formatter, "Headscale node lookup returned {status}"),
            Self::ResponseTooLarge => {
                formatter.write_str("Headscale node response exceeds the byte limit")
            }
            Self::Json(_) => formatter.write_str("Headscale node response is not valid JSON"),
            Self::InvalidNode(reason) => {
                write!(formatter, "Headscale source identity rejected: {reason}")
            }
            Self::NodeNotFound => {
                formatter.write_str("no Headscale node has joined with this enrollment key yet")
            }
        }
    }
}

impl std::error::Error for PeerVerificationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

type Result<T> = std::result::Result<T, PeerVerificationError>;

/// Fields are immutable outside this module. Persisted source leases must still
/// fence this observation with owner/session, task and source-expiry checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedOverlayNode {
    node_id: u64,
    preauth_key_id: u64,
    ipv4: Ipv4Addr,
    observed_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
}

impl VerifiedOverlayNode {
    pub fn node_id(&self) -> u64 {
        self.node_id
    }
    pub fn preauth_key_id(&self) -> u64 {
        self.preauth_key_id
    }
    pub fn ipv4(&self) -> Ipv4Addr {
        self.ipv4
    }
    pub fn observed_at(&self) -> DateTime<Utc> {
        self.observed_at
    }
    pub fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.expires_at
    }
}

/// Hints only save work. A node hint is re-checked with one node lookup before use,
/// so a stale or reassigned hint cannot authorize a different node.
#[derive(Default)]
struct NodeHints {
    nodes: HashMap<u64, u64>,
    missing: HashMap<u64, Instant>,
}

impl NodeHints {
    fn node(&self, key_id: u64) -> Option<u64> {
        self.nodes.get(&key_id).copied()
    }

    fn missing_recently(&self, key_id: u64, now: Instant) -> bool {
        self.missing
            .get(&key_id)
            .is_some_and(|seen| now.saturating_duration_since(*seen) < NEGATIVE_HINT_TTL)
    }

    fn remember_node(&mut self, key_id: u64, node_id: u64) {
        self.missing.remove(&key_id);
        make_room(&mut self.nodes, key_id);
        self.nodes.insert(key_id, node_id);
    }

    fn remember_missing(&mut self, key_id: u64, now: Instant) {
        make_room(&mut self.missing, key_id);
        self.missing.insert(key_id, now);
    }

    fn forget_node(&mut self, key_id: u64) {
        self.nodes.remove(&key_id);
    }
}

/// Evicts one arbitrary entry when a new key would exceed the bound.
fn make_room<V>(map: &mut HashMap<u64, V>, key_id: u64) {
    if !map.contains_key(&key_id) && map.len() >= MAX_NODE_HINTS {
        if let Some(evicted) = map.keys().next().copied() {
            map.remove(&evicted);
        }
    }
}

/// One reusable HTTP pool pinned to an operator-configured control-plane URL.
/// Peer input cannot select the host, path, namespace or authorization header.
#[derive(Clone)]
pub struct HeadscalePeerVerifier {
    base_url: Url,
    namespace: String,
    http: Client,
    node_hints: Arc<Mutex<NodeHints>>,
}

impl HeadscalePeerVerifier {
    pub fn new(base_url: &str, api_key: &str, namespace: &str) -> Result<Self> {
        let mut base_url = Url::parse(base_url.trim())
            .map_err(|_| PeerVerificationError::Configuration("invalid base URL"))?;
        if !matches!(base_url.scheme(), "http" | "https")
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(PeerVerificationError::Configuration(
                "base URL must be an HTTP(S) endpoint without credentials, query or fragment",
            ));
        }
        let path = base_url.path().trim_end_matches('/');
        let path = if path.ends_with("/api/v1") {
            format!("{path}/")
        } else {
            format!("{path}/api/v1/")
        };
        base_url.set_path(&path);
        let namespace = namespace.trim();
        if namespace.is_empty() || namespace.len() > 255 {
            return Err(PeerVerificationError::Configuration(
                "missing or oversized namespace",
            ));
        }
        let api_key = api_key.trim();
        if api_key.is_empty() || api_key.len() > MAX_API_KEY_BYTES {
            return Err(PeerVerificationError::Configuration(
                "missing or oversized API key",
            ));
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {api_key}"))
            .map_err(|_| PeerVerificationError::Configuration("invalid API key header"))?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization);
        let http = Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(8))
            .no_proxy()
            .build()
            .map_err(PeerVerificationError::Http)?;
        Ok(Self {
            base_url,
            namespace: namespace.into(),
            http,
            node_hints: Arc::new(Mutex::new(NodeHints::default())),
        })
    }

    /// `expected_key_id` must be recorded by Nodepool when issuing enrollment,
    /// not taken from the peer. Modern Headscale masks the secret in responses;
    /// the stable key ID, not the displayed key, binds a node to that enrollment.
    /// The consumed key's expiry is not the enrolled node's expiry.
    pub async fn verify_node(
        &self,
        node_id: u64,
        expected_key_id: u64,
    ) -> Result<VerifiedOverlayNode> {
        if node_id == 0 || expected_key_id == 0 {
            return Err(PeerVerificationError::InvalidNode(
                "missing enrollment identity",
            ));
        }
        let url = self
            .base_url
            .join(&format!("node/{node_id}"))
            .map_err(|_| PeerVerificationError::Configuration("invalid node lookup URL"))?;
        let response = self.get_json(url, MAX_NODE_RESPONSE_BYTES).await?;
        validate_node(
            &response,
            node_id,
            expected_key_id,
            &self.namespace,
            Utc::now(),
        )
    }

    /// Finds the one node that joined with `expected_key_id`. A remembered node is
    /// re-checked with one lookup. The shared namespace is listed only when there is
    /// no usable hint, and that listing is bounded. A second match fails closed. The
    /// key ID must come from Nodepool's enrollment record.
    pub async fn resolve_node_by_key(&self, expected_key_id: u64) -> Result<VerifiedOverlayNode> {
        if expected_key_id == 0 {
            return Err(PeerVerificationError::InvalidNode(
                "missing enrollment identity",
            ));
        }
        // The hint is copied out first, so the lock is never held across the lookup.
        let hinted = self.hints().node(expected_key_id);
        if let Some(hinted) = hinted {
            match self.verify_node(hinted, expected_key_id).await {
                Ok(node) => return Ok(node),
                // A reassigned or removed node only means the hint is stale.
                Err(PeerVerificationError::InvalidNode(_)) => {
                    self.hints().forget_node(expected_key_id);
                }
                Err(PeerVerificationError::Status(status)) if status == StatusCode::NOT_FOUND => {
                    self.hints().forget_node(expected_key_id);
                }
                // Transport and server failures are reported, never answered by a listing.
                Err(error) => return Err(error),
            }
        }
        if self
            .hints()
            .missing_recently(expected_key_id, Instant::now())
        {
            return Err(PeerVerificationError::NodeNotFound);
        }
        match self.list_node_by_key(expected_key_id).await {
            Ok(node) => {
                self.hints().remember_node(expected_key_id, node.node_id());
                Ok(node)
            }
            Err(PeerVerificationError::NodeNotFound) => {
                self.hints()
                    .remember_missing(expected_key_id, Instant::now());
                Err(PeerVerificationError::NodeNotFound)
            }
            Err(error) => Err(error),
        }
    }

    async fn list_node_by_key(&self, expected_key_id: u64) -> Result<VerifiedOverlayNode> {
        let url = self
            .base_url
            .join("node")
            .map_err(|_| PeerVerificationError::Configuration("invalid node list URL"))?;
        let response = self.get_json(url, MAX_NODE_LIST_RESPONSE_BYTES).await?;
        select_node_by_key(&response, expected_key_id, &self.namespace, Utc::now())
    }

    /// Poisoning cannot make a hint trusted, so a poisoned lock is still usable.
    fn hints(&self) -> std::sync::MutexGuard<'_, NodeHints> {
        self.node_hints
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    async fn get_json(&self, url: Url, limit: usize) -> Result<Value> {
        let mut response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(PeerVerificationError::Http)?;
        if response.status() != StatusCode::OK {
            // Never include controller response bodies: they may contain keys.
            return Err(PeerVerificationError::Status(response.status()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > limit as u64)
        {
            return Err(PeerVerificationError::ResponseTooLarge);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(PeerVerificationError::Http)?
        {
            if body
                .len()
                .checked_add(chunk.len())
                .is_none_or(|size| size > limit)
            {
                return Err(PeerVerificationError::ResponseTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(PeerVerificationError::Json)
    }
}

fn positive_id(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    let id = value.as_u64().or_else(|| {
        let text = value.as_str()?;
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        text.parse().ok()
    })?;
    (id > 0).then_some(id)
}

fn validate_node(
    response: &Value,
    node_id: u64,
    expected_key_id: u64,
    namespace: &str,
    now: DateTime<Utc>,
) -> Result<VerifiedOverlayNode> {
    let node = response
        .get("node")
        .filter(|node| node.is_object())
        .ok_or(PeerVerificationError::InvalidNode("missing node"))?;
    validate_node_object(node, node_id, expected_key_id, namespace, now)
}

/// Picks the single node enrolled with `expected_key_id`. Unrelated nodes in the
/// reply are ignored, and a second match with the same key is ambiguous.
fn select_node_by_key(
    response: &Value,
    expected_key_id: u64,
    namespace: &str,
    now: DateTime<Utc>,
) -> Result<VerifiedOverlayNode> {
    let nodes = response
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or(PeerVerificationError::InvalidNode("missing node list"))?;
    let mut matches = nodes
        .iter()
        .filter(|node| positive_id(node.pointer("/preAuthKey/id")) == Some(expected_key_id));
    let node = matches.next().ok_or(PeerVerificationError::NodeNotFound)?;
    if matches.next().is_some() {
        return Err(PeerVerificationError::InvalidNode(
            "enrollment key matches more than one node",
        ));
    }
    let node_id = positive_id(node.get("id"))
        .ok_or(PeerVerificationError::InvalidNode("missing node identity"))?;
    validate_node_object(node, node_id, expected_key_id, namespace, now)
}

fn validate_node_object(
    node: &Value,
    node_id: u64,
    expected_key_id: u64,
    namespace: &str,
    now: DateTime<Utc>,
) -> Result<VerifiedOverlayNode> {
    if positive_id(node.get("id")) != Some(node_id)
        || node.pointer("/user/name").and_then(Value::as_str) != Some(namespace)
        || node.get("online").and_then(Value::as_bool) != Some(true)
    {
        return Err(PeerVerificationError::InvalidNode(
            "node, namespace or online state mismatch",
        ));
    }
    let preauth_key_id = positive_id(node.pointer("/preAuthKey/id")).ok_or(
        PeerVerificationError::InvalidNode("missing enrollment key ID"),
    )?;
    if preauth_key_id != expected_key_id
        || node
            .pointer("/preAuthKey/reusable")
            .and_then(Value::as_bool)
            != Some(false)
    {
        return Err(PeerVerificationError::InvalidNode(
            "single-use enrollment key mismatch",
        ));
    }
    // Headscale v1 represents no expiry as null or Go's zero time. Some
    // protobuf JSON encoders omit an unset timestamp entirely.
    let expires_at = match node.get("expiry") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text == "0001-01-01T00:00:00Z" => None,
        Some(Value::String(text)) => Some(
            DateTime::parse_from_rfc3339(text)
                .map_err(|_| PeerVerificationError::InvalidNode("malformed node expiry"))?
                .with_timezone(&Utc),
        ),
        _ => return Err(PeerVerificationError::InvalidNode("malformed node expiry")),
    };
    if expires_at.is_some_and(|expiry| expiry <= now) {
        return Err(PeerVerificationError::InvalidNode("expired node"));
    }
    let addresses = node
        .get("ipAddresses")
        .and_then(Value::as_array)
        .ok_or(PeerVerificationError::InvalidNode("missing node addresses"))?;
    let mut ipv4 = None;
    for address in addresses {
        let address = address
            .as_str()
            .and_then(|text| text.parse::<IpAddr>().ok())
            .ok_or(PeerVerificationError::InvalidNode("malformed node address"))?;
        if let IpAddr::V4(address) = address {
            let octets = address.octets();
            if octets[0] != 100
                || !(64..=127).contains(&octets[1])
                || ipv4.replace(address).is_some()
            {
                return Err(PeerVerificationError::InvalidNode(
                    "ambiguous or non-overlay IPv4 address",
                ));
            }
        }
    }
    let ipv4 = ipv4.ok_or(PeerVerificationError::InvalidNode(
        "missing overlay IPv4 address",
    ))?;
    Ok(VerifiedOverlayNode {
        node_id,
        preauth_key_id,
        ipv4,
        observed_at: now,
        expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn node() -> Value {
        json!({"node": {
            "id": "7", "user": {"name": "nodepool"}, "online": true,
            "ipAddresses": ["100.64.1.2", "fd7a:115c:a1e0::7"], "expiry": null,
            "preAuthKey": {"id": "11", "key": "masked-controller-key", "reusable": false,
                "expiration": "2020-01-01T00:00:00Z"}
        }})
    }

    fn check(value: &Value) -> Result<VerifiedOverlayNode> {
        validate_node(value, 7, 11, "nodepool", Utc::now())
    }

    #[test]
    fn peer_identity_requires_the_controller_node_namespace_and_enrollment_key() {
        let verified = check(&node()).unwrap();
        assert_eq!(verified.node_id(), 7);
        assert_eq!(verified.preauth_key_id(), 11);
        assert_eq!(verified.ipv4(), Ipv4Addr::new(100, 64, 1, 2));
        assert!(verified.expires_at().is_none());
        for (field, value) in [
            ("/node/id", json!("8")),
            ("/node/id", json!(0)),
            ("/node/id", json!(7.0)),
            ("/node/user/name", json!("someone-else")),
            ("/node/user", Value::Null),
            ("/node/online", json!(false)),
            ("/node/online", json!("true")),
            ("/node/preAuthKey/id", json!("12")),
            ("/node/preAuthKey/reusable", json!(true)),
            ("/node/preAuthKey/id", json!("0")),
            ("/node/preAuthKey", Value::Null),
        ] {
            let mut altered = node();
            *altered.pointer_mut(field).unwrap() = value;
            assert!(check(&altered).is_err(), "accepted {field}");
        }
        let mut numeric = node();
        numeric["node"]["id"] = json!(7);
        numeric["node"]["preAuthKey"]["id"] = json!(11);
        assert!(check(&numeric).is_ok());
        assert!(check(&json!({})).is_err());
    }

    #[test]
    fn peer_identity_requires_one_overlay_ipv4_and_a_live_node_expiry() {
        for addresses in [
            json!([]),
            json!(["127.0.0.1"]),
            json!(["10.0.0.1"]),
            json!(["169.254.169.254"]),
            json!(["8.8.8.8"]),
            json!(["100.63.255.255"]),
            json!(["100.128.0.1"]),
            json!(["::1"]),
            json!(["100.64.1.2", "100.64.1.3"]),
            json!(["100.64.1.2", "not-an-address"]),
            json!(["http://100.64.1.2"]),
        ] {
            let mut altered = node();
            altered["node"]["ipAddresses"] = addresses;
            assert!(check(&altered).is_err());
        }
        for expiry in [json!(false), json!("never"), json!("2020-01-01T00:00:00Z")] {
            let mut altered = node();
            altered["node"]["expiry"] = expiry;
            assert!(check(&altered).is_err());
        }
        let now = Utc::now();
        let mut altered = node();
        altered["node"]["expiry"] = json!(now.to_rfc3339());
        assert!(validate_node(&altered, 7, 11, "nodepool", now).is_err());
        let future = now + chrono::Duration::hours(1);
        altered["node"]["expiry"] = json!(future.to_rfc3339());
        assert_eq!(check(&altered).unwrap().expires_at(), Some(future));
        altered["node"]["expiry"] = json!("0001-01-01T00:00:00Z");
        assert!(check(&altered).unwrap().expires_at().is_none());
        altered["node"].as_object_mut().unwrap().remove("expiry");
        assert!(check(&altered).unwrap().expires_at().is_none());
    }

    #[test]
    fn peer_verifier_rejects_ambiguous_control_urls_and_empty_credentials() {
        for url in [
            "",
            "file:///tmp/controller",
            "https://name:secret@headscale.local",
            "https://headscale.local/?node=1",
            "https://headscale.local/#node",
        ] {
            assert!(HeadscalePeerVerifier::new(url, "api-key", "nodepool").is_err());
        }
        for key in [
            "".into(),
            "bad\nkey".into(),
            "k".repeat(MAX_API_KEY_BYTES + 1),
        ] {
            assert!(
                HeadscalePeerVerifier::new("https://headscale.local", &key, "nodepool").is_err()
            );
        }
        assert!(HeadscalePeerVerifier::new("https://headscale.local", "api-key", "").is_err());
        let verifier = HeadscalePeerVerifier::new(
            "https://headscale.local/prefix/api/v1/",
            "api-key",
            "nodepool",
        )
        .unwrap();
        assert_eq!(
            verifier.base_url.as_str(),
            "https://headscale.local/prefix/api/v1/"
        );
    }

    async fn serve(response: Vec<u8>) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let len = stream.read(&mut buffer).await.unwrap();
                assert!(len > 0 && request.len() < 16 * 1024);
                request.extend_from_slice(&buffer[..len]);
            }
            // The client can close early after reading an oversized Content-Length.
            let _ = stream.write_all(&response).await;
            String::from_utf8(request).unwrap()
        });
        (url, task)
    }

    fn response(body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    #[tokio::test]
    async fn peer_verifier_reads_the_authenticated_control_api_without_trusting_a_hostname() {
        let (url, server) = serve(response(&serde_json::to_vec(&node()).unwrap())).await;
        let verifier = HeadscalePeerVerifier::new(&url, "api-test-key", "nodepool").unwrap();
        let verified = verifier.verify_node(7, 11).await.unwrap();
        assert_eq!(verified.ipv4(), Ipv4Addr::new(100, 64, 1, 2));
        let request = server.await.unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /api/v1/node/7 http/1.1\r\n"));
        assert!(request.contains("authorization: bearer api-test-key\r\n"));
    }

    #[tokio::test]
    async fn peer_verifier_rejects_missing_ids_before_network_access() {
        let verifier =
            HeadscalePeerVerifier::new("http://127.0.0.1:1", "api-test-key", "nodepool").unwrap();
        for (node_id, key_id) in [(0, 11), (7, 0)] {
            assert!(matches!(
                verifier.verify_node(node_id, key_id).await,
                Err(PeerVerificationError::InvalidNode(_))
            ));
        }
    }

    #[tokio::test]
    async fn peer_verifier_reuses_one_control_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let body = serde_json::to_vec(&node()).unwrap();
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
                body.len()
            );
            for _ in 0..2 {
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let len = stream.read(&mut buffer).await.unwrap();
                    assert!(len > 0 && request.len() < 16 * 1024);
                    request.extend_from_slice(&buffer[..len]);
                }
                stream.write_all(header.as_bytes()).await.unwrap();
                stream.write_all(&body).await.unwrap();
            }
        });
        let verifier = HeadscalePeerVerifier::new(&url, "api-test-key", "nodepool").unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            verifier.verify_node(7, 11).await.unwrap();
            verifier.verify_node(7, 11).await.unwrap();
            server.await.unwrap();
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn peer_verifier_does_not_follow_redirects_or_echo_response_secrets() {
        let trap = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let redirect = format!("HTTP/1.1 302 Found\r\nLocation: http://{}/leak\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", trap.local_addr().unwrap());
        let (url, server) = serve(redirect.into_bytes()).await;
        let verifier = HeadscalePeerVerifier::new(&url, "api-test-key", "nodepool").unwrap();
        assert!(matches!(
            verifier.verify_node(7, 11).await,
            Err(PeerVerificationError::Status(StatusCode::FOUND))
        ));
        server.await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), trap.accept())
                .await
                .is_err()
        );
        let (url, server) = serve(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 11\r\nConnection: close\r\n\r\nsecret-text".to_vec()).await;
        let verifier = HeadscalePeerVerifier::new(&url, "api-test-key", "nodepool").unwrap();
        let error = verifier.verify_node(7, 11).await.unwrap_err();
        assert!(!error.to_string().contains("secret-text"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn peer_verifier_bounds_both_fixed_and_chunked_response_bodies() {
        let oversized = vec![b'x'; MAX_NODE_RESPONSE_BYTES + 1];
        let mut chunked = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",
            oversized.len()
        )
        .into_bytes();
        chunked.extend_from_slice(&oversized);
        chunked.extend_from_slice(b"\r\n0\r\n\r\n");
        for body in [response(&oversized), chunked] {
            let (url, server) = serve(body).await;
            let verifier = HeadscalePeerVerifier::new(&url, "api-test-key", "nodepool").unwrap();
            assert!(matches!(
                verifier.verify_node(7, 11).await,
                Err(PeerVerificationError::ResponseTooLarge)
            ));
            server.await.unwrap();
        }
        let (url, server) = serve(response(b"not-json")).await;
        let verifier = HeadscalePeerVerifier::new(&url, "api-test-key", "nodepool").unwrap();
        assert!(matches!(
            verifier.verify_node(7, 11).await,
            Err(PeerVerificationError::Json(_))
        ));
        server.await.unwrap();
    }

    fn listed(nodes: Vec<Value>) -> Value {
        json!({"nodes": nodes})
    }

    fn other_node(id: &str, key: &str) -> Value {
        json!({"id": id, "user": {"name": "nodepool"}, "online": true,
            "ipAddresses": ["100.64.9.9"], "expiry": null,
            "preAuthKey": {"id": key, "key": "masked-controller-key", "reusable": false}})
    }

    #[test]
    fn node_list_selects_the_only_node_enrolled_with_the_key() {
        let list = listed(vec![
            other_node("3", "12"),
            node()["node"].clone(),
            json!({"id": "4", "user": {"name": "nodepool"}, "online": true,
                "ipAddresses": ["100.64.9.8"], "expiry": null}),
        ]);
        let verified = select_node_by_key(&list, 11, "nodepool", Utc::now()).unwrap();
        assert_eq!(verified.node_id(), 7);
        assert_eq!(verified.preauth_key_id(), 11);
        assert_eq!(verified.ipv4(), Ipv4Addr::new(100, 64, 1, 2));
    }

    #[test]
    fn node_list_reports_not_joined_and_refuses_ambiguous_or_foreign_matches() {
        let now = Utc::now();
        assert!(matches!(
            select_node_by_key(&listed(vec![other_node("3", "12")]), 11, "nodepool", now),
            Err(PeerVerificationError::NodeNotFound)
        ));
        let twice = listed(vec![node()["node"].clone(), node()["node"].clone()]);
        assert!(matches!(
            select_node_by_key(&twice, 11, "nodepool", now),
            Err(PeerVerificationError::InvalidNode(_))
        ));
        let mut foreign = node()["node"].clone();
        foreign["user"]["name"] = json!("someone-else");
        assert!(matches!(
            select_node_by_key(&listed(vec![foreign]), 11, "nodepool", now),
            Err(PeerVerificationError::InvalidNode(_))
        ));
        for malformed in [json!({}), json!({"nodes": "node"}), json!({"node": {}})] {
            assert!(matches!(
                select_node_by_key(&malformed, 11, "nodepool", now),
                Err(PeerVerificationError::InvalidNode(_))
            ));
        }
    }

    #[tokio::test]
    async fn peer_verifier_resolves_the_enrolled_node_from_one_bounded_list() {
        let list = listed(vec![other_node("3", "12"), node()["node"].clone()]);
        let (url, server) = serve(response(&serde_json::to_vec(&list).unwrap())).await;
        let verifier = HeadscalePeerVerifier::new(&url, "api-test-key", "nodepool").unwrap();
        let verified = verifier.resolve_node_by_key(11).await.unwrap();
        assert_eq!(verified.node_id(), 7);
        let request = server.await.unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /api/v1/node "));
        assert!(request.contains("authorization: bearer api-test-key\r\n"));
    }

    #[tokio::test]
    async fn peer_verifier_bounds_the_node_list_before_reading_it() {
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MAX_NODE_LIST_RESPONSE_BYTES + 1
        );
        let (url, server) = serve(header.into_bytes()).await;
        let verifier = HeadscalePeerVerifier::new(&url, "api-test-key", "nodepool").unwrap();
        assert!(matches!(
            verifier.resolve_node_by_key(11).await,
            Err(PeerVerificationError::ResponseTooLarge)
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn peer_verifier_rejects_a_missing_enrollment_key_before_network_access() {
        let verifier =
            HeadscalePeerVerifier::new("http://127.0.0.1:1", "api-test-key", "nodepool").unwrap();
        assert!(matches!(
            verifier.resolve_node_by_key(0).await,
            Err(PeerVerificationError::InvalidNode(_))
        ));
    }

    /// Answers one canned response per connection and records each request line.
    async fn serve_sequence(
        responses: Vec<Vec<u8>>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut lines = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let len = stream.read(&mut buffer).await.unwrap();
                    assert!(len > 0 && request.len() < 16 * 1024);
                    request.extend_from_slice(&buffer[..len]);
                }
                let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
                lines.push(request.lines().next().unwrap().to_owned());
                let _ = stream.write_all(&response).await;
            }
            lines
        });
        (url, task)
    }

    fn listed_node(id: &str, key: &str) -> Value {
        json!({"id": id, "user": {"name": "nodepool"}, "online": true,
            "ipAddresses": [format!("100.64.1.{id}")], "expiry": null,
            "preAuthKey": {"id": key, "key": "masked-controller-key", "reusable": false}})
    }

    #[tokio::test]
    async fn resolution_reuses_a_verified_hint_and_relists_only_when_it_goes_stale() {
        let enrolled = listed_node("7", "11");
        let reassigned = listed_node("7", "12");
        let (url, server) = serve_sequence(vec![
            response(&serde_json::to_vec(&listed(vec![enrolled.clone()])).unwrap()),
            response(&serde_json::to_vec(&json!({"node": enrolled})).unwrap()),
            response(&serde_json::to_vec(&json!({"node": reassigned})).unwrap()),
            response(&serde_json::to_vec(&listed(vec![listed_node("9", "11")])).unwrap()),
        ])
        .await;
        let verifier = HeadscalePeerVerifier::new(&url, "api-test-key", "nodepool").unwrap();
        assert_eq!(verifier.resolve_node_by_key(11).await.unwrap().node_id(), 7);
        assert_eq!(verifier.resolve_node_by_key(11).await.unwrap().node_id(), 7);
        assert_eq!(verifier.resolve_node_by_key(11).await.unwrap().node_id(), 9);
        assert_eq!(
            server.await.unwrap(),
            [
                "get /api/v1/node http/1.1",
                "get /api/v1/node/7 http/1.1",
                "get /api/v1/node/7 http/1.1",
                "get /api/v1/node http/1.1",
            ]
        );
    }

    #[tokio::test]
    async fn a_hint_that_disappears_is_forgotten_and_a_missing_node_is_reported() {
        let (url, server) = serve_sequence(vec![
            response(&serde_json::to_vec(&listed(vec![listed_node("7", "11")])).unwrap()),
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            response(&serde_json::to_vec(&listed(vec![])).unwrap()),
        ])
        .await;
        let verifier = HeadscalePeerVerifier::new(&url, "api-test-key", "nodepool").unwrap();
        assert_eq!(verifier.resolve_node_by_key(11).await.unwrap().node_id(), 7);
        assert!(matches!(
            verifier.resolve_node_by_key(11).await,
            Err(PeerVerificationError::NodeNotFound)
        ));
        assert_eq!(
            server.await.unwrap(),
            [
                "get /api/v1/node http/1.1",
                "get /api/v1/node/7 http/1.1",
                "get /api/v1/node http/1.1",
            ]
        );
    }

    #[tokio::test]
    async fn a_failed_hint_check_is_reported_without_a_listing_fallback() {
        let (url, server) = serve_sequence(vec![
            response(&serde_json::to_vec(&listed(vec![listed_node("7", "11")])).unwrap()),
            b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec(),
        ])
        .await;
        let verifier = HeadscalePeerVerifier::new(&url, "api-test-key", "nodepool").unwrap();
        assert_eq!(verifier.resolve_node_by_key(11).await.unwrap().node_id(), 7);
        assert!(matches!(
            verifier.resolve_node_by_key(11).await,
            Err(PeerVerificationError::Status(status)) if status == StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert_eq!(server.await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn unjoined_keys_are_not_relisted_within_the_negative_ttl() {
        let (url, server) = serve_sequence(vec![response(
            &serde_json::to_vec(&listed(vec![])).unwrap(),
        )])
        .await;
        let verifier = HeadscalePeerVerifier::new(&url, "api-test-key", "nodepool").unwrap();
        assert!(matches!(
            verifier.resolve_node_by_key(11).await,
            Err(PeerVerificationError::NodeNotFound)
        ));
        assert!(matches!(
            verifier.resolve_node_by_key(11).await,
            Err(PeerVerificationError::NodeNotFound)
        ));
        assert_eq!(server.await.unwrap(), ["get /api/v1/node http/1.1"]);
    }

    #[test]
    fn node_hints_stay_bounded_expire_and_forget_only_the_named_key() {
        let mut hints = NodeHints::default();
        for key in 1..=(MAX_NODE_HINTS as u64 + 16) {
            hints.remember_node(key, key + 1_000);
        }
        assert_eq!(hints.nodes.len(), MAX_NODE_HINTS);
        let before = hints.nodes.len();
        hints.forget_node(u64::MAX);
        assert_eq!(hints.nodes.len(), before);

        let now = Instant::now();
        hints.remember_missing(7, now);
        assert!(hints.missing_recently(7, now + Duration::from_secs(5)));
        assert!(!hints.missing_recently(7, now + NEGATIVE_HINT_TTL));
    }
}
