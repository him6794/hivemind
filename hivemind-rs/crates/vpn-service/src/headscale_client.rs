//! Headscale client: communicates with a self-hosted Tailscale coordination server.
//!
//! Headscale is an open-source implementation of the Tailscale control server.
//! This client creates pre-auth keys for workers and manages node lifecycle.

use anyhow::{Context, Result};
use reqwest::{Client, Method, Url};
use serde_json::Value;
use std::time::Duration;

const MAX_CONTROL_RESPONSE_BYTES: usize = 64 * 1024;

/// A secret and stable ID returned together by the trusted issuance endpoint.
/// The ID must never be substituted with a value supplied by the enrolling peer.
pub struct IdentifiedPreAuthKey {
    id: i64,
    key: String,
}

impl std::fmt::Debug for IdentifiedPreAuthKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IdentifiedPreAuthKey")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl IdentifiedPreAuthKey {
    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    fn from_response(response: &Value) -> Result<Self> {
        let record = response
            .get("preAuthKey")
            .or_else(|| response.get("pre_auth_key"))
            .unwrap_or(response);
        let id = record.get("id").and_then(|value| {
            value.as_i64().or_else(|| {
                let text = value.as_str()?;
                if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                text.parse().ok()
            })
        });
        let id = id
            .filter(|id| *id > 0)
            .context("Headscale issuance omitted a valid key ID")?;
        let key = record
            .get("key")
            .and_then(Value::as_str)
            .filter(|key| {
                !key.is_empty()
                    && key.len() <= 255
                    && key.bytes().all(|byte| byte.is_ascii_graphic())
                    && !key.contains('*')
            })
            .context("Headscale issuance omitted an unmasked key")?;
        if record.get("reusable").and_then(Value::as_bool) != Some(false) {
            anyhow::bail!("Headscale issuance did not confirm a single-use key");
        }
        Ok(Self {
            id,
            key: key.into(),
        })
    }
}

pub struct HeadscaleClient {
    base_url: String,
    api_key: String,
    http: Client,
}

impl HeadscaleClient {
    pub fn new(base_url: &str, api_key: &str) -> Result<Self> {
        Ok(Self {
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            http: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(8))
                .no_proxy()
                .build()
                .context("constructing Headscale control client")?,
        })
    }

    fn ensure_configured(&self) -> Result<()> {
        if self.base_url.trim().is_empty() {
            anyhow::bail!("Headscale base_url is required");
        }
        if self.api_key.trim().is_empty() || self.api_key.trim().len() > 1024 {
            anyhow::bail!("Headscale api_key is missing or oversized");
        }
        let url = Url::parse(&self.base_url).context("invalid Headscale base URL")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || self.base_url.len() > 4096
        {
            anyhow::bail!(
                "Headscale base URL must be HTTP(S) without credentials, query or fragment"
            );
        }
        Ok(())
    }

    pub(crate) fn controller_url(&self) -> Result<String> {
        self.ensure_configured()?;
        Ok(self.api_url("")?.to_string())
    }

    fn api_url(&self, path: &str) -> Result<Url> {
        let base = if self.base_url.ends_with("/api/v1") {
            self.base_url.clone()
        } else {
            format!("{}/api/v1", self.base_url)
        };
        Ok(Url::parse(&format!(
            "{}/{}",
            base,
            path.trim_start_matches('/')
        ))?)
    }

    fn public_url(&self, path: &str) -> Result<Url> {
        Ok(Url::parse(&format!(
            "{}/{}",
            self.base_url,
            path.trim_start_matches('/')
        ))?)
    }

    async fn request_json(&self, method: Method, url: Url, body: Option<Value>) -> Result<Value> {
        self.ensure_configured()?;
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(self.api_key.trim());
        if let Some(body) = body {
            request = request.json(&body);
        }
        let mut response = request
            .send()
            .await
            .context("Headscale control request failed")?;
        let status = response.status();
        if !status.is_success() {
            // Error bodies may contain enrollment secrets; never relay them to callers.
            anyhow::bail!("Headscale request failed with {status}");
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_CONTROL_RESPONSE_BYTES as u64)
        {
            anyhow::bail!("Headscale control response exceeds the byte limit");
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body
                .len()
                .checked_add(chunk.len())
                .is_none_or(|size| size > MAX_CONTROL_RESPONSE_BYTES)
            {
                anyhow::bail!("Headscale control response exceeds the byte limit");
            }
            body.extend_from_slice(&chunk);
        }
        if body.iter().all(u8::is_ascii_whitespace) {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&body).context("invalid Headscale control response JSON")
    }

    fn extract_preauth_key(response: &Value) -> Option<String> {
        response
            .pointer("/preAuthKey/key")
            .or_else(|| response.pointer("/pre_auth_key/key"))
            .or_else(|| response.pointer("/key"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(ToString::to_string)
    }

    /// Create a pre-authentication key for a worker node.
    pub async fn create_preauth_key(&self, user: &str) -> Result<String> {
        self.ensure_configured()?;
        let user = user.trim();
        if user.is_empty() {
            anyhow::bail!("Headscale preauth key user is required");
        }
        let user_id = self.resolve_user_id(user).await?;
        let response = self
            .request_json(
                Method::POST,
                self.api_url("preauthkey")?,
                Some(serde_json::json!({
                    "user": user_id.to_string(),
                    "reusable": false,
                    "ephemeral": true,
                    "expiration": (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
                })),
            )
            .await?;
        let key = Self::extract_preauth_key(&response).ok_or_else(|| {
            anyhow::anyhow!("Headscale preauth key response did not contain a key")
        })?;
        tracing::info!(
            "Headscale {}: created preauth key for user {}",
            self.base_url,
            user,
        );
        Ok(key)
    }

    async fn resolve_user_id(&self, user: &str) -> Result<u64> {
        self.ensure_configured()?;
        if user.is_empty() || user.len() > 255 {
            anyhow::bail!("Headscale preauth key user is missing or oversized");
        }
        let response = self
            .request_json(Method::GET, self.api_url("user")?, None)
            .await?;
        Self::user_id_from_response(&response, user)
    }

    fn user_id_from_response(response: &Value, user: &str) -> Result<u64> {
        let users = response
            .get("users")
            .and_then(Value::as_array)
            .context("Headscale response omitted the user list")?;
        let mut matches = users
            .iter()
            .filter(|record| record.get("name").and_then(Value::as_str) == Some(user));
        let record = matches
            .next()
            .context("Headscale namespace does not exist")?;
        if matches.next().is_some() {
            anyhow::bail!("Headscale namespace is ambiguous");
        }
        let id = record.get("id").and_then(|value| {
            value.as_u64().or_else(|| {
                let text = value.as_str()?;
                if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                text.parse().ok()
            })
        });
        id.filter(|id| *id > 0)
            .context("Headscale namespace has an invalid user ID")
    }

    /// Create a Headscale user if the API supports it. Missing endpoint is ignored.
    pub async fn ensure_user(&self, user: &str) -> Result<()> {
        self.ensure_configured()?;
        let user = user.trim();
        if user.is_empty() {
            anyhow::bail!("Headscale user is required");
        }
        match self
            .request_json(
                Method::POST,
                self.api_url("user")?,
                Some(serde_json::json!({ "name": user })),
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => {
                let msg = err.to_string().to_lowercase();
                if msg.contains("already") || msg.contains("exists") || msg.contains("409") {
                    Ok(())
                } else {
                    // Some Headscale versions use /user, others pre-create users out-of-band.
                    tracing::warn!("Headscale ensure_user soft-failed for {}: {}", user, err);
                    Ok(())
                }
            }
        }
    }

    /// Create a pre-authentication key for a Headscale user with explicit options.
    pub async fn create_preauth_key_for_user(
        &self,
        user: &str,
        reusable: bool,
        ephemeral: bool,
    ) -> Result<String> {
        let response = self
            .create_user_key_response(user, reusable, ephemeral)
            .await?;
        Self::extract_preauth_key(&response)
            .context("Headscale preauth key response did not contain a key")
    }

    /// Issue the single-use key and its controller ID as one indivisible result.
    pub async fn create_identified_user_key(&self, user: &str) -> Result<IdentifiedPreAuthKey> {
        let response = self.create_user_key_response(user, false, false).await?;
        IdentifiedPreAuthKey::from_response(&response)
    }

    async fn create_user_key_response(
        &self,
        user: &str,
        reusable: bool,
        ephemeral: bool,
    ) -> Result<Value> {
        self.ensure_configured()?;
        let user = user.trim();
        if user.is_empty() || user.len() > 255 {
            anyhow::bail!("Headscale preauth key user is missing or oversized");
        }
        let user_id = self.resolve_user_id(user).await?;
        self.request_json(
            Method::POST,
            self.api_url("preauthkey")?,
            Some(serde_json::json!({
                "user": user_id.to_string(),
                "reusable": reusable,
                "ephemeral": ephemeral,
                "expiration": (chrono::Utc::now() + chrono::Duration::hours(24)).to_rfc3339(),
            })),
        )
        .await
    }

    /// Delete/expire a node from the Tailscale network
    pub async fn delete_node(&self, node_id: &str) -> Result<()> {
        self.ensure_configured()?;
        let node_id = node_id.trim();
        if node_id.is_empty() {
            anyhow::bail!("Headscale node_id is required");
        }
        self.request_json(
            Method::DELETE,
            self.api_url(&format!("node/{node_id}"))?,
            None,
        )
        .await?;
        tracing::info!("Headscale {}: deleted node {}", self.base_url, node_id);
        Ok(())
    }

    /// Get the DERP map (relay servers for NAT traversal)
    pub async fn get_derp_map(&self) -> Result<String> {
        self.ensure_configured()?;
        let response = self
            .http
            .get(self.public_url("derpmap")?)
            .bearer_auth(self.api_key.trim())
            .send()
            .await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("Headscale DERP map request failed with {status}: {text}");
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_preauth_key_accepts_headscale_shapes() {
        let response = serde_json::json!({
            "preAuthKey": {
                "key": "hs-preauth-real"
            }
        });
        assert_eq!(
            HeadscaleClient::extract_preauth_key(&response).as_deref(),
            Some("hs-preauth-real")
        );

        let response = serde_json::json!({ "key": "direct-key" });
        assert_eq!(
            HeadscaleClient::extract_preauth_key(&response).as_deref(),
            Some("direct-key")
        );
    }

    #[test]
    fn enrollment_user_resolution_requires_an_unambiguous_controller_id() {
        for id in [serde_json::json!(3), serde_json::json!("3")] {
            let response = serde_json::json!({"users": [{"name": "nodepool", "id": id}]});
            assert_eq!(
                HeadscaleClient::user_id_from_response(&response, "nodepool").unwrap(),
                3
            );
        }
        for response in [
            serde_json::json!({}),
            serde_json::json!({"users": []}),
            serde_json::json!({"users": [{"name": "other", "id": "3"}]}),
            serde_json::json!({"users": [{"name": "nodepool", "id": 0}]}),
            serde_json::json!({"users": [{"name": "nodepool", "id": 3.0}]}),
            serde_json::json!({"users": [{"name": "nodepool", "id": "-3"}]}),
            serde_json::json!({"users": [{"name": "nodepool", "id": " 3"}]}),
            serde_json::json!({"users": [{"name": "nodepool", "id": "3"}, {"name": "nodepool", "id": "4"}]}),
        ] {
            assert!(HeadscaleClient::user_id_from_response(&response, "nodepool").is_err());
        }
    }

    #[test]
    fn identified_enrollment_key_requires_a_single_controller_record() {
        for envelope in ["preAuthKey", "pre_auth_key", ""] {
            for id in [serde_json::json!(11), serde_json::json!("11")] {
                let record = serde_json::json!({"id": id, "key": "secret-key", "reusable": false});
                let response = if envelope.is_empty() {
                    record
                } else {
                    serde_json::json!({envelope: record})
                };
                let key = IdentifiedPreAuthKey::from_response(&response).unwrap();
                assert_eq!(key.id(), 11);
                assert_eq!(key.key(), "secret-key");
                assert!(!format!("{key:?}").contains("secret-key"));
            }
        }
        for id in [
            serde_json::json!(0),
            serde_json::json!(-1),
            serde_json::json!(1.0),
            serde_json::json!("+11"),
            serde_json::json!(" 11"),
            serde_json::json!(u64::MAX),
            serde_json::Value::Null,
        ] {
            let response = serde_json::json!({"id": id, "key": "secret-key", "reusable": false});
            assert!(IdentifiedPreAuthKey::from_response(&response).is_err());
        }
        for response in [
            serde_json::json!({"id": 11, "key": "secret-key", "reusable": true}),
            serde_json::json!({"id": 11, "key": "secret-key"}),
            serde_json::json!({"id": 11, "key": "masked***", "reusable": false}),
            serde_json::json!({"id": 11, "key": "bad\nkey", "reusable": false}),
            serde_json::json!({"id": 11, "key": "x".repeat(256), "reusable": false}),
            serde_json::json!({"id": 11, "key": "", "reusable": false}),
            serde_json::json!({"preAuthKey": {"key": "secret-key", "reusable": false}, "id": 11}),
            serde_json::json!({"preAuthKey": {"id": 11, "reusable": false}, "key": "secret-key"}),
        ] {
            let error = IdentifiedPreAuthKey::from_response(&response).unwrap_err();
            assert!(!format!("{error:?}").contains("secret-key"));
        }
    }

    #[tokio::test]
    async fn test_create_preauth_key_requires_configuration() {
        let client = HeadscaleClient::new("", "").unwrap();
        let error = client
            .create_preauth_key("worker-test-1")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("base_url"));
    }
}
