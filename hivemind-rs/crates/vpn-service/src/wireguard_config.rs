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
        let config = generate_wireguard_config(
            "worker-1",
            "100.64.0.50",
            "key1",
            "key2",
            "endpoint:51820",
        );
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
        let config = generate_wireguard_config(
            "w1",
            "100.64.0.1",
            "pk",
            "pubk",
            "vpn.example.com:51820",
        );
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
