//! Partner-Roster-Reader + Online-Kandidaten-Aufbau für den Auto-Raid.
//! Port von `raid/services/raid_data_sources.py`
//! `load_partner_roster_for_raid` (Z. 203) + `build_online_partner_candidates` (Z. 240).
//!
//! Prod-Schema: `twitch_streamers_partner_state.is_partner_active` ist **INTEGER**
//! (=1 aktiv). Raid/Auth-Gates gelten nur fuer die Quelle; Ziele brauchen kein
//! eigenes `raid_enabled`.

use std::collections::HashMap;

use sqlx::PgPool;

/// Ein raid-fähiger Partner aus dem Roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartnerRosterEntry {
    pub twitch_login: String,
    pub twitch_user_id: String,
    pub raid_enabled: bool,
}

/// Stream-Daten eines live Partners (Eingabe für den Kandidaten-Aufbau).
#[derive(Debug, Clone, Default)]
pub struct StreamData {
    pub viewer_count: i32,
    pub followers_total: i32,
    pub started_at: Option<String>,
    pub game_name: Option<String>,
}

/// Ein live Partner-Kandidat (Roster-Eintrag + Stream-Daten).
#[derive(Debug, Clone)]
pub struct OnlineCandidate {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub raid_enabled: bool,
    pub stream: StreamData,
}

#[derive(sqlx::FromRow)]
struct RosterRow {
    twitch_login: Option<String>,
    twitch_user_id: Option<String>,
}

#[derive(Clone)]
pub struct PartnerRosterStore {
    pool: PgPool,
}

impl PartnerRosterStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Lädt alle aktiven Partner außer der Quelle. Zielkandidaten werden nur
    /// durch `is_partner_active` gegatet; die Raid-Toggles gehoeren zur Quelle.
    pub async fn load_roster(
        &self,
        source_user_id: &str,
    ) -> Result<Vec<PartnerRosterEntry>, sqlx::Error> {
        let rows: Vec<RosterRow> = sqlx::query_as!(
            RosterRow,
            r#"
            SELECT DISTINCT
                   s.twitch_login AS "twitch_login?",
                   s.twitch_user_id AS "twitch_user_id?"
              FROM twitch_streamers_partner_state s
             WHERE s.is_partner_active = 1
               AND s.twitch_user_id IS NOT NULL
               AND s.twitch_login IS NOT NULL
               AND s.twitch_user_id <> $1
            "#,
            source_user_id
        )
        .fetch_all(&self.pool)
        .await?;

        let mut partners = Vec::new();
        for row in rows {
            let login = row.twitch_login.unwrap_or_default().trim().to_lowercase();
            let user_id = row.twitch_user_id.unwrap_or_default().trim().to_string();
            if login.is_empty() || user_id.is_empty() {
                continue;
            }
            partners.push(PartnerRosterEntry {
                twitch_login: login,
                twitch_user_id: user_id,
                raid_enabled: true,
            });
        }
        Ok(partners)
    }
}

/// Baut aus dem Roster + den live Streams die Online-Kandidaten — nur Partner,
/// die gerade streamen (Stream-Daten vorhanden). Reine Funktion (Python Z. 240).
pub fn build_online_candidates(
    roster: &[PartnerRosterEntry],
    streams_by_login: &HashMap<String, StreamData>,
) -> Vec<OnlineCandidate> {
    roster
        .iter()
        .filter_map(|partner| {
            let stream = streams_by_login.get(&partner.twitch_login)?;
            Some(OnlineCandidate {
                twitch_user_id: partner.twitch_user_id.clone(),
                twitch_login: partner.twitch_login.clone(),
                raid_enabled: partner.raid_enabled,
                stream: stream.clone(),
            })
        })
        .collect()
}
