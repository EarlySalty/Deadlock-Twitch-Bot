//! Daten-Layer für die Admin-Bulk-Config der Partner-Flags.
//!
//! Port von `bot/storage/partner_registry.py:bulk_update_partner_flags` +
//! `bot/analytics/api_admin.py:_admin_load_streamer_config_snapshots`. Setzt die
//! Notification-/Raid-Flags (`raid_bot_enabled`, `live_ping_enabled`,
//! `silent_ban`, `silent_raid`) auf `twitch_partners` für **alle aktiven**
//! Partner auf einmal und liefert Aggregat-Snapshots fürs Dashboard.
//!
//! Diese Flags sind dieselben Spalten, die die Chat-Commands `!silentban`/
//! `!silentraid` und die Streamer-Selbstbedienung (`silent_settings`) je Kanal
//! toggeln — die Admin-Bulk-Variante setzt sie netzweit.

use serde_json::{json, Value};
use sqlx::{PgPool, QueryBuilder};

pub const SCOPE_ACTIVE: &str = "active";
pub const SCOPE_ALL: &str = "all";

/// Welche Flags gesetzt werden sollen (`None` = unverändert).
#[derive(Debug, Default, Clone)]
pub struct PartnerFlagUpdate {
    pub raid_bot_enabled: Option<bool>,
    pub live_ping_enabled: Option<bool>,
    pub silent_ban: Option<bool>,
    pub silent_raid: Option<bool>,
}

/// Normalisiert einen Scope-String (Python `_admin_parse_scope`).
/// `None`/leer → `active`; sonst lowercase, muss `active`|`all` sein, sonst `None`.
pub fn parse_admin_scope(raw: Option<&str>) -> Option<String> {
    match raw.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        None => Some(SCOPE_ACTIVE.to_string()),
        Some(s) => {
            let lower = s.to_lowercase();
            if lower == SCOPE_ACTIVE || lower == SCOPE_ALL {
                Some(lower)
            } else {
                None
            }
        }
    }
}

fn scope_filter_sql(scope: &str) -> &'static str {
    if scope == SCOPE_ALL {
        "1=1"
    } else {
        "status = 'active'"
    }
}

/// Setzt die angegebenen Flags auf allen **aktiven** Partnern und gibt die Zahl
/// der aktiven Partner zurück (Python `bulk_update_partner_flags`).
///
/// Quirk 1:1 übernommen: der Scope wird **immer** auf `active` gezwungen — das
/// Bulk-Update betrifft nie nicht-aktive Partner, auch wenn der Endpoint `all`
/// erhält (nur die Snapshot-Zählung respektiert `all`).
pub async fn bulk_update_partner_flags(
    pool: &PgPool,
    flags: &PartnerFlagUpdate,
) -> Result<i64, sqlx::Error> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partners WHERE status = 'active'")
        .fetch_one(pool)
        .await?;

    // Spaltennamen stammen aus festem Set (kein User-Input) → sicher.
    let assignments: Vec<(&str, i32)> = [
        ("raid_bot_enabled", flags.raid_bot_enabled),
        ("live_ping_enabled", flags.live_ping_enabled),
        ("silent_ban", flags.silent_ban),
        ("silent_raid", flags.silent_raid),
    ]
    .into_iter()
    .filter_map(|(col, v)| v.map(|b| (col, b as i32)))
    .collect();

    if assignments.is_empty() {
        return Ok(total);
    }

    let mut qb = QueryBuilder::new("UPDATE twitch_partners SET ");
    for (i, (col, val)) in assignments.iter().enumerate() {
        if i > 0 {
            qb.push(", ");
        }
        qb.push(*col).push(" = ").push_bind(*val);
    }
    qb.push(" WHERE status = 'active'");
    qb.build().execute(pool).await?;

    Ok(total)
}

/// Aggregierte Flag-Zählungen über den Scope (Python
/// `_admin_load_streamer_config_snapshots`).
#[derive(Debug, Clone)]
pub struct ConfigSnapshots {
    pub scope: String,
    pub total: i64,
    pub raid_bot_enabled_count: i64,
    pub live_ping_enabled_count: i64,
    pub silent_ban_count: i64,
    pub silent_raid_count: i64,
}

impl ConfigSnapshots {
    /// Basis-Snapshot der Raid-Flags (camelCase, 1:1 Python).
    pub fn raid_snapshot(&self) -> Value {
        json!({
            "managedScope": self.scope,
            "scope": self.scope,
            "totalManagedStreamers": self.total,
            "raidBotEnabledCount": self.raid_bot_enabled_count,
            "livePingEnabledCount": self.live_ping_enabled_count,
            "allRaidBotEnabled": self.total > 0 && self.raid_bot_enabled_count == self.total,
            "allLivePingEnabled": self.total > 0 && self.live_ping_enabled_count == self.total,
        })
    }

    /// Basis-Snapshot der Chat-Flags (camelCase, 1:1 Python).
    pub fn chat_snapshot(&self) -> Value {
        json!({
            "managedScope": self.scope,
            "scope": self.scope,
            "totalManagedStreamers": self.total,
            "silentBanCount": self.silent_ban_count,
            "silentRaidCount": self.silent_raid_count,
            "allSilentBan": self.total > 0 && self.silent_ban_count == self.total,
            "allSilentRaid": self.total > 0 && self.silent_raid_count == self.total,
        })
    }
}

/// Lädt die Aggregat-Snapshots über den Scope (`active` → nur aktive Partner,
/// `all` → alle).
pub async fn load_streamer_config_snapshots(
    pool: &PgPool,
    scope: &str,
) -> Result<ConfigSnapshots, sqlx::Error> {
    let where_clause = scope_filter_sql(scope);
    // where_clause stammt aus festem Set (active→status='active', all→1=1).
    let sql = format!(
        "SELECT \
            COUNT(*) AS total, \
            COUNT(*) FILTER (WHERE raid_bot_enabled = 1) AS raid_bot, \
            COUNT(*) FILTER (WHERE COALESCE(live_ping_enabled, 1) = 1) AS live_ping, \
            COUNT(*) FILTER (WHERE silent_ban = 1) AS silent_ban, \
            COUNT(*) FILTER (WHERE silent_raid = 1) AS silent_raid \
         FROM twitch_partners WHERE {where_clause}"
    );
    let row: (i64, i64, i64, i64, i64) = sqlx::query_as(&sql).fetch_one(pool).await?;
    Ok(ConfigSnapshots {
        scope: scope.to_string(),
        total: row.0,
        raid_bot_enabled_count: row.1,
        live_ping_enabled_count: row.2,
        silent_ban_count: row.3,
        silent_raid_count: row.4,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn parse_scope_varianten() {
        assert_eq!(parse_admin_scope(None).as_deref(), Some("active"));
        assert_eq!(parse_admin_scope(Some("")).as_deref(), Some("active"));
        assert_eq!(parse_admin_scope(Some("ALL")).as_deref(), Some("all"));
        assert_eq!(parse_admin_scope(Some("active")).as_deref(), Some("active"));
        assert_eq!(parse_admin_scope(Some("bogus")), None);
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_partners (\
                twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, status TEXT, \
                raid_bot_enabled INTEGER DEFAULT 0, live_ping_enabled INTEGER DEFAULT 1, \
                silent_ban INTEGER DEFAULT 0, silent_raid INTEGER DEFAULT 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    async fn seed(pool: &PgPool, uid: &str, status: &str) {
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) VALUES ($1, $1, $2)")
            .bind(uid)
            .bind(status)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn bulk_update_nur_aktive_plus_snapshot() {
        let Some(pool) = make_pool("t_admincfg_bulk").await else { return };
        seed(&pool, "a1", "active").await;
        seed(&pool, "a2", "active").await;
        seed(&pool, "d1", "departnered").await; // nicht aktiv

        // Bulk: raid_bot ein + silent_ban ein → nur die 2 aktiven betroffen.
        let count = bulk_update_partner_flags(
            &pool,
            &PartnerFlagUpdate { raid_bot_enabled: Some(true), silent_ban: Some(true), ..Default::default() },
        )
        .await
        .unwrap();
        assert_eq!(count, 2, "nur aktive Partner zählen");

        // departnered bleibt unangetastet (raid_bot_enabled default 0).
        let dep: i32 = sqlx::query_scalar("SELECT raid_bot_enabled FROM twitch_partners WHERE twitch_user_id = 'd1'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(dep, 0);

        // Snapshot scope=active: 2 total, 2 raid_bot, 2 silent_ban → all*=true.
        let snap = load_streamer_config_snapshots(&pool, "active").await.unwrap();
        assert_eq!(snap.total, 2);
        assert_eq!(snap.raid_bot_enabled_count, 2);
        assert_eq!(snap.silent_ban_count, 2);
        assert_eq!(snap.raid_snapshot()["allRaidBotEnabled"], true);
        assert_eq!(snap.chat_snapshot()["allSilentBan"], true);

        // Snapshot scope=all: 3 total (inkl. departnered), aber nur 2 raid_bot → all*=false.
        let snap_all = load_streamer_config_snapshots(&pool, "all").await.unwrap();
        assert_eq!(snap_all.total, 3);
        assert_eq!(snap_all.raid_bot_enabled_count, 2);
        assert_eq!(snap_all.raid_snapshot()["allRaidBotEnabled"], false);
    }
}
