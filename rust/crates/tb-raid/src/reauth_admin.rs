//! Bulk-Re-Auth-Primitive (`snapshot_and_flag_reauth`) — P2.34.
//!
//! Port von `bot/raid/auth.py:1168-1189`. Nach einer Scope-Profil-Änderung
//! (z. B. neue Bits-/Subs-/Ads-Scopes) muss der Operator **alle** Streamer in
//! einem Schwung zur Neu-Autorisierung zwingen. Diese Primitive führt das
//! Massen-UPDATE aus; die Discord-DM-Schleife des Python-Befehls bleibt bewusst
//! draußen (B10-Ausschluss, kein Discord-DM in Rust) — der Admin-Dashboard-Pfad
//! (P3.7) ruft nur diese SQL-Operation auf.
//!
//! Das WHERE-Prädikat spiegelt Python exakt: nur Zeilen, die noch nicht
//! `needs_reauth=TRUE` sind **und** irgendein Token-/Autorisierungs-Material
//! tragen, werden geflaggt — leere Platzhalter-Zeilen bleiben unberührt.

use sqlx::PgPool;

/// DB-Store für die Bulk-Re-Auth-Operation.
#[derive(Clone)]
pub struct ReauthAdminStore {
    pool: PgPool,
}

impl ReauthAdminStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Setzt `needs_reauth=TRUE` und `reauth_notified_at=NULL` für **alle**
    /// token-tragenden Zeilen in `twitch_raid_auth`, die noch nicht geflaggt
    /// sind. Liefert die Anzahl betroffener Zeilen.
    ///
    /// Port von `auth.py:1168-1189` — identisches WHERE-Prädikat (mind. eine
    /// Token-/Autorisierungs-Quelle vorhanden), kein Discord-DM.
    pub async fn snapshot_and_flag_reauth(&self) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE twitch_raid_auth
                SET needs_reauth = TRUE,
                    reauth_notified_at = NULL
              WHERE needs_reauth IS NOT TRUE
                AND (
                        access_token_enc IS NOT NULL
                     OR refresh_token_enc IS NOT NULL
                     OR NULLIF(access_token, '') IS NOT NULL
                     OR NULLIF(refresh_token, '') IS NOT NULL
                     OR authorized_at IS NOT NULL
                )",
        )
        .execute(&self.pool)
        .await?;

        let count = result.rows_affected();
        tracing::info!(
            count,
            "snapshot_and_flag_reauth: {count} Tokens auf needs_reauth=TRUE gesetzt",
        );
        Ok(count)
    }
}

/// Port-Trait für die Admin-Schicht (tb-internal-api), damit der Handler nicht
/// direkt auf den DB-Store typisiert ist (testbar via Stub).
#[async_trait::async_trait]
pub trait BulkReauthPort: Send + Sync {
    /// Siehe [`ReauthAdminStore::snapshot_and_flag_reauth`].
    async fn snapshot_and_flag_reauth(&self) -> Result<u64, sqlx::Error>;
}

#[async_trait::async_trait]
impl BulkReauthPort for ReauthAdminStore {
    async fn snapshot_and_flag_reauth(&self) -> Result<u64, sqlx::Error> {
        ReauthAdminStore::snapshot_and_flag_reauth(self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db_url() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
    }

    async fn setup_db(schema: &str) -> PgPool {
        let url = test_db_url().expect("TB_TEST_DATABASE_URL muss gesetzt sein");
        let admin = PgPool::connect(&url).await.expect("Test-DB-Verbindung");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;

        let schema_owned = schema.to_string();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .after_connect(move |conn, _| {
                let schema = schema_owned.clone();
                Box::pin(async move {
                    sqlx::query(&format!("SET search_path TO {schema}"))
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .expect("Schema-Pool");

        sqlx::query(
            "CREATE TABLE twitch_raid_auth (
                twitch_user_id text PRIMARY KEY,
                twitch_login text,
                access_token text,
                refresh_token text,
                access_token_enc bytea,
                refresh_token_enc bytea,
                authorized_at timestamptz,
                needs_reauth boolean DEFAULT false,
                reauth_notified_at timestamptz)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn flags_all_token_bearing_rows_in_one_call() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("ra_bulk_flag").await;

        // Zeile A: plaintext access_token, needs_reauth NULL.
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, access_token, needs_reauth)
             VALUES ('a', 'tok-a', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Zeile B: nur authorized_at, needs_reauth=false.
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, authorized_at, needs_reauth)
             VALUES ('b', now(), false)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Zeile C: verschlüsselter Token (bytea), needs_reauth=false + alter Notify-Stamp.
        sqlx::query(
            "INSERT INTO twitch_raid_auth
                (twitch_user_id, access_token_enc, needs_reauth, reauth_notified_at)
             VALUES ('c', '\\x00'::bytea, false, now())",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Zeile D: KEIN Token-Material → bleibt unberührt.
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, access_token, needs_reauth)
             VALUES ('d', '', false)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Zeile E: schon needs_reauth=TRUE → nicht erneut gezählt.
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, access_token, needs_reauth)
             VALUES ('e', 'tok-e', TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let store = ReauthAdminStore::new(pool.clone());
        let count = store.snapshot_and_flag_reauth().await.unwrap();
        // A, B, C werden geflaggt. D (kein Token) und E (schon TRUE) nicht.
        assert_eq!(count, 3);

        // A/B/C sind jetzt TRUE, reauth_notified_at NULL.
        let flagged: Vec<(String, Option<bool>, Option<chrono::DateTime<chrono::Utc>>)> =
            sqlx::query_as(
                "SELECT twitch_user_id, needs_reauth, reauth_notified_at
                   FROM twitch_raid_auth
                  WHERE twitch_user_id IN ('a','b','c')
                  ORDER BY twitch_user_id",
            )
            .fetch_all(&pool)
            .await
            .unwrap();
        for (_, needs, notified) in &flagged {
            assert_eq!(*needs, Some(true));
            assert!(notified.is_none());
        }

        // D bleibt unberührt.
        let d_needs: Option<bool> =
            sqlx::query_scalar("SELECT needs_reauth FROM twitch_raid_auth WHERE twitch_user_id='d'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(d_needs, Some(false));

        // Zweiter Aufruf: nichts mehr zu flaggen (idempotent).
        let count2 = store.snapshot_and_flag_reauth().await.unwrap();
        assert_eq!(count2, 0);
    }
}
