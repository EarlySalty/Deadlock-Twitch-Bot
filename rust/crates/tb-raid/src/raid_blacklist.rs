//! Raid-Ziel-Blacklist (`twitch_raid_blacklist`) plus harte globale Bans.
//! Port von `raid/services/raid_blacklist.py`
//! (`_is_blacklisted` + `_store_blacklist_entry`).
//!
//! Prod-Schema (verifiziert): `target_id`/`target_login`/`reason`/`added_at`
//! alle **TEXT**; PK ist `target_login`. `is_blacklisted` matcht per ID **oder**
//! Login (lowercase). Beim Eintragen wird eine evtl. abweichende Zeile mit
//! gleicher `target_id` aber anderem Login vorher entfernt (Drift-Cleanup).

use std::collections::HashSet;

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::PgPool;

#[derive(Clone)]
pub struct RaidBlacklistStore {
    pool: PgPool,
}

impl RaidBlacklistStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Ist das Ziel raid-geblacklistet? Match per `target_id` ODER `lower(target_login)`.
    pub async fn is_blacklisted(
        &self,
        target_id: Option<&str>,
        target_login: &str,
    ) -> Result<bool, sqlx::Error> {
        let target_id = target_id.map(str::trim).filter(|s| !s.is_empty());
        let login = target_login.trim().to_lowercase();
        let row: Option<i32> = sqlx::query_scalar!(
            r#"
            SELECT 1 AS "found!" FROM twitch_raid_blacklist
            WHERE (target_id IS NOT NULL AND target_id = $1)
               OR lower(target_login) = $2
            LIMIT 1
            "#,
            target_id,
            &login
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    /// Ist das Ziel hart global gebannt? Match per `chatter_id` ODER
    /// `lower(chatter_login)`. Diese Sperre ist härter als die Raid-Blacklist.
    pub async fn is_hard_banned(
        &self,
        target_id: Option<&str>,
        target_login: &str,
    ) -> Result<bool, sqlx::Error> {
        let target_id = target_id.map(str::trim).filter(|s| !s.is_empty());
        let login = target_login.trim().to_lowercase();
        let row: Option<i32> = sqlx::query_scalar(
            r#"
            SELECT 1 FROM twitch_chatter_global_ban
            WHERE (NULLIF(chatter_id, '') IS NOT NULL AND chatter_id = $1)
               OR lower(chatter_login) = $2
            LIMIT 1
            "#,
        )
        .bind(target_id)
        .bind(&login)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    /// Trägt ein Ziel ein (UPSERT auf `target_login`). Leerer Login → no-op.
    /// Mit `target_id` wird zuvor eine fremde Login-Zeile gleicher ID entfernt.
    pub async fn add(
        &self,
        target_id: Option<&str>,
        target_login: &str,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let login = target_login.trim().to_lowercase();
        if login.is_empty() {
            return Ok(());
        }
        let target_id = target_id.map(str::trim).filter(|s| !s.is_empty());
        let added_at = now.to_rfc3339_opts(SecondsFormat::Secs, false);

        let mut tx = self.pool.begin().await?;
        if let Some(tid) = target_id {
            sqlx::query!(
                "DELETE FROM twitch_raid_blacklist
                  WHERE target_id = $1 AND lower(target_login) <> $2",
                tid,
                &login
            )
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query!(
            r#"
            INSERT INTO twitch_raid_blacklist (target_id, target_login, reason, added_at)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (target_login) DO UPDATE SET
                target_id = COALESCE(EXCLUDED.target_id, twitch_raid_blacklist.target_id),
                reason    = EXCLUDED.reason,
                added_at  = EXCLUDED.added_at
            "#,
            target_id,
            &login,
            reason,
            &added_at
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Lädt alle Auto-Raid-Sperren als `(ids, logins)`-Sets für Set-Filterung
    /// im Auswahl-Loop. Neben der expliziten Raid-Blacklist zählen hier auch
    /// globale Chatter-Bans.
    /// Logins lowercase, leere IDs/Logins ausgelassen.
    pub async fn load_all(&self) -> Result<(HashSet<String>, HashSet<String>), sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT target_id AS "target_id?", target_login AS "target_login!"
            FROM twitch_raid_blacklist

            UNION ALL

            SELECT NULLIF(chatter_id, '') AS "target_id?", chatter_login AS "target_login!"
            FROM twitch_chatter_global_ban
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        let mut ids = HashSet::new();
        let mut logins = HashSet::new();
        for row in rows {
            let id = row.target_id;
            let login = row.target_login;
            if let Some(id) = id.map(|i| i.trim().to_string()).filter(|i| !i.is_empty()) {
                ids.insert(id);
            }
            let login = login.trim().to_lowercase();
            if !login.is_empty() {
                logins.insert(login);
            }
        }
        Ok((ids, logins))
    }

    /// Lädt nur harte globale Bans als `(ids, logins)`-Sets.
    pub async fn load_hard_bans(&self) -> Result<(HashSet<String>, HashSet<String>), sqlx::Error> {
        let rows = sqlx::query_as::<_, (Option<String>, String)>(
            r#"
            SELECT NULLIF(chatter_id, ''), chatter_login
            FROM twitch_chatter_global_ban
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        let mut ids = HashSet::new();
        let mut logins = HashSet::new();
        for (target_id, target_login) in rows {
            if let Some(id) = target_id
                .map(|i| i.trim().to_string())
                .filter(|i| !i.is_empty())
            {
                ids.insert(id);
            }
            let login = target_login.trim().to_lowercase();
            if !login.is_empty() {
                logins.insert(login);
            }
        }
        Ok((ids, logins))
    }

    /// Entfernt ein Ziel per Login. Liefert `true` wenn etwas gelöscht wurde.
    pub async fn remove(&self, target_login: &str) -> Result<bool, sqlx::Error> {
        let login = target_login.trim().to_lowercase();
        let result = sqlx::query!(
            "DELETE FROM twitch_raid_blacklist WHERE lower(target_login) = $1",
            &login
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}
