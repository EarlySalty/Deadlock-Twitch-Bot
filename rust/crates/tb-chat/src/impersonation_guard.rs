use crate::api::ChatApi;
use crate::crew_guard::registry_impersonator;
use crate::global_chatter_ban::GlobalChatterBanEnforcer;
use crate::moderation::{ModerationEngine, ModerationEvidence};
use crate::types::{ChatMessageEvent, SendOutcome};
use async_trait::async_trait;
use regex::Regex;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};
use tracing::{info, warn};

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

fn six_v_six() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b6\s*(v|vs\.?|versus|on|gegen|x)\s*6\b").ok())
        .as_ref()
}

fn private_cue() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"privat|turnier|tournament|scrim|lobby|custom").ok())
        .as_ref()
}

fn invite_cue() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(bock|lust|willst|wollt|kommt|komm)\b").ok())
        .as_ref()
}

fn matches(re: Option<&'static Regex>, text: &str) -> bool {
    re.is_some_and(|re| re.is_match(text))
}

pub fn private_6v6_pitch(text: &str) -> bool {
    let text = text.to_lowercase();
    if !matches(six_v_six(), &text) {
        return false;
    }
    matches(private_cue(), &text) || (matches(invite_cue(), &text) && text.contains("match"))
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
    pub fn alert_text(&self) -> &'static str {
        match self.action {
            "timeout_statement" => "Timeout (10 Min.) gesetzt und Hinweis im Chat gepostet.",
            "timeout" => "Timeout (10 Min.) gesetzt, Hinweis kam in den letzten 30 Min. schon.",
            "statement" => "Timeout fehlgeschlagen, Hinweis im Chat gepostet.",
            "timeout_failed" => "Timeout und Hinweis sind fehlgeschlagen.",
            "foreign_warning" => "Warnhinweis im fremden Chat gepostet.",
            "foreign_warning_throttled" => {
                "Kein neuer Warnhinweis, Sperrfrist oder Stundenlimit greift."
            }
            _ => "Warnhinweis übersprungen, Senden nicht möglich.",
        }
    }
}

#[async_trait]
pub trait ImpersonationPort: Send + Sync {
    async fn timeout(&self, event: &ChatMessageEvent, duration_secs: u32, reason: &str) -> bool;
    async fn post(&self, broadcaster_id: &str, text: &str) -> bool;
    async fn banned_by_us(&self, event: &ChatMessageEvent) -> bool;
}

pub struct PartnerActions {
    moderation: Arc<ModerationEngine>,
    api: Arc<dyn ChatApi>,
}

impl PartnerActions {
    pub fn new(moderation: Arc<ModerationEngine>, api: Arc<dyn ChatApi>) -> Self {
        Self { moderation, api }
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

    async fn post(&self, broadcaster_id: &str, text: &str) -> bool {
        sent(self.api.send_message(broadcaster_id, text).await)
    }

    async fn banned_by_us(&self, _event: &ChatMessageEvent) -> bool {
        false
    }
}

pub struct ForeignActions {
    api: Option<Arc<dyn ChatApi>>,
    ban_list: Option<Arc<GlobalChatterBanEnforcer>>,
}

impl ForeignActions {
    pub fn new(
        api: Option<Arc<dyn ChatApi>>,
        ban_list: Option<Arc<GlobalChatterBanEnforcer>>,
    ) -> Self {
        Self { api, ban_list }
    }
}

#[async_trait]
impl ImpersonationPort for ForeignActions {
    async fn timeout(&self, _event: &ChatMessageEvent, _duration_secs: u32, _reason: &str) -> bool {
        false
    }

    async fn post(&self, broadcaster_id: &str, text: &str) -> bool {
        match &self.api {
            Some(api) => sent(api.send_message(broadcaster_id, text).await),
            None => false,
        }
    }

    async fn banned_by_us(&self, event: &ChatMessageEvent) -> bool {
        match &self.ban_list {
            Some(ban_list) => ban_list.is_banned(event).await,
            None => false,
        }
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

pub struct ImpersonationGuard {
    mode: GuardMode,
    bot_login: String,
    port: Arc<dyn ImpersonationPort>,
    throttle: Mutex<Throttle>,
}

impl ImpersonationGuard {
    pub fn new(mode: GuardMode, bot_login: String, port: Arc<dyn ImpersonationPort>) -> Self {
        Self {
            mode,
            bot_login,
            port,
            throttle: Mutex::new(Throttle::default()),
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
        let reason = self.reason(event, trusted).await?;
        let action = match self.mode {
            GuardMode::Partner => self.partner(event).await,
            GuardMode::Foreign => self.foreign(event).await,
        };
        info!(
            channel = %event.broadcaster_user_login,
            chatter = %event.chatter_user_login,
            action,
            reason,
            "Impersonation-Guard entschieden"
        );
        Some(ImpersonationOutcome { action, reason })
    }

    async fn partner(&self, event: &ChatMessageEvent) -> &'static str {
        let timed_out = self.port.timeout(event, TIMEOUT_SECS, TIMEOUT_REASON).await;
        let posted = self.reserve(event)
            && self
                .port
                .post(&event.broadcaster_user_id, &self.text(event))
                .await;
        match (timed_out, posted) {
            (true, true) => "timeout_statement",
            (true, false) => "timeout",
            (false, true) => "statement",
            (false, false) => "timeout_failed",
        }
    }

    async fn foreign(&self, event: &ChatMessageEvent) -> &'static str {
        if !self.reserve(event) {
            return "foreign_warning_throttled";
        }
        if self
            .port
            .post(&event.broadcaster_user_id, &self.text(event))
            .await
        {
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

        async fn post(&self, _broadcaster_id: &str, text: &str) -> bool {
            self.posts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(text.to_string());
            true
        }

        async fn banned_by_us(&self, event: &ChatMessageEvent) -> bool {
            self.banned.contains(&event.chatter_user_id)
        }
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
            "6 gegen 6 private lobby",
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
}
