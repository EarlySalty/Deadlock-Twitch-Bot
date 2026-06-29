//! Aggregation für `GET /twitch/api/admin/system/oauth-scopes` (P1.29 / P2.74).
//!
//! Liefert pro autorisiertem Streamer den OAuth-Scope-Status: gewährte und
//! fehlende Scopes, ob ein Re-Auth nötig ist, plus den Partner-Status. Quelle
//! ist `twitch_raid_auth` (eine Zeile pro autorisiertem Twitch-Account), das
//! per Login/User-ID an die kanonische Partner-Zeile aus
//! `twitch_partners_all_state` gejoint wird.
//!
//! Python-Vorbild: `bot/analytics/api_admin.py::_load_admin_oauth_scope_rows`
//! + `_api_admin_system_oauth_scopes`.

use sqlx::PgPool;

/// Erforderliche Broadcaster-Scopes (Python: `BASE_STREAMER_SCOPES`).
pub const REQUIRED_SCOPES: &[&str] = &[
    "channel:manage:raids",
    "channel:manage:moderators",
    "channel:bot",
    "clips:edit",
    "channel:read:ads",
    "bits:read",
    "channel:read:redemptions",
];

/// Besonders kritische Scopes (Python: `BASE_CRITICAL_STREAMER_SCOPES`).
pub const CRITICAL_SCOPES: &[&str] = &["bits:read", "channel:read:redemptions"];

/// Spalten-Labels für die Frontend-Tabelle (Python: `_SCOPE_COLUMN_LABELS`).
pub const SCOPE_COLUMN_LABELS: &[(&str, &str)] = &[
    ("channel:manage:raids", "Raids"),
    ("channel:manage:moderators", "Mods"),
    ("channel:bot", "Bot"),
    ("clips:edit", "Clips"),
    ("channel:read:ads", "Ads"),
    ("bits:read", "Bits"),
    ("channel:read:redemptions", "Points"),
];

/// Eine Rohzeile der Scope-Aggregation (ein autorisierter Account).
#[derive(Debug, Clone)]
pub struct OAuthScopeRow {
    pub effective_login: String,
    pub display_name: Option<String>,
    pub scopes_raw: Option<String>,
    pub needs_reauth: bool,
    pub status: Option<String>,
    pub archived_at: Option<String>,
    pub manual_partner_opt_out: bool,
    pub technical_pause_reason: Option<String>,
}

/// Aufbereiteter Scope-Status eines Accounts (Python: `_admin_scope_snapshot`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeSnapshot {
    pub connected: bool,
    /// `connected` | `partial` | `missing` | `reauth`
    pub status: &'static str,
    pub needs_reauth: bool,
    pub granted_scopes: Vec<String>,
    pub missing_scopes: Vec<String>,
}

/// Berechnet den Scope-Snapshot aus rohem Scope-String und `needs_reauth`.
pub fn scope_snapshot(scopes_raw: Option<&str>, needs_reauth: bool) -> ScopeSnapshot {
    let mut granted: Vec<String> = scopes_raw
        .unwrap_or("")
        .split_whitespace()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    granted.sort();
    granted.dedup();

    let missing: Vec<String> = REQUIRED_SCOPES
        .iter()
        .filter(|req| !granted.iter().any(|g| g == *req))
        .map(|s| s.to_string())
        .collect();

    let connected = !granted.is_empty();
    let status = if needs_reauth {
        "reauth"
    } else if !connected {
        "missing"
    } else if !missing.is_empty() {
        "partial"
    } else {
        "connected"
    };

    ScopeSnapshot {
        connected,
        status,
        needs_reauth,
        granted_scopes: granted,
        missing_scopes: missing,
    }
}

/// Leitet den Partner-Status ab (Python: `_admin_partner_status`).
pub fn partner_status(
    status: Option<&str>,
    archived_at: Option<&str>,
    manual_partner_opt_out: bool,
    technical_pause_reason: Option<&str>,
) -> &'static str {
    let pause = technical_pause_reason.unwrap_or("").trim().to_lowercase();
    if pause == "blocked" {
        return "blocked";
    }
    if pause == "token_error" {
        return "token_error";
    }
    if manual_partner_opt_out {
        return "non_partner";
    }
    let normalized = status.unwrap_or("").trim().to_lowercase();
    if normalized == "departnered" {
        return "departnered";
    }
    let archived = archived_at.map(|s| !s.trim().is_empty()).unwrap_or(false);
    if normalized == "archived" || archived {
        return "archived";
    }
    "active"
}

/// SQL-Rohzeile der Scope-Aggregation (vor der typisierten Aufbereitung).
type ScopeRawRow = (
    Option<String>, // effective_login
    Option<String>, // display_name
    Option<String>, // scopes
    Option<bool>,   // needs_reauth
    Option<String>, // status
    Option<String>, // archived_at
    Option<i64>,    // manual_partner_opt_out
    Option<String>, // technical_pause_reason
);

/// Lädt die Scope-Rohzeilen aus der DB.
///
/// Joint `twitch_raid_auth` an die kanonische (deduplizierte) Partner-Zeile aus
/// `twitch_partners_all_state` — bevorzugt Match per Twitch-User-ID, sonst per
/// Login. Sortiert nach effektivem Login. `authorized_at` ist im
/// Migrationsschema ein TEXT-Zeitstempel.
pub async fn load_oauth_scope_rows(pool: &PgPool) -> Result<Vec<OAuthScopeRow>, sqlx::Error> {
    let rows: Vec<ScopeRawRow> = sqlx::query_as(
        r#"
        WITH auth_rows AS (
            SELECT
                ROW_NUMBER() OVER (
                    ORDER BY
                        CASE WHEN authorized_at IS NULL THEN 1 ELSE 0 END,
                        authorized_at DESC,
                        LOWER(COALESCE(NULLIF(TRIM(twitch_login), ''), '')),
                        LOWER(COALESCE(NULLIF(TRIM(twitch_user_id), ''), ''))
                ) AS auth_row_id,
                twitch_login,
                twitch_user_id,
                scopes,
                needs_reauth
            FROM twitch_raid_auth
        ),
        partner_state AS (
            SELECT twitch_login, twitch_user_id, discord_display_name,
                   manual_partner_opt_out, archived_at, status, technical_pause_reason
            FROM (
                SELECT
                    s.twitch_login, s.twitch_user_id, s.discord_display_name,
                    s.manual_partner_opt_out, s.archived_at, s.status,
                    s.technical_pause_reason,
                    ROW_NUMBER() OVER (
                        PARTITION BY LOWER(s.twitch_login)
                        ORDER BY
                            CASE WHEN s.status = 'active' THEN 0 ELSE 1 END,
                            LOWER(s.twitch_login) ASC
                    ) AS rn
                FROM twitch_partners_all_state s
                WHERE COALESCE(TRIM(s.twitch_login), '') <> ''
            ) ranked
            WHERE rn = 1
        ),
        ranked_matches AS (
            SELECT
                a.auth_row_id, a.twitch_login, a.twitch_user_id, a.scopes,
                a.needs_reauth,
                s.twitch_login AS partner_login, s.discord_display_name,
                s.archived_at, s.manual_partner_opt_out, s.status,
                s.technical_pause_reason,
                ROW_NUMBER() OVER (
                    PARTITION BY a.auth_row_id
                    ORDER BY
                        CASE
                            WHEN NULLIF(TRIM(COALESCE(a.twitch_user_id, '')), '') IS NOT NULL
                                 AND NULLIF(TRIM(COALESCE(s.twitch_user_id, '')), '') IS NOT NULL
                                 AND LOWER(TRIM(a.twitch_user_id)) = LOWER(TRIM(s.twitch_user_id))
                            THEN 0
                            WHEN LOWER(COALESCE(a.twitch_login, '')) = LOWER(s.twitch_login)
                            THEN 1
                            ELSE 2
                        END,
                        LOWER(COALESCE(s.twitch_login, '')) ASC
                ) AS rn
            FROM auth_rows a
            LEFT JOIN partner_state s
                ON (
                    NULLIF(TRIM(COALESCE(a.twitch_user_id, '')), '') IS NOT NULL
                    AND NULLIF(TRIM(COALESCE(s.twitch_user_id, '')), '') IS NOT NULL
                    AND LOWER(TRIM(a.twitch_user_id)) = LOWER(TRIM(s.twitch_user_id))
                )
                OR LOWER(COALESCE(a.twitch_login, '')) = LOWER(s.twitch_login)
        )
        SELECT
            COALESCE(
                NULLIF(TRIM(partner_login), ''),
                NULLIF(TRIM(twitch_login), ''),
                NULLIF(TRIM(twitch_user_id), '')
            ) AS effective_login,
            discord_display_name,
            scopes,
            COALESCE(needs_reauth, FALSE) AS needs_reauth,
            status,
            archived_at::text,
            COALESCE(manual_partner_opt_out, 0)::bigint AS manual_partner_opt_out,
            technical_pause_reason
        FROM ranked_matches
        WHERE rn = 1
        ORDER BY
            LOWER(
                COALESCE(
                    NULLIF(TRIM(partner_login), ''),
                    NULLIF(TRIM(twitch_login), ''),
                    NULLIF(TRIM(twitch_user_id), '')
                )
            ) ASC,
            auth_row_id ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let login = r.0?.trim().to_lowercase();
            if login.is_empty() {
                return None;
            }
            Some(OAuthScopeRow {
                effective_login: login,
                display_name: r.1,
                scopes_raw: r.2,
                needs_reauth: r.3.unwrap_or(false),
                status: r.4,
                archived_at: r.5,
                manual_partner_opt_out: r.6.unwrap_or(0) != 0,
                technical_pause_reason: r.7,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn scope_snapshot_connected_wenn_alle_da() {
        let all = REQUIRED_SCOPES.join(" ");
        let snap = scope_snapshot(Some(&all), false);
        assert_eq!(snap.status, "connected");
        assert!(snap.missing_scopes.is_empty());
        assert!(snap.connected);
    }

    #[test]
    fn scope_snapshot_partial_und_reauth() {
        let snap = scope_snapshot(Some("bits:read"), false);
        assert_eq!(snap.status, "partial");
        assert!(snap.missing_scopes.contains(&"clips:edit".to_string()));

        let reauth = scope_snapshot(Some("bits:read"), true);
        assert_eq!(reauth.status, "reauth");

        let missing = scope_snapshot(Some(""), false);
        assert_eq!(missing.status, "missing");
        assert!(!missing.connected);
    }

    #[test]
    fn partner_status_prioritaeten() {
        assert_eq!(
            partner_status(Some("active"), None, false, Some("blocked")),
            "blocked"
        );
        assert_eq!(
            partner_status(Some("active"), None, true, None),
            "non_partner"
        );
        assert_eq!(
            partner_status(Some("departnered"), None, false, None),
            "departnered"
        );
        assert_eq!(
            partner_status(Some("active"), Some("2024-01-01"), false, None),
            "archived"
        );
        assert_eq!(
            partner_status(Some("active"), Some(""), false, None),
            "active"
        );
    }

    async fn pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
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
        let options = PgConnectOptions::from_str(&dsn)
            .ok()?
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .ok()?;
        for ddl in [
            "CREATE TABLE twitch_raid_auth (twitch_login TEXT, twitch_user_id TEXT, scopes TEXT, needs_reauth BOOLEAN DEFAULT FALSE, authorized_at TEXT)",
            "CREATE TABLE twitch_partners_all_state (twitch_login TEXT, twitch_user_id TEXT, discord_display_name TEXT, manual_partner_opt_out INTEGER DEFAULT 0, archived_at TEXT, status TEXT, technical_pause_reason TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.ok()?;
        }
        Some(pool)
    }

    #[tokio::test]
    async fn laedt_per_streamer_scope_status() {
        let Some(pool) = pool("t_oauth_scopes_status").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_login, twitch_user_id, scopes, needs_reauth, authorized_at) \
             VALUES ('streamerx','42','channel:bot bits:read',FALSE, NOW()::TEXT)",
        )
        .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners_all_state \
             (twitch_login, twitch_user_id, discord_display_name, manual_partner_opt_out, archived_at, status, technical_pause_reason) \
             VALUES ('streamerx','42','StreamerX',0,NULL,'active',NULL)",
        )
        .execute(&pool).await.unwrap();

        let rows = load_oauth_scope_rows(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].effective_login, "streamerx");
        assert_eq!(rows[0].display_name.as_deref(), Some("StreamerX"));
        let snap = scope_snapshot(rows[0].scopes_raw.as_deref(), rows[0].needs_reauth);
        assert_eq!(snap.status, "partial");
    }

    #[tokio::test]
    async fn laedt_bool_needs_reauth_ohne_coalesce_typmix() {
        let Some(pool) = pool("t_oauth_scopes_bool").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_login, twitch_user_id, scopes, needs_reauth, authorized_at) \
             VALUES ('reauthx','43','bits:read',TRUE, NOW()::TEXT)",
        )
        .execute(&pool).await.unwrap();

        let rows = load_oauth_scope_rows(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].effective_login, "reauthx");
        assert!(rows[0].needs_reauth);
    }

    #[tokio::test]
    async fn leeres_schema_liefert_leere_liste() {
        let Some(pool) = pool("t_oauth_scopes_empty").await else {
            return;
        };
        let rows = load_oauth_scope_rows(&pool).await.unwrap();
        assert!(rows.is_empty());
    }
}
