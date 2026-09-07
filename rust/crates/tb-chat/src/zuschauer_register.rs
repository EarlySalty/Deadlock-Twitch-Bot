use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use unicode_normalization::UnicodeNormalization;

use crate::mention_scoring::WHITELISTED_BOTS;
use crate::types::ChatMessageEvent;

pub const PRIOR_COMMUNITY: f64 = 0.8;
pub const PRIOR_PARTNER: f64 = 0.2;
pub const GATE_MAX_P: f64 = 0.35;

const NAMENS_SCORE_EXACT_UNIQUE: f64 = 0.90;
const NAMENS_SCORE_EXACT_AMBIG: f64 = 0.50;
const NAMENS_SCORE_HIGH: f64 = 0.60;
const NAMENS_SCORE_MID: f64 = 0.10;
const RATIO_HIGH: f64 = 0.93;
const RATIO_MID: f64 = 0.82;

const COMMUNITY_CHANNEL: &str = "dach_lock";
const MEMBER_INDEX_TTL: Duration = Duration::from_secs(3600);
const MEMBER_INDEX_NEGATIVE_TTL: Duration = Duration::from_secs(60);
const REFRESH_AFTER: Duration = Duration::from_secs(7 * 86400);

static AFFIXES: &[&str] = &[
    "ttv", "live", "twitch", "stream", "streams", "streamer", "yt", "youtube", "tv", "official",
    "real", "the", "its", "im", "iam", "gg",
];

fn leet_replace(c: char) -> char {
    match c {
        '0' => 'o',
        '1' => 'i',
        '3' => 'e',
        '4' => 'a',
        '5' => 's',
        '7' => 't',
        '$' => 's',
        '@' => 'a',
        '8' => 'b',
        _ => c,
    }
}

fn norm_key(value: &str) -> String {
    let ascii: String = value
        .nfkd()
        .filter(|c| c.is_ascii())
        .flat_map(|c| c.to_lowercase())
        .map(leet_replace)
        .collect();

    let tokens: Vec<&str> = {
        let mut start = 0;
        let mut result = Vec::new();
        let bytes = ascii.as_bytes();
        let mut i = 0;
        while i <= bytes.len() {
            let boundary = i == bytes.len() || !(bytes[i].is_ascii_alphanumeric());
            if boundary {
                if i > start {
                    result.push(&ascii[start..i]);
                }
                start = i + 1;
            }
            i += 1;
        }
        result
    };

    let kept: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|t| !AFFIXES.contains(t))
        .collect();

    let used = if kept.is_empty() { &tokens } else { &kept };
    used.join("")
}

fn similarity(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    strsim::jaro_winkler(a, b)
}

#[derive(Debug, Clone)]
pub struct MemberLite {
    pub id: String,
    pub name: String,
    pub global_name: Option<String>,
    pub nick: Option<String>,
}

pub struct MemberIndex {
    exact: HashMap<String, Vec<MemberLite>>,
}

impl MemberIndex {
    pub fn build(members: &[MemberLite]) -> Self {
        let mut exact: HashMap<String, Vec<MemberLite>> = HashMap::new();
        for m in members {
            let keys = [
                norm_key(&m.name),
                m.global_name.as_deref().map(norm_key).unwrap_or_default(),
                m.nick.as_deref().map(norm_key).unwrap_or_default(),
            ];
            let unique_keys: HashSet<_> = keys.iter().filter(|k| !k.is_empty()).collect();
            for key in unique_keys {
                exact.entry(key.clone()).or_default().push(m.clone());
            }
        }
        Self { exact }
    }

    fn best_match(&self, login_key: &str) -> Option<(MemberLite, f64, bool)> {
        if let Some(members) = self.exact.get(login_key) {
            let exact_unique = members.len() == 1;
            return Some((members[0].clone(), 1.0, exact_unique));
        }
        let mut best: Option<(MemberLite, f64)> = None;
        for (key, members) in &self.exact {
            let ratio = similarity(login_key, key);
            if ratio > best.as_ref().map(|(_, r)| *r).unwrap_or(0.0) {
                best = Some((members[0].clone(), ratio));
            }
        }
        best.map(|(m, r)| (m, r, false))
    }
}

fn prior_quelle(prior: f64) -> &'static str {
    if (prior - PRIOR_COMMUNITY).abs() < f64::EPSILON {
        "dach_lock"
    } else {
        "partner"
    }
}

fn matched_field(login_key: &str, m: &MemberLite) -> String {
    let candidates = [
        ("username", norm_key(&m.name)),
        (
            "global_name",
            m.global_name.as_deref().map(norm_key).unwrap_or_default(),
        ),
        ("nick", m.nick.as_deref().map(norm_key).unwrap_or_default()),
    ];
    let mut best = ("username", -1.0);
    for (label, key) in &candidates {
        if key.is_empty() {
            continue;
        }
        let r = similarity(login_key, key);
        if r > best.1 {
            best = (label, r);
        }
    }
    best.0.to_string()
}

pub fn score(
    twitch_login: &str,
    index: &MemberIndex,
    prior: f64,
    hard_discord_id: Option<&str>,
) -> (f64, Option<String>, serde_json::Value) {
    if let Some(discord_id) = hard_discord_id {
        let signals = serde_json::json!({
            "stufe": "hart",
            "hard_match": true,
            "namens_score": 1.0,
            "namens_quelle": serde_json::Value::Null,
            "match_ratio": 1.0,
            "prior": prior,
            "prior_quelle": prior_quelle(prior),
            "discord_user_id": discord_id,
        });
        return (1.0, Some(discord_id.to_string()), signals);
    }

    let login_key = norm_key(twitch_login);
    let mut namens_score = 0.0;
    let mut discord_id: Option<String> = None;
    let mut namens_quelle = serde_json::Value::Null;
    let mut match_ratio = 0.0;
    let mut stufe = "kein_treffer";

    if !login_key.is_empty() {
        if let Some((member, ratio, exact_unique)) = index.best_match(&login_key) {
            if (ratio - 1.0).abs() < f64::EPSILON {
                match_ratio = ratio;
                namens_quelle = serde_json::Value::String(matched_field(&login_key, &member));
                if exact_unique {
                    namens_score = NAMENS_SCORE_EXACT_UNIQUE;
                    discord_id = Some(member.id.clone());
                    stufe = "exakt_eindeutig";
                } else {
                    namens_score = NAMENS_SCORE_EXACT_AMBIG;
                    stufe = "exakt_mehrdeutig";
                }
            } else if ratio >= RATIO_HIGH {
                match_ratio = ratio;
                namens_score = NAMENS_SCORE_HIGH;
                discord_id = Some(member.id.clone());
                namens_quelle = serde_json::Value::String(matched_field(&login_key, &member));
                stufe = "high";
            } else if ratio >= RATIO_MID {
                match_ratio = ratio;
                namens_score = NAMENS_SCORE_MID;
                namens_quelle = serde_json::Value::String(matched_field(&login_key, &member));
                stufe = "mid";
            }
        }
    }

    let p = 1.0 - (1.0 - prior) * (1.0 - namens_score);
    let signals = serde_json::json!({
        "stufe": stufe,
        "hard_match": false,
        "namens_score": namens_score,
        "namens_quelle": namens_quelle,
        "match_ratio": match_ratio,
        "prior": prior,
        "prior_quelle": prior_quelle(prior),
        "discord_user_id": discord_id.clone(),
    });
    (p, discord_id, signals)
}

#[derive(Debug, Clone)]
pub struct RegisterEntry {
    pub twitch_user_id: String,
    pub twitch_login: Option<String>,
    pub discord_user_id: Option<String>,
    pub p: f64,
    pub signals: serde_json::Value,
    pub first_partner_channel: Option<String>,
    pub first_seen_at: Option<DateTime<Utc>>,
    pub computed_at: DateTime<Utc>,
}

#[async_trait]
pub trait MemberIndexSource: Send + Sync {
    async fn fetch_members(&self) -> Option<Vec<MemberLite>>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum GateOutcome {
    Pass,
    Reject(&'static str),
}

struct MemberIndexCacheState {
    built_at: Instant,
    index: Arc<MemberIndex>,
}

pub struct ZuschauerRegister {
    pool: PgPool,
    member_source: Arc<dyn MemberIndexSource>,
    cache: tokio::sync::Mutex<Option<MemberIndexCacheState>>,
    fetch_fail_at: tokio::sync::Mutex<Option<Instant>>,
}

impl ZuschauerRegister {
    pub fn new(pool: PgPool, member_source: Arc<dyn MemberIndexSource>) -> Self {
        Self {
            pool,
            member_source,
            cache: tokio::sync::Mutex::new(None),
            fetch_fail_at: tokio::sync::Mutex::new(None),
        }
    }

    pub async fn member_index(&self) -> Option<Arc<MemberIndex>> {
        {
            let guard = self.cache.lock().await;
            if let Some(state) = guard.as_ref() {
                if state.built_at.elapsed() < MEMBER_INDEX_TTL {
                    return Some(state.index.clone());
                }
            }
        }
        {
            let fail_guard = self.fetch_fail_at.lock().await;
            if let Some(failed_at) = fail_guard.as_ref() {
                if failed_at.elapsed() < MEMBER_INDEX_NEGATIVE_TTL {
                    let guard = self.cache.lock().await;
                    return guard.as_ref().map(|s| s.index.clone());
                }
            }
        }
        match self.member_source.fetch_members().await {
            Some(members) => {
                let index = Arc::new(MemberIndex::build(&members));
                {
                    let mut guard = self.cache.lock().await;
                    *guard = Some(MemberIndexCacheState {
                        built_at: Instant::now(),
                        index: index.clone(),
                    });
                }
                *self.fetch_fail_at.lock().await = None;
                Some(index)
            }
            None => {
                *self.fetch_fail_at.lock().await = Some(Instant::now());
                let guard = self.cache.lock().await;
                guard.as_ref().map(|s| s.index.clone())
            }
        }
    }

    pub async fn load(&self, twitch_user_id: &str) -> Result<Option<RegisterEntry>, sqlx::Error> {
        let row = sqlx::query!(
            r#"SELECT twitch_user_id, twitch_login, discord_user_id,
                      community_probability, signals::text AS "signals!",
                      first_partner_channel, first_seen_at, computed_at
                 FROM twitch_zuschauer_register
                WHERE twitch_user_id = $1"#,
            twitch_user_id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| RegisterEntry {
            twitch_user_id: row.twitch_user_id,
            twitch_login: row.twitch_login,
            discord_user_id: row.discord_user_id,
            p: row.community_probability,
            signals: serde_json::from_str(&row.signals)
                .unwrap_or_else(|_| serde_json::json!({})),
            first_partner_channel: row.first_partner_channel,
            first_seen_at: row.first_seen_at,
            computed_at: row.computed_at,
        }))
    }

    pub async fn upsert(&self, entry: &RegisterEntry) -> bool {
        let signals_text =
            serde_json::to_string(&entry.signals).unwrap_or_else(|_| "{}".to_string());
        match sqlx::query!(
            r#"INSERT INTO twitch_zuschauer_register
                 (twitch_user_id, twitch_login, discord_user_id, community_probability,
                  signals, first_partner_channel, first_seen_at, computed_at)
               VALUES ($1, $2, $3, $4, $5::text::jsonb, $6, $7, $8)
               ON CONFLICT (twitch_user_id) DO UPDATE SET
                 twitch_login = EXCLUDED.twitch_login,
                 discord_user_id = EXCLUDED.discord_user_id,
                 community_probability = EXCLUDED.community_probability,
                 signals = EXCLUDED.signals,
                 first_partner_channel = COALESCE(
                     twitch_zuschauer_register.first_partner_channel,
                     EXCLUDED.first_partner_channel
                 ),
                 first_seen_at = COALESCE(
                     twitch_zuschauer_register.first_seen_at,
                     EXCLUDED.first_seen_at
                 ),
                 computed_at = EXCLUDED.computed_at"#,
            entry.twitch_user_id,
            entry.twitch_login,
            entry.discord_user_id,
            entry.p,
            signals_text,
            entry.first_partner_channel,
            entry.first_seen_at,
            entry.computed_at,
        )
        .execute(&self.pool)
        .await
        {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(%error, "zuschauer-register: upsert fehlgeschlagen");
                false
            }
        }
    }

    async fn hard_discord_id(&self, twitch_user_id: &str) -> Option<String> {
        sqlx::query_scalar!(
            r#"SELECT discord_user_id FROM twitch_streamer_identities
                WHERE twitch_user_id = $1 AND discord_user_id IS NOT NULL
                LIMIT 1"#,
            twitch_user_id,
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .flatten()
    }

    fn prior_for_channel(&self, channel_login: &str) -> f64 {
        if channel_login.eq_ignore_ascii_case(COMMUNITY_CHANNEL) {
            PRIOR_COMMUNITY
        } else {
            PRIOR_PARTNER
        }
    }

    pub async fn ensure_current(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        channel_login: &str,
    ) -> Option<RegisterEntry> {
        let existing = match self.load(twitch_user_id).await {
            Ok(existing) => existing,
            Err(error) => {
                tracing::warn!(%error, "zuschauer-register: laden fehlgeschlagen, blockiere");
                return None;
            }
        };

        let now = Utc::now();

        if let Some(entry) = existing {
            let age = (now - entry.computed_at).num_seconds();
            let fresh = age >= 0 && (age as u64) < REFRESH_AFTER.as_secs();
            let erstes_auftauchen = entry.first_seen_at.is_none();

            if fresh && !erstes_auftauchen {
                return Some(entry);
            }

            let (p, discord_id, signals, computed_at) = if fresh {
                let prior = self.prior_for_channel(channel_login);
                let namens_score = entry
                    .signals
                    .get("namens_score")
                    .and_then(|value| value.as_f64())
                    .unwrap_or(0.0);
                let p = 1.0 - (1.0 - prior) * (1.0 - namens_score);
                let mut signals = entry.signals.clone();
                if let Some(obj) = signals.as_object_mut() {
                    obj.insert("prior".to_string(), serde_json::json!(prior));
                    obj.insert(
                        "prior_quelle".to_string(),
                        serde_json::json!(prior_quelle(prior)),
                    );
                }
                (p, entry.discord_user_id.clone(), signals, entry.computed_at)
            } else {
                let index = self.member_index().await?;
                let hard = self.hard_discord_id(twitch_user_id).await;
                let prior = self.prior_for_channel(channel_login);
                let (p, discord_id, signals) = score(twitch_login, &index, prior, hard.as_deref());
                (p, discord_id.or(entry.discord_user_id.clone()), signals, now)
            };

            let updated = RegisterEntry {
                twitch_user_id: entry.twitch_user_id.clone(),
                twitch_login: Some(twitch_login.to_string()),
                discord_user_id: discord_id,
                p,
                signals,
                first_partner_channel: entry
                    .first_partner_channel
                    .clone()
                    .or_else(|| Some(channel_login.to_string())),
                first_seen_at: entry.first_seen_at.or(Some(now)),
                computed_at,
            };
            if !self.upsert(&updated).await {
                return None;
            }
            return Some(updated);
        }

        let index = self.member_index().await?;
        let hard = self.hard_discord_id(twitch_user_id).await;
        let prior = self.prior_for_channel(channel_login);
        let (p, discord_id, signals) = score(twitch_login, &index, prior, hard.as_deref());
        let entry = RegisterEntry {
            twitch_user_id: twitch_user_id.to_string(),
            twitch_login: Some(twitch_login.to_string()),
            discord_user_id: discord_id,
            p,
            signals,
            first_partner_channel: Some(channel_login.to_string()),
            first_seen_at: Some(now),
            computed_at: now,
        };
        if !self.upsert(&entry).await {
            return None;
        }
        Some(entry)
    }

    pub async fn current_session(
        &self,
        channel_login: &str,
    ) -> Result<Option<(i64, DateTime<Utc>)>, sqlx::Error> {
        sqlx::query!(
            r#"SELECT s.id AS "id!", s.started_at AS "started_at!"
                 FROM twitch_stream_sessions s
                 JOIN twitch_live_state ls ON ls.active_session_id = s.id
                WHERE LOWER(ls.streamer_login) = LOWER($1) AND ls.is_live = 1
                LIMIT 1"#,
            channel_login,
        )
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(|row| (row.id, row.started_at)))
    }

    async fn is_excluded(&self, twitch_user_id: &str) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS (
                 SELECT 1 FROM twitch_partners WHERE twitch_user_id = $1
                 UNION ALL SELECT 1 FROM twitch_streamers WHERE twitch_user_id = $1
                 UNION ALL SELECT 1 FROM twitch_raid_auth WHERE twitch_user_id = $1
                 UNION ALL SELECT 1 FROM twitch_partner_signup_denylist WHERE twitch_user_id = $1
                 UNION ALL SELECT 1 FROM twitch_scout_pitch_blacklist WHERE twitch_user_id = $1
                 UNION ALL SELECT 1 FROM twitch_partner_outreach
                     WHERE twitch_user_id = $1 AND contacted_at IS NOT NULL
               ) AS "exists!""#,
            twitch_user_id,
        )
        .fetch_one(&self.pool)
        .await
    }

    pub async fn gate(&self, event: &ChatMessageEvent) -> GateOutcome {
        let login = event.chatter_user_login.to_lowercase();
        if event.is_mod_or_broadcaster() || WHITELISTED_BOTS.contains(&login.as_str()) {
            return GateOutcome::Reject("broadcaster_mod_bot");
        }

        let channel_login = &event.broadcaster_user_login;
        let Some(entry) = self
            .ensure_current(&event.chatter_user_id, &event.chatter_user_login, channel_login)
            .await
        else {
            return GateOutcome::Reject("register_fehlt");
        };

        if entry.p >= GATE_MAX_P {
            return GateOutcome::Reject("register_community");
        }

        match self.current_session(channel_login).await {
            Ok(Some((_, session_start))) => match entry.first_seen_at {
                Some(first_seen) if first_seen < session_start => {
                    return GateOutcome::Reject("kein_neuling");
                }
                None => return GateOutcome::Reject("kein_neuling"),
                _ => {}
            },
            Ok(None) => return GateOutcome::Reject("kein_neuling"),
            Err(error) => {
                tracing::warn!(%error, "zuschauer-register: session-start nicht lesbar, blockiere");
                return GateOutcome::Reject("kein_neuling");
            }
        }

        match self.is_excluded(&event.chatter_user_id).await {
            Ok(true) => return GateOutcome::Reject("partner_oder_streamer"),
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(%error, "zuschauer-register: ausschluss nicht lesbar, blockiere");
                return GateOutcome::Reject("partner_oder_streamer");
            }
        }

        GateOutcome::Pass
    }

    pub async fn signals_for(&self, twitch_user_id: &str) -> Option<serde_json::Value> {
        self.load(twitch_user_id)
            .await
            .ok()
            .flatten()
            .map(|entry| serde_json::json!({ "p": entry.p, "signals": entry.signals }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, name: &str) -> MemberLite {
        MemberLite {
            id: id.to_string(),
            name: name.to_string(),
            global_name: None,
            nick: None,
        }
    }

    #[test]
    fn norm_key_gleich_wie_streamer_link() {
        assert_eq!(norm_key("EarlySaltyTTV"), "earlysaltyttv");
        assert_eq!(norm_key("Early_Salty_TTV"), "earlysalty");
        assert_eq!(norm_key("L33tN4me"), "leetname");
    }

    #[test]
    fn score_harte_zuordnung_ist_eins() {
        let index = MemberIndex::build(&[]);
        let (p, discord, signals) = score("egal", &index, PRIOR_PARTNER, Some("42"));
        assert_eq!(p, 1.0);
        assert_eq!(discord.as_deref(), Some("42"));
        assert_eq!(signals["hard_match"], serde_json::json!(true));
    }

    #[test]
    fn score_exakter_eindeutiger_treffer_hoch() {
        let index = MemberIndex::build(&[member("100", "earlysalty")]);
        let (p, discord, _signals) = score("earlysalty", &index, PRIOR_PARTNER, None);
        assert!(p > GATE_MAX_P, "erwartet p>{GATE_MAX_P}, war {p}");
        assert_eq!(discord.as_deref(), Some("100"));
    }

    #[test]
    fn score_kein_treffer_partnerkanal_niedrig() {
        let index = MemberIndex::build(&[member("100", "jemandanderes")]);
        let (p, discord, _signals) = score("voelligfremd", &index, PRIOR_PARTNER, None);
        assert!((p - PRIOR_PARTNER).abs() < 1e-9, "war {p}");
        assert!(discord.is_none());
    }

    #[test]
    fn score_community_prior_hoch() {
        let index = MemberIndex::build(&[]);
        let (p, _discord, _signals) = score("niemand", &index, PRIOR_COMMUNITY, None);
        assert!((p - PRIOR_COMMUNITY).abs() < 1e-9, "war {p}");
        assert!(p >= GATE_MAX_P);
    }

    #[test]
    fn score_kombination_formel() {
        let index = MemberIndex::build(&[member("100", "kombiname")]);
        let (p, _discord, _signals) = score("kombiname", &index, 0.5, None);
        let erwartet = 1.0 - (1.0 - 0.5) * (1.0 - NAMENS_SCORE_EXACT_UNIQUE);
        assert!((p - erwartet).abs() < 1e-9, "war {p}, erwartet {erwartet}");
    }

    #[test]
    fn member_index_exakt_mehrdeutig_gibt_niedrigeren_score() {
        let ambig = MemberIndex::build(&[member("1", "dax"), member("2", "dax")]);
        let (p_ambig, d1, s1) = score("dax", &ambig, PRIOR_PARTNER, None);
        let erwartet_ambig = 1.0 - (1.0 - PRIOR_PARTNER) * (1.0 - NAMENS_SCORE_EXACT_AMBIG);
        assert!((p_ambig - erwartet_ambig).abs() < 1e-9, "ambig p war {p_ambig}");
        assert!(d1.is_none(), "mehrdeutig darf keine Discord-ID setzen");
        assert_eq!(s1["stufe"], serde_json::json!("exakt_mehrdeutig"));

        let unique = MemberIndex::build(&[member("1", "dax")]);
        let (p_unique, _d2, _s2) = score("dax", &unique, PRIOR_PARTNER, None);
        assert!(p_ambig < p_unique, "ambig {p_ambig} sollte < unique {p_unique}");
    }

    #[test]
    fn score_beste_aehnlichkeit_075_ist_kein_signal() {
        let mut members = Vec::new();
        for i in 0..60u32 {
            let suffix = "abcdefghijklmnopqrstuvwxyz"
                .chars()
                .nth((i % 26) as usize)
                .unwrap();
            members.push(member(&format!("m{i}"), &format!("fremdwort{suffix}")));
        }
        members.push(member("999", "gzmeranxy"));
        let index = MemberIndex::build(&members);
        let (p, discord, signals) = score("gamername", &index, PRIOR_PARTNER, None);
        assert!(discord.is_none(), "0,75-Treffer darf keine Discord-ID liefern");
        assert_eq!(signals["stufe"], serde_json::json!("kein_treffer"));
        assert_eq!(signals["namens_score"], serde_json::json!(0.0));
        assert!((p - PRIOR_PARTNER).abs() < 1e-9, "kein Signal soll p=prior geben, war {p}");
        assert!(p < GATE_MAX_P, "kein Signal soll das Gate passieren, war {p}");
    }

    #[test]
    fn score_mid_ist_niedriges_signal_ohne_discord() {
        let index = MemberIndex::build(&[member("100", "mittalnaom")]);
        let (p, discord, signals) = score("mittelname", &index, PRIOR_PARTNER, None);
        assert!(discord.is_none(), "MID darf keine Discord-ID setzen");
        assert_eq!(signals["stufe"], serde_json::json!("mid"));
        assert_eq!(signals["namens_score"], serde_json::json!(NAMENS_SCORE_MID));
        let erwartet = 1.0 - (1.0 - PRIOR_PARTNER) * (1.0 - NAMENS_SCORE_MID);
        assert!((p - erwartet).abs() < 1e-9, "war {p}, erwartet {erwartet}");
        assert!(p < GATE_MAX_P, "MID mit Partner-Prior soll das Gate passieren, war {p}");
    }

    #[test]
    fn score_high_setzt_discord_und_lehnt_gate_ab() {
        let index = MemberIndex::build(&[member("100", "nanigxmer")]);
        let (p, discord, signals) = score("nanigamer", &index, PRIOR_PARTNER, None);
        assert_eq!(discord.as_deref(), Some("100"), "HIGH setzt die Discord-ID");
        assert_eq!(signals["stufe"], serde_json::json!("high"));
        assert_eq!(signals["namens_score"], serde_json::json!(NAMENS_SCORE_HIGH));
        let erwartet = 1.0 - (1.0 - PRIOR_PARTNER) * (1.0 - NAMENS_SCORE_HIGH);
        assert!((p - erwartet).abs() < 1e-9, "war {p}, erwartet {erwartet}");
        assert!((p - 0.68).abs() < 1e-9, "HIGH mit Partner-Prior soll p=0,68 geben, war {p}");
        assert!(p >= GATE_MAX_P, "HIGH soll das Gate ablehnen, war {p}");
    }
}
