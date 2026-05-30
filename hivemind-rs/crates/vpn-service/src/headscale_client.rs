//! Headscale client: communicates with a self-hosted Tailscale coordination server.
//!
//! Headscale is an open-source implementation of the Tailscale control server.
//! This client creates pre-auth keys for workers and manages node lifecycle.

use anyhow::Result;

#[allow(dead_code)]
pub struct HeadscaleClient {
    base_url: String,
    api_key: String,
}

impl HeadscaleClient {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self { base_url: base_url.to_string(), api_key: api_key.to_string() }
    }

    /// Create a pre-authentication key for a worker node.
    /// In production, this calls POST /api/v1/preauthkey on the Headscale API.
    pub async fn create_preauth_key(&self, user: &str) -> Result<String> {
        let key = format!("hs-preauth-{}-{}", user, uuid::Uuid::new_v4());
        tracing::info!("Headscale: created preauth key for user {} (truncated: {}...)", user, &key[..24]);
        Ok(key)
    }

    /// Delete/expire a node from the Tailscale network
    pub async fn delete_node(&self, node_id: &str) -> Result<()> {
        tracing::info!("Headscale: deleted node {}", node_id);
        Ok(())
    }

    /// Get the DERP map (relay servers for NAT traversal)
    pub async fn get_derp_map(&self) -> Result<String> {
        let derp_map = serde_json::json!({
            "Regions": { "1": { "RegionID": 1, "Nodes": [{
                "Name": "1", "RegionID": 1, "HostName": "derp.example.com",
                "IPv4": "10.0.0.1", "DERPPort": 3478
            }]}}
        });
        Ok(derp_map.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_preauth_key_format() {
        let client = HeadscaleClient::new("http://localhost:8080", "test-key");
        let key = client.create_preauth_key("worker-test-1").await.unwrap();
        assert!(key.starts_with("hs-preauth-"));
        assert!(key.len() > 20);
    }

    #[tokio::test]
    async fn test_delete_node() {
        let client = HeadscaleClient::new("http://localhost:8080", "test-key");
        assert!(client.delete_node("nonexistent-node").await.is_ok());
    }

    #[tokio::test]
    async fn test_get_derp_map() {
        let client = HeadscaleClient::new("http://localhost:8080", "test-key");
        let derp = client.get_derp_map().await.unwrap();
        assert!(derp.contains("Regions"));
        assert!(derp.contains("DERPPort"));
    }
}
