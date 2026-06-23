//! Startup-Backfill fuer `twitch_streamers.twitch_user_id`.
//!
//! Port von Python `bot/base.py::_sync_missing_user_ids`: einmal beim Start
//! fehlende IDs erst aus `twitch_raid_auth` uebernehmen, danach verbleibende
//! Logins via Helix `/users` aufloesen.

use sqlx::PgPool;
use tb_transport_twitch::{HelixClient, HelixError};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct UserIdBackfillReport {
    pub from_raid_auth: u64,
    pub missing_after_raid_auth: usize,
    pub helix_requested: bool,
    pub helix_results: usize,
    pub from_helix: u64,
    pub still_missing: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTwitchUser {
    pub login: String,
    pub id: String,
}

/// Fuehrt den kompletten Boot-Backfill best-effort aus. Fehler werden geloggt
/// und brechen den Bot-Start nicht ab.
pub async fn sync_missing_user_ids(
    pool: &PgPool,
    helix: Option<&HelixClient>,
) -> UserIdBackfillReport {
    let mut report = UserIdBackfillReport::default();

    match sync_from_raid_auth(pool).await {
        Ok(rows) => {
            report.from_raid_auth = rows;
            if rows > 0 {
                tracing::info!(
                    updated = rows,
                    "_sync_missing_user_ids: user_ids aus raid_auth uebernommen"
                );
            }
        }
        Err(error) => {
            tracing::error!(%error, "_sync_missing_user_ids: Phase 1 (raid_auth) fehlgeschlagen");
        }
    }

    let missing = match load_missing_logins(pool).await {
        Ok(logins) => logins,
        Err(error) => {
            tracing::error!(%error, "_sync_missing_user_ids: fehlende Logins nicht ladbar");
            return report;
        }
    };
    report.missing_after_raid_auth = missing.len();
    if missing.is_empty() {
        tracing::debug!("_sync_missing_user_ids: alle user_ids vorhanden, nichts zu tun");
        return report;
    }

    let Some(helix) = helix else {
        tracing::warn!(
            missing = missing.len(),
            ?missing,
            "_sync_missing_user_ids: HelixClient fehlt, Rest kann nicht aufgeloest werden"
        );
        report.still_missing = missing;
        return report;
    };

    report.helix_requested = true;
    tracing::info!(
        missing = missing.len(),
        "_sync_missing_user_ids: Logins ohne user_id, frage Twitch API ab"
    );
    let users = match fetch_user_ids(helix, &missing).await {
        Ok(users) => users,
        Err(error) => {
            tracing::error!(%error, "_sync_missing_user_ids: API-Aufruf fehlgeschlagen");
            report.still_missing = missing;
            return report;
        }
    };
    report.helix_results = users.len();
    if users.is_empty() {
        tracing::warn!(
            ?missing,
            "_sync_missing_user_ids: API hat keine Ergebnisse zurueckgegeben"
        );
        report.still_missing = missing;
        return report;
    }

    match apply_resolved_user_ids(pool, &users).await {
        Ok(rows) => {
            report.from_helix = rows;
            tracing::info!(
                updated = rows,
                returned = users.len(),
                "_sync_missing_user_ids: user_ids per API aktualisiert"
            );
        }
        Err(error) => {
            tracing::error!(%error, "_sync_missing_user_ids: DB-Update nach API-Aufruf fehlgeschlagen");
            report.still_missing = missing;
            return report;
        }
    }

    match load_missing_logins(pool).await {
        Ok(still_missing) if still_missing.is_empty() => {
            tracing::info!("_sync_missing_user_ids: alle user_ids erfolgreich gesetzt");
        }
        Ok(still_missing) => {
            tracing::warn!(
                missing = still_missing.len(),
                ?still_missing,
                "_sync_missing_user_ids: Logins konnten nicht aufgeloest werden"
            );
            report.still_missing = still_missing;
        }
        Err(error) => {
            tracing::debug!(%error, "_sync_missing_user_ids: abschliessender Check fehlgeschlagen");
        }
    }

    report
}

async fn sync_from_raid_auth(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE twitch_streamers s
           SET twitch_user_id = ra.twitch_user_id
          FROM (
                SELECT DISTINCT ON (lower(trim(twitch_login)))
                       lower(trim(twitch_login)) AS twitch_login,
                       twitch_user_id
                  FROM twitch_raid_auth
                 WHERE NULLIF(trim(twitch_login), '') IS NOT NULL
                   AND NULLIF(trim(twitch_user_id), '') IS NOT NULL
                 ORDER BY lower(trim(twitch_login)),
                          authorized_at DESC NULLS LAST,
                          last_refreshed_at DESC NULLS LAST
               ) ra
         WHERE s.twitch_user_id IS NULL
           AND lower(trim(s.twitch_login)) = ra.twitch_login
        "#,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

async fn load_missing_logins(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT twitch_login
           FROM twitch_streamers
          WHERE twitch_user_id IS NULL
          ORDER BY lower(trim(twitch_login))",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|login| login.trim().to_string())
        .filter(|login| !login.is_empty())
        .collect())
}

async fn fetch_user_ids(
    helix: &HelixClient,
    missing: &[String],
) -> Result<Vec<ResolvedTwitchUser>, HelixError> {
    let login_refs: Vec<&str> = missing.iter().map(String::as_str).collect();
    let users = helix.get_users(&login_refs).await?;
    Ok(users
        .into_iter()
        .filter_map(|(login, user)| {
            let id = user.id.trim();
            (!id.is_empty()).then(|| ResolvedTwitchUser {
                login,
                id: id.to_string(),
            })
        })
        .collect())
}

pub async fn apply_resolved_user_ids(
    pool: &PgPool,
    users: &[ResolvedTwitchUser],
) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut updated = 0u64;
    for user in users {
        if user.login.trim().is_empty() || user.id.trim().is_empty() {
            continue;
        }
        let result = sqlx::query(
            "UPDATE twitch_streamers
                SET twitch_user_id = $1
              WHERE lower(twitch_login) = lower($2)
                AND twitch_user_id IS NULL",
        )
        .bind(user.id.trim())
        .bind(user.login.trim())
        .execute(&mut *tx)
        .await?;
        updated += result.rows_affected();
    }
    tx.commit().await?;
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

    async fn pool_in_schema(schema: &str) -> Option<PgPool> {
        let dsn = match std::env::var("TB_TEST_DATABASE_URL") {
            Ok(dsn) => dsn,
            Err(_) => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return None;
            }
        };
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .ok()?;
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .ok()?;
        admin.close().await;

        let opts = PgConnectOptions::from_str(&dsn)
            .ok()?
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .ok()?;
        sqlx::query(
            "CREATE TABLE twitch_streamers (
                twitch_login TEXT PRIMARY KEY,
                twitch_user_id TEXT
            )",
        )
        .execute(&pool)
        .await
        .ok()?;
        sqlx::query(
            "CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT,
                twitch_login TEXT,
                authorized_at TIMESTAMPTZ,
                last_refreshed_at TIMESTAMPTZ
            )",
        )
        .execute(&pool)
        .await
        .ok()?;
        Some(pool)
    }

    #[tokio::test]
    async fn phase1_uebernimmt_ids_aus_raid_auth_idempotent() {
        let Some(pool) = pool_in_schema("tb_bot_uid_backfill_phase1").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id)
             VALUES ('Known', '1'), ('Missing', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_login, twitch_user_id, authorized_at)
             VALUES ('missing', '42', NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(sync_from_raid_auth(&pool).await.unwrap(), 1);
        assert_eq!(sync_from_raid_auth(&pool).await.unwrap(), 0);
        let id: Option<String> = sqlx::query_scalar(
            "SELECT twitch_user_id FROM twitch_streamers WHERE twitch_login = 'Missing'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(id.as_deref(), Some("42"));
    }

    #[tokio::test]
    async fn phase2_update_bleibt_auf_null_ids_beschraenkt() {
        let Some(pool) = pool_in_schema("tb_bot_uid_backfill_phase2").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id)
             VALUES ('Known', '1'), ('NeedsApi', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let updated = apply_resolved_user_ids(
            &pool,
            &[
                ResolvedTwitchUser {
                    login: "known".to_string(),
                    id: "SHOULD_NOT_WIN".to_string(),
                },
                ResolvedTwitchUser {
                    login: "needsapi".to_string(),
                    id: "99".to_string(),
                },
            ],
        )
        .await
        .unwrap();
        assert_eq!(updated, 1);

        let known: Option<String> = sqlx::query_scalar(
            "SELECT twitch_user_id FROM twitch_streamers WHERE twitch_login = 'Known'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let api: Option<String> = sqlx::query_scalar(
            "SELECT twitch_user_id FROM twitch_streamers WHERE twitch_login = 'NeedsApi'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(known.as_deref(), Some("1"));
        assert_eq!(api.as_deref(), Some("99"));
    }
}
