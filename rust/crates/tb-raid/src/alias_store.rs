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
///
/// **Identität hängt ausschließlich an der Twitch-User-ID.** Logins sind bei
/// Twitch nicht dauerhaft: gibt jemand seinen Namen auf, kann ihn ein Fremder
/// übernehmen, und ein Namens-Match würde diesen Fremden als Zweit-Account
/// durchgehen lassen. Die IDs bleiben dagegen für immer an einem Konto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasGroup {
    /// Klammer über die Accounts: die Twitch-User-ID des Hauptaccounts.
    pub person_key: String,
    /// Twitch-User-IDs aller Accounts.
    pub user_ids: Vec<String>,
    /// Logins aller Accounts zum Zeitpunkt des Eintrags, klein geschrieben.
    /// Nur für Logs und Anzeige, **nie** für die Zuordnung.
    pub logins: Vec<String>,
    /// Account, an den Whispers gehen. `None` = keiner als Haupt markiert,
    /// dann bleibt es beim raidenden Account.
    pub primary_user_id: Option<String>,
}

impl AliasGroup {
    /// Ob dieser Account zur Gruppe gehört. Vergleicht **nur** die
    /// Twitch-User-ID; ein leerer oder unbekannter Wert gehört nie dazu.
    pub fn contains(&self, user_id: &str) -> bool {
        let user_id = user_id.trim();
        !user_id.is_empty() && self.user_ids.iter().any(|id| id == user_id)
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

    /// Sucht die Alias-Gruppe eines Accounts anhand seiner Twitch-User-ID.
    /// `None` = keine Zweit-Accounts eingetragen, dann gilt der Account wie
    /// bisher als er selbst.
    ///
    /// Bewusst keine Login-Suche: siehe [`AliasGroup`].
    pub async fn group_for(&self, user_id: &str) -> Result<Option<AliasGroup>, sqlx::Error> {
        let user_id = user_id.trim();
        if user_id.is_empty() {
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
                WHERE twitch_user_id = $1
                LIMIT 1
            )
            ORDER BY is_primary DESC, twitch_login
            "#,
            user_id,
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
    ///
    /// `person_key` ist die Twitch-User-ID des Hauptaccounts, kein Login.
    /// `twitch_login` wird nur mitgeführt, damit Logs und Dashboard lesbar
    /// bleiben; für die Zuordnung zählt er nicht.
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
            person_key.trim(),
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
        // denoshock (993954638) streamt, inderwoche (93677289) schreibt mit.
        AliasGroup {
            person_key: "993954638".to_string(),
            user_ids: vec!["993954638".to_string(), "93677289".to_string()],
            logins: vec!["denoshock".to_string(), "inderwoche".to_string()],
            primary_user_id: Some("993954638".to_string()),
        }
    }

    #[test]
    fn gruppe_erkennt_beide_accounts_an_der_id() {
        assert!(group().contains("993954638"));
        assert!(group().contains("93677289"));
        assert!(!group().contains("333"));
    }

    #[test]
    fn logins_zaehlen_nie_als_identitaet() {
        // Twitch gibt aufgegebene Namen wieder frei. Ein Namens-Match würde
        // den nächsten Inhaber als Zweit-Account durchgehen lassen.
        assert!(!group().contains("inderwoche"));
        assert!(!group().contains("denoshock"));
    }

    #[test]
    fn leere_id_matcht_nie() {
        assert!(!group().contains(""));
        assert!(!group().contains("   "));
    }

    #[test]
    fn umgebender_leerraum_stoert_die_id_nicht() {
        assert!(group().contains("  93677289 "));
    }
}
