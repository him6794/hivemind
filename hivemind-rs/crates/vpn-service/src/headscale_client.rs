//! Headscale client: communicates with a self-hosted Tailscale coordination server.
//!
//! Headscale is an open-source implementation of the Tailscale control server.
//! This client creates pre-auth keys for workers and manages node lifecycle.

use anyhow::Result;
use reqwest::{Client, Method, Url};
use serde_json::Value;

pub struct HeadscaleClient {
    base_url: String,
    api_key: String,
    http: Client,
}

impl HeadscaleClient {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            http: Client::new(),
        }
    }

    fn ensure_configured(&self) -> Result<()> {
        if self.base_url.trim().is_empty() {
            anyhow::bail!("Headscale base_url is required");
        }
        if self.api_key.trim().is_empty() {
            anyhow::bail!("Headscale api_key is required");
        }
        Ok(())
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
        let response = request.send().await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("Headscale request failed with {status}: {text}");
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        Ok(serde_json::from_str(&text)?)
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
        let response = self
            .request_json(
                Method::POST,
                self.api_url("preauthkey")?,
                Some(serde_json::json!({
                    "user": user,
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
        self.ensure_configured()?;
        let user = user.trim();
        if user.is_empty() {
            anyhow::bail!("Headscale preauth key user is required");
        }
        let response = self
            .request_json(
                Method::POST,
                self.api_url("preauthkey")?,
                Some(serde_json::json!({
                    "user": user,
                    "reusable": reusable,
                    "ephemeral": ephemeral,
                    "expiration": (chrono::Utc::now() + chrono::Duration::hours(24)).to_rfc3339(),
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

    #[tokio::test]
    async fn test_create_preauth_key_requires_configuration() {
        let client = HeadscaleClient::new("", "");
        let error = client
            .create_preauth_key("worker-test-1")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("base_url"));
    }
}
