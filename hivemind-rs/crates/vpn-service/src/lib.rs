//! Tailscale-based self-hosted VPN with NAT hole-punching.
//! Uses a Headscale coordination server for peer discovery.

pub mod headscale_client;
pub mod wireguard_config;
pub mod peer_manager;

use anyhow::Result;
use hivemind_config::HivemindConfig;
use hivemind_database::DatabaseManager;
use hivemind_models::VpnPeer;

pub struct VpnService {
    client: headscale_client::HeadscaleClient,
    db: DatabaseManager,
    config: HivemindConfig,
}

impl VpnService {
    pub fn new(config: HivemindConfig, db: DatabaseManager) -> Self {
        let client = headscale_client::HeadscaleClient::new(
            &config.vpn.headscale_url, &config.vpn.headscale_api_key,
        );
        Self { client, db, config }
    }

    pub async fn join_vpn(&self, worker_id: &str, hostname: &str, _auth_token: &str) -> Result<VpnPeer> {
        let virtual_ip = peer_manager::allocate_ip(&self.db, &self.config).await?;
        let auth_key = self.client.create_preauth_key(worker_id).await?;

        let peer = VpnPeer {
            id: uuid::Uuid::new_v4(), worker_id: worker_id.to_string(),
            hostname: hostname.to_string(), virtual_ip: virtual_ip.clone(),
            auth_key: auth_key.clone(), online: false,
            last_seen: chrono::Utc::now(), created_at: chrono::Utc::now(),
        };

        sqlx::query(
            "INSERT INTO vpn_peers (worker_id, hostname, virtual_ip, auth_key, online, last_seen)
             VALUES ($1, $2, $3, $4, false, NOW())
             ON CONFLICT (worker_id) DO UPDATE SET
                 hostname = $2, virtual_ip = $3, auth_key = $4, last_seen = NOW()",
        )
        .bind(&peer.worker_id).bind(&peer.hostname).bind(&peer.virtual_ip).bind(&peer.auth_key)
        .execute(&self.db.pool).await?;

        tracing::info!("VPN join: worker={} hostname={} ip={}", worker_id, hostname, virtual_ip);
        Ok(peer)
    }

    pub async fn leave_vpn(&self, worker_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM vpn_peers WHERE worker_id = $1")
            .bind(worker_id).execute(&self.db.pool).await?;
        self.client.delete_node(worker_id).await?;
        tracing::info!("VPN leave: worker={}", worker_id);
        Ok(())
    }

    pub async fn get_task_peers(&self, _task_id: &str) -> Result<Vec<PeerInfo>> {
        let peers = sqlx::query_as::<_, VpnPeer>(
            "SELECT * FROM vpn_peers WHERE online = true",
        ).fetch_all(&self.db.pool).await?;
        Ok(peers.into_iter().map(|p| PeerInfo {
            worker_id: p.worker_id, virtual_ip: p.virtual_ip,
            hostname: p.hostname, online: p.online,
            last_seen: p.last_seen.timestamp(),
        }).collect())
    }

    pub async fn update_vpn_status(&self, worker_id: &str, virtual_ip: &str, online: bool) -> Result<()> {
        sqlx::query(
            "UPDATE vpn_peers SET virtual_ip = $1, online = $2, last_seen = NOW() WHERE worker_id = $3",
        )
        .bind(virtual_ip).bind(online).bind(worker_id)
        .execute(&self.db.pool).await?;
        Ok(())
    }

    pub async fn list_online_peers(&self) -> Result<Vec<VpnPeer>> {
        sqlx::query_as::<_, VpnPeer>(
            "SELECT * FROM vpn_peers WHERE online = true",
        ).fetch_all(&self.db.pool).await.map_err(Into::into)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PeerInfo {
    pub worker_id: String,
    pub virtual_ip: String,
    pub hostname: String,
    pub online: bool,
    pub last_seen: i64,
}
