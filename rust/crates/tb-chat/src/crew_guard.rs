use crate::pipeline::{CrewRadarAlert, ModAlerter};
use crate::scam_pitch::AccountAgePort;
use crate::style_score::{score as style_score, Centroid, StyleBreakdown};
use crate::types::ChatMessageEvent;
use crate::zuschauer_register::{reserviere_radar_meldung, unauffaellig};
use chrono::{Timelike, Utc};
use chrono_tz::Europe::Berlin;
use regex::Regex;
use sqlx::PgPool;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use tracing::{error, warn};

#[derive(Debug, Clone)]
pub struct CrewRadarLog {
    pub channel_login: String,
    pub chatter_login: String,
    pub chatter_id: Option<String>,
    pub account_age_days: Option<i64>,
    pub style_score: u8,
    pub style_breakdown: StyleBreakdown,
    pub time_window_match: bool,
    pub messages: Vec<String>,
    pub llm_verdict: String,
    pub llm_confidence: Option<f32>,
    pub llm_reasoning: Option<String>,
    pub action_taken: String,
    pub source: String,
}

pub async fn persist_radar_log(pool: &PgPool, record: &CrewRadarLog) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO twitch_crew_radar_log \
         (channel_login, chatter_login, chatter_id, account_age_days, style_score, \
          style_breakdown, time_window_match, messages, llm_verdict, llm_confidence, \
          llm_reasoning, action_taken, source) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(&record.channel_login)
    .bind(&record.chatter_login)
    .bind(&record.chatter_id)
    .bind(record.account_age_days)
    .bind(i16::from(record.style_score))
    .bind(serde_json::json!(&record.style_breakdown))
    .bind(record.time_window_match)
    .bind(serde_json::json!(&record.messages))
    .bind(&record.llm_verdict)
    .bind(record.llm_confidence)
    .bind(&record.llm_reasoning)
    .bind(&record.action_taken)
    .bind(&record.source)
    .execute(pool)
    .await?;
    Ok(())
}

struct CrewAccount {
    twitch_user_id: &'static str,
    login: &'static str,
    has_behavioral_evidence: bool,
}

const CREW_REGISTRY: &[CrewAccount] = &[
    CrewAccount {
        twitch_user_id: "89018048",
        login: "blackhusky45",
        has_behavioral_evidence: true,
    },
    CrewAccount {
        twitch_user_id: "147713656",
        login: "helmbombenricky",
        has_behavioral_evidence: true,
    },
    CrewAccount {
        twitch_user_id: "823493023",
        login: "skifahrertv",
        has_behavioral_evidence: true,
    },
    CrewAccount {
        twitch_user_id: "595804185",
        login: "h4teme666",
        has_behavioral_evidence: false,
    },
    CrewAccount {
        twitch_user_id: "1445014969",
        login: "mr_horizont",
        has_behavioral_evidence: false,
    },
    CrewAccount {
        twitch_user_id: "771345179",
        login: "wall_horizon",
        has_behavioral_evidence: true,
    },
    CrewAccount {
        twitch_user_id: "1505528697",
        login: "deadlock_germany",
        has_behavioral_evidence: true,
    },
];

const RIVAL_INVITE_CODES: &[&str] = &[
    "ZWSNyNfdG",
    "W7kCyBBcf",
    "XtXbc4ER",
    "cXndRbd2",
    "SBRrArXjHf",
];

#[derive(Debug, Clone, PartialEq)]
pub enum CrewSignal {
    HardId {
        login: &'static str,
        has_evidence: bool,
    },
    HardInvite {
        code: String,
    },
    Trigger {
        hits: Vec<&'static str>,
    },
    None,
}

fn trigger_matchers() -> &'static [(&'static str, Regex)] {
    static MATCHERS: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    MATCHERS
        .get_or_init(|| {
            [
                ("helmbomben", r"helmbomben"),
                ("ricky", r"\bricky\b"),
                ("freund-gebannt", r"(freund|kollege).{0,40}(gebannt|banned)"),
                ("gebannt-freund", r"(gebannt|banned).{0,40}(freund|kollege)"),
                ("nani", r"\bna[nm]i\b"),
                ("bot-von-nani", r"bot von na[nm]i"),
                ("bannliste", r"bann?liste"),
            ]
            .into_iter()
            .filter_map(
                |(label, pattern)| match Regex::new(&format!("(?i){pattern}")) {
                    Ok(re) => Some((label, re)),
                    Err(err) => {
                        warn!("crew_guard: ungueltiges Trigger-Regex {label}: {err}");
                        None
                    }
                },
            )
            .collect()
        })
        .as_slice()
}

fn invite_matcher() -> Option<&'static Regex> {
    static MATCHER: OnceLock<Option<Regex>> = OnceLock::new();
    MATCHER
        .get_or_init(|| {
            let codes = RIVAL_INVITE_CODES.join("|");
            Regex::new(&format!(r"(?i)discord\.gg/({codes})")).ok()
        })
        .as_ref()
}

fn trigger_hits(content: &str) -> Vec<&'static str> {
    trigger_matchers()
        .iter()
        .filter(|(_, re)| re.is_match(content))
        .map(|(label, _)| *label)
        .collect()
}

pub fn screen(content: &str, chatter_id: Option<&str>) -> CrewSignal {
    if let Some(id) = chatter_id {
        let id = id.trim();
        if let Some(account) = CREW_REGISTRY.iter().find(|acc| acc.twitch_user_id == id) {
            return CrewSignal::HardId {
                login: account.login,
                has_evidence: account.has_behavioral_evidence,
            };
        }
    }

    if let Some(re) = invite_matcher() {
        if let Some(code) = re.captures(content).and_then(|caps| caps.get(1)) {
            return CrewSignal::HardInvite {
                code: code.as_str().to_string(),
            };
        }
    }

    let hits = trigger_hits(content);
    if !hits.is_empty() {
        return CrewSignal::Trigger { hits };
    }

    CrewSignal::None
}
const CONTEXT_WINDOW: usize = 6;
const CONTEXT_MAX_KEYS: usize = 4096;

#[derive(Default)]
struct ContextStore {
    windows: HashMap<(String, String), VecDeque<String>>,
    order: VecDeque<(String, String)>,
}

struct ChatterContextBuffer {
    inner: Mutex<ContextStore>,
}

impl ChatterContextBuffer {
    fn new() -> Self {
        Self {
            inner: Mutex::new(ContextStore::default()),
        }
    }

    fn snapshot_then_push(&self, channel: &str, identity: &str, content: &str) -> Vec<String> {
        let key = (channel.to_string(), identity.to_string());
        let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let store = &mut *guard;

        let existing = store.windows.get(&key);
        let prev: Vec<String> = existing
            .map(|window| window.iter().cloned().collect())
            .unwrap_or_default();
        let existed = existing.is_some();

        let window = store.windows.entry(key.clone()).or_default();
        window.push_back(content.to_string());
        while window.len() > CONTEXT_WINDOW {
            window.pop_front();
        }

        if !existed {
            store.order.push_back(key);
            while store.order.len() > CONTEXT_MAX_KEYS {
                if let Some(oldest) = store.order.pop_front() {
                    store.windows.remove(&oldest);
                } else {
                    break;
                }
            }
        }
        prev
    }
}

pub fn evidence_logins() -> Vec<&'static str> {
    CREW_REGISTRY
        .iter()
        .filter(|a| a.has_behavioral_evidence)
        .map(|a| a.login)
        .collect()
}

pub struct CrewGuard {
    enabled: bool,
    alerter: Arc<ModAlerter>,
    pool: PgPool,
    bot_user_id: String,
    account_age: Arc<dyn AccountAgePort>,
    centroid: Arc<Centroid>,
    context: ChatterContextBuffer,
}

impl CrewGuard {
    pub fn new(
        enabled: bool,
        alerter: Arc<ModAlerter>,
        pool: PgPool,
        bot_user_id: String,
        account_age: Arc<dyn AccountAgePort>,
        centroid: Arc<Centroid>,
        _notify_only: bool,
    ) -> Self {
        Self {
            enabled,
            alerter,
            pool,
            bot_user_id,
            account_age,
            centroid,
            context: ChatterContextBuffer::new(),
        }
    }

    pub fn observe(&self, event: &ChatMessageEvent) {
        if !self.enabled
            || event.chatter_user_id.trim().is_empty()
            || event.chatter_user_id == self.bot_user_id
            || event.text().is_empty()
        {
            return;
        }
        let channel = event.broadcaster_user_login.to_lowercase();
        let login = event.chatter_user_login.to_lowercase();
        let id = event.chatter_user_id.clone();
        let content = event.text().to_string();
        let mut messages = self.context.snapshot_then_push(&channel, &id, &content);
        messages.push(content.clone());
        if messages.len() > CONTEXT_WINDOW {
            messages.remove(0);
        }
        let signal = screen(&content, Some(&id));
        let pool = self.pool.clone();
        let alerter = self.alerter.clone();
        let age = self.account_age.clone();
        let centroid = self.centroid.clone();
        tokio::spawn(async move {
            let clean = match unauffaellig(&pool, &id).await {
                Ok(value) => value,
                Err(error) => {
                    warn!(%error, "Crew-Guard: Kontoregister nicht lesbar");
                    false
                }
            };
            if matches!(signal, CrewSignal::None)
                || (clean && matches!(signal, CrewSignal::Trigger { .. }))
            {
                return;
            }
            let repetitions = match reserviere_radar_meldung(&pool, &id).await {
                Ok(Some(value)) => value,
                Ok(None) => return,
                Err(error) => {
                    error!(%error, "Crew-Guard: Meldung konnte nicht reserviert werden");
                    return;
                }
            };
            let account_age_days = age.user_created_at_days(&id, &login).await;
            let style = style_score(&messages, &centroid);
            let (verdict, detail) = match signal {
                CrewSignal::HardId { has_evidence, .. } => (
                    "hard_id",
                    if has_evidence {
                        "Bekanntes Konto mit früherem Verhaltensbeleg.".to_string()
                    } else {
                        "Bekanntes Konto ohne eigenen Verhaltensbeleg.".to_string()
                    },
                ),
                CrewSignal::HardInvite { code } => {
                    ("hard_invite", format!("Bekannter Einladungslink: {code}"))
                }
                CrewSignal::Trigger { hits } => (
                    "pattern",
                    format!("Mehrdeutige Textmuster: {}", hits.join(", ")),
                ),
                CrewSignal::None => return,
            };
            let mut message = format!("🔎 **{login}** in #{channel}: Muster gesichtet.\n{detail}\nKein Urteil über das Konto. Es wurde nichts moderiert.\n**Stilähnlichkeit:** {} %\n**Nachrichten:**\n{}", style.total,
                messages.iter().map(|m| format!("> {}", m.chars().take(300).collect::<String>())).collect::<Vec<_>>().join("\n"));
            if repetitions > 0 {
                message.push_str(&format!(
                    "\nSeit der letzten Meldung {} weitere Treffer.",
                    repetitions
                ));
            }
            let log = CrewRadarLog {
                channel_login: channel.clone(),
                chatter_login: login.clone(),
                chatter_id: Some(id.clone()),
                account_age_days,
                style_score: style.total,
                style_breakdown: style.breakdown,
                time_window_match: matches!(Utc::now().with_timezone(&Berlin).hour(), 0..=5 | 13..=17 | 20..=22),
                messages,
                llm_verdict: verdict.to_string(),
                llm_confidence: None,
                llm_reasoning: Some(detail),
                action_taken: "none".into(),
                source: "passive_patterns".into(),
            };
            if let Err(error) = persist_radar_log(&pool, &log).await {
                error!(%error, "Crew-Guard: Muster konnte nicht gespeichert werden");
                return;
            }
            alerter.send_crew_campaign(CrewRadarAlert {
                message,
                chatter_login: login,
                chatter_id: id,
                channel_login: channel,
                style_score: style.total,
                verdict: verdict.to_string(),
                notify_only: true,
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const POSITIVES: [&str; 5] = [
        "hey mal ne frage warum ist mein freund helmbombenricky gebannt bei dir ?",
        "hast du den bot von nani drinne? du bannst unbewusst viele leute wegen der bannliste",
        "wenn du willst zeig ich dir was fuer ne scheisse nani macht, betitelt ihn als rassist",
        "komm bei uns rein, unser discord: https://discord.gg/SBRrArXjHf",
        "https://discord.gg/ZWSNyNfdG",
    ];
    const NEGATIVES: [&str; 5] = [
        "warum ist mein freund eigentlich gebannt? hab ich was verpasst",
        "nani spielt echt gut heute lol",
        "gibts ne bannliste fuer den chat oder wie",
        "ricky komm ins game",
        "welcher discord invite war das nochmal fuers turnier",
    ];

    #[test]
    fn wall_horizon_ist_bekanntes_konto() {
        match screen("alles gut bei dir?", Some("771345179")) {
            CrewSignal::HardId {
                login,
                has_evidence,
            } => {
                assert_eq!(login, "wall_horizon");
                assert!(has_evidence, "Verhaltensbeleg vom 2026-07-06 liegt vor");
            }
            other => panic!("erwartet HardId, war {other:?}"),
        }
    }

    #[test]
    fn textbasierte_positiva_ohne_invite_sind_trigger() {
        for text in POSITIVES.into_iter().take(3) {
            let signal = screen(text, None);
            assert!(
                matches!(signal, CrewSignal::Trigger { .. }),
                "erwartet Trigger für {text:?}, war {signal:?}"
            );
        }
    }

    #[test]
    fn invite_positiva_sind_hard_invite() {
        match screen(POSITIVES[3], None) {
            CrewSignal::HardInvite { code } => assert_eq!(code, "SBRrArXjHf"),
            other => panic!("erwartet HardInvite, war {other:?}"),
        }
        match screen(POSITIVES[4], None) {
            CrewSignal::HardInvite { code } => assert_eq!(code, "ZWSNyNfdG"),
            other => panic!("erwartet HardInvite, war {other:?}"),
        }
    }

    #[test]
    fn kein_negativ_ist_hart() {
        for text in NEGATIVES {
            let signal = screen(text, None);
            assert!(
                !matches!(
                    signal,
                    CrewSignal::HardId { .. } | CrewSignal::HardInvite { .. }
                ),
                "Negativ {text:?} darf nicht HART sein, war {signal:?}"
            );
        }
    }

    #[test]
    fn hard_id_per_chatter_id_erkannt() {
        match screen("hallo zusammen, alles gut?", Some("147713656")) {
            CrewSignal::HardId {
                login,
                has_evidence,
            } => {
                assert_eq!(login, "helmbombenricky");
                assert!(has_evidence);
            }
            other => panic!("erwartet HardId, war {other:?}"),
        }
    }

    #[test]
    fn hard_id_schlaegt_invite() {
        // Registriertes Konto + Rival-Invite → HardId gewinnt (Priorität).
        let signal = screen("https://discord.gg/ZWSNyNfdG", Some("595804185"));
        assert!(
            matches!(
                signal,
                CrewSignal::HardId {
                    has_evidence: false,
                    ..
                }
            ),
            "erwartet HardId (Priorität), war {signal:?}"
        );
    }

    #[test]
    fn unbekannte_id_faellt_auf_textsignal_zurueck() {
        // Fremde ID + Invite → HardInvite (kein HardId).
        match screen("https://discord.gg/W7kCyBBcf", Some("999999999")) {
            CrewSignal::HardInvite { code } => assert_eq!(code, "W7kCyBBcf"),
            other => panic!("erwartet HardInvite, war {other:?}"),
        }
    }

    #[test]
    fn harmloser_text_ohne_id_ist_none() {
        assert_eq!(screen("gg wp schönes match", None), CrewSignal::None);
    }

    #[test]
    fn kontextpuffer_liefert_vorherige_ohne_aktuelle_und_verdraengt() {
        let buf = ChatterContextBuffer::new();
        // Erste Nachricht: kein Vorlauf.
        assert!(buf.snapshot_then_push("nani", "u1", "m1").is_empty());
        // Zweite sieht m1, aber NICHT sich selbst.
        assert_eq!(
            buf.snapshot_then_push("nani", "u1", "m2"),
            vec!["m1".to_string()]
        );
        assert_eq!(
            buf.snapshot_then_push("nani", "u1", "m3"),
            vec!["m1".to_string(), "m2".to_string()]
        );
        // Anderer User im selben Kanal ist getrennt.
        assert!(buf.snapshot_then_push("nani", "u2", "x1").is_empty());
        // Fenster begrenzt: nur die letzten CONTEXT_WINDOW Nachrichten.
        for i in 0..20 {
            buf.snapshot_then_push("nani", "u3", &format!("n{i}"));
        }
        let prev = buf.snapshot_then_push("nani", "u3", "final");
        assert_eq!(prev.len(), CONTEXT_WINDOW);
        assert_eq!(prev.last().map(String::as_str), Some("n19"));
    }
}
