//! Nachschlag in der Signup-Denylist (`twitch_partner_signup_denylist`).
//!
//! Eigenständiger Zustand, getrennt von [`crate::raid_blacklist`]: die
//! Raid-Blacklist steuert die Raid-Ziel-Auswahl, dieser Zustand steuert die
//! Aufnahme ins Partnerprogramm. Richtungsregel: ein Signup-Block zieht einen
//! Raid-Blacklist-Eintrag nach sich (Präfix
//! [`tb_domain::RAID_BLACKLIST_REASON_PREFIX`]), ein Raid-Blacklist-Eintrag
//! aber keinen Signup-Block.
//!
//! Die Queries hier nutzen bewusst `sqlx::query_as` mit `bind` statt der
//! compile-time-geprüften Makros: die Tabelle ist neu und liegt noch nicht im
//! Offline-Cache (`rust/scripts/sqlx-prepare.sh`), sonst bräuchte jeder Build
//! eine erreichbare DB. Präzedenz im selben Crate:
//! `raid_blacklist.rs::is_hard_banned`.

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

/// Schlägt einen Signup-Block nach. Match per `twitch_user_id` ODER
/// `lower(twitch_login)` — der Login ist der Fallback, wenn an der Aufrufstelle
/// nur er bekannt ist. Die ID gewinnt, damit eine Umbenennung den Block nicht
/// aushebelt.
///
/// `Ok(None)` heißt nachweislich "kein Block". Ein DB-Fehler kommt als `Err`
/// zurück und darf an der Aufrufstelle NICHT als "kein Block" behandelt werden.
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

/// Wie [`lookup`], aber loggt das Ergebnis inklusive Pfadangabe und gibt bei
/// einem DB-Fehler `Err(())` zurück. Aufrufer behandeln `Err` als "Signup
/// abbrechen" (fail-closed): ein nicht beantwortbarer Nachschlag darf keinen
/// gesperrten Streamer durchlassen.
pub async fn lookup_or_fail_closed<'e, E>(
    executor: E,
    twitch_user_id: Option<&str>,
    twitch_login: &str,
    pfad: &str,
) -> Result<Option<SignupBlock>, ()>
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
            Err(())
        }
    }
}
