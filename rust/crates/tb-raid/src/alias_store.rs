//! Zweit-Accounts einem Menschen zuordnen (`twitch_streamer_aliases`).
//!
//! ## Warum
//!
//! Wer zwei Kanäle betreibt, ist für den Bot bisher zwei fremde Personen.
//! Raidet Account A und der Mensch schreibt vom Account B aus im Zielchat,
//! sieht der Bot Schweigen und schickt eine Erinnerung, die keinen Fehler
//! benennt. Mit dem Mapping zählt jede Nachricht von **irgendeinem** seiner
//! Accounts als seine.
//!
//! Der Store liefert nur die Zuordnung; wer sie wie nutzt, entscheiden die
//! Aufrufer (Greeting-Monitor, Score-Aggregation, Whisper-Ziel).

use sqlx::PgPool;

/// Alle Accounts einer Person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasGroup {
    /// Klammer über die Accounts, praktisch der Login des Hauptaccounts.
    pub person_key: String,
    /// Twitch-User-IDs aller Accounts.
    pub user_ids: Vec<String>,
    /// Logins aller Accounts, klein geschrieben.
    pub logins: Vec<String>,
    /// Account, an den Whispers gehen. `None` = keiner als Haupt markiert,
    /// dann bleibt es beim raidenden Account.
    pub primary_user_id: Option<String>,
}

impl AliasGroup {
    /// Ob dieser Account zur Gruppe gehört. Vergleicht ID und Login, weil je
    /// nach Ereignis mal das eine, mal das andere bekannt ist.
    pub fn contains(&self, user_id: &str, login: &str) -> bool {
        let user_id = user_id.trim();
        let login = login.trim().trim_start_matches('@').to_lowercase();
        (!user_id.is_empty() && self.user_ids.iter().any(|id| id == user_id))
            || (!login.is_empty() && self.logins.contains(&login))
    }
}

#[derive(Clone)]
pub struct AliasStore {
    pool: PgPool,
}

impl AliasStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Sucht die Alias-Gruppe eines Accounts. `None` = der Account hat keine
    /// eingetragenen Zweit-Accounts, dann gilt er wie bisher als er selbst.
    ///
    /// Die Suche geht über ID **oder** Login, weil je nach Ereignis nur eins
    /// davon vorliegt.
    pub async fn group_for(
        &self,
        user_id: &str,
        login: &str,
    ) -> Result<Option<AliasGroup>, sqlx::Error> {
        let user_id = user_id.trim();
        let login = login.trim().trim_start_matches('@').to_lowercase();
        if user_id.is_empty() && login.is_empty() {
            return Ok(None);
        }

        let rows = sqlx::query!(
            r#"
            SELECT twitch_user_id AS "twitch_user_id!",
                   LOWER(twitch_login) AS "twitch_login!",
                   person_key AS "person_key!",
                   is_primary AS "is_primary!"
            FROM twitch_streamer_aliases
            WHERE person_key = (
                SELECT person_key FROM twitch_streamer_aliases
                WHERE twitch_user_id = $1 OR LOWER(twitch_login) = $2
                LIMIT 1
            )
            ORDER BY is_primary DESC, twitch_login
            "#,
            user_id,
            login,
        )
        .fetch_all(&self.pool)
        .await?;

        if rows.is_empty() {
            return Ok(None);
        }

        let person_key = rows[0].person_key.clone();
        let primary_user_id = rows
            .iter()
            .find(|row| row.is_primary)
            .map(|row| row.twitch_user_id.clone());
        Ok(Some(AliasGroup {
            person_key,
            user_ids: rows.iter().map(|r| r.twitch_user_id.clone()).collect(),
            logins: rows.iter().map(|r| r.twitch_login.clone()).collect(),
            primary_user_id,
        }))
    }

    /// Trägt einen Account unter einem `person_key` ein oder verschiebt ihn
    /// dorthin. Idempotent.
    pub async fn upsert(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        person_key: &str,
        is_primary: bool,
        note: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO twitch_streamer_aliases
                (twitch_user_id, twitch_login, person_key, is_primary, note)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (twitch_user_id) DO UPDATE SET
                twitch_login = EXCLUDED.twitch_login,
                person_key   = EXCLUDED.person_key,
                is_primary   = EXCLUDED.is_primary,
                note         = EXCLUDED.note
            "#,
            twitch_user_id.trim(),
            twitch_login.trim().trim_start_matches('@').to_lowercase(),
            person_key.trim().to_lowercase(),
            is_primary,
            note,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Entfernt einen Account aus dem Mapping.
    pub async fn remove(&self, twitch_user_id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query!(
            "DELETE FROM twitch_streamer_aliases WHERE twitch_user_id = $1",
            twitch_user_id.trim(),
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> AliasGroup {
        AliasGroup {
            person_key: "denoshick".to_string(),
            user_ids: vec!["111".to_string(), "222".to_string()],
            logins: vec!["denoshick".to_string(), "denoshick2".to_string()],
            primary_user_id: Some("111".to_string()),
        }
    }

    #[test]
    fn gruppe_erkennt_beide_accounts_per_id() {
        assert!(group().contains("111", ""));
        assert!(group().contains("222", ""));
        assert!(!group().contains("333", ""));
    }

    #[test]
    fn gruppe_erkennt_beide_accounts_per_login() {
        assert!(group().contains("", "denoshick"));
        assert!(group().contains("", "denoshick2"));
        assert!(!group().contains("", "jemand_anders"));
    }

    #[test]
    fn login_vergleich_ignoriert_schreibweise_und_mention() {
        assert!(group().contains("", "@DenoShick2"));
        assert!(group().contains("", "  DENOSHICK  "));
    }

    #[test]
    fn leere_angaben_matchen_nie() {
        assert!(!group().contains("", ""));
        assert!(!group().contains("  ", " @ "));
    }

    #[test]
    fn eine_passende_angabe_reicht() {
        // Twitch liefert je nach Ereignis mal nur die ID, mal nur den Login.
        assert!(group().contains("222", "voellig_fremder_login"));
        assert!(group().contains("999", "denoshick"));
    }
}
