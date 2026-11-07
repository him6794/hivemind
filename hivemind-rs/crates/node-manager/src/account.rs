//! Website account lifecycle: email verification, the welcome grant from the system pool,
//! password change and recovery, and username reminders.
//!
//! Verification and reset tokens are random, kept only as SHA-256 hashes, single-use, and
//! expire. Nodepool sends the mail itself, so a reset token never travels back to a caller
//! of the gRPC API. Human verification (Turnstile) is enforced by the website backend.

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use hivemind_models::SYSTEM_ACCOUNT_USERNAME;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub const WELCOME_GRANT_CPT: i64 = 1000;
pub const PURPOSE_EMAIL_VERIFICATION: &str = "email_verification";
pub const PURPOSE_PASSWORD_RESET: &str = "password_reset";
pub const GRANT_NONE: &str = "none";
pub const GRANT_GRANTED: &str = "granted";
pub const GRANT_POOL_EMPTY: &str = "pool_empty";
pub const GRANT_ALREADY_GRANTED: &str = "already_granted";

const EMAIL_MAX_BYTES: usize = 254;
const PASSWORD_MIN_BYTES: usize = 8;
const PASSWORD_MAX_BYTES: usize = 72;
const RESEND_COOLDOWN_SECS: i64 = 60;
const RESEND_API_URL: &str = "https://api.resend.com/emails";
const RESEND_TIMEOUT_SECS: u64 = 10;
const DEFAULT_SITE_URL: &str = "http://localhost:3000";

pub fn email_verification_ttl() -> Duration {
    Duration::hours(24)
}

pub fn password_reset_ttl() -> Duration {
    Duration::hours(1)
}

fn resend_cooldown() -> Duration {
    Duration::seconds(RESEND_COOLDOWN_SECS)
}

/// Where account mail goes and which site URL its links point to.
#[derive(Clone, Debug)]
pub struct AccountSettings {
    pub site_url: String,
    pub mail: MailTransport,
}

#[derive(Clone, Debug)]
pub enum MailTransport {
    /// Nothing is configured, so sending fails and no token is written anywhere.
    Disabled,
    /// Local development only: each message is written as a text file in this directory.
    File(PathBuf),
    /// Production: each message is sent through the Resend HTTP API.
    Resend(ResendConfig),
}

/// Resend API settings. `Debug` hides the key, so it cannot reach a log line.
#[derive(Clone)]
pub struct ResendConfig {
    api_key: String,
    from: String,
    endpoint: String,
    client: reqwest::Client,
}

impl ResendConfig {
    fn new(api_key: String, from: String, endpoint: String) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(RESEND_TIMEOUT_SECS))
            .build()
            .map_err(|error| anyhow!("could not build the Resend HTTP client: {error}"))?;
        Ok(Self {
            api_key,
            from,
            endpoint,
            client,
        })
    }
}

impl std::fmt::Debug for ResendConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResendConfig")
            .field("api_key", &"<redacted>")
            .field("from", &self.from)
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl AccountSettings {
    /// Reads the account mail settings from the process environment. See `from_lookup`.
    pub fn load_from_env() -> Result<Self> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Settings for `GrpcUserService::new`. Invalid mail settings disable mail and log the
    /// reason. Startup calls `load_from_env` first and refuses to start on the same error.
    pub fn from_env() -> Self {
        Self::load_from_env().unwrap_or_else(|error| {
            tracing::error!(error = %error, "account mail is disabled: its settings are invalid");
            Self {
                site_url: DEFAULT_SITE_URL.to_string(),
                mail: MailTransport::Disabled,
            }
        })
    }

    /// `MAIL_TRANSPORT` selects `file` (local development) or `resend`. `disabled` or unset turns
    /// mail off. Any other value is refused, so a typo cannot turn mail off without a warning.
    /// `resend` also needs `RESEND_API_KEY`, `MAIL_FROM`, and `PUBLIC_SITE_URL`,
    /// so a production message never carries a `localhost` link by accident.
    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let value = |name: &str| {
            lookup(name)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let mail = match value("MAIL_TRANSPORT").as_deref() {
            Some("file") => MailTransport::File(
                value("MAIL_OUTBOX_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("dev-mail")),
            ),
            Some("resend") => MailTransport::Resend(ResendConfig::new(
                value("RESEND_API_KEY").ok_or_else(|| {
                    anyhow!("RESEND_API_KEY must be set when MAIL_TRANSPORT=resend")
                })?,
                value("MAIL_FROM")
                    .ok_or_else(|| anyhow!("MAIL_FROM must be set when MAIL_TRANSPORT=resend"))?,
                RESEND_API_URL.to_string(),
            )?),
            None | Some("disabled") => MailTransport::Disabled,
            Some(_) => {
                return Err(anyhow!("MAIL_TRANSPORT must be file, resend, or disabled"));
            }
        };
        let site_url = match value("PUBLIC_SITE_URL") {
            Some(url) => url.trim_end_matches('/').to_string(),
            None if matches!(mail, MailTransport::Resend(_)) => {
                return Err(anyhow!(
                    "PUBLIC_SITE_URL must be set when MAIL_TRANSPORT=resend"
                ));
            }
            None => DEFAULT_SITE_URL.to_string(),
        };
        Ok(Self { site_url, mail })
    }
}

/// Lowercases and checks an email address. This is a shape check, not a delivery check.
pub fn normalize_email(raw: &str) -> Result<String, &'static str> {
    const INVALID: &str = "Enter a valid email address";
    let email = raw.trim().to_ascii_lowercase();
    if email.is_empty() || email.len() > EMAIL_MAX_BYTES || email.chars().any(char::is_whitespace) {
        return Err(INVALID);
    }
    let Some((local, domain)) = email.split_once('@') else {
        return Err(INVALID);
    };
    let local_ok = !local.is_empty() && !local.contains('@');
    let domain_ok = domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains("..")
        && domain.split('.').all(|label| {
            !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    if local_ok && domain_ok {
        Ok(email)
    } else {
        Err(INVALID)
    }
}

/// Bcrypt only reads the first 72 bytes, so longer passwords are refused instead of truncated.
pub fn validate_password(password: &str) -> Result<(), &'static str> {
    if password.len() < PASSWORD_MIN_BYTES {
        return Err("Password must be at least 8 characters");
    }
    if password.len() > PASSWORD_MAX_BYTES {
        return Err("Password must be at most 72 bytes");
    }
    Ok(())
}

pub fn new_token() -> String {
    hex::encode(rand::random::<[u8; 32]>())
}

pub fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// One account row as the account operations need it.
#[derive(Debug, Clone)]
pub struct AccountRecord {
    pub id: Uuid,
    pub username: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub balance: i64,
    pub welcome_grant_status: String,
    pub password_hash: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum EmailClaimError {
    InUse,
    AlreadyBound,
    CredentialsChanged,
}

impl std::fmt::Display for EmailClaimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InUse => "This email address is already in use.",
            Self::AlreadyBound => "This account already has an email address.",
            Self::CredentialsChanged => "Account credentials changed; sign in again",
        })
    }
}
impl std::error::Error for EmailClaimError {}

// Serialize absent and expired claims across instances, then lock all affected
// accounts in ID order. Binding never locks its own account before the email key.
async fn lock_email_claim(
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
    actor: Option<Uuid>,
) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 834723))")
        .bind(email)
        .execute(&mut **tx)
        .await?;
    sqlx::query("SELECT id FROM users WHERE LOWER(email) = $1 OR id = $2 ORDER BY id FOR UPDATE")
        .bind(email)
        .bind(actor)
        .fetch_all(&mut **tx)
        .await?;
    sqlx::query(
        "WITH detached AS (
            UPDATE users SET email = NULL, email_claim_expires_at = NULL, updated_at = NOW()
            WHERE LOWER(email) = $1 AND email_verified_at IS NULL
              AND email_claim_expires_at <= clock_timestamp() RETURNING id
         ) UPDATE account_tokens SET used_at = NOW()
           WHERE user_id IN (SELECT id FROM detached) AND purpose = $2 AND used_at IS NULL",
    )
    .bind(email)
    .bind(PURPOSE_EMAIL_VERIFICATION)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn ensure_email_available(tx: &mut Transaction<'_, Postgres>, email: &str) -> Result<()> {
    let taken: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE LOWER(email) = $1)")
            .bind(email)
            .fetch_one(&mut **tx)
            .await?;
    if taken {
        return Err(EmailClaimError::InUse.into());
    }
    Ok(())
}

pub async fn create_unverified_user(
    pool: &PgPool,
    username: &str,
    password_hash: &str,
    email: &str,
) -> Result<Uuid> {
    let email = normalize_email(email).map_err(|message| anyhow!(message))?;
    let mut tx = pool.begin().await?;
    lock_email_claim(&mut tx, &email, None).await?;
    ensure_email_available(&mut tx, &email).await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (username, password_hash, balance, email, email_claim_expires_at, welcome_grant_status)
         VALUES ($1, $2, 0, $3, clock_timestamp() + INTERVAL '24 hours', 'none') RETURNING id",
    )
    .bind(username).bind(password_hash).bind(&email).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(id)
}

/// Bind only a missing or expired unverified email, without changing account assets.
/// The hash checked by the caller is compared again while the account is locked.
pub async fn bind_email(
    pool: &PgPool,
    user_id: Uuid,
    password_hash: &str,
    email: &str,
) -> Result<()> {
    let email = normalize_email(email).map_err(|message| anyhow!(message))?;
    let mut tx = pool.begin().await?;
    lock_email_claim(&mut tx, &email, Some(user_id)).await?;
    let current: Option<(Option<String>, bool)> = sqlx::query_as(
        "SELECT email, email_verified_at IS NOT NULL OR COALESCE(email_claim_expires_at > clock_timestamp(), false)
         FROM users WHERE id = $1 AND password_hash = $2 AND is_active = true",
    ).bind(user_id).bind(password_hash).fetch_optional(&mut *tx).await?;
    let Some((old_email, live)) = current else {
        return Err(EmailClaimError::CredentialsChanged.into());
    };
    if old_email.is_some() && live {
        return Err(EmailClaimError::AlreadyBound.into());
    }
    ensure_email_available(&mut tx, &email).await?;
    sqlx::query("UPDATE account_tokens SET used_at = NOW() WHERE user_id = $1 AND purpose = $2 AND used_at IS NULL")
        .bind(user_id).bind(PURPOSE_EMAIL_VERIFICATION).execute(&mut *tx).await?;
    sqlx::query(
        "UPDATE users SET email = $2, email_verified_at = NULL,
            email_claim_expires_at = clock_timestamp() + INTERVAL '24 hours', updated_at = NOW() WHERE id = $1",
    ).bind(user_id).bind(&email).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

// The columns the account queries read, in the order `record_from_row` expects them.
type AccountRow = (Uuid, String, Option<String>, bool, i64, String, String);

pub async fn find_active_account(pool: &PgPool, username: &str) -> Result<Option<AccountRecord>> {
    let row: Option<AccountRow> = sqlx::query_as(
        "SELECT id, username,
                CASE WHEN email_verified_at IS NOT NULL OR email_claim_expires_at > clock_timestamp() THEN email END,
                email_verified_at IS NOT NULL, balance, welcome_grant_status, password_hash
         FROM users
         WHERE username = $1 AND is_active = true",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(record_from_row))
}

/// Finds the active account whose verified address matches. Unverified addresses are never
/// used for recovery, because nobody has yet proven they control the address.
pub async fn find_verified_by_email(pool: &PgPool, email: &str) -> Result<Option<AccountRecord>> {
    let row: Option<AccountRow> = sqlx::query_as(
        "SELECT id, username, email, email_verified_at IS NOT NULL, balance,
                welcome_grant_status, password_hash
         FROM users
         WHERE LOWER(email) = $1 AND email_verified_at IS NOT NULL AND is_active = true",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(record_from_row))
}

/// Atomically reserve one username reminder per account per minute. The timestamp
/// survives restarts and failed delivery, so retries cannot flood the mail provider.
pub async fn reserve_username_reminder(pool: &PgPool, email: &str) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "UPDATE users SET username_reminder_requested_at = clock_timestamp()
         WHERE LOWER(email) = $1 AND email_verified_at IS NOT NULL AND is_active = true
           AND (username_reminder_requested_at IS NULL
                OR username_reminder_requested_at <= clock_timestamp() - INTERVAL '60 seconds')
         RETURNING username",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?)
}

/// Finds verified ownership (including disabled accounts) or an unexpired reservation.
/// An expired unverified claim is available again, without deleting its account.
pub async fn find_registered_by_email(pool: &PgPool, email: &str) -> Result<Option<AccountRecord>> {
    let row: Option<AccountRow> = sqlx::query_as(
        "SELECT id, username, email, email_verified_at IS NOT NULL, balance,
                welcome_grant_status, password_hash
         FROM users
         WHERE LOWER(email) = $1
           AND (email_verified_at IS NOT NULL OR email_claim_expires_at > clock_timestamp())
         LIMIT 1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(record_from_row))
}

fn record_from_row(
    (id, username, email, email_verified, balance, welcome_grant_status, password_hash): (
        Uuid,
        String,
        Option<String>,
        bool,
        i64,
        String,
        String,
    ),
) -> AccountRecord {
    AccountRecord {
        id,
        username,
        email,
        email_verified,
        balance,
        welcome_grant_status,
        password_hash,
    }
}

/// Stores a new token and retires any older unused token of the same purpose.
pub async fn issue_token(
    pool: &PgPool,
    user_id: Uuid,
    purpose: &str,
    ttl: Duration,
) -> Result<String> {
    issue_token_for_email(pool, user_id, purpose, ttl, None).await
}

async fn issue_token_for_email(
    pool: &PgPool,
    user_id: Uuid,
    purpose: &str,
    ttl: Duration,
    expected_email: Option<&str>,
) -> Result<String> {
    let token = new_token();
    let mut tx = pool.begin().await?;
    // Issuance and consumption lock the account before its tokens, including when
    // no earlier token exists. This serializes replacements without lock inversion.
    let (email, verified, claim_expiry): (Option<String>, bool, Option<DateTime<Utc>>) =
        sqlx::query_as(
            "SELECT email, email_verified_at IS NOT NULL, email_claim_expires_at
         FROM users WHERE id = $1 AND is_active = true FOR UPDATE",
        )
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
    if let Some(expected) = expected_email {
        anyhow::ensure!(
            email.as_deref() == Some(expected),
            "email changed before token issuance"
        );
    }
    let mut expires_at = Utc::now() + ttl;
    if purpose == PURPOSE_EMAIL_VERIFICATION {
        anyhow::ensure!(
            !verified && email.is_some(),
            "email is missing or already verified"
        );
        let claim_expiry = claim_expiry
            .filter(|expiry| *expiry > Utc::now())
            .ok_or_else(|| anyhow!("email reservation expired; bind an email address again"))?;
        expires_at = expires_at.min(claim_expiry);
    }
    sqlx::query(
        "UPDATE account_tokens SET used_at = NOW()
         WHERE user_id = $1 AND purpose = $2 AND used_at IS NULL",
    )
    .bind(user_id)
    .bind(purpose)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO account_tokens (user_id, purpose, token_hash, expires_at, email_address)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user_id)
    .bind(purpose)
    .bind(token_hash(&token))
    .bind(expires_at)
    .bind(email)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(token)
}

pub async fn cooldown_active(pool: &PgPool, user_id: Uuid, purpose: &str) -> Result<bool> {
    let last: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT MAX(created_at) FROM account_tokens WHERE user_id = $1 AND purpose = $2",
    )
    .bind(user_id)
    .bind(purpose)
    .fetch_one(pool)
    .await?;
    Ok(last.is_some_and(|issued| Utc::now() - issued < resend_cooldown()))
}

/// Marks a token used and returns its owner. An unknown, used, or expired token returns None.
pub async fn consume_token(
    tx: &mut Transaction<'_, Postgres>,
    token: &str,
    purpose: &str,
) -> Result<Option<Uuid>> {
    let hash = token_hash(token);
    let owner: Option<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM account_tokens
         WHERE token_hash = $1 AND purpose = $2 AND used_at IS NULL AND expires_at > clock_timestamp()",
    )
    .bind(&hash)
    .bind(purpose)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(owner) = owner else {
        return Ok(None);
    };
    let active: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE id = $1 AND is_active = true FOR UPDATE")
            .bind(owner)
            .fetch_optional(&mut **tx)
            .await?;
    if active.is_none() {
        return Ok(None);
    }
    // Recheck after acquiring the account lock: a replacement or another use may
    // have committed while we waited. Consuming one link also retires legacy siblings.
    let user_id: Option<Uuid> = sqlx::query_scalar(
        "UPDATE account_tokens t SET used_at = NOW()
         WHERE token_hash = $1 AND purpose = $2 AND used_at IS NULL AND expires_at > clock_timestamp()
           AND (purpose <> 'email_verification' OR EXISTS (
               SELECT 1 FROM users u WHERE u.id = t.user_id AND u.email = t.email_address
                 AND u.email_verified_at IS NULL AND u.email_claim_expires_at > clock_timestamp()
           )) RETURNING user_id",
    )
    .bind(&hash)
    .bind(purpose)
    .fetch_optional(&mut **tx)
    .await?;
    if user_id.is_some() {
        sqlx::query(
            "UPDATE account_tokens SET used_at = NOW()
             WHERE user_id = $1 AND purpose = $2 AND used_at IS NULL",
        )
        .bind(owner)
        .bind(purpose)
        .execute(&mut **tx)
        .await?;
    }
    Ok(user_id)
}

pub async fn email_used_by_verified_account(
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
    except: Uuid,
) -> Result<bool> {
    let taken: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM users
            WHERE LOWER(email) = $1 AND email_verified_at IS NOT NULL AND id <> $2
         )",
    )
    .bind(email)
    .bind(except)
    .fetch_one(&mut **tx)
    .await?;
    Ok(taken)
}

/// Pays the welcome grant from the system pool, once, if the pool still holds enough CPT.
/// Runs inside the caller's transaction, so the debit, the credit, and the ledger rows commit
/// together or not at all.
pub async fn grant_welcome_if_eligible(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<&'static str> {
    let (username, status): (String, String) =
        sqlx::query_as("SELECT username, welcome_grant_status FROM users WHERE id = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_one(&mut **tx)
            .await?;
    if status == GRANT_GRANTED {
        return Ok(GRANT_ALREADY_GRANTED);
    }

    let pool_balance: i64 =
        sqlx::query_scalar("SELECT balance FROM users WHERE username = $1 FOR UPDATE")
            .bind(SYSTEM_ACCOUNT_USERNAME)
            .fetch_one(&mut **tx)
            .await
            .map_err(|error| anyhow!("system account is unavailable: {error}"))?;
    if pool_balance < WELCOME_GRANT_CPT {
        sqlx::query("UPDATE users SET welcome_grant_status = $2, updated_at = NOW() WHERE id = $1")
            .bind(user_id)
            .bind(GRANT_POOL_EMPTY)
            .execute(&mut **tx)
            .await?;
        return Ok(GRANT_POOL_EMPTY);
    }

    sqlx::query("UPDATE users SET balance = balance - $1, updated_at = NOW() WHERE username = $2")
        .bind(WELCOME_GRANT_CPT)
        .bind(SYSTEM_ACCOUNT_USERNAME)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "UPDATE users
         SET balance = balance + $1, welcome_grant_status = $3, updated_at = NOW()
         WHERE id = $2",
    )
    .bind(WELCOME_GRANT_CPT)
    .bind(user_id)
    .bind(GRANT_GRANTED)
    .execute(&mut **tx)
    .await?;

    let task_id = format!("welcome-grant:{user_id}");
    let debit_key = format!("{task_id}:debit");
    let credit_key = format!("{task_id}:credit");
    sqlx::query(
        "INSERT INTO ledger_entries (
            task_id, payer_user, provider_worker_id, provider_user,
            kind, amount_cpt, currency, status, idempotency_key
         ) VALUES
         ($1, $2, NULL, $3, 'system_welcome_debit', $4, 'CPT', 'settled', $5),
         ($1, $2, NULL, $3, 'system_welcome_credit', $4, 'CPT', 'settled', $6)
         ON CONFLICT (idempotency_key) DO NOTHING",
    )
    .bind(&task_id)
    .bind(SYSTEM_ACCOUNT_USERNAME)
    .bind(&username)
    .bind(WELCOME_GRANT_CPT)
    .bind(&debit_key)
    .bind(&credit_key)
    .execute(&mut **tx)
    .await?;
    Ok(GRANT_GRANTED)
}

pub struct Mail<'a> {
    pub to: &'a str,
    pub subject: &'a str,
    pub body: &'a str,
}

/// Delivers one message through the configured transport.
pub async fn deliver(settings: &AccountSettings, mail: Mail<'_>) -> Result<()> {
    match &settings.mail {
        MailTransport::Disabled => Err(anyhow!("mail transport is not configured")),
        MailTransport::File(dir) => {
            let dir = dir.clone();
            let name = format!(
                "{}-{}.eml",
                Utc::now().format("%Y%m%dT%H%M%S%6fZ"),
                Uuid::new_v4()
            );
            let message = format!(
                "To: {}\nSubject: {}\nContent-Type: text/plain; charset=utf-8\n\n{}\n",
                mail.to, mail.subject, mail.body
            );
            tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join(name), message)
            })
            .await??;
            Ok(())
        }
        MailTransport::Resend(config) => send_with_resend(config, &mail).await,
    }
}

/// Sends one message through the Resend API. A failure names the HTTP status and Resend's
/// error name, never the key or the recipient address.
async fn send_with_resend(config: &ResendConfig, mail: &Mail<'_>) -> Result<()> {
    let response = config
        .client
        .post(&config.endpoint)
        .bearer_auth(&config.api_key)
        .json(&serde_json::json!({
            "from": config.from,
            "to": [mail.to],
            "subject": mail.subject,
            "text": mail.body,
        }))
        .send()
        .await
        .map_err(|error| anyhow!("Resend request failed: {error}"))?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let reason = response
        .json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|body| {
            body.get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "no error name".to_string());
    Err(anyhow!(
        "Resend rejected the message: HTTP {status} ({reason})"
    ))
}

pub async fn send_verification_mail(
    settings: &AccountSettings,
    pool: &PgPool,
    user_id: Uuid,
    email: &str,
) -> Result<()> {
    let token = issue_token_for_email(
        pool,
        user_id,
        PURPOSE_EMAIL_VERIFICATION,
        email_verification_ttl(),
        Some(email),
    )
    .await?;
    let link = format!("{}/#/verify-email/{token}", settings.site_url);
    let body = format!(
        "Open this link to verify your email address. Verifying it adds the welcome credit to your account.\n\n{link}\n\nThe link expires no later than 24 hours after this address was added to the account. Resending does not extend that deadline. If you did not add this address, ignore this message; the unverified reservation will expire."
    );
    deliver(
        settings,
        Mail {
            to: email,
            subject: "Verify your Hivemind email",
            body: &body,
        },
    )
    .await
}

pub async fn send_reset_mail(
    settings: &AccountSettings,
    pool: &PgPool,
    user_id: Uuid,
    email: &str,
) -> Result<()> {
    let token = issue_token(pool, user_id, PURPOSE_PASSWORD_RESET, password_reset_ttl()).await?;
    let link = format!("{}/#/reset-password/{token}", settings.site_url);
    let body = format!(
        "Open this link to choose a new password.\n\n{link}\n\nThe link works once and expires in 1 hour. If you did not ask for a reset, ignore this message."
    );
    deliver(
        settings,
        Mail {
            to: email,
            subject: "Reset your Hivemind password",
            body: &body,
        },
    )
    .await
}

pub async fn send_username_mail(
    settings: &AccountSettings,
    email: &str,
    username: &str,
) -> Result<()> {
    let body = format!(
        "The Hivemind username for this email address is: {username}\n\nIf you did not ask for this, ignore this message."
    );
    deliver(
        settings,
        Mail {
            to: email,
            subject: "Your Hivemind username",
            body: &body,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn email_shape_is_checked_and_lowercased() {
        assert_eq!(
            normalize_email(" User@Example.COM ").unwrap(),
            "user@example.com"
        );
        assert_eq!(
            normalize_email("a.b-c@sub.example.org").unwrap(),
            "a.b-c@sub.example.org"
        );
        for bad in [
            "",
            "plain",
            "@example.com",
            "user@",
            "user@localhost",
            "user@example..com",
            "user@.example.com",
            "user@example.com.",
            "us er@example.com",
            "a@b@example.com",
        ] {
            assert!(normalize_email(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn password_length_is_counted_in_bytes() {
        assert!(validate_password("1234567").is_err());
        assert!(validate_password("12345678").is_ok());
        assert!(validate_password(&"a".repeat(72)).is_ok());
        assert!(validate_password(&"a".repeat(73)).is_err());
        // Two bytes per character: 37 characters is 74 bytes, so it is refused.
        assert!(validate_password(&"é".repeat(37)).is_err());
    }

    #[tokio::test]
    async fn concurrent_email_claims_have_one_owner_and_expired_claims_are_reusable() {
        let fixture = hivemind_database::postgres::create_isolated_test_pool("email_claims")
            .await
            .expect("isolated test database required");
        let pool = &fixture.pool;
        hivemind_database::postgres::run_migrations(pool)
            .await
            .unwrap();
        let (first, second) = tokio::join!(
            create_unverified_user(pool, "first", "hash", " Shared@example.test "),
            create_unverified_user(pool, "second", "hash", "shared@example.test"),
        );
        assert_ne!(first.is_ok(), second.is_ok());
        let (original, failure) = if let Ok(id) = first {
            (id, second.unwrap_err())
        } else {
            (second.unwrap(), first.unwrap_err())
        };
        assert_eq!(
            failure.downcast_ref::<EmailClaimError>(),
            Some(&EmailClaimError::InUse)
        );
        let old_token = issue_token(
            pool,
            original,
            PURPOSE_EMAIL_VERIFICATION,
            email_verification_ttl(),
        )
        .await
        .unwrap();
        sqlx::query("UPDATE users SET balance = 42, email_claim_expires_at = NOW() - INTERVAL '1 second' WHERE id = $1")
            .bind(original).execute(pool).await.unwrap();
        assert!(find_registered_by_email(pool, "shared@example.test")
            .await
            .unwrap()
            .is_none());
        let mut tx = pool.begin().await.unwrap();
        assert!(
            consume_token(&mut tx, &old_token, PURPOSE_EMAIL_VERIFICATION)
                .await
                .unwrap()
                .is_none()
        );
        tx.rollback().await.unwrap();

        let replacement =
            create_unverified_user(pool, "replacement", "hash", "shared@example.test")
                .await
                .unwrap();
        assert_ne!(original, replacement);
        let old: (Option<String>, i64) =
            sqlx::query_as("SELECT email, balance FROM users WHERE id = $1")
                .bind(original)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(old, (None, 42));
        let retired: bool = sqlx::query_scalar(
            "SELECT used_at IS NOT NULL FROM account_tokens WHERE token_hash = $1",
        )
        .bind(token_hash(&old_token))
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(retired);

        // A disabled, verified owner remains authoritative even after the old deadline.
        sqlx::query("UPDATE users SET email_verified_at = NOW(), is_active = false, email_claim_expires_at = NOW() - INTERVAL '1 day' WHERE id = $1")
            .bind(replacement).execute(pool).await.unwrap();
        assert!(
            create_unverified_user(pool, "third", "hash", "shared@example.test")
                .await
                .is_err()
        );
        assert!(
            find_registered_by_email(pool, "shared@example.test")
                .await
                .unwrap()
                .unwrap()
                .email_verified
        );
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn email_rebinding_invalidates_old_proofs_and_resending_cannot_extend_a_claim() {
        let fixture = hivemind_database::postgres::create_isolated_test_pool("email_rebinding")
            .await
            .expect("isolated test database required");
        let pool = &fixture.pool;
        hivemind_database::postgres::run_migrations(pool)
            .await
            .unwrap();
        let id = create_unverified_user(pool, "owner", "hash", "old@example.test")
            .await
            .unwrap();
        let first = issue_token(
            pool,
            id,
            PURPOSE_EMAIL_VERIFICATION,
            email_verification_ttl(),
        )
        .await
        .unwrap();
        let deadline: DateTime<Utc> =
            sqlx::query_scalar("SELECT email_claim_expires_at FROM users WHERE id = $1")
                .bind(id)
                .fetch_one(pool)
                .await
                .unwrap();
        let second = issue_token(
            pool,
            id,
            PURPOSE_EMAIL_VERIFICATION,
            email_verification_ttl(),
        )
        .await
        .unwrap();
        let (claim, link): (DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
            "SELECT u.email_claim_expires_at, t.expires_at FROM users u JOIN account_tokens t ON t.user_id = u.id WHERE t.token_hash = $1",
        ).bind(token_hash(&second)).fetch_one(pool).await.unwrap();
        assert_eq!(claim, deadline);
        assert!(link <= deadline);
        assert!(bind_email(pool, id, "hash", "new@example.test")
            .await
            .is_err());
        sqlx::query(
            "UPDATE users SET email_claim_expires_at = NOW() - INTERVAL '1 second' WHERE id = $1",
        )
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
        assert!(bind_email(pool, id, "wrong-hash", "new@example.test")
            .await
            .is_err());
        bind_email(pool, id, "hash", "new@example.test")
            .await
            .unwrap();
        assert!(issue_token_for_email(
            pool,
            id,
            PURPOSE_EMAIL_VERIFICATION,
            email_verification_ttl(),
            Some("old@example.test")
        )
        .await
        .is_err());
        for token in [first, second] {
            let mut tx = pool.begin().await.unwrap();
            assert!(consume_token(&mut tx, &token, PURPOSE_EMAIL_VERIFICATION)
                .await
                .unwrap()
                .is_none());
            tx.rollback().await.unwrap();
        }
        let new = issue_token_for_email(
            pool,
            id,
            PURPOSE_EMAIL_VERIFICATION,
            email_verification_ttl(),
            Some("new@example.test"),
        )
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert_eq!(
            consume_token(&mut tx, &new, PURPOSE_EMAIL_VERIFICATION)
                .await
                .unwrap(),
            Some(id)
        );
        tx.commit().await.unwrap();
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn failed_registration_rolls_back_expired_email_detachment() {
        let fixture =
            hivemind_database::postgres::create_isolated_test_pool("email_claim_rollback")
                .await
                .expect("isolated test database required");
        let pool = &fixture.pool;
        hivemind_database::postgres::run_migrations(pool)
            .await
            .unwrap();
        let id = create_unverified_user(pool, "owner", "hash", "old@example.test")
            .await
            .unwrap();
        let token = issue_token(
            pool,
            id,
            PURPOSE_EMAIL_VERIFICATION,
            email_verification_ttl(),
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE users SET email_claim_expires_at = NOW() - INTERVAL '1 second' WHERE id = $1",
        )
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
        assert!(
            create_unverified_user(pool, "owner", "hash", "old@example.test")
                .await
                .is_err()
        );
        let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(email, "old@example.test");
        let used: bool = sqlx::query_scalar(
            "SELECT used_at IS NOT NULL FROM account_tokens WHERE token_hash = $1",
        )
        .bind(token_hash(&token))
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(!used);
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn parallel_token_issuance_leaves_only_the_latest_token_usable() {
        let fixture = hivemind_database::postgres::create_isolated_test_pool("account_token_race")
            .await
            .expect("isolated test database required");
        hivemind_database::postgres::run_migrations(&fixture.pool)
            .await
            .unwrap();
        let owner = create_unverified_user(
            &fixture.pool,
            "token-owner",
            "test-hash",
            "token@example.test",
        )
        .await
        .unwrap();
        let mut blocker = fixture.pool.begin().await.unwrap();
        let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *blocker)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM users WHERE id = $1 FOR UPDATE")
            .bind(owner)
            .execute(&mut *blocker)
            .await
            .unwrap();
        let spawn_issue = || {
            let pool = fixture.pool.clone();
            tokio::spawn(async move {
                issue_token(&pool, owner, PURPOSE_PASSWORD_RESET, password_reset_ttl()).await
            })
        };
        let first = spawn_issue();
        let second = spawn_issue();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let waiting: i64 = sqlx::query_scalar(
                    "WITH RECURSIVE blocked(pid) AS (
                        SELECT pid FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid))
                        UNION
                        SELECT a.pid FROM pg_stat_activity a JOIN blocked b ON b.pid = ANY(pg_blocking_pids(a.pid))
                     ) SELECT COUNT(*) FROM blocked",
                ).bind(blocker_pid).fetch_one(&fixture.pool).await.unwrap();
                if waiting >= 2 { break; }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.expect("both token issuers must reach the account lock");
        blocker.commit().await.unwrap();
        let first = first.await.unwrap().unwrap();
        let second = second.await.unwrap().unwrap();
        assert_ne!(first, second);
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM account_tokens WHERE user_id = $1 AND used_at IS NULL",
        )
        .bind(owner)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(active, 1);
        fixture.cleanup().await.unwrap();
    }

    #[tokio::test]
    async fn consuming_a_token_retires_other_outstanding_links_of_the_same_purpose() {
        let fixture =
            hivemind_database::postgres::create_isolated_test_pool("account_token_siblings")
                .await
                .expect("isolated test database required");
        hivemind_database::postgres::run_migrations(&fixture.pool)
            .await
            .unwrap();
        let owner = create_unverified_user(
            &fixture.pool,
            "token-owner",
            "test-hash",
            "token@example.test",
        )
        .await
        .unwrap();
        let first = new_token();
        let second = new_token();
        for token in [&first, &second] {
            sqlx::query(
                "INSERT INTO account_tokens (user_id, purpose, token_hash, expires_at)
                 VALUES ($1, $2, $3, NOW() + INTERVAL '1 hour')",
            )
            .bind(owner)
            .bind(PURPOSE_PASSWORD_RESET)
            .bind(token_hash(token))
            .execute(&fixture.pool)
            .await
            .unwrap();
        }
        let mut tx = fixture.pool.begin().await.unwrap();
        assert_eq!(
            consume_token(&mut tx, &first, PURPOSE_PASSWORD_RESET)
                .await
                .unwrap(),
            Some(owner)
        );
        tx.commit().await.unwrap();
        let mut tx = fixture.pool.begin().await.unwrap();
        assert_eq!(
            consume_token(&mut tx, &second, PURPOSE_PASSWORD_RESET)
                .await
                .unwrap(),
            None
        );
        tx.rollback().await.unwrap();
        fixture.cleanup().await.unwrap();
    }

    #[test]
    fn tokens_are_random_hex_and_hash_deterministically() {
        let first = new_token();
        let second = new_token();
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, second);
        assert_eq!(token_hash(&first), token_hash(&first));
        assert_eq!(token_hash(&first).len(), 64);
        assert_ne!(token_hash(&first), first);
    }

    #[tokio::test]
    async fn file_transport_writes_the_message_and_disabled_refuses() {
        let dir = std::env::temp_dir().join(format!("hivemind-mail-test-{}", Uuid::new_v4()));
        let settings = AccountSettings {
            site_url: "http://localhost:3000".into(),
            mail: MailTransport::File(dir.clone()),
        };
        deliver(
            &settings,
            Mail {
                to: "user@example.com",
                subject: "Subject line",
                body: "Body text",
            },
        )
        .await
        .unwrap();
        let mut files = std::fs::read_dir(&dir).unwrap();
        let entry = files.next().unwrap().unwrap();
        let content = std::fs::read_to_string(entry.path()).unwrap();
        assert!(content.contains("To: user@example.com"));
        assert!(content.contains("Subject: Subject line"));
        assert!(content.ends_with("Body text\n"));
        std::fs::remove_dir_all(&dir).ok();

        let disabled = AccountSettings {
            site_url: "http://localhost:3000".into(),
            mail: MailTransport::Disabled,
        };
        assert!(deliver(
            &disabled,
            Mail {
                to: "user@example.com",
                subject: "x",
                body: "y",
            },
        )
        .await
        .is_err());
    }

    /// Accepts one connection, answers it with `status` and `body`, and returns the request.
    async fn one_shot_server(
        status: &'static str,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut stream).await;
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            request
        });
        (format!("http://{address}/emails"), server)
    }

    /// Reads a request head and the `Content-Length` bytes that follow it.
    async fn read_http_request(stream: &mut tokio::net::TcpStream) -> String {
        let mut buffer = Vec::new();
        loop {
            let mut chunk = [0_u8; 1024];
            let read = stream.read(&mut chunk).await.unwrap();
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(head_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buffer[..head_end]).to_ascii_lowercase();
                let length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if buffer.len() >= head_end + 4 + length {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&buffer).into_owned()
    }

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let values: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        move |name: &str| values.get(name).cloned()
    }

    #[tokio::test]
    async fn resend_sends_the_message_with_the_key_in_the_header() {
        let (endpoint, server) = one_shot_server("200 OK", r#"{"id":"msg_1"}"#).await;
        let config = ResendConfig::new(
            "re_test_key".into(),
            "Hivemind <noreply@hivemind.justin0711.com>".into(),
            endpoint,
        )
        .unwrap();
        let settings = AccountSettings {
            site_url: "http://localhost:3000".into(),
            mail: MailTransport::Resend(config),
        };
        deliver(
            &settings,
            Mail {
                to: "user@example.com",
                subject: "Subject line",
                body: "Body text",
            },
        )
        .await
        .unwrap();

        let request = server.await.unwrap();
        assert!(request.starts_with("POST /emails HTTP/1.1"), "{request}");
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer re_test_key"));
        let body: serde_json::Value =
            serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["from"], "Hivemind <noreply@hivemind.justin0711.com>");
        assert_eq!(body["to"][0], "user@example.com");
        assert_eq!(body["subject"], "Subject line");
        assert_eq!(body["text"], "Body text");
    }

    #[tokio::test]
    async fn resend_rejection_names_the_status_but_not_the_key() {
        let (endpoint, server) = one_shot_server(
            "403 Forbidden",
            r#"{"statusCode":403,"name":"validation_error","message":"The domain is not verified."}"#,
        )
        .await;
        let config = ResendConfig::new(
            "re_test_key".into(),
            "Hivemind <noreply@hivemind.justin0711.com>".into(),
            endpoint,
        )
        .unwrap();
        let error = send_with_resend(
            &config,
            &Mail {
                to: "user@example.com",
                subject: "Subject line",
                body: "Body text",
            },
        )
        .await
        .unwrap_err()
        .to_string();
        server.await.unwrap();

        assert!(error.contains("403"), "{error}");
        assert!(error.contains("validation_error"), "{error}");
        assert!(!error.contains("re_test_key"), "{error}");
        assert!(!format!("{config:?}").contains("re_test_key"));
    }

    #[test]
    fn resend_settings_need_a_key_a_sender_and_a_site_url() {
        let complete = [
            ("MAIL_TRANSPORT", "resend"),
            ("RESEND_API_KEY", "re_test_key"),
            ("MAIL_FROM", "Hivemind <noreply@hivemind.justin0711.com>"),
            ("PUBLIC_SITE_URL", "https://hivemind.justin0711.com/"),
        ];
        let settings = AccountSettings::from_lookup(lookup(&complete)).unwrap();
        assert_eq!(settings.site_url, "https://hivemind.justin0711.com");
        assert!(matches!(settings.mail, MailTransport::Resend(_)));

        for missing in ["RESEND_API_KEY", "MAIL_FROM", "PUBLIC_SITE_URL"] {
            let partial: Vec<(&str, &str)> = complete
                .iter()
                .copied()
                .filter(|(name, _)| *name != missing)
                .collect();
            assert!(
                AccountSettings::from_lookup(lookup(&partial)).is_err(),
                "{missing} should be required for resend"
            );
        }
    }

    #[test]
    fn file_and_unset_transports_need_no_mail_secrets() {
        let file = AccountSettings::from_lookup(lookup(&[("MAIL_TRANSPORT", "file")])).unwrap();
        assert!(matches!(file.mail, MailTransport::File(_)));
        let unset = AccountSettings::from_lookup(lookup(&[])).unwrap();
        assert!(matches!(unset.mail, MailTransport::Disabled));
        assert_eq!(unset.site_url, "http://localhost:3000");
    }

    #[test]
    fn an_unknown_mail_transport_is_refused_not_ignored() {
        for transport in ["Resend", "smtp", "none"] {
            assert!(
                AccountSettings::from_lookup(lookup(&[("MAIL_TRANSPORT", transport)])).is_err(),
                "{transport} should be refused"
            );
        }
        let disabled =
            AccountSettings::from_lookup(lookup(&[("MAIL_TRANSPORT", "disabled")])).unwrap();
        assert!(matches!(disabled.mail, MailTransport::Disabled));
    }

    /// Needs network access. The key is deliberately invalid, so Resend must refuse the request
    /// and no mail is sent. The point is to prove the TLS and response path against the real API.
    #[tokio::test]
    #[ignore = "needs network access to api.resend.com"]
    async fn resend_api_answers_over_tls_for_an_invalid_key() {
        let config = ResendConfig::new(
            "re_invalid_test_key".into(),
            "Hivemind <noreply@hivemind.justin0711.com>".into(),
            RESEND_API_URL.into(),
        )
        .unwrap();
        let error = send_with_resend(
            &config,
            &Mail {
                to: "user@example.com",
                subject: "Probe",
                body: "Probe",
            },
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("Resend rejected the message: HTTP 4"),
            "{error}"
        );
    }
}
