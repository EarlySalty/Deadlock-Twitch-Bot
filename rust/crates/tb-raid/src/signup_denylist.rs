use sqlx::{postgres::PgRow, PgExecutor, Row};
use tb_domain::SignupBlock;

fn row_to_block(row: PgRow) -> SignupBlock {
    SignupBlock {
        twitch_user_id: row.get::<String, _>("twitch_user_id"),
        twitch_login: row.get::<String, _>("twitch_login"),
        reason: row.get::<String, _>("reason"),
        public_message: row.get::<Option<String>, _>("public_message"),
    }
}

pub async fn lookup<'e, E>(
    executor: E,
    twitch_user_id: Option<&str>,
    twitch_login: &str,
) -> Result<Option<SignupBlock>, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    let user_id = twitch_user_id.map(str::trim).filter(|s| !s.is_empty());
    let login = twitch_login.trim().to_lowercase();
    if user_id.is_none() && login.is_empty() {
        return Ok(None);
    }

    let row = sqlx::query(
        r#"
        SELECT twitch_user_id, twitch_login, reason, public_message
        FROM twitch_partner_signup_denylist
        WHERE ($1::text IS NOT NULL AND twitch_user_id = $1)
           OR ($2::text <> '' AND lower(twitch_login) = $2)
        ORDER BY (twitch_user_id = $1) DESC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(&login)
    .fetch_optional(executor)
    .await?;

    Ok(row.map(row_to_block))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignupBlockLookupFailed;

impl std::fmt::Display for SignupBlockLookupFailed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("signup_block_lookup_failed")
    }
}

impl std::error::Error for SignupBlockLookupFailed {}

pub async fn lookup_or_fail_closed<'e, E>(
    executor: E,
    twitch_user_id: Option<&str>,
    twitch_login: &str,
    pfad: &str,
) -> Result<Option<SignupBlock>, SignupBlockLookupFailed>
where
    E: PgExecutor<'e>,
{
    match lookup(executor, twitch_user_id, twitch_login).await {
        Ok(Some(block)) => {
            tracing::warn!(
                twitch_user_id = %block.twitch_user_id,
                twitch_login = %block.twitch_login,
                reason = %block.reason,
                %pfad,
                "Signup-Block greift: Aufnahme ins Partnerprogramm abgelehnt"
            );
            Ok(Some(block))
        }
        Ok(None) => {
            tracing::debug!(
                twitch_user_id = %twitch_user_id.unwrap_or(""),
                twitch_login = %twitch_login,
                %pfad,
                "Signup-Block geprueft: kein Eintrag"
            );
            Ok(None)
        }
        Err(error) => {
            tracing::error!(
                %error,
                twitch_user_id = %twitch_user_id.unwrap_or(""),
                twitch_login = %twitch_login,
                %pfad,
                "Signup-Block-Nachschlag fehlgeschlagen, Signup wird abgebrochen (fail-closed)"
            );
            Err(SignupBlockLookupFailed)
        }
    }
}
