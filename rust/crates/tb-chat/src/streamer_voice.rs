//! Requested co-play help, not advertising. Existing Discord members and mods
//! are eligible too. The existing LFG judge and recent-chat port are reused.
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use dashmap::DashMap;
use regex::Regex;
use tokio::sync::Mutex;

use crate::{
    api::ChatApi,
    commands::InviteReplyNotifier,
    lfg_pitch::{LfgJudge, LfgJudgeInput, LfgVerdictKind, LfgVerdictSource, RecentChatPort},
    types::{ChatMessageEvent, SendOutcome},
};

const COOLDOWN: Duration = Duration::from_secs(30);
const FOLLOWUP_WINDOW: Duration = Duration::from_secs(600);

fn join_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?ix)
        \bmit(?:spiel|zock|mach)(?:en|n|e|st|t)?\b
        | \b(kann(?:st|ste)?|könnt\w*|koennt\w*|darf|dürfte)\b[^.!?]{0,30}\b(ich|mich)\b[^.!?]{0,80}\b(mit|dazu|dabei|einlad\w*|invit\w*)\b
        | \b(hau|hänge|haenge)\s+mich\b[^.!?]{0,25}\bdazu\b
        | \b(invite|inv|adde?|einladen)\s+mich\b
        | \bnoch\s+(platz|slots?)\s*(frei|für|fuer|\?)
        | \b(can|may|could)\s+i\s+(join|play\s+with\s+you)\b
        | \b(can|could)\s+you\s+invite\s+me\b
        | \binvite\s+me\b
    ").expect("voice join prefilter"))
}

fn access_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(spielzugang|beta|beta-key|early\s+access|zugang\s+(zu|zum|für|fuer)|kein\w*\s+(zugang|deadlock|spiel)|game\s+access)\b").expect("game access prefilter"))
}

fn full_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?ix)^\s*(?:aber\s+)?(?:(?:ihr\s+seid|wir\s+sind)|(?:der\s+|euer\s+)?(?:voice|sprachkanal|kanal|channel|call)\s+ist|(?:the\s+)?(?:voice|channel|call)\s+is)\s+(?:schon\s+|leider\s+)?(?:voll|full)[.!?\s]*$").expect("voice full followup"))
}

/// Public for the access-question guard: a group invitation must not be
/// mistaken for a beta key just because it contains the word 'einladen'.
pub fn is_group_join_request(text: &str) -> bool {
    !text.trim().starts_with('!') && !access_re().is_match(text) && join_re().is_match(text)
}

/// Only explicit co-play/Discord context rules out a game-access question.
/// An ambiguous invitation still goes through the access judge.
pub fn is_explicit_group_join_request(text: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    is_group_join_request(text)
        && RE
            .get_or_init(|| {
                Regex::new(
        r"(?i)\b(mitspiel\w*|mitzock\w*|discord|dc|voice|sprachkanal|runde|lobby|gruppe|party)\b"
    ).expect("explicit group context")
            })
            .is_match(text)
}

#[derive(Clone, Debug)]
pub struct VoiceInvite {
    pub url: String,
    pub slot_added: bool,
}

#[async_trait]
pub trait StreamerVoicePort: Send + Sync {
    async fn invite_for(
        &self,
        broadcaster_id: &str,
        message_id: &str,
    ) -> Result<Option<VoiceInvite>, String>;
}

#[derive(Default)]
struct ChannelState {
    last_attempt: Option<Instant>,
    seen: HashMap<String, Instant>,
    confirmed: HashMap<String, Instant>,
}

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Continue,
    Handled,
    Reply(String),
}

pub struct StreamerVoiceResponder {
    port: Arc<dyn StreamerVoicePort>,
    judge: Arc<dyn LfgJudge>,
    recent_chat: Arc<dyn RecentChatPort>,
    notifier: Option<Arc<dyn InviteReplyNotifier>>,
    states: DashMap<String, Arc<Mutex<ChannelState>>>,
}

impl StreamerVoiceResponder {
    pub fn new(
        port: Arc<dyn StreamerVoicePort>,
        judge: Arc<dyn LfgJudge>,
        recent_chat: Arc<dyn RecentChatPort>,
        notifier: Option<Arc<dyn InviteReplyNotifier>>,
    ) -> Self {
        Self {
            port,
            judge,
            recent_chat,
            notifier,
            states: DashMap::new(),
        }
    }

    /// True also suppresses competing pitches while this channel's request is
    /// already in flight. Never let a delayed LLM produce two separate replies.
    pub async fn maybe_respond(
        &self,
        event: &ChatMessageEvent,
        api: &dyn ChatApi,
        bot_id: &str,
    ) -> bool {
        if !is_group_join_request(event.text()) && !full_re().is_match(event.text()) {
            return false;
        }
        if event.is_broadcaster()
            || !valid_id(&event.broadcaster_user_id)
            || !valid_id(&event.chatter_user_id)
            || event.message_id.trim().is_empty()
        {
            return false;
        }
        // Do not hijack a viewer-to-viewer reply as an invitation to the host.
        if event.reply.as_ref().is_some_and(|reply| {
            !reply.parent_user_id.is_empty()
                && reply.parent_user_id != event.broadcaster_user_id
                && reply.parent_user_id != bot_id
        }) {
            return false;
        }
        let state = Arc::clone(
            self.states
                .entry(event.broadcaster_user_id.clone())
                .or_default()
                .value(),
        );
        let Ok(mut state) = state.try_lock_owned() else {
            return true;
        };
        match self.prepare(&mut state, event, Instant::now()).await {
            Decision::Continue => false,
            Decision::Handled => true,
            Decision::Reply(message) => {
                if matches!(
                    api.send_message(&event.broadcaster_user_id, &message).await,
                    Ok(SendOutcome::Sent)
                ) {
                    state
                        .confirmed
                        .insert(event.chatter_user_id.clone(), Instant::now());
                    if let Some(notifier) = &self.notifier {
                        notifier
                            .note_invite_reply(&event.broadcaster_user_login)
                            .await;
                    }
                }
                true
            }
        }
    }

    async fn prepare(
        &self,
        state: &mut ChannelState,
        event: &ChatMessageEvent,
        now: Instant,
    ) -> Decision {
        state
            .seen
            .retain(|_, seen| now.saturating_duration_since(*seen) < FOLLOWUP_WINDOW);
        state
            .confirmed
            .retain(|_, seen| now.saturating_duration_since(*seen) < FOLLOWUP_WINDOW);
        let followup = full_re().is_match(event.text())
            && state.confirmed.contains_key(&event.chatter_user_id);
        if !followup && !is_group_join_request(event.text()) {
            return Decision::Continue;
        }
        if state.seen.contains_key(&event.message_id)
            || state
                .last_attempt
                .is_some_and(|last| now.saturating_duration_since(last) < COOLDOWN)
        {
            return Decision::Handled;
        }
        state.last_attempt = Some(now);
        state.seen.insert(event.message_id.clone(), now);
        if !followup {
            let verdict = self
                .judge
                .judge(LfgJudgeInput {
                    message: event.message.text.clone(),
                    recent_chat: self
                        .recent_chat
                        .recent_chat(&event.broadcaster_user_login, &event.message_id)
                        .await,
                })
                .await;
            if verdict.source != LfgVerdictSource::Model
                || verdict.verdict != LfgVerdictKind::Yes
                || !verdict.confidence.is_finite()
                || verdict.confidence < 0.7
            {
                return Decision::Continue;
            }
        }
        match self
            .port
            .invite_for(&event.broadcaster_user_id, &event.message_id)
            .await
        {
            Ok(Some(invite)) if valid_invite(&invite.url) => {
                let slot = if invite.slot_added {
                    " Ich habe den Kanal um einen Platz erweitert."
                } else {
                    ""
                };
                Decision::Reply(format!(
                    "@{} Hier kommst du zu {} in den Sprachkanal: {}{}",
                    event.chatter_user_login, event.broadcaster_user_login, invite.url, slot
                ))
            }
            Ok(None) => Decision::Continue,
            _ => {
                // Cooldown is claimed before I/O; no repeated warnings or false
                // success messages while Discord is unavailable.
                tracing::debug!(channel_id = %event.broadcaster_user_id, "Streamer-Voice-Invite derzeit nicht verfügbar");
                Decision::Handled
            }
        }
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|id| id > 0)
}

fn valid_invite(value: &str) -> bool {
    value
        .strip_prefix("https://discord.gg/")
        .is_some_and(|code| {
            !code.is_empty()
                && code.len() <= 128
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lfg_pitch::{LfgVerdict, LfgVerdictSource};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakePort {
        calls: AtomicUsize,
        result: Result<Option<VoiceInvite>, String>,
    }
    #[async_trait]
    impl StreamerVoicePort for FakePort {
        async fn invite_for(
            &self,
            broadcaster: &str,
            message: &str,
        ) -> Result<Option<VoiceInvite>, String> {
            assert_eq!(broadcaster, "123");
            assert!(!message.is_empty());
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result.clone()
        }
    }
    struct Judge(bool);
    #[async_trait]
    impl LfgJudge for Judge {
        async fn judge(&self, _: LfgJudgeInput) -> LfgVerdict {
            LfgVerdict {
                verdict: if self.0 {
                    LfgVerdictKind::Yes
                } else {
                    LfgVerdictKind::No
                },
                confidence: 0.95,
                reasoning: String::new(),
                source: LfgVerdictSource::Model,
            }
        }
    }
    struct History;
    #[async_trait]
    impl RecentChatPort for History {
        async fn recent_chat(&self, _: &str, _: &str) -> Vec<String> {
            vec!["Streamer: Komm gerne dazu".into()]
        }
    }
    fn event(text: &str) -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "123".into(),
            broadcaster_user_login: "streamer".into(),
            chatter_user_id: "456".into(),
            chatter_user_login: "viewer".into(),
            message_id: "message-1".into(),
            message: crate::types::ChatMessageBody {
                text: text.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
    fn responder(
        result: Result<Option<VoiceInvite>, String>,
        yes: bool,
    ) -> (StreamerVoiceResponder, Arc<FakePort>) {
        let port = Arc::new(FakePort {
            calls: AtomicUsize::new(0),
            result,
        });
        (
            StreamerVoiceResponder::new(
                port.clone(),
                Arc::new(Judge(yes)),
                Arc::new(History),
                None,
            ),
            port,
        )
    }
    fn ready() -> Result<Option<VoiceInvite>, String> {
        Ok(Some(VoiceInvite {
            url: "https://discord.gg/testCode".into(),
            slot_added: true,
        }))
    }

    #[test]
    fn screenshot_group_invite_is_not_a_game_key() {
        for text in [
            "Kannste mich nach der runde über dc direk einladen?",
            "Kann ich mitspielen?",
            "kann ich mit?",
            "invite mich",
            "Can I join?",
            "noch Platz frei?",
        ] {
            assert!(is_group_join_request(text), "{text}");
        }
        for text in [
            "Wie bekomme ich Zugang zum Spiel?",
            "Kannst du mich für die Beta einladen?",
            "Ich suche Mitspieler",
            "!mitspielen",
            "Ihr seid voll gut",
            "Wie kann ich mitspielen, habe keinen Zugang?",
        ] {
            assert!(!is_group_join_request(text), "{text}");
        }
    }

    #[tokio::test]
    async fn join_reply_uses_confirmed_channel_and_no_game_access_pitch() {
        let (responder, port) = responder(ready(), true);
        let result = responder
            .prepare(
                &mut ChannelState::default(),
                &event("Kannste mich nach der Runde einladen?"),
                Instant::now(),
            )
            .await;
        let Decision::Reply(text) = result else {
            panic!("expected voice reply");
        };
        assert!(text.contains("https://discord.gg/testCode"));
        assert!(text.contains("um einen Platz erweitert"));
        assert!(!text.contains("Steam Freundescode"));
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn negative_judge_never_mutates_discord() {
        let (responder, port) = responder(ready(), false);
        assert_eq!(
            responder
                .prepare(
                    &mut ChannelState::default(),
                    &event("Ich will nicht mitspielen"),
                    Instant::now()
                )
                .await,
            Decision::Continue
        );
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn duplicates_and_channel_bursts_do_not_create_more_slots() {
        let (responder, port) = responder(ready(), true);
        let mut state = ChannelState::default();
        let now = Instant::now();
        let first = event("Kann ich mitspielen?");
        assert!(matches!(
            responder.prepare(&mut state, &first, now).await,
            Decision::Reply(_)
        ));
        assert_eq!(
            responder
                .prepare(&mut state, &first, now + Duration::from_secs(60))
                .await,
            Decision::Handled
        );
        let mut second = event("Kann ich mitspielen?");
        second.message_id = "message-2".into();
        assert_eq!(
            responder
                .prepare(&mut state, &second, now + Duration::from_secs(1))
                .await,
            Decision::Handled
        );
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn full_followup_requires_a_recent_successful_invite_for_that_viewer() {
        let (responder, port) = responder(ready(), false);
        let mut state = ChannelState::default();
        let now = Instant::now();
        assert_eq!(
            responder
                .prepare(&mut state, &event("Ihr seid voll"), now)
                .await,
            Decision::Continue
        );
        state.confirmed.insert("456".into(), now);
        assert!(matches!(
            responder
                .prepare(&mut state, &event("Ihr seid voll"), now)
                .await,
            Decision::Reply(_)
        ));
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
        let mut later = event("Ihr seid voll");
        later.message_id = "later".into();
        assert_eq!(
            responder
                .prepare(&mut state, &later, now + FOLLOWUP_WINDOW)
                .await,
            Decision::Continue
        );
    }

    #[tokio::test]
    async fn unavailable_private_or_missing_voice_never_claims_success() {
        let (responder, _) = responder(Ok(None), true);
        assert_eq!(
            responder
                .prepare(
                    &mut ChannelState::default(),
                    &event("Kann ich mitspielen?"),
                    Instant::now()
                )
                .await,
            Decision::Continue
        );
        let (responder, _) = self::responder(Err("offline".into()), true);
        assert_eq!(
            responder
                .prepare(
                    &mut ChannelState::default(),
                    &event("Kann ich mitspielen?"),
                    Instant::now()
                )
                .await,
            Decision::Handled
        );
    }

    #[test]
    fn rejects_untrusted_urls_and_missing_ids() {
        assert!(valid_invite("https://discord.gg/abc_12-X"));
        for url in [
            "https://discord.gg/",
            "https://evil.invalid/a",
            "https://discord.gg/abc?x=1",
            "https://discord.gg/abc extra",
        ] {
            assert!(!valid_invite(url));
        }
        for id in ["", "0", "123name", "-1"] {
            assert!(!valid_id(id));
        }
    }
}
