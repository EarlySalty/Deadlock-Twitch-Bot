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

    pub async fn source_language(
        &self,
        source_user_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let community = tb_db::guild_membership::community_guild_id()?;
        let Some(home) =
            tb_db::guild_membership::home_guild_id(&self.pool, source_user_id, community).await?
        else {
            return Ok(None);
        };
        let language: Option<String> = sqlx::query_scalar(
            "SELECT COALESCE(NULLIF(i.broadcaster_language, ''), g.language)
             FROM twitch_streamer_identities i CROSS JOIN discord_guilds g
             WHERE i.twitch_user_id=$1 AND g.guild_id=$2",
        )
        .bind(source_user_id)
        .bind(home)
        .fetch_optional(&self.pool)
        .await?;
        Ok(language
            .as_deref()
            .and_then(tb_db::guild_membership::language_group))
    }

    /// Lädt alle aktiven Partner außer der Quelle. Zielkandidaten werden nur
    /// durch `is_partner_active` gegatet; die Raid-Toggles gehoeren zur Quelle.
    pub async fn load_roster(
        &self,
        source_user_id: &str,
    ) -> Result<Vec<PartnerRosterEntry>, sqlx::Error> {
        let community = tb_db::guild_membership::community_guild_id()?;
        let Some(home) =
            tb_db::guild_membership::home_guild_id(&self.pool, source_user_id, community).await?
        else {
            return Ok(Vec::new());
        };
        let source_language = self.source_language(source_user_id).await?;
        let source_guilds: Vec<(i64, bool)> = sqlx::query_as(
            "SELECT sg.guild_id, g.cross_server_raids FROM streamer_guilds sg JOIN discord_guilds g USING(guild_id)
             WHERE sg.twitch_user_id=$1 AND sg.active AND g.status='active' AND g.removed_at IS NULL")
            .bind(source_user_id).fetch_all(&self.pool).await?;
        let candidates: Vec<(String, String, String, i64, bool)> = sqlx::query_as(
            "SELECT DISTINCT i.twitch_login, i.twitch_user_id,
                    COALESCE(NULLIF(i.broadcaster_language, ''), home.language), sg.guild_id, g.cross_server_raids
             FROM twitch_streamer_identities i
             JOIN partner_guild_active p USING(twitch_user_id)
             CROSS JOIN LATERAL (
                 SELECT hg.language FROM streamer_guilds hs JOIN discord_guilds hg USING(guild_id)
                 WHERE hs.twitch_user_id=i.twitch_user_id AND hs.active AND hg.status='active' AND hg.removed_at IS NULL
                 ORDER BY (hs.guild_id=$2) DESC, hs.joined_at, hs.guild_id LIMIT 1
             ) home
             JOIN streamer_guilds sg USING(twitch_user_id)
             JOIN discord_guilds g USING(guild_id)
             WHERE sg.active AND g.status='active' AND g.removed_at IS NULL
               AND i.twitch_user_id <> $1 AND i.twitch_login IS NOT NULL")
            .bind(source_user_id).bind(community).fetch_all(&self.pool).await?;
        let mut grouped: HashMap<String, (String, bool, Vec<(i64, bool)>)> = HashMap::new();
        for (login, id, language, guild, enabled) in candidates {
            let same_language = source_language.is_some()
                && tb_db::guild_membership::language_group(&language) == source_language;
            let entry = grouped
                .entry(id)
                .or_insert_with(|| (login, same_language, Vec::new()));
            entry.2.push((guild, enabled));
        }
        let mut rows = Vec::new();
        for (id, (login, same_language, guilds)) in grouped {
            if tb_db::guild_membership::raid_scope_allows(
                &source_guilds,
                home,
                &guilds,
                same_language,
            ) {
                rows.push(RosterRow {
                    twitch_login: Some(login),
                    twitch_user_id: Some(id),
                });
            }
        }
        rows.sort_by(|a, b| a.twitch_user_id.cmp(&b.twitch_user_id));

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
