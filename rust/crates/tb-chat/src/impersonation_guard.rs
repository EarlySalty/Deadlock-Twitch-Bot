use crate::api::ChatApi;
use crate::crew_guard::{registry_account, registry_impersonator};
use crate::global_chatter_ban::GlobalChatterBanEnforcer;
use crate::moderation::{ModerationEngine, ModerationEvidence};
use crate::promos::NoopSuppressionCheck;
use crate::suppression_guard::{ManualPartnerOptOutCheck, SuppressionGuardChatApi};
use crate::types::{ChatMessageEvent, SendOutcome};
use async_trait::async_trait;
use regex::Regex;
use sqlx::PgPool;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

pub const OWN_BOT_LOGIN: &str = "deutschedeadlockcommunity";
pub const TIMEOUT_SECS: u32 = 600;
pub const TIMEOUT_REASON: &str =
    "Gibt sich als Deutsche Deadlock Community aus und lädt zu privaten 6v6-Matches ein";
pub const PARTNER_STATEMENT: &str = "Hinweis der Deutschen Deadlock Community: {login} gehört nicht zu uns und gibt sich nur als uns aus. Wir laden niemanden per Chat zu privaten 6v6-Turnieren oder Matches ein. Bitte nicht darauf eingehen. Offiziell gibt es uns nur als DeutscheDeadlockCommunity auf Twitch, auf deutsche-deadlock-community.de und auf unserem Discord. Wenn ihr unsicher seid, fragt dort bei uns nach.";
pub const FOREIGN_WARNING: &str = "Hinweis der Deutschen Deadlock Community: Der Account {login} gehört nicht zu uns und ahmt uns nur nach. Unser offizieller Account ist DeutscheDeadlockCommunity. Wir laden niemanden per Chat zu privaten 6v6-Turnieren oder Matches ein. Offiziell gibt es uns nur auf Twitch, auf deutsche-deadlock-community.de und auf unserem Discord. Wenn ihr unsicher seid, fragt dort bei uns nach.";

const PARTNER_STATEMENT_COOLDOWN: Duration = Duration::from_secs(30 * 60);
const FOREIGN_WARNING_COOLDOWN: Duration = Duration::from_secs(12 * 60 * 60);
const FOREIGN_WARNING_WINDOW: Duration = Duration::from_secs(60 * 60);
const FOREIGN_WARNING_LIMIT: usize = 10;
const THROTTLE_MAX_KEYS: usize = 4096;
const ELLIPTICAL_OFFER_MAX_WORDS: usize = 4;
const CREW_SIGHTING_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);
const REGULAR_MIN_DAYS: i64 = 3;
const NO_CREW_SIGHTING: &str = "no_crew_sighting";
const FOREIGN_WARNING_THROTTLED: &str = "foreign_warning_throttled";
const REGULAR_DAYS_SQL: &str = "SELECT COUNT(DISTINCT day) FROM ( \
     SELECT (message_ts AT TIME ZONE 'Europe/Berlin')::date AS day FROM twitch_chat_messages \
      WHERE streamer_login = $1 AND chatter_login = $2 AND message_ts >= NOW() - INTERVAL '60 days' \
     UNION ALL \
     SELECT (sent_at AT TIME ZONE 'Europe/Berlin')::date AS day FROM category_chat_messages \
      WHERE room_user_id = $3 AND chatter_user_id = $4 AND sent_at >= NOW() - INTERVAL '60 days' \
     ) days WHERE day < (NOW() AT TIME ZONE 'Europe/Berlin')::date";

fn six_v_six() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b6\s*(v|vs\.?|versus|on|gegen|x)\s*6\b").ok())
        .as_ref()
}

fn private_cue() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"privat|turnier|tournament|scrim|lobby|custom|match").ok())
        .as_ref()
}

fn negated_interest() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(kein\w*|null)\s+(bock|lust)\b").ok())
        .as_ref()
}

fn invite_cue() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"\b(bock|lust|willst|wollt|kommst|mitspielen|mitmachen|mitzocken|joinen|join|dm|whisper|flüster\w*)\b",
            r"|\b(komm|kommt)\s+(mit|ihr|vorbei)\b",
            r"|\bwer\s+(will|kommt|hat\s+(bock|lust)|ist\s+dabei|macht\s+mit)\b",
            r"|\b(hast\s+du|habt\s+ihr|hat\s+wer|hat\s+jemand)\s+(bock|lust|zeit)\b",
            r"|\beinladung\w*\b",
            r"|\bdabei\s+sein\b",
            r"|\b(melde\s+dich|meldet\s+euch)\b",
            r"|\b(schreib|schreibt)\s+(mir|mich)\b",
        ))
        .ok()
    })
    .as_ref()
}

fn matches(re: Option<&'static Regex>, text: &str) -> bool {
    re.is_some_and(|re| re.is_match(text))
}

fn addresses_people(text: &str) -> bool {
    let affirmed = match negated_interest() {
        Some(re) => re.replace_all(text, " ").into_owned(),
        None => text.to_string(),
    };
    matches(invite_cue(), &affirmed)
}

fn elliptical_private_offer(text: &str) -> bool {
    let Some(re) = six_v_six() else {
        return false;
    };
    let compact = re.replace_all(text, "6v6");
    let trimmed = compact.trim_end();
    trimmed.ends_with('?')
        && trimmed.contains("privat")
        && trimmed.split_whitespace().count() <= ELLIPTICAL_OFFER_MAX_WORDS
}

pub fn private_6v6_pitch(text: &str) -> bool {
    let text = text.to_lowercase();
    matches(six_v_six(), &text)
        && matches(private_cue(), &text)
        && (addresses_people(&text) || elliptical_private_offer(&text))
}

pub fn lookalike_login(login: &str, bot_login: &str) -> bool {
    let login = login.trim().to_lowercase();
    if login == OWN_BOT_LOGIN || login == bot_login.trim().to_lowercase() {
        return false;
    }
    login.contains("deadlock")
        && ["german", "deutsch", "community"]
            .iter()
            .any(|cue| login.contains(cue))
}

pub fn channel_community(event: &ChatMessageEvent) -> bool {
    event.chatter_user_id == event.broadcaster_user_id
        || event
            .badges
            .iter()
            .any(|b| matches!(b.set_id.as_str(), "broadcaster" | "moderator" | "vip"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardMode {
    Partner,
    Foreign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImpersonationOutcome {
    pub action: &'static str,
    pub reason: &'static str,
}

impl ImpersonationOutcome {
    pub fn acted(&self) -> bool {
        !matches!(self.action, FOREIGN_WARNING_THROTTLED | NO_CREW_SIGHTING)
    }

    pub fn alert_text(&self) -> &'static str {
        match self.action {
            "timeout_statement" => "Timeout (10 Min.) gesetzt und Hinweis im Chat gepostet.",
            "timeout" => "Timeout (10 Min.) gesetzt, Hinweis kam in den letzten 30 Min. schon.",
            "statement" => "Timeout fehlgeschlagen, Hinweis im Chat gepostet.",
            "timeout_failed" => "Timeout und Hinweis sind fehlgeschlagen.",
            "foreign_warning" => "Warnhinweis im fremden Chat gepostet.",
            FOREIGN_WARNING_THROTTLED => {
                "Kein neuer Warnhinweis, Sperrfrist oder Stundenlimit greift."
            }
            NO_CREW_SIGHTING => {
                "Keine Aktion, in den letzten 24 Std. war kein bekannter Crew-Account in diesem Kanal."
            }
            _ => "Warnhinweis übersprungen, Senden nicht möglich.",
        }
    }
}

#[async_trait]
pub trait ImpersonationPort: Send + Sync {
    async fn timeout(&self, event: &ChatMessageEvent, duration_secs: u32, reason: &str) -> bool;
    async fn post(&self, event: &ChatMessageEvent, text: &str) -> bool;
    async fn banned_by_us(&self, event: &ChatMessageEvent) -> bool;
    async fn partner_channel(&self, event: &ChatMessageEvent) -> bool;
    async fn channel_regular(&self, event: &ChatMessageEvent) -> bool;
}

pub async fn channel_regular(pool: &PgPool, event: &ChatMessageEvent) -> bool {
    let days = sqlx::query_scalar::<_, i64>(REGULAR_DAYS_SQL)
        .bind(event.broadcaster_user_login.trim().to_lowercase())
        .bind(event.chatter_user_login.trim().to_lowercase())
        .bind(event.broadcaster_user_id.trim())
        .bind(event.chatter_user_id.trim())
        .fetch_one(pool)
        .await;
    match days {
        Ok(days) => days >= REGULAR_MIN_DAYS,
        Err(error) => {
            warn!(%error, "Impersonation-Guard: Chatverlauf nicht lesbar, Chatter gilt als Stammgast");
            true
        }
    }
}

pub struct PartnerActions {
    moderation: Arc<ModerationEngine>,
    api: Arc<dyn ChatApi>,
    opt_out: Arc<dyn ManualPartnerOptOutCheck>,
    pool: PgPool,
}

impl PartnerActions {
    pub fn new(
        moderation: Arc<ModerationEngine>,
        api: Arc<dyn ChatApi>,
        opt_out: Arc<dyn ManualPartnerOptOutCheck>,
        pool: PgPool,
    ) -> Self {
        Self {
            moderation,
            api,
            opt_out,
            pool,
        }
    }
}

#[async_trait]
impl ImpersonationPort for PartnerActions {
    async fn timeout(&self, event: &ChatMessageEvent, duration_secs: u32, reason: &str) -> bool {
        self.moderation
            .timeout_and_cleanup_with_evidence(
                &event.broadcaster_user_login.to_lowercase(),
                &event.broadcaster_user_id,
                &event.chatter_user_login.to_lowercase(),
                &event.chatter_user_id,
                &event.message_id,
                event.text(),
                duration_secs,
                reason,
                ModerationEvidence {
                    source_path: "impersonation",
                    reason,
                    score: None,
                    account_age_days: None,
                },
            )
            .await
    }

    async fn post(&self, event: &ChatMessageEvent, text: &str) -> bool {
        let api = SuppressionGuardChatApi::new(
            Arc::clone(&self.api),
            Arc::new(NoopSuppressionCheck),
            Arc::clone(&self.opt_out),
            "impersonation",
            event.broadcaster_user_login.as_str(),
        );
        sent(api.send_message(&event.broadcaster_user_id, text).await)
    }

    async fn banned_by_us(&self, _event: &ChatMessageEvent) -> bool {
        false
    }

    async fn partner_channel(&self, _event: &ChatMessageEvent) -> bool {
        true
    }

    async fn channel_regular(&self, event: &ChatMessageEvent) -> bool {
        channel_regular(&self.pool, event).await
    }
}

#[async_trait]
pub trait PartnerPipelineChannels: Send + Sync {
    async fn handled_by_partner_pipeline(&self, broadcaster_id: &str) -> bool;
}

pub struct ForeignActions {
    api: Option<Arc<dyn ChatApi>>,
    ban_list: Option<Arc<GlobalChatterBanEnforcer>>,
    partner_channels: Arc<dyn PartnerPipelineChannels>,
    pool: PgPool,
}

impl ForeignActions {
    pub fn new(
        api: Option<Arc<dyn ChatApi>>,
        ban_list: Option<Arc<GlobalChatterBanEnforcer>>,
        partner_channels: Arc<dyn PartnerPipelineChannels>,
        pool: PgPool,
    ) -> Self {
        Self {
            api,
            ban_list,
            partner_channels,
            pool,
        }
    }
}

#[async_trait]
impl ImpersonationPort for ForeignActions {
    async fn timeout(&self, _event: &ChatMessageEvent, _duration_secs: u32, _reason: &str) -> bool {
        false
    }

    async fn post(&self, event: &ChatMessageEvent, text: &str) -> bool {
        match &self.api {
            Some(api) => sent(api.send_message(&event.broadcaster_user_id, text).await),
            None => false,
        }
    }

    async fn banned_by_us(&self, event: &ChatMessageEvent) -> bool {
        match &self.ban_list {
            Some(ban_list) => ban_list.is_banned(event).await,
            None => false,
        }
    }

    async fn partner_channel(&self, event: &ChatMessageEvent) -> bool {
        self.partner_channels
            .handled_by_partner_pipeline(&event.broadcaster_user_id)
            .await
    }

    async fn channel_regular(&self, event: &ChatMessageEvent) -> bool {
        channel_regular(&self.pool, event).await
    }
}

fn sent(result: Result<SendOutcome, String>) -> bool {
    match result {
        Ok(SendOutcome::Sent) => true,
        Ok(outcome) => {
            warn!(?outcome, "Impersonation-Guard: Hinweis nicht zugestellt");
            false
        }
        Err(error) => {
            warn!(%error, "Impersonation-Guard: Hinweis nicht gesendet");
            false
        }
    }
}

#[derive(Default)]
struct Throttle {
    last: HashMap<(String, String), Instant>,
    order: VecDeque<(String, String)>,
    global: VecDeque<Instant>,
}

impl Throttle {
    fn reserve(
        &mut self,
        key: (String, String),
        cooldown: Duration,
        global_limit: Option<usize>,
        now: Instant,
    ) -> bool {
        if self
            .last
            .get(&key)
            .is_some_and(|at| now.duration_since(*at) < cooldown)
        {
            return false;
        }
        if let Some(limit) = global_limit {
            while self
                .global
                .front()
                .is_some_and(|at| now.duration_since(*at) >= FOREIGN_WARNING_WINDOW)
            {
                self.global.pop_front();
            }
            if self.global.len() >= limit {
                return false;
            }
            self.global.push_back(now);
        }
        if self.last.insert(key.clone(), now).is_none() {
            self.order.push_back(key);
            while self.order.len() > THROTTLE_MAX_KEYS {
                if let Some(oldest) = self.order.pop_front() {
                    self.last.remove(&oldest);
                }
            }
        }
        true
    }
}

#[derive(Default)]
struct Sightings {
    last: HashMap<String, Instant>,
}

impl Sightings {
    fn record(&mut self, channel: &str, now: Instant) {
        if self.last.len() >= THROTTLE_MAX_KEYS {
            self.last
                .retain(|_, at| now.duration_since(*at) < CREW_SIGHTING_WINDOW);
        }
        self.last.insert(channel.to_string(), now);
    }

    fn recent(&self, channel: &str, now: Instant) -> bool {
        self.last
            .get(channel)
            .is_some_and(|at| now.duration_since(*at) < CREW_SIGHTING_WINDOW)
    }
}

pub struct ImpersonationGuard {
    mode: GuardMode,
    bot_login: String,
    port: Arc<dyn ImpersonationPort>,
    throttle: Mutex<Throttle>,
    sightings: Mutex<Sightings>,
}

impl ImpersonationGuard {
    pub fn new(mode: GuardMode, bot_login: String, port: Arc<dyn ImpersonationPort>) -> Self {
        Self {
            mode,
            bot_login,
            port,
            throttle: Mutex::new(Throttle::default()),
            sightings: Mutex::new(Sightings::default()),
        }
    }

    async fn reason(&self, event: &ChatMessageEvent, trusted: bool) -> Option<&'static str> {
        let id = event.chatter_user_id.trim();
        if registry_impersonator(id) {
            return Some("Bekannter Account, der sich als Community ausgibt.");
        }
        let community = trusted
            || channel_community(event)
            || crate::safe_list::is_safe(Some(id), &event.chatter_user_login);
        if community || !private_6v6_pitch(event.text()) {
            return None;
        }
        match self.mode {
            GuardMode::Partner => Some("Einladung zu einem privaten 6v6-Match."),
            GuardMode::Foreign if lookalike_login(&event.chatter_user_login, &self.bot_login) => {
                Some("Nachahmer-Name mit Einladung zu einem privaten 6v6-Match.")
            }
            GuardMode::Foreign if self.port.banned_by_us(event).await => {
                Some("Von uns gebannter Account mit Einladung zu einem privaten 6v6-Match.")
            }
            GuardMode::Foreign => None,
        }
    }

    fn reserve(&self, event: &ChatMessageEvent) -> bool {
        let key = (
            event.broadcaster_user_id.clone(),
            event.chatter_user_id.clone(),
        );
        let (cooldown, limit) = match self.mode {
            GuardMode::Partner => (PARTNER_STATEMENT_COOLDOWN, None),
            GuardMode::Foreign => (FOREIGN_WARNING_COOLDOWN, Some(FOREIGN_WARNING_LIMIT)),
        };
        self.throttle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .reserve(key, cooldown, limit, Instant::now())
    }

    fn text(&self, event: &ChatMessageEvent) -> String {
        let template = match self.mode {
            GuardMode::Partner => PARTNER_STATEMENT,
            GuardMode::Foreign => FOREIGN_WARNING,
        };
        template.replace("{login}", &event.chatter_user_login)
    }

    pub async fn handle(
        &self,
        event: &ChatMessageEvent,
        trusted: bool,
    ) -> Option<ImpersonationOutcome> {
        let registry = registry_account(&event.chatter_user_id);
        if registry {
            self.sightings
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .record(&event.broadcaster_user_id, Instant::now());
        }
        let reason = self.reason(event, trusted).await?;
        if self.mode == GuardMode::Foreign && self.port.partner_channel(event).await {
            return None;
        }
        if !registry && self.port.channel_regular(event).await {
            return None;
        }
        let action = if !registry && !self.crew_sighted(event) {
            NO_CREW_SIGHTING
        } else {
            match self.mode {
                GuardMode::Partner => self.partner(event).await,
                GuardMode::Foreign => self.foreign(event).await,
            }
        };
        let outcome = ImpersonationOutcome { action, reason };
        if outcome.acted() {
            info!(
                channel = %event.broadcaster_user_login,
                chatter = %event.chatter_user_login,
                action,
                reason,
                "Impersonation-Guard entschieden"
            );
        } else {
            debug!(
                channel = %event.broadcaster_user_login,
                chatter = %event.chatter_user_login,
                action,
                reason,
                "Impersonation-Guard gedrosselt"
            );
        }
        Some(outcome)
    }

    fn crew_sighted(&self, event: &ChatMessageEvent) -> bool {
        self.sightings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recent(&event.broadcaster_user_id, Instant::now())
    }

    async fn partner(&self, event: &ChatMessageEvent) -> &'static str {
        let timed_out = self.port.timeout(event, TIMEOUT_SECS, TIMEOUT_REASON).await;
        let posted = self.reserve(event) && self.port.post(event, &self.text(event)).await;
        match (timed_out, posted) {
            (true, true) => "timeout_statement",
            (true, false) => "timeout",
            (false, true) => "statement",
            (false, false) => "timeout_failed",
        }
    }

    async fn foreign(&self, event: &ChatMessageEvent) -> &'static str {
        if !self.reserve(event) {
            return FOREIGN_WARNING_THROTTLED;
        }
        if self.port.post(event, &self.text(event)).await {
            "foreign_warning"
        } else {
            "foreign_warning_skipped"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatBadge, ChatMessageBody};

    #[derive(Default)]
    struct Recorder {
        timeouts: Mutex<Vec<u32>>,
        posts: Mutex<Vec<String>>,
        banned: Vec<String>,
        regulars: Vec<String>,
        partner: bool,
    }

    #[async_trait]
    impl ImpersonationPort for Recorder {
        async fn timeout(&self, _event: &ChatMessageEvent, duration: u32, _reason: &str) -> bool {
            self.timeouts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(duration);
            true
        }

        async fn post(&self, _event: &ChatMessageEvent, text: &str) -> bool {
            self.posts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(text.to_string());
            true
        }

        async fn banned_by_us(&self, event: &ChatMessageEvent) -> bool {
            self.banned.contains(&event.chatter_user_id)
        }

        async fn partner_channel(&self, _event: &ChatMessageEvent) -> bool {
            self.partner
        }

        async fn channel_regular(&self, event: &ChatMessageEvent) -> bool {
            self.regulars.contains(&event.chatter_user_id)
        }
    }

    const CREW_ID: &str = "89018048";

    async fn sight(guard: &ImpersonationGuard, channel: &str) {
        let mut message = event(CREW_ID, "blackhusky45", "hallo");
        message.broadcaster_user_id = channel.into();
        assert_eq!(guard.handle(&message, false).await, None);
    }

    impl Recorder {
        fn counts(&self) -> (Vec<u32>, Vec<String>) {
            (
                self.timeouts
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone(),
                self.posts
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone(),
            )
        }
    }

    fn event(id: &str, login: &str, text: &str) -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "500".into(),
            broadcaster_user_login: "befadi".into(),
            chatter_user_id: id.into(),
            chatter_user_login: login.into(),
            message_id: "m1".into(),
            message: ChatMessageBody {
                text: text.into(),
                fragments: Vec::new(),
            },
            ..Default::default()
        }
    }

    fn guard(mode: GuardMode) -> (ImpersonationGuard, Arc<Recorder>) {
        let recorder = Arc::new(Recorder::default());
        let guard = ImpersonationGuard::new(
            mode,
            OWN_BOT_LOGIN.into(),
            Arc::clone(&recorder) as Arc<dyn ImpersonationPort>,
        );
        (guard, recorder)
    }

    const REAL_MESSAGE: &str = "Moin hast bock auf n 6 vs 6 privat match?";

    #[test]
    fn erkennt_private_6v6_einladungen() {
        for text in [
            REAL_MESSAGE,
            "6v6 privat turnier?",
            "wer hat lust auf ein privates 6on6",
            "6 gegen 6 private lobby wer kommt",
            "6 gegen 6 private lobby, wer will mitmachen",
            "Einladung zum privaten 6v6 Turnier heute Abend",
            "habt ihr bock auf ein 6v6 scrim",
            "6v6 privat match? schreib mir",
            "Moin 6v6 privat match?",
            "willst du ein 6v6 match spielen",
        ] {
            assert!(private_6v6_pitch(text), "{text}");
        }
    }

    #[test]
    fn normale_6v6_gespraeche_sind_keine_einladung() {
        for text in [
            "6v6 ist halt der normale modus",
            "wir waren 6 gegen 6 und haben verloren",
            "privat match heute?",
            "16 vs 64 lol",
            "die 6v6 lobby hat ewig geladen",
            "das 6v6 turnier gestern war krass",
            "hast du das 6v6 turnier gestern gesehen",
            "wer schaut heute das 6v6 turnier?",
            "6v6 scrim war gut",
            "das custom 6v6 match gestern war krass",
            "hab keinen bock mehr auf 6v6 scrims",
            "das 6v6 turnier kommt nächste woche",
            "gibt es heute noch ein privates 6v6 turnier?",
            "willst du heute 6v6 spielen",
        ] {
            assert!(!private_6v6_pitch(text), "{text}");
        }
    }

    #[test]
    fn nachahmer_namen_ohne_eigenen_bot() {
        assert!(lookalike_login("deadlock_germany", OWN_BOT_LOGIN));
        assert!(lookalike_login("DeadlockDeutschland", OWN_BOT_LOGIN));
        assert!(lookalike_login("deadlock_community_de", OWN_BOT_LOGIN));
        assert!(!lookalike_login("DeutscheDeadlockCommunity", OWN_BOT_LOGIN));
        assert!(!lookalike_login("deadlockfan", OWN_BOT_LOGIN));
        assert!(!lookalike_login("germanplayer", OWN_BOT_LOGIN));
    }

    #[tokio::test]
    async fn deadlock_germany_im_fremden_chat_bekommt_genau_eine_warnung() {
        let (guard, recorder) = guard(GuardMode::Foreign);
        let message = event("1505528697", "deadlock_germany", REAL_MESSAGE);
        let first = guard.handle(&message, false).await;
        let second = guard.handle(&message, false).await;
        assert_eq!(first.map(|o| o.action), Some("foreign_warning"));
        assert_eq!(second.map(|o| o.action), Some("foreign_warning_throttled"));
        let (timeouts, posts) = recorder.counts();
        assert!(timeouts.is_empty());
        assert_eq!(posts.len(), 1);
        assert!(posts[0].contains("Der Account deadlock_germany gehört nicht zu uns"));
    }

    #[tokio::test]
    async fn fremder_guard_bleibt_in_partnerkanaelen_still() {
        let recorder = Arc::new(Recorder {
            partner: true,
            banned: vec!["66".into()],
            ..Recorder::default()
        });
        let guard = ImpersonationGuard::new(
            GuardMode::Foreign,
            OWN_BOT_LOGIN.into(),
            Arc::clone(&recorder) as Arc<dyn ImpersonationPort>,
        );
        for message in [
            event("1505528697", "deadlock_germany", REAL_MESSAGE),
            event("1505528697", "deadlock_germany", "hallo"),
            event("66", "zufallsname", REAL_MESSAGE),
        ] {
            assert_eq!(guard.handle(&message, false).await, None);
        }
        assert_eq!(recorder.counts(), (Vec::new(), Vec::new()));
    }

    #[tokio::test]
    async fn fremder_chat_ohne_nachahmer_namen_bleibt_still() {
        let (guard, recorder) = guard(GuardMode::Foreign);
        assert_eq!(
            guard
                .handle(&event("77", "randomviewer", REAL_MESSAGE), false)
                .await,
            None
        );
        assert_eq!(recorder.counts(), (Vec::new(), Vec::new()));
    }

    #[tokio::test]
    async fn gebannter_account_mit_6v6_einladung_wird_im_fremden_chat_gewarnt() {
        let recorder = Arc::new(Recorder {
            banned: vec!["66".into()],
            ..Recorder::default()
        });
        let guard = ImpersonationGuard::new(
            GuardMode::Foreign,
            OWN_BOT_LOGIN.into(),
            Arc::clone(&recorder) as Arc<dyn ImpersonationPort>,
        );
        sight(&guard, "500").await;
        let pitch = guard
            .handle(&event("66", "zufallsname", REAL_MESSAGE), false)
            .await;
        let smalltalk = guard
            .handle(&event("66", "zufallsname", "gg wp"), false)
            .await;
        assert_eq!(pitch.map(|o| o.action), Some("foreign_warning"));
        assert_eq!(smalltalk, None);
        let (timeouts, posts) = recorder.counts();
        assert!(timeouts.is_empty());
        assert_eq!(posts.len(), 1);
        assert!(posts[0].contains("Der Account zufallsname gehört nicht zu uns"));
    }

    #[tokio::test]
    async fn fremde_warnungen_sind_stuendlich_begrenzt() {
        let (guard, recorder) = guard(GuardMode::Foreign);
        for index in 0..12 {
            let mut message = event("1505528697", "deadlock_germany", "hi");
            message.broadcaster_user_id = format!("kanal-{index}");
            guard.handle(&message, false).await;
        }
        assert_eq!(recorder.counts().1.len(), FOREIGN_WARNING_LIMIT);
    }

    #[tokio::test]
    async fn partner_pitch_gibt_timeout_und_einmal_statement() {
        let (guard, recorder) = guard(GuardMode::Partner);
        sight(&guard, "500").await;
        let message = event("88", "fremder", REAL_MESSAGE);
        let first = guard.handle(&message, false).await;
        let second = guard.handle(&message, false).await;
        assert_eq!(first.map(|o| o.action), Some("timeout_statement"));
        assert_eq!(second.map(|o| o.action), Some("timeout"));
        let (timeouts, posts) = recorder.counts();
        assert_eq!(timeouts, vec![TIMEOUT_SECS, TIMEOUT_SECS]);
        assert_eq!(TIMEOUT_SECS, 600);
        assert_eq!(posts.len(), 1);
        assert!(posts[0].starts_with("Hinweis der Deutschen Deadlock Community: fremder gehört"));
    }

    #[tokio::test]
    async fn partner_registry_account_wird_auch_ohne_pitch_getimeoutet() {
        let (guard, recorder) = guard(GuardMode::Partner);
        let outcome = guard
            .handle(&event("1505528697", "deadlock_germany", "hallo"), true)
            .await;
        assert_eq!(outcome.map(|o| o.action), Some("timeout_statement"));
        assert_eq!(recorder.counts().0, vec![600]);
    }

    #[tokio::test]
    async fn community_mitglieder_bleiben_unberuehrt() {
        let (guard, recorder) = guard(GuardMode::Partner);
        let mut moderator = event("90", "mod", REAL_MESSAGE);
        moderator.badges = vec![ChatBadge {
            set_id: "moderator".into(),
            id: "1".into(),
            info: String::new(),
        }];
        assert_eq!(guard.handle(&moderator, false).await, None);
        assert_eq!(
            guard
                .handle(&event("91", "partner", REAL_MESSAGE), true)
                .await,
            None
        );
        let mut broadcaster = event("500", "befadi", REAL_MESSAGE);
        broadcaster.broadcaster_user_id = "500".into();
        assert_eq!(guard.handle(&broadcaster, false).await, None);
        assert_eq!(recorder.counts(), (Vec::new(), Vec::new()));
    }

    #[tokio::test]
    async fn stammgaeste_bleiben_in_beiden_pfaden_unberuehrt() {
        for mode in [GuardMode::Partner, GuardMode::Foreign] {
            let recorder = Arc::new(Recorder {
                regulars: vec!["88".into(), "1505528697".into()],
                banned: vec!["88".into()],
                ..Recorder::default()
            });
            let guard = ImpersonationGuard::new(
                mode,
                OWN_BOT_LOGIN.into(),
                Arc::clone(&recorder) as Arc<dyn ImpersonationPort>,
            );
            sight(&guard, "500").await;
            assert_eq!(
                guard
                    .handle(&event("88", "stammgast", REAL_MESSAGE), false)
                    .await,
                None
            );
            let crew = guard
                .handle(
                    &event("1505528697", "deadlock_germany", REAL_MESSAGE),
                    false,
                )
                .await;
            assert!(crew.is_some_and(|o| o.acted()), "{mode:?}");
            assert_eq!(recorder.counts().1.len(), 1, "{mode:?}");
        }
    }

    #[tokio::test]
    async fn ohne_crew_sichtung_gibt_es_nur_eine_meldung() {
        for mode in [GuardMode::Partner, GuardMode::Foreign] {
            let recorder = Arc::new(Recorder {
                banned: vec!["88".into()],
                ..Recorder::default()
            });
            let guard = ImpersonationGuard::new(
                mode,
                OWN_BOT_LOGIN.into(),
                Arc::clone(&recorder) as Arc<dyn ImpersonationPort>,
            );
            let outcome = guard
                .handle(&event("88", "fremder", REAL_MESSAGE), false)
                .await;
            assert_eq!(
                outcome.map(|o| o.action),
                Some(NO_CREW_SIGHTING),
                "{mode:?}"
            );
            assert!(outcome.is_some_and(|o| !o.acted()));
            assert_eq!(recorder.counts(), (Vec::new(), Vec::new()), "{mode:?}");
        }
    }

    #[tokio::test]
    async fn crew_sichtung_im_selben_kanal_schaltet_aktion_frei() {
        for (mode, expected) in [
            (GuardMode::Partner, "timeout_statement"),
            (GuardMode::Foreign, "foreign_warning"),
        ] {
            let recorder = Arc::new(Recorder {
                banned: vec!["88".into()],
                ..Recorder::default()
            });
            let guard = ImpersonationGuard::new(
                mode,
                OWN_BOT_LOGIN.into(),
                Arc::clone(&recorder) as Arc<dyn ImpersonationPort>,
            );
            sight(&guard, "500").await;
            let outcome = guard
                .handle(&event("88", "fremder", REAL_MESSAGE), false)
                .await;
            assert_eq!(outcome.map(|o| o.action), Some(expected), "{mode:?}");
            assert_eq!(recorder.counts().1.len(), 1, "{mode:?}");
        }
    }

    #[tokio::test]
    async fn crew_sichtung_in_anderem_kanal_zaehlt_nicht() {
        for mode in [GuardMode::Partner, GuardMode::Foreign] {
            let recorder = Arc::new(Recorder {
                banned: vec!["88".into()],
                ..Recorder::default()
            });
            let guard = ImpersonationGuard::new(
                mode,
                OWN_BOT_LOGIN.into(),
                Arc::clone(&recorder) as Arc<dyn ImpersonationPort>,
            );
            sight(&guard, "999").await;
            let outcome = guard
                .handle(&event("88", "fremder", REAL_MESSAGE), false)
                .await;
            assert_eq!(
                outcome.map(|o| o.action),
                Some(NO_CREW_SIGHTING),
                "{mode:?}"
            );
            assert_eq!(recorder.counts(), (Vec::new(), Vec::new()), "{mode:?}");
        }
    }

    #[test]
    fn crew_sichtung_verfaellt_nach_24_stunden() {
        let mut sightings = Sightings::default();
        let start = Instant::now();
        sightings.record("500", start);
        assert!(sightings.recent("500", start + Duration::from_secs(23 * 60 * 60)));
        assert!(!sightings.recent("500", start + CREW_SIGHTING_WINDOW));
        assert!(!sightings.recent("501", start));
    }
}
