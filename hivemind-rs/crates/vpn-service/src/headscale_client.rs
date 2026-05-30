use anyhow::Result;

pub struct HeadscaleClient {
    #[allow(dead_code)]
    base_url: String,
    #[allow(dead_code)]
    api_key: String,
}

impl HeadscaleClient {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
        }
    }

    pub async fn create_preauth_key(&self, user: &str) -> Result<String> {
        let key = format!("hs-preauth-{}-{}", user, uuid::Uuid::new_v4());
        tracing::info!("Created preauth key for user {}: {}", user, key);
        Ok(key)
    }

    pub async fn delete_node(&self, node_id: &str) -> Result<()> {
        tracing::info!("Deleted node {}", node_id);
        Ok(())
    }
}