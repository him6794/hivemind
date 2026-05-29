
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