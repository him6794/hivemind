use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

pub const ENROLLMENT_CREDENTIAL_TTL_SECS: i64 = 10 * 60;
pub const ENROLLMENT_CREDENTIAL_MAX_BYTES: usize = 256;
pub const ENROLLMENT_CLIENT_INSTANCE_MAX_BYTES: usize = 128;
const ENROLLMENT_CREDENTIAL_PREFIX: &str = "hm-enroll-v1";
const ENROLLMENT_SECRET_HEX_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollmentRole {
    Master,
    Worker,
}

impl EnrollmentRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Master => "master",
            Self::Worker => "worker",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "master" => Ok(Self::Master),
            "worker" => Ok(Self::Worker),
            _ => bail!("enrollment role must be master or worker"),
        }
    }
}

#[derive(Clone)]
pub struct IssuedEnrollmentCredential {
    pub credential_id: Uuid,
    pub owner: String,
    pub role: EnrollmentRole,
    pub client_instance_id: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub token: String,
    pub token_sha256: String,
}

impl std::fmt::Debug for IssuedEnrollmentCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IssuedEnrollmentCredential")
            .field("credential_id", &self.credential_id)
            .field("owner", &self.owner)
            .field("role", &self.role)
            .field("client_instance_id", &self.client_instance_id)
            .field("issued_at", &self.issued_at)
            .field("expires_at", &self.expires_at)
            .field("token", &"<redacted>")
            .field("token_sha256", &self.token_sha256)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct RedeemedEnrollment {
    pub credential_id: Uuid,
    pub owner: String,
    pub role: EnrollmentRole,
    pub client_instance_id: String,
    pub identity_id: Uuid,
    pub worker_id: Option<String>,
    pub expires_at: DateTime<Utc>,
}

pub fn validate_client_instance_id(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("client_instance_id is required");
    }
    if value.len() > ENROLLMENT_CLIENT_INSTANCE_MAX_BYTES {
        bail!("client_instance_id is too long");
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!("client_instance_id contains unsupported characters");
    }
    Ok(value.to_string())
}

pub fn issue_credential(
    owner: &str,
    role: EnrollmentRole,
    client_instance_id: &str,
) -> Result<IssuedEnrollmentCredential> {
    let owner = owner.trim();
    if owner.is_empty() {
        bail!("authenticated owner is required");
    }
    let client_instance_id = validate_client_instance_id(client_instance_id)?;
    let credential_id = Uuid::new_v4();
    let mut secret = [0u8; 32];
    OsRng.fill_bytes(&mut secret);
    let token = format!(
        "{ENROLLMENT_CREDENTIAL_PREFIX}.{}.{}",
        credential_id.simple(),
        hex::encode(secret)
    );
    let issued_at = Utc::now();
    let expires_at = issued_at + Duration::seconds(ENROLLMENT_CREDENTIAL_TTL_SECS);
    Ok(IssuedEnrollmentCredential {
        credential_id,
        owner: owner.to_string(),
        role,
        client_instance_id,
        issued_at,
        expires_at,
        token_sha256: token_sha256(&token),
        token,
    })
}

pub fn parse_credential_id(token: &str) -> Result<Uuid> {
    if token.len() > ENROLLMENT_CREDENTIAL_MAX_BYTES {
        bail!("enrollment credential is too long");
    }
    let mut parts = token.split('.');
    if parts.next() != Some(ENROLLMENT_CREDENTIAL_PREFIX) {
        bail!("invalid enrollment credential");
    }
    let credential_id = parts
        .next()
        .context("invalid enrollment credential id")
        .and_then(|value| Uuid::parse_str(value).context("invalid enrollment credential id"))?;
    let secret = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || secret.len() != ENROLLMENT_SECRET_HEX_BYTES
        || !secret.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("invalid enrollment credential secret");
    }
    Ok(credential_id)
}

pub fn token_sha256(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub async fn store_credential(
    pool: &PgPool,
    credential: &IssuedEnrollmentCredential,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO enrollment_credentials
            (credential_id, owner, role, client_instance_id, token_sha256,
             issued_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(credential.credential_id)
    .bind(&credential.owner)
    .bind(credential.role.as_str())
    .bind(&credential.client_instance_id)
    .bind(&credential.token_sha256)
    .bind(credential.issued_at)
    .bind(credential.expires_at)
    .execute(pool)
    .await
    .context("failed to persist enrollment credential")?;
    Ok(())
}

pub async fn redeem_credential(pool: &PgPool, token: &str) -> Result<RedeemedEnrollment> {
    let credential_id = parse_credential_id(token)?;
    let token_sha256 = token_sha256(token);
    let mut transaction = pool
        .begin()
        .await
        .context("failed to start enrollment redemption transaction")?;

    let row = sqlx::query(
        "SELECT owner, role, client_instance_id, token_sha256, issued_at,
                expires_at, redeemed_at
         FROM enrollment_credentials
         WHERE credential_id = $1
         FOR UPDATE",
    )
    .bind(credential_id)
    .fetch_optional(&mut *transaction)
    .await
    .context("failed to load enrollment credential")?
    .ok_or_else(|| anyhow::anyhow!("invalid enrollment credential"))?;

    let stored_hash: String = sqlx::Row::try_get(&row, "token_sha256")?;
    let owner: String = sqlx::Row::try_get(&row, "owner")?;
    let role_value: String = sqlx::Row::try_get(&row, "role")?;
    let client_instance_id: String = sqlx::Row::try_get(&row, "client_instance_id")?;
    let expires_at: DateTime<Utc> = sqlx::Row::try_get(&row, "expires_at")?;
    let redeemed_at: Option<DateTime<Utc>> = sqlx::Row::try_get(&row, "redeemed_at")?;

    if stored_hash != token_sha256 {
        bail!("invalid enrollment credential");
    }
    if redeemed_at.is_some() {
        bail!("enrollment credential has already been redeemed");
    }
    if expires_at <= Utc::now() {
        bail!("enrollment credential has expired");
    }
    let role = EnrollmentRole::parse(&role_value)?;

    let identity_id = Uuid::new_v4();
    let worker_id =
        (role == EnrollmentRole::Worker).then(|| format!("hm-worker-{}", identity_id.simple()));
    let identity_row = sqlx::query(
        "INSERT INTO client_identities
            (identity_id, owner, role, client_instance_id, worker_id)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (owner, role, client_instance_id)
         DO UPDATE SET updated_at = NOW()
         RETURNING identity_id, worker_id",
    )
    .bind(identity_id)
    .bind(&owner)
    .bind(role.as_str())
    .bind(&client_instance_id)
    .bind(&worker_id)
    .fetch_one(&mut *transaction)
    .await
    .context("failed to create or recover client identity")?;
    let identity_id: Uuid = sqlx::Row::try_get(&identity_row, "identity_id")?;
    let worker_id: Option<String> = sqlx::Row::try_get(&identity_row, "worker_id")?;

    sqlx::query(
        "UPDATE enrollment_credentials
         SET redeemed_at = NOW(), redeemed_identity_id = $2
         WHERE credential_id = $1 AND redeemed_at IS NULL",
    )
    .bind(credential_id)
    .bind(identity_id)
    .execute(&mut *transaction)
    .await
    .context("failed to mark enrollment credential redeemed")?;

    transaction
        .commit()
        .await
        .context("failed to commit enrollment redemption")?;

    Ok(RedeemedEnrollment {
        credential_id,
        owner,
        role,
        client_instance_id,
        identity_id,
        worker_id,
        expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issued_credentials_are_bounded_and_parseable_without_persisting_secret() {
        let issued = issue_credential("alice", EnrollmentRole::Worker, "device-1").unwrap();
        assert!(issued.token.starts_with("hm-enroll-v1."));
        assert_eq!(
            parse_credential_id(&issued.token).unwrap(),
            issued.credential_id
        );
        assert_eq!(issued.token_sha256, token_sha256(&issued.token));
        assert!(!issued.token_sha256.contains(&issued.token));
        assert!(!format!("{issued:?}").contains(&issued.token));
        assert!(issued.expires_at > issued.issued_at);
    }

    #[test]
    fn credential_validation_rejects_malformed_or_oversized_inputs() {
        assert!(parse_credential_id("not-a-credential").is_err());
        assert!(parse_credential_id(&"x".repeat(ENROLLMENT_CREDENTIAL_MAX_BYTES + 1)).is_err());
        assert!(validate_client_instance_id("bad id").is_err());
        assert!(validate_client_instance_id(
            "x".repeat(ENROLLMENT_CLIENT_INSTANCE_MAX_BYTES + 1)
                .as_str()
        )
        .is_err());
    }

    #[test]
    fn roles_are_explicit_and_case_insensitive() {
        assert_eq!(
            EnrollmentRole::parse("WORKER").unwrap(),
            EnrollmentRole::Worker
        );
        assert_eq!(EnrollmentRole::Master.as_str(), "master");
        assert!(EnrollmentRole::parse("provider").is_err());
    }

    async fn test_pool(test_name: &str) -> Option<hivemind_database::postgres::IsolatedTestPool> {
        let fixture = match hivemind_database::postgres::create_isolated_test_pool(test_name).await
        {
            Ok(fixture) => fixture,
            Err(error) => {
                eprintln!("skipping enrollment database test: {error}");
                return None;
            }
        };
        if let Err(error) = hivemind_database::postgres::run_migrations(&fixture.pool).await {
            eprintln!("skipping enrollment database test: {error}");
            fixture.cleanup().await.ok();
            return None;
        }
        Some(fixture)
    }

    async fn insert_test_user(pool: &PgPool, username: &str) {
        sqlx::query(
            "INSERT INTO users (username, password_hash) VALUES ($1, 'enrollment-test-hash')",
        )
        .bind(username)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn database_redemption_is_single_use_and_recovers_identity() {
        let Some(fixture) = test_pool("auth_enrollment_recovery").await else {
            return;
        };
        let owner = format!("enrollment-owner-{}", Uuid::new_v4());
        insert_test_user(&fixture.pool, &owner).await;

        let issued = issue_credential(&owner, EnrollmentRole::Worker, "device-1").unwrap();
        store_credential(&fixture.pool, &issued).await.unwrap();
        let persisted_hash: String = sqlx::query_scalar(
            "SELECT token_sha256 FROM enrollment_credentials WHERE credential_id = $1",
        )
        .bind(issued.credential_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(persisted_hash, issued.token_sha256);
        assert_ne!(persisted_hash, issued.token);

        let first = redeem_credential(&fixture.pool, &issued.token)
            .await
            .unwrap();
        assert_eq!(first.owner, owner);
        assert_eq!(first.role, EnrollmentRole::Worker);
        assert_eq!(first.client_instance_id, "device-1");
        assert!(first
            .worker_id
            .as_deref()
            .is_some_and(|worker_id| worker_id.starts_with("hm-worker-")));

        let replay = redeem_credential(&fixture.pool, &issued.token).await;
        assert!(replay.is_err());

        let recovered_credential =
            issue_credential(&owner, EnrollmentRole::Worker, "device-1").unwrap();
        store_credential(&fixture.pool, &recovered_credential)
            .await
            .unwrap();
        let recovered = redeem_credential(&fixture.pool, &recovered_credential.token)
            .await
            .unwrap();
        assert_eq!(recovered.identity_id, first.identity_id);
        assert_eq!(recovered.worker_id, first.worker_id);

        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn database_redemption_rejects_expired_credentials() {
        let Some(fixture) = test_pool("auth_enrollment_expiry").await else {
            return;
        };
        let owner = format!("enrollment-expiry-{}", Uuid::new_v4());
        insert_test_user(&fixture.pool, &owner).await;
        let issued = issue_credential(&owner, EnrollmentRole::Master, "device-expired").unwrap();
        store_credential(&fixture.pool, &issued).await.unwrap();
        sqlx::query(
            "UPDATE enrollment_credentials SET expires_at = NOW() - INTERVAL '1 second' WHERE credential_id = $1",
        )
        .bind(issued.credential_id)
        .execute(&fixture.pool)
        .await
        .unwrap();

        let error = redeem_credential(&fixture.pool, &issued.token)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("expired"), "{error}");
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_redemption_allows_only_one_winner() {
        let Some(fixture) = test_pool("auth_enrollment_concurrency").await else {
            return;
        };
        let owner = format!("enrollment-concurrent-{}", Uuid::new_v4());
        insert_test_user(&fixture.pool, &owner).await;
        let issued = issue_credential(&owner, EnrollmentRole::Worker, "device-race").unwrap();
        store_credential(&fixture.pool, &issued).await.unwrap();

        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(8));
        let mut redemptions = Vec::new();
        for _ in 0..8 {
            let pool = fixture.pool.clone();
            let token = issued.token.clone();
            let barrier = barrier.clone();
            redemptions.push(tokio::spawn(async move {
                barrier.wait().await;
                redeem_credential(&pool, &token).await
            }));
        }
        let mut successful_redemptions = 0;
        for redemption in redemptions {
            if redemption.await.unwrap().is_ok() {
                successful_redemptions += 1;
            }
        }
        assert_eq!(successful_redemptions, 1);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM client_identities WHERE owner = $1 AND client_instance_id = $2",
            )
            .bind(&owner)
            .bind("device-race")
            .fetch_one(&fixture.pool)
            .await
            .unwrap(),
            1
        );
        fixture.cleanup().await.unwrap();
    }
}
