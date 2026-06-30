//! Auto-Deaktivierung des Engagement-Layers, wenn ein Channel offline geht
//! (Port von `bot/engagement/auto_off.py`).
//!
//! Wird vom Stream-Offline-Handler best-effort aufgerufen. Idempotent: der
//! `enabled = TRUE`-Guard im `WHERE` verhindert No-Op-Updates und liefert über
//! die betroffene Zeilenzahl, ob tatsächlich umgeschaltet wurde (0 = war nicht
//! mehr aktiv, sonst die Anzahl deaktivierter Zeilen).
//!
//! **Parität zu Python (bewusst KEIN Lowercasing):** Python bindet
//! `channel_login` roh in das case-sensitive `WHERE channel_login = %s`. Diese
//! Funktion tut dasselbe und normalisiert den Login NICHT — die Normalisierung
//! ist Sache des Aufrufers bzw. der Schreibseite, damit der Vergleich exakt auf
//! denselben Wert trifft, der in `twitch_engagement_settings` steht. (Die
//! Rust-Binary-Seite hat den Login bisher zusätzlich gelowercased; das ist die
//! Divergenz, die diese kanonische Funktion auflöst.)

use sqlx::PgPool;

/// Setzt `enabled = FALSE` für `channel_login`, falls aktuell aktiv.
///
/// Liefert die Anzahl umgeschalteter Zeilen (0 = Channel war nicht mehr aktiv).
/// Leerer Login → 0 ohne DB-Zugriff. Der Login wird **nicht** normalisiert
/// (case-sensitiver Vergleich, Python-Parität).
pub async fn auto_disable_on_offline(
    pool: &PgPool,
    channel_login: &str,
) -> Result<u64, sqlx::Error> {
    if channel_login.is_empty() {
        return Ok(0);
    }
    let result = sqlx::query!(
        "UPDATE twitch_engagement_settings \
            SET enabled = FALSE, updated_at = NOW() \
          WHERE channel_login = $1 AND enabled = TRUE",
        channel_login
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_engagement_settings (\
                channel_login TEXT PRIMARY KEY, \
                enabled BOOLEAN NOT NULL DEFAULT FALSE, \
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    async fn enabled_of(pool: &PgPool, channel: &str) -> Option<bool> {
        sqlx::query_scalar("SELECT enabled FROM twitch_engagement_settings WHERE channel_login = $1")
            .bind(channel)
            .fetch_optional(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn deaktiviert_aktiven_channel() {
        let Some(pool) = make_pool("t_eng_autooff_active").await else { return };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled) VALUES ('nani', TRUE)")
            .execute(&pool).await.unwrap();
        let n = auto_disable_on_offline(&pool, "nani").await.unwrap();
        assert_eq!(n, 1, "ein aktiver Channel wird umgeschaltet");
        assert_eq!(enabled_of(&pool, "nani").await, Some(false));
    }

    #[tokio::test]
    async fn idempotent_bei_bereits_aus() {
        let Some(pool) = make_pool("t_eng_autooff_idem").await else { return };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled) VALUES ('nani', FALSE)")
            .execute(&pool).await.unwrap();
        let n = auto_disable_on_offline(&pool, "nani").await.unwrap();
        assert_eq!(n, 0, "bereits AUS → kein No-Op-Update (enabled=TRUE-Guard)");
    }

    /// Kern der Divergenz: der Vergleich ist case-sensitiv und der Login wird
    /// NICHT gelowercased. Ein abweichend geschriebener Login trifft die Zeile
    /// nicht — exakt wie Python.
    #[tokio::test]
    async fn case_sensitiv_kein_lowercasing() {
        let Some(pool) = make_pool("t_eng_autooff_case").await else { return };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled) VALUES ('MixedCase', TRUE)")
            .execute(&pool).await.unwrap();
        // Aufruf mit gelowercaster Variante → trifft die Zeile NICHT (case-sensitiv).
        let n = auto_disable_on_offline(&pool, "mixedcase").await.unwrap();
        assert_eq!(n, 0, "gelowercaster Login trifft den exakt geschriebenen nicht");
        assert_eq!(enabled_of(&pool, "MixedCase").await, Some(true), "Originalzeile unangetastet");
        // Exakter Login → trifft.
        let n = auto_disable_on_offline(&pool, "MixedCase").await.unwrap();
        assert_eq!(n, 1);
        assert_eq!(enabled_of(&pool, "MixedCase").await, Some(false));
    }

    #[tokio::test]
    async fn leerer_login_kein_db_zugriff() {
        let Some(pool) = make_pool("t_eng_autooff_empty").await else { return };
        assert_eq!(auto_disable_on_offline(&pool, "").await.unwrap(), 0);
    }
}
