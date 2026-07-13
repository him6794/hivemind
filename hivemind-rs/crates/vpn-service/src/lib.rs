//! Tailscale-based self-hosted VPN with NAT hole-punching.
//! Uses a Headscale coordination server for peer discovery.

pub mod grpc_server;
pub mod headscale_client;
pub mod peer_manager;
pub mod wireguard_config;

use anyhow::Result;
use hivemind_auth::jwt_service::JwtService;
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
            &config.vpn.headscale_url,
            &config.vpn.headscale_api_key,
        );
        Self { client, db, config }
    }

    async fn load_online_peers(&self) -> Result<Vec<VpnPeer>> {
        sqlx::query_as::<_, VpnPeer>("SELECT * FROM vpn_peers WHERE online = true")
            .fetch_all(&self.db.pool)
            .await
            .map_err(Into::into)
    }

    async fn authorize_worker(&self, worker_id: &str, auth_token: &str) -> Result<()> {
        let claims = JwtService::new(
            &self.config.auth.jwt_secret,
            self.config.auth.token_expiry_hours,
        )
        .validate(auth_token)?;
        if claims.sub == worker_id {
            return Ok(());
        }

        let owner: Option<String> =
            sqlx::query_scalar("SELECT username FROM worker_nodes WHERE worker_id = $1")
                .bind(worker_id)
                .fetch_optional(&self.db.pool)
                .await?;

        match owner {
            Some(owner) if owner == claims.sub => Ok(()),
            Some(_) => anyhow::bail!("VPN token is not authorized for worker"),
            None => anyhow::bail!("worker is not registered"),
        }
    }

    pub async fn join_vpn(
        &self,
        worker_id: &str,
        hostname: &str,
        auth_token: &str,
    ) -> Result<VpnPeer> {
        self.authorize_worker(worker_id, auth_token).await?;
        let virtual_ip = peer_manager::allocate_ip(&self.db, &self.config).await?;
        let auth_key = self.client.create_preauth_key(worker_id).await?;
        let peer = VpnPeer {
            id: uuid::Uuid::new_v4(),
            worker_id: worker_id.to_string(),
            hostname: hostname.to_string(),
            virtual_ip: virtual_ip.clone(),
            auth_key: auth_key.clone(),
            online: false,
            last_seen: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
        };
        sqlx::query(
            "INSERT INTO vpn_peers (worker_id, hostname, virtual_ip, auth_key, online, last_seen)
             VALUES ($1, $2, $3, $4, false, NOW())
             ON CONFLICT (worker_id) DO UPDATE SET
                 hostname = $2, virtual_ip = $3, auth_key = $4, last_seen = NOW()",
        )
        .bind(&peer.worker_id)
        .bind(&peer.hostname)
        .bind(&peer.virtual_ip)
        .bind(&peer.auth_key)
        .execute(&self.db.pool)
        .await?;
        tracing::info!(
            "VPN join: worker={} hostname={} ip={}",
            worker_id,
            hostname,
            virtual_ip
        );
        Ok(peer)
    }

    pub async fn leave_vpn(&self, worker_id: &str, auth_token: &str) -> Result<()> {
        self.authorize_worker(worker_id, auth_token).await?;
        sqlx::query("DELETE FROM vpn_peers WHERE worker_id = $1")
            .bind(worker_id)
            .execute(&self.db.pool)
            .await?;
        self.client.delete_node(worker_id).await?;
        tracing::info!("VPN leave: worker={}", worker_id);
        Ok(())
    }

    pub async fn get_task_peers(
        &self,
        task_id: &str,
        worker_id: &str,
        auth_token: &str,
    ) -> Result<Vec<PeerInfo>> {
        self.authorize_worker(worker_id, auth_token).await?;
        let task_worker_id: Option<String> = sqlx::query_scalar(
            "SELECT worker_id FROM tasks WHERE task_id = $1 AND worker_id IS NOT NULL",
        )
        .bind(task_id)
        .fetch_optional(&self.db.pool)
        .await?;
        let Some(task_worker_id) = task_worker_id else {
            return Ok(Vec::new());
        };
        if task_worker_id != worker_id {
            anyhow::bail!("VPN token is not authorized for task peers");
        }

        let peers = sqlx::query_as::<_, VpnPeer>(
            "SELECT * FROM vpn_peers WHERE online = true AND worker_id = $1",
        )
        .bind(&task_worker_id)
        .fetch_all(&self.db.pool)
        .await?;
        Ok(peers
            .into_iter()
            .map(|p| PeerInfo {
                worker_id: p.worker_id,
                virtual_ip: p.virtual_ip,
                hostname: p.hostname,
                online: p.online,
                last_seen: p.last_seen.timestamp(),
            })
            .collect())
    }

    pub async fn update_vpn_status(
        &self,
        worker_id: &str,
        virtual_ip: &str,
        online: bool,
        auth_token: &str,
    ) -> Result<()> {
        self.authorize_worker(worker_id, auth_token).await?;
        sqlx::query("UPDATE vpn_peers SET virtual_ip = $1, online = $2, last_seen = NOW() WHERE worker_id = $3")
            .bind(virtual_ip).bind(online).bind(worker_id).execute(&self.db.pool).await?;
        Ok(())
    }

    pub async fn list_online_peers(&self) -> Result<Vec<VpnPeer>> {
        self.load_online_peers().await
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use hivemind_auth::jwt_service::JwtService;
    use hivemind_models::Claims;

    async fn test_service(
        test_name: &str,
    ) -> Option<(VpnService, hivemind_database::postgres::IsolatedTestPool)> {
        let mut config = HivemindConfig::for_test();
        config.auth.jwt_secret = "vpn-test-secret".into();
        config.vpn.headscale_url = "http://localhost:8080".into();
        config.vpn.headscale_api_key = "test-key".into();
        let fixture = hivemind_database::postgres::create_isolated_test_pool(test_name)
            .await
            .ok()?;
        hivemind_database::postgres::run_migrations(&fixture.pool)
            .await
            .ok()?;
        let db = DatabaseManager {
            pool: fixture.pool.clone(),
        };
        Some((VpnService::new(config, db), fixture))
    }

    fn token_for(secret: &str, subject: &str) -> String {
        JwtService::new(secret, 24)
            .encode_claims(&Claims {
                sub: subject.into(),
                user_id: uuid::Uuid::new_v4().to_string(),
                role: None,
                exp: (Utc::now().timestamp() + 3600) as usize,
                iat: Utc::now().timestamp() as usize,
            })
            .unwrap()
    }

    #[tokio::test]
    async fn join_vpn_rejects_invalid_auth_token_before_creating_peer() {
        let (service, fixture) = match test_service("vpn_join_invalid_token").await {
            Some(parts) => parts,
            None => return,
        };
        let schema: String = sqlx::query_scalar("SELECT current_schema()")
            .fetch_one(&service.db.pool)
            .await
            .unwrap();
        assert!(
            schema.starts_with("hm_test_"),
            "expected isolated test schema, got {schema}"
        );
        let worker_id = format!("vpn-auth-worker-{}", uuid::Uuid::new_v4());
        sqlx::query(
            "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb)
             VALUES ($1, $2, '10.0.0.10', 4, 16)",
        )
        .bind(&worker_id)
        .bind(&worker_id)
        .execute(&service.db.pool)
        .await
        .unwrap();

        let result = service
            .join_vpn(&worker_id, "worker-host", "not-a-valid-token")
            .await;
        assert!(result.is_err());

        let peer_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM vpn_peers WHERE worker_id = $1)")
                .bind(&worker_id)
                .fetch_one(&service.db.pool)
                .await
                .unwrap();
        assert!(!peer_exists);

        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = $1")
            .bind(&worker_id)
            .execute(&service.db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn get_task_peers_returns_only_peer_assigned_to_authorized_task() {
        let (service, fixture) = match test_service("vpn_task_peers_scope").await {
            Some(parts) => parts,
            None => return,
        };
        let unique = uuid::Uuid::new_v4().to_string();
        let owner = format!("vpn-owner-{unique}");
        let assigned_worker = format!("vpn-worker-assigned-{unique}");
        let other_worker = format!("vpn-worker-other-{unique}");
        let task_id = format!("vpn-task-{unique}");
        let token = token_for(&service.config.auth.jwt_secret, &assigned_worker);
        let assigned_ip = format!("100.64.{}.{}", unique.as_bytes()[0], unique.as_bytes()[1]);
        let other_ip = format!("100.64.{}.{}", unique.as_bytes()[2], unique.as_bytes()[3]);

        sqlx::query(
            "INSERT INTO users (username, password_hash, balance) VALUES ($1, 'hash', 100)",
        )
        .bind(&owner)
        .execute(&service.db.pool)
        .await
        .unwrap();
        for worker_id in [&assigned_worker, &other_worker] {
            sqlx::query(
                "INSERT INTO worker_nodes (worker_id, username, ip, cpu_cores, memory_gb)
                 VALUES ($1, $1, '10.0.0.11', 4, 16)",
            )
            .bind(worker_id)
            .execute(&service.db.pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO tasks (task_id, owner, worker_id, status)
             VALUES ($1, $2, $3, 'ASSIGNED')",
        )
        .bind(&task_id)
        .bind(&owner)
        .bind(&assigned_worker)
        .execute(&service.db.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO vpn_peers (worker_id, hostname, virtual_ip, auth_key, online)
             VALUES ($1, 'assigned-host', $3, 'assigned-key', true),
                    ($2, 'other-host', $4, 'other-key', true)",
        )
        .bind(&assigned_worker)
        .bind(&other_worker)
        .bind(&assigned_ip)
        .bind(&other_ip)
        .execute(&service.db.pool)
        .await
        .unwrap();

        let peers = service
            .get_task_peers(&task_id, &assigned_worker, &token)
            .await
            .unwrap();

        sqlx::query("DELETE FROM vpn_peers WHERE worker_id = ANY($1)")
            .bind(vec![assigned_worker.clone(), other_worker.clone()])
            .execute(&service.db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM tasks WHERE task_id = $1")
            .bind(&task_id)
            .execute(&service.db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM worker_nodes WHERE worker_id = ANY($1)")
            .bind(vec![assigned_worker.clone(), other_worker.clone()])
            .execute(&service.db.pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(&owner)
            .execute(&service.db.pool)
            .await
            .ok();
        fixture.cleanup().await.ok();

        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].worker_id, assigned_worker);
    }
}
