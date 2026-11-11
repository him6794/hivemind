//! A recurring allowance of CPT from the system pool, so an account can try the platform without buying anything.
//!
//! The allowance is off unless `ACCOUNT_DAILY_ALLOWANCE_CPT` is set above zero. An invalid value turns it off as
//! well: a typo must not hand out CPT. When it is on, an account may claim the amount once in every 24 hours. A
//! claim is refused when paying it would take the system pool below `ACCOUNT_ALLOWANCE_POOL_FLOOR_CPT`, so the
//! finite pool keeps enough to pay settlements and welcome grants.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use hivemind_models::SYSTEM_ACCOUNT_USERNAME;
use sqlx::PgPool;
use uuid::Uuid;

/// Hours between two claims by the same account.
pub const ALLOWANCE_INTERVAL_HOURS: i64 = 24;
/// The pool floor when none is configured. A tenth of the system account's starting balance.
pub const DEFAULT_POOL_FLOOR_CPT: i64 = 100_000_000;

/// The allowance's amount and the pool floor. `amount_cpt` of zero means the allowance is off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowanceSettings {
    pub amount_cpt: i64,
    pub pool_floor_cpt: i64,
}

impl AllowanceSettings {
    pub fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let read = |name: &str| {
            lookup(name)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        // Anything that is not a non-negative whole number turns the allowance off, rather than guessing.
        let amount_cpt = match read("ACCOUNT_DAILY_ALLOWANCE_CPT") {
            None => 0,
            Some(value) => {
                match value.parse::<i64>() {
                    Ok(amount) if amount >= 0 => amount,
                    _ => {
                        tracing::error!("ACCOUNT_DAILY_ALLOWANCE_CPT is not a whole number; the allowance is off");
                        0
                    }
                }
            }
        };
        let pool_floor_cpt = match read("ACCOUNT_ALLOWANCE_POOL_FLOOR_CPT") {
            None => DEFAULT_POOL_FLOOR_CPT,
            Some(value) => {
                match value.parse::<i64>() {
                    Ok(floor) if floor >= 0 => floor,
                    _ => {
                        tracing::error!("ACCOUNT_ALLOWANCE_POOL_FLOOR_CPT is invalid; the default floor is used");
                        DEFAULT_POOL_FLOOR_CPT
                    }
                }
            }
        };
        Self {
            amount_cpt,
            pool_floor_cpt,
        }
    }
}

/// What a claim did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllowanceOutcome {
    Disabled,
    UnknownAccount,
    TooSoon {
        next_claim_at: DateTime<Utc>,
    },
    PoolTooLow,
    Granted {
        amount_cpt: i64,
        balance_cpt: i64,
        next_claim_at: DateTime<Utc>,
    },
}

/// Claims the allowance for an account. The account row, the pool row, the balances, the claim time, and the
/// ledger rows commit together, so a claim either happens completely or not at all.
pub async fn claim(
    pool: &PgPool,
    settings: &AllowanceSettings,
    username: &str,
) -> Result<AllowanceOutcome> {
    if settings.amount_cpt == 0 {
        return Ok(AllowanceOutcome::Disabled);
    }
    let amount = settings.amount_cpt;
    let mut tx = pool.begin().await?;
    // The account row is locked first and the pool row second, in the same order as the welcome grant.
    let account: Option<(Uuid, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT id, last_allowance_at FROM users WHERE username = $1 AND is_active = true FOR UPDATE",
    )
    .bind(username)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((user_id, last_claim)) = account else {
        tx.rollback().await?;
        return Ok(AllowanceOutcome::UnknownAccount);
    };
    let now = Utc::now();
    if let Some(last_claim) = last_claim {
        let next = last_claim + Duration::hours(ALLOWANCE_INTERVAL_HOURS);
        if now < next {
            tx.rollback().await?;
            return Ok(AllowanceOutcome::TooSoon {
                next_claim_at: next,
            });
        }
    }
    let pool_balance: i64 =
        sqlx::query_scalar("SELECT balance FROM users WHERE username = $1 FOR UPDATE")
            .bind(SYSTEM_ACCOUNT_USERNAME)
            .fetch_one(&mut *tx)
            .await?;
    if pool_balance - amount < settings.pool_floor_cpt {
        tx.rollback().await?;
        return Ok(AllowanceOutcome::PoolTooLow);
    }

    sqlx::query("UPDATE users SET balance = balance - $1, updated_at = NOW() WHERE username = $2")
        .bind(amount)
        .bind(SYSTEM_ACCOUNT_USERNAME)
        .execute(&mut *tx)
        .await?;
    let balance_cpt: i64 = sqlx::query_scalar(
        "UPDATE users SET balance = balance + $1, last_allowance_at = $2, updated_at = NOW()
         WHERE id = $3 RETURNING balance",
    )
    .bind(amount)
    .bind(now)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    let claim_task = format!("allowance:{}", Uuid::new_v4());
    let debit_key = format!("{claim_task}:debit");
    let credit_key = format!("{claim_task}:credit");
    sqlx::query(
        "INSERT INTO ledger_entries (
            task_id, payer_user, provider_worker_id, provider_user,
            kind, amount_cpt, currency, status, idempotency_key
         ) VALUES
         ($1, $2, NULL, $3, 'system_allowance_debit', $4, 'CPT', 'settled', $5),
         ($1, $2, NULL, $3, 'system_allowance_credit', $4, 'CPT', 'settled', $6)
         ON CONFLICT (idempotency_key) DO NOTHING",
    )
    .bind(&claim_task)
    .bind(SYSTEM_ACCOUNT_USERNAME)
    .bind(username)
    .bind(amount)
    .bind(&debit_key)
    .bind(&credit_key)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(AllowanceOutcome::Granted {
        amount_cpt: amount,
        balance_cpt,
        next_claim_at: now + Duration::hours(ALLOWANCE_INTERVAL_HOURS),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        }
    }

    #[test]
    fn the_allowance_is_off_unless_an_amount_is_configured() {
        let unset = AllowanceSettings::from_lookup(lookup(&[]));
        assert_eq!(unset.amount_cpt, 0);
        assert_eq!(unset.pool_floor_cpt, DEFAULT_POOL_FLOOR_CPT);

        let set =
            AllowanceSettings::from_lookup(lookup(&[("ACCOUNT_DAILY_ALLOWANCE_CPT", " 50 ")]));
        assert_eq!(set.amount_cpt, 50);
    }

    #[test]
    fn an_invalid_amount_turns_the_allowance_off_instead_of_guessing() {
        for bad in ["-5", "fifty", "1.5"] {
            let settings =
                AllowanceSettings::from_lookup(lookup(&[("ACCOUNT_DAILY_ALLOWANCE_CPT", bad)]));
            assert_eq!(settings.amount_cpt, 0, "{bad}");
        }
    }

    #[test]
    fn the_pool_floor_can_be_set_and_an_invalid_one_falls_back_to_the_default() {
        let set =
            AllowanceSettings::from_lookup(lookup(&[("ACCOUNT_ALLOWANCE_POOL_FLOOR_CPT", "7000")]));
        assert_eq!(set.pool_floor_cpt, 7000);
        let bad =
            AllowanceSettings::from_lookup(lookup(&[("ACCOUNT_ALLOWANCE_POOL_FLOOR_CPT", "-1")]));
        assert_eq!(bad.pool_floor_cpt, DEFAULT_POOL_FLOOR_CPT);
    }

    async fn setup(
        test_name: &str,
    ) -> Option<(PgPool, hivemind_database::postgres::IsolatedTestPool)> {
        let config = hivemind_config::HivemindConfig::for_test();
        let fixture =
            hivemind_database::postgres::create_isolated_test_pool_with_config(&config, test_name)
                .await
                .ok()?;
        hivemind_database::postgres::run_migrations(&fixture.pool)
            .await
            .ok()?;
        Some((fixture.pool.clone(), fixture))
    }

    async fn balance(pool: &PgPool, username: &str) -> i64 {
        sqlx::query_scalar("SELECT balance FROM users WHERE username = $1")
            .bind(username)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn account(pool: &PgPool, username: &str) {
        sqlx::query(
            "INSERT INTO users (username, password_hash, balance, is_active)
             VALUES ($1, '!test-only', 0, true)",
        )
        .bind(username)
        .execute(pool)
        .await
        .unwrap();
    }

    fn on(amount: i64, floor: i64) -> AllowanceSettings {
        AllowanceSettings {
            amount_cpt: amount,
            pool_floor_cpt: floor,
        }
    }

    #[tokio::test]
    async fn a_claim_moves_the_amount_from_the_pool_once_in_each_day() {
        let Some((pool, fixture)) = setup("allowance_claims").await else {
            return;
        };
        account(&pool, "reader").await;
        let pool_before = balance(&pool, SYSTEM_ACCOUNT_USERNAME).await;

        let first = claim(&pool, &on(50, 0), "reader").await.unwrap();
        assert!(
            matches!(
                first,
                AllowanceOutcome::Granted {
                    amount_cpt: 50,
                    balance_cpt: 50,
                    ..
                }
            ),
            "{first:?}"
        );
        assert_eq!(balance(&pool, "reader").await, 50);
        assert_eq!(
            balance(&pool, SYSTEM_ACCOUNT_USERNAME).await,
            pool_before - 50
        );

        let again = claim(&pool, &on(50, 0), "reader").await.unwrap();
        assert!(
            matches!(again, AllowanceOutcome::TooSoon { .. }),
            "{again:?}"
        );
        assert_eq!(
            balance(&pool, "reader").await,
            50,
            "a refused claim moves nothing"
        );

        // A day later the account may claim again.
        sqlx::query("UPDATE users SET last_allowance_at = NOW() - INTERVAL '25 hours' WHERE username = 'reader'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(matches!(
            claim(&pool, &on(50, 0), "reader").await.unwrap(),
            AllowanceOutcome::Granted {
                balance_cpt: 100,
                ..
            }
        ));
        assert_eq!(balance(&pool, "reader").await, 100);

        let kinds: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM ledger_entries WHERE kind IN ('system_allowance_debit', 'system_allowance_credit')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            kinds, 4,
            "each granted claim writes one debit and one credit"
        );
        fixture.cleanup().await.ok();
    }

    #[tokio::test]
    async fn a_claim_is_refused_when_off_unknown_or_when_it_would_break_the_pool_floor() {
        let Some((pool, fixture)) = setup("allowance_refusals").await else {
            return;
        };
        account(&pool, "reader").await;
        assert_eq!(
            claim(&pool, &on(0, 0), "reader").await.unwrap(),
            AllowanceOutcome::Disabled
        );
        assert_eq!(
            claim(&pool, &on(50, 0), "nobody").await.unwrap(),
            AllowanceOutcome::UnknownAccount
        );
        let floor = balance(&pool, SYSTEM_ACCOUNT_USERNAME).await;
        assert_eq!(
            claim(&pool, &on(50, floor), "reader").await.unwrap(),
            AllowanceOutcome::PoolTooLow
        );
        assert_eq!(balance(&pool, "reader").await, 0);
        fixture.cleanup().await.ok();
    }
}
