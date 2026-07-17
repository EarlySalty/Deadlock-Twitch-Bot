//! DB-Queries für den Global-Ban-Mechanismus.
//!
//! Alle Funktionen nehmen `&PgPool` und geben typisierte Ergebnisse zurück.
//! Kein HTTP, kein Serde.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

// ── Typen ─────────────────────────────────────────────────────────────────────

/// Ein Eintrag in `twitch_chatter_global_ban`.
#[derive(Debug, sqlx::FromRow)]
pub struct GlobalBanEntry {
    pub chatter_login: String,
    pub chatter_id: Option<String>,
    pub reason: Option<String>,
    pub added_by: Option<String>,
    pub added_at: Option<DateTime<Utc>>,
}

/// Kanalbezogene Einstellung für die Anwendung globaler Bans.
#[derive(Debug, sqlx::FromRow)]
pub struct ChannelEnforcement {
    pub twitch_login: String,
    pub global_ban_enforcement_enabled: bool,
}

// ── Fingerprint für healthz ───────────────────────────────────────────────────

/// Berechnet SHA-256 über `information_schema.tables`-Metadaten.
///
/// Liefert `None` wenn die Query fehlschlägt (kein Hard-Fehler — healthz soll
/// trotzdem antworten).
pub async fn db_schema_fingerprint(pool: &PgPool) -> Result<String, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        SELECT
            COALESCE(table_schema, '') AS "table_schema!",
            COALESCE(table_name, '') AS "table_name!",
            COALESCE(table_type, '') AS "table_type!"
        FROM information_schema.tables
        WHERE table_schema NOT IN ('information_schema', 'pg_catalog')
        ORDER BY table_schema, table_name
        "#,
    )
    .fetch_all(pool)
    .await?;

    let combined: String = rows
        .iter()
        .map(|row| format!("{}.{}:{}", row.table_schema, row.table_name, row.table_type))
        .collect::<Vec<_>>()
        .join("\n");

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(combined.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

// ── Global-Ban Add ────────────────────────────────────────────────────────────

/// Upsert in `twitch_chatter_global_ban`.
///
/// Global-Ban (gesperrter Chatter über alle Kanäle) und Raid-Blacklist (Kanäle,
/// die nicht als Raid-Ziel angefahren werden) sind hier zwei getrennte
/// Mechanismen. Achtung, bewusste Abweichung vom Python-Original:
/// `pg.add_chatter_global_ban` spiegelte jeden Global-Ban zusätzlich in
/// `twitch_raid_blacklist` (Einbahn global→raid). Diese Kopplung wurde mit
/// Changelog #129 absichtlich aufgehoben — die Raid-Blacklist wird
/// ausschließlich über die dedizierten Raid-Blacklist-Routen gepflegt.
pub async fn add_ban(
    pool: &PgPool,
    login: &str,
    chatter_id: Option<&str>,
    reason: Option<&str>,
    added_by: Option<&str>,
) -> Result<(), sqlx::Error> {
    let login = login.to_lowercase();

    sqlx::query!(
        r#"
        INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id, reason, added_by)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (chatter_login) DO UPDATE SET
            chatter_id = COALESCE(EXCLUDED.chatter_id, twitch_chatter_global_ban.chatter_id),
            reason     = EXCLUDED.reason,
            added_by   = EXCLUDED.added_by,
            added_at   = NOW()
        "#,
        &login,
        chatter_id,
        reason,
        added_by
    )
    .execute(pool)
    .await?;

    Ok(())
}

// ── Global-Ban Remove ─────────────────────────────────────────────────────────

/// Löscht aus `twitch_chatter_global_ban` + `twitch_chatter_global_ban_applied`.
///
/// Gibt zurück ob tatsächlich ein Eintrag gelöscht wurde (`rows_affected > 0`).
pub async fn remove_ban(pool: &PgPool, login: &str) -> Result<bool, sqlx::Error> {
    let login = login.to_lowercase();
    let mut tx = pool.begin().await?;

    let result = sqlx::query!(
        "DELETE FROM twitch_chatter_global_ban WHERE chatter_login = $1",
        &login
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        "DELETE FROM twitch_chatter_global_ban_applied WHERE chatter_login = $1",
        &login
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(result.rows_affected() > 0)
}

// ── Global-Ban Check ──────────────────────────────────────────────────────────

/// Prüft ob `login` (oder optional `chatter_id`) gebannt ist.
///
/// `chatter_id` wird als Leer-String übergeben wenn nicht bekannt — die Query
/// filtert `chatter_id <> ''` damit kein False-Positive entsteht.
pub async fn check_ban(pool: &PgPool, login: &str, chatter_id: &str) -> Result<bool, sqlx::Error> {
    let login = login.to_lowercase();
    let row = sqlx::query_scalar!(
        r#"
        SELECT 1 AS "found!"
        FROM twitch_chatter_global_ban
        WHERE chatter_login = $1
           OR (chatter_id IS NOT NULL AND chatter_id = $2 AND chatter_id <> '')
        LIMIT 1
        "#,
        &login,
        chatter_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.is_some())
}

// ── Global-Ban List ───────────────────────────────────────────────────────────

/// Alle Einträge, neueste zuerst.
pub async fn list_bans(pool: &PgPool) -> Result<Vec<GlobalBanEntry>, sqlx::Error> {
    sqlx::query_as!(
        GlobalBanEntry,
        r#"
        SELECT
            chatter_login AS "chatter_login!",
            chatter_id,
            reason,
            added_by,
            added_at AS "added_at?"
        FROM twitch_chatter_global_ban
        ORDER BY added_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
}

/// Aktive Kanäle samt Global-Ban-Enforcement-Flag.
pub async fn list_channel_enforcement(
    pool: &PgPool,
) -> Result<Vec<ChannelEnforcement>, sqlx::Error> {
    sqlx::query_as::<_, ChannelEnforcement>(
        "SELECT LOWER(twitch_login) AS twitch_login, global_ban_enforcement_enabled \
         FROM twitch_partners \
         WHERE status = 'active' \
           AND admin_archived_at IS NULL \
           AND departnered_at IS NULL \
         ORDER BY LOWER(twitch_login)",
    )
    .fetch_all(pool)
    .await
}

/// Setzt das Global-Ban-Enforcement-Flag eines aktiven Kanals.
pub async fn set_channel_enforcement(
    pool: &PgPool,
    login: &str,
    enabled: bool,
) -> Result<Option<ChannelEnforcement>, sqlx::Error> {
    sqlx::query_as::<_, ChannelEnforcement>(
        "UPDATE twitch_partners \
         SET global_ban_enforcement_enabled = $2 \
         WHERE LOWER(twitch_login) = LOWER($1) \
           AND status = 'active' \
           AND admin_archived_at IS NULL \
           AND departnered_at IS NULL \
         RETURNING LOWER(twitch_login) AS twitch_login, global_ban_enforcement_enabled",
    )
    .bind(login)
    .bind(enabled)
    .fetch_optional(pool)
    .await
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    /// Schema-isolierter Pool: eigenes Postgres-Schema, DDL für alle 3 Tabellen.
    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");

        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_chatter_global_ban (
                chatter_login  TEXT PRIMARY KEY,
                chatter_id     TEXT,
                reason         TEXT,
                added_by       TEXT,
                added_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_chatter_global_ban");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_chatter_global_ban_applied (
                id             BIGSERIAL PRIMARY KEY,
                chatter_login  TEXT NOT NULL,
                applied_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_chatter_global_ban_applied");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raid_blacklist (
                id             BIGSERIAL PRIMARY KEY,
                target_id      TEXT,
                target_login   TEXT NOT NULL,
                reason         TEXT,
                UNIQUE (target_login)
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_raid_blacklist");

        // Sauberer Zustand vor jedem Test
        sqlx::query(
            "TRUNCATE twitch_chatter_global_ban, twitch_chatter_global_ban_applied, twitch_raid_blacklist",
        )
        .execute(&pool)
        .await
        .expect("TRUNCATE");

        pool
    }

    #[tokio::test]
    async fn add_ban_schreibt_global_ban_ohne_raid_blacklist_mirror() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_gb_add").await;

        add_ban(
            &pool,
            "boser_user",
            Some("id123"),
            Some("Spam"),
            Some("admin"),
        )
        .await
        .expect("add_ban");

        // In Global-Ban-Tabelle
        let banned = check_ban(&pool, "boser_user", "").await.expect("check");
        assert!(banned, "Eintrag muss in twitch_chatter_global_ban sein");

        // Global-Ban und Raid-Blacklist sind getrennt: KEIN Mirror.
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT target_login FROM twitch_raid_blacklist WHERE target_login = $1",
        )
        .bind("boser_user")
        .fetch_optional(&pool)
        .await
        .expect("blacklist select");
        assert!(
            row.is_none(),
            "Global-Ban darf NICHT in twitch_raid_blacklist spiegeln"
        );
    }

    #[tokio::test]
    async fn remove_ban_loescht_aus_beiden_tabellen() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_gb_remove").await;

        add_ban(&pool, "zu_loeschender", None, None, None)
            .await
            .expect("add_ban");

        // applied-Eintrag manuell einfügen (wird von Python-Bot gesetzt)
        sqlx::query("INSERT INTO twitch_chatter_global_ban_applied (chatter_login) VALUES ($1)")
            .bind("zu_loeschender")
            .execute(&pool)
            .await
            .expect("applied insert");

        let removed = remove_ban(&pool, "zu_loeschender")
            .await
            .expect("remove_ban");
        assert!(removed, "removed muss true sein wenn Eintrag existiert");

        let banned = check_ban(&pool, "zu_loeschender", "").await.expect("check");
        assert!(!banned, "Nach remove_ban muss check false liefern");

        let applied: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM twitch_chatter_global_ban_applied WHERE chatter_login = $1",
        )
        .bind("zu_loeschender")
        .fetch_optional(&pool)
        .await
        .expect("applied select");
        assert!(
            applied.is_none(),
            "applied-Eintrag muss ebenfalls gelöscht sein"
        );
    }

    #[tokio::test]
    async fn check_ban_gibt_false_bei_unbekanntem_login() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_gb_check_false").await;

        let banned = check_ban(&pool, "unbekannt", "").await.expect("check");
        assert!(!banned);
    }

    #[tokio::test]
    async fn check_ban_gibt_true_nach_add() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_gb_check_true").await;

        add_ban(&pool, "bekannter", Some("uid99"), None, None)
            .await
            .expect("add_ban");

        // Check via Login
        let by_login = check_ban(&pool, "bekannter", "")
            .await
            .expect("check login");
        assert!(by_login, "Match via chatter_login muss funktionieren");

        // Check via chatter_id
        let by_id = check_ban(&pool, "anderer_login", "uid99")
            .await
            .expect("check id");
        assert!(by_id, "Match via chatter_id muss funktionieren");
    }

    #[tokio::test]
    async fn list_ban_gibt_leere_liste_bei_leerer_tabelle() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_gb_list_empty").await;

        let entries = list_bans(&pool).await.expect("list_bans");
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn list_ban_gibt_eintraege_nach_add() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_gb_list_entries").await;

        add_ban(&pool, "alpha", None, Some("Grund A"), Some("mod1"))
            .await
            .expect("add alpha");
        add_ban(&pool, "beta", Some("id_b"), Some("Grund B"), None)
            .await
            .expect("add beta");

        let entries = list_bans(&pool).await.expect("list_bans");
        assert_eq!(entries.len(), 2);
        // Neueste zuerst (beta wurde zuletzt eingefügt)
        assert_eq!(entries[0].chatter_login, "beta");
    }
}
