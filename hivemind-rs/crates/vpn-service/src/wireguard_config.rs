use boringtun::x25519::{PublicKey, StaticSecret};
use hex;
use rand::rngs::OsRng;

pub fn generate_wireguard_config(
    _worker_id: &str,
    virtual_ip: &str,
    private_key: &str,
    peer_public_key: &str,
    endpoint: &str,
) -> String {
    format!(
        r#"[Interface]
PrivateKey = {}
Address = {}/32
DNS = 1.1.1.1

[Peer]
PublicKey = {}
AllowedIPs = 100.64.0.0/10
Endpoint = {}
PersistentKeepalive = 25
"#,
        private_key, virtual_ip, peer_public_key, endpoint
    )
}

/// Generate a new WireGuard keypair (private key and public key)
pub fn generate_keypair() -> (String, String) {
    let private_key = StaticSecret::random_from_rng(OsRng);
    let public_key = PublicKey::from(&private_key);
    (
        hex::encode(private_key.to_bytes()),
        hex::encode(public_key.to_bytes()),
    )
}

/// Get the platform's WireGuard public key from config or environment.
/// This is the nodepool's WireGuard public key that clients will connect to.
pub fn get_platform_public_key(config: &crate::HivemindConfig) -> String {
    // First check environment variable for platform public key
    if let Ok(key) = std::env::var("HIVEMIND_WG_PLATFORM_PUBLIC_KEY") {
        let key = key.trim();
        if !key.is_empty() {
            return key.to_string();
        }
    }
    // Fall back to config (String, not Option<String>)
    let key = config.vpn.wireguard_platform_public_key.trim();
    if !key.is_empty() {
        return key.to_string();
    }
    // If neither is set, return empty - the caller must handle this
    String::new()
}

/// Get the platform's WireGuard endpoint from config or environment.
pub fn get_platform_endpoint(config: &crate::HivemindConfig) -> String {
    // First check environment variable
    if let Ok(endpoint) = std::env::var("HIVEMIND_WG_PLATFORM_ENDPOINT") {
        let endpoint = endpoint.trim();
        if !endpoint.is_empty() {
            return endpoint.to_string();
        }
    }
    // Fall back to config (String, not Option<String>)
    let endpoint = config.vpn.wireguard_platform_endpoint.trim();
    if !endpoint.is_empty() {
        return endpoint.to_string();
    }
    // Default to the standard WireGuard port on the nodepool VPN IP
    "100.64.0.1:51820".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_config_contains_interface() {
        let config = generate_wireguard_config(
            "worker-1",
            "100.64.0.10",
            "privatekey123",
            "publickey456",
            "vpn.example.com:51820",
        );
        assert!(config.contains("[Interface]"));
        assert!(config.contains("[Peer]"));
    }

    #[test]
    fn test_generate_config_contains_virtual_ip() {
        let config =
            generate_wireguard_config("worker-1", "100.64.0.50", "key1", "key2", "endpoint:51820");
        assert!(config.contains("100.64.0.50/32"));
    }

    #[test]
    fn test_generate_config_contains_keys() {
        let config = generate_wireguard_config(
            "w1",
            "100.64.0.1",
            "my-private-key",
            "my-public-key",
            "vpn.test:51820",
        );
        assert!(config.contains("PrivateKey = my-private-key"));
        assert!(config.contains("PublicKey = my-public-key"));
    }

    #[test]
    fn test_generate_config_contains_endpoint() {
        let config =
            generate_wireguard_config("w1", "100.64.0.1", "pk", "pubk", "vpn.example.com:51820");
        assert!(config.contains("Endpoint = vpn.example.com:51820"));
    }

    #[test]
    fn test_generate_config_contains_keepalive() {
        let config = generate_wireguard_config("w1", "100.64.0.1", "pk", "pubk", "ep");
        assert!(config.contains("PersistentKeepalive = 25"));
    }

    #[test]
    fn test_generate_config_allowed_ips() {
        let config = generate_wireguard_config("w1", "100.64.0.1", "pk", "pubk", "ep");
        assert!(config.contains("AllowedIPs = 100.64.0.0/10"));
    }
}
