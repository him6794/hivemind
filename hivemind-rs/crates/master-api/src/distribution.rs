use anyhow::{Context, Result};
use hivemind_client_runtime::{current_vpn_session, ClientRole};
use hivemind_config::HivemindConfig;
use hivemind_torrent_service::{transfer, SeedStore, TorrentService};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

/// Master-owned task package distribution.
///
/// The master keeps the uploaded package and serves pieces. Nodepool only
/// receives the resulting magnet/torrent reference and tracker metadata.
pub struct MasterDistribution {
    torrent_service: TorrentService,
    announce_url: String,
    seed_addr: SocketAddr,
    configured_seed_advertise_host: Option<String>,
}

impl MasterDistribution {
    pub async fn start(config: &HivemindConfig) -> Result<Arc<Self>> {
        let seed_addr: SocketAddr = config
            .torrent
            .seed_listen_addr
            .parse()
            .context("invalid torrent seed listen address")?;
        let seed_store = SeedStore::new();
        let handle = transfer::start_seed_listener(seed_addr, seed_store.clone()).await?;
        std::mem::forget(handle);

        let configured_seed_host = config
            .torrent
            .seed_advertise_host
            .clone()
            .filter(|host| !host.trim().is_empty());
        Ok(Arc::new(Self {
            torrent_service: TorrentService::with_dirs(
                config.torrent.api_dir.clone().into(),
                config.torrent.bt_dir.clone().into(),
                seed_store,
            ),
            announce_url: config.torrent.announce_url.clone(),
            seed_addr,
            configured_seed_advertise_host: configured_seed_host,
        }))
    }

    pub async fn seed_package(
        &self,
        package_data: &[u8],
        package_filename: &str,
    ) -> Result<String> {
        let info = self
            .torrent_service
            .package_bytes_to_torrent(
                package_data,
                Path::new(package_filename),
                &self.announce_url,
            )
            .await?;
        let seed_advertise_host = if let Some(host) = self.configured_seed_advertise_host.clone() {
            host
        } else if let Some(host) = current_vpn_session(ClientRole::Master)
            .await
            .and_then(|session| session.overlay_ip.clone())
        {
            host
        } else {
            match self.seed_addr.ip() {
                std::net::IpAddr::V4(ip) if ip.is_unspecified() => "127.0.0.1".into(),
                std::net::IpAddr::V6(ip) if ip.is_unspecified() => "::1".into(),
                ip => ip.to_string(),
            }
        };
        let peer_id = format!("master-seeder-{}", &info.info_hash[..8]);
        transfer::announce_to_tracker(
            &self.announce_url,
            &info.info_hash,
            &peer_id,
            self.seed_addr.port(),
            0,
        )
        .await
        .context("failed to announce master torrent seeder")?;
        tracing::info!(
            "Master seeded task package {} at {}:{}",
            info.info_hash,
            seed_advertise_host,
            self.seed_addr.port()
        );
        Ok(info.magnet_uri)
    }
}
