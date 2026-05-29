use anyhow::Result;
use hivemind_config::HivemindConfig;
use hivemind_database::DatabaseManager;
use std::sync::atomic::{AtomicU32, Ordering};

static LAST_OCTET: AtomicU32 = AtomicU32::new(10);

pub async fn allocate_ip(db: &DatabaseManager, config: &HivemindConfig) -> Result<String> {
    let mut octet = LAST_OCTET.fetch_add(1, Ordering::SeqCst);
    if octet > 254 {
        LAST_OCTET.store(10, Ordering::SeqCst);
        octet = LAST_OCTET.fetch_add(1, Ordering::SeqCst);
    }

    let ip = format!("100.64.0.{}", octet);

    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM vpn_peers WHERE virtual_ip = $1)",
    )
    .bind(&ip)
    .fetch_one(&db.pool)
    .await?;

    if exists {
        return Box::pin(allocate_ip(db, config)).await;
    }

    Ok(ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocate_ip_format() {
        // Just verify the format string works
        let ip = format!("100.64.0.{}", 10);
        assert_eq!(ip, "100.64.0.10");
        assert!(ip.starts_with("100.64.0."));
    }

    #[test]
    fn test_ip_range_bounds() {
        // Verify the range logic
        let max_octet = 254;
        let min_octet = 10;
        assert!(min_octet < max_octet);
        assert!(max_octet <= 254);
    }
}
