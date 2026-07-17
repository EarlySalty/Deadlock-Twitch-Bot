//! Admin-CRUD für die Raid-Ziel-Blacklist (`twitch_raid_blacklist`).
//!
//! Spiegelt die Python-Admin-Methoden aus `bot/dashboard/mixin.py`
//! (`_dashboard_raid_blacklist_*`) — bewusst getrennt von der Raid-Pipeline-
//! Store (`tb_raid::RaidBlacklistStore`), genau wie Python `dashboard/mixin.py`
//! von `raid/services/raid_blacklist.py` trennt (Admin-CRUD ≠ Auswahl-Loop).
//!
//! Prod-Schema (verifiziert): `target_id`/`target_login`/`reason`/`added_at`
//! alle TEXT; PK ist `target_login`; `added_at TEXT DEFAULT CURRENT_TIMESTAMP`.
//! Alle Funktionen erwarten einen bereits normalisierten (lowercase) Login.
//! Kein HTTP, kein Serde-JSON — nur Query-Logik.

use sqlx::PgPool;

/// Ein Blacklist-Eintrag (für die List-Route).
#[derive(Debug, sqlx::FromRow)]
pub struct BlacklistEntry {
    pub target_login: String,
    pub reason: Option<String>,
    pub added_at: Option<String>,
}

/// Upsert: trägt `login` mit `reason` ein. `target_id` bleibt NULL, `added_at`
/// kommt aus dem Spalten-Default bzw. wird bei Konflikt auf `CURRENT_TIMESTAMP`
/// gesetzt — byte-identisch zu `_dashboard_raid_blacklist_add`.
pub async fn add_manual(pool: &PgPool, login: &str, reason: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO twitch_raid_blacklist (target_id, target_login, reason)
        VALUES (NULL, $1, $2)
        ON CONFLICT (target_login) DO UPDATE SET
            reason   = EXCLUDED.reason,
            added_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(login)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(())
}

/// Löscht per Login (case-insensitiv). `true` wenn ein Eintrag entfernt wurde.
/// Parität zu `_dashboard_raid_blacklist_remove`.
pub async fn remove(pool: &PgPool, login: &str) -> Result<bool, sqlx::Error> {
    let result =
        sqlx::query("DELETE FROM twitch_raid_blacklist WHERE lower(target_login) = lower($1)")
            .bind(login)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

/// Liefert `(reason, added_at)` wenn `login` geblacklistet ist, sonst `None`.
/// NULL-Werte werden zu `""` (Parität zu `str(row[x] or "")`).
pub async fn check_entry(
    pool: &PgPool,
    login: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT reason, added_at FROM twitch_raid_blacklist WHERE lower(target_login) = lower($1)",
    )
    .bind(login)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(reason, added_at)| (reason.unwrap_or_default(), added_at.unwrap_or_default())))
}

/// Alle Einträge, neueste zuerst (`ORDER BY added_at DESC`).
pub async fn list_entries(pool: &PgPool) -> Result<Vec<BlacklistEntry>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT target_login, reason, added_at
        FROM twitch_raid_blacklist
        ORDER BY added_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    /// Gibt die DSN zurück oder bricht den Test ab. Mit `TB_TEST_REQUIRE_DB=1`
    /// wird statt des stillen Skips ein panic ausgelöst (CI erzwingt echte DB).
    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    /// Schema-isolierter Pool mit prod-treuer DDL (DROP+CREATE für Hermetik).
    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");

        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen");

        sqlx::query(
            r#"
            CREATE TABLE twitch_raid_blacklist (
                target_id    TEXT,
                target_login TEXT NOT NULL PRIMARY KEY,
                reason       TEXT,
                added_at     TEXT DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_raid_blacklist");

        pool
    }

    #[tokio::test]
    async fn add_then_check_liefert_reason() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_rbl_add_check").await;

        add_manual(&pool, "ziel", "manual_ban:absolut")
            .await
            .expect("add");

        let entry = check_entry(&pool, "ziel").await.expect("check");
        let (reason, added_at) = entry.expect("muss vorhanden sein");
        assert_eq!(reason, "manual_ban:absolut");
        assert!(
            !added_at.is_empty(),
            "added_at muss aus Default gesetzt sein"
        );
    }

    #[tokio::test]
    async fn check_unbekannt_ist_none() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_rbl_check_none").await;
        assert!(check_entry(&pool, "niemand")
            .await
            .expect("check")
            .is_none());
    }

    #[tokio::test]
    async fn remove_existing_true_unbekannt_false() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_rbl_remove").await;

        add_manual(&pool, "weg", "x").await.expect("add");
        assert!(remove(&pool, "weg").await.expect("remove vorhanden"));
        assert!(!remove(&pool, "weg").await.expect("remove erneut"));
        assert!(!remove(&pool, "nie").await.expect("remove unbekannt"));
    }

    #[tokio::test]
    async fn upsert_aktualisiert_reason() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_rbl_upsert").await;

        add_manual(&pool, "ziel", "grund_a").await.expect("add 1");
        add_manual(&pool, "ziel", "grund_b").await.expect("add 2");

        let (reason, _) = check_entry(&pool, "ziel")
            .await
            .expect("check")
            .expect("vorhanden");
        assert_eq!(reason, "grund_b");

        // Kein Duplikat (PK target_login).
        let entries = list_entries(&pool).await.expect("list");
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn list_desc_nach_added_at() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_rbl_list_desc").await;

        // Explizite added_at-Werte, um die DESC-Ordnung deterministisch zu prüfen.
        sqlx::query(
            "INSERT INTO twitch_raid_blacklist (target_login, reason, added_at) VALUES ($1,$2,$3)",
        )
        .bind("alt")
        .bind("r1")
        .bind("2026-01-01 00:00:00+00")
        .execute(&pool)
        .await
        .expect("insert alt");
        sqlx::query(
            "INSERT INTO twitch_raid_blacklist (target_login, reason, added_at) VALUES ($1,$2,$3)",
        )
        .bind("neu")
        .bind("r2")
        .bind("2026-02-01 00:00:00+00")
        .execute(&pool)
        .await
        .expect("insert neu");

        let entries = list_entries(&pool).await.expect("list");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].target_login, "neu", "neueste zuerst");
        assert_eq!(entries[1].target_login, "alt");
    }
}
