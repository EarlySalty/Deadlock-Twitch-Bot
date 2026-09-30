use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use tb_chat::moderation::TimeoutGuard;
use tb_chat::pipeline::BrainChatPort;
use tb_chat::types::{ChatMessageEvent, SendOutcome};
use tb_chat::ChatApi;
use tb_config::dashboard_options::{BrainClientMode, BrainClientOptions};
use tb_config::operations::BrainChatOptions;
use tb_knowledge::brain::{BrainAdapterError, BrainKnowledgeAdapter, KnowledgeReply};

const NO_EVIDENCE_REPLIES: [&str; 3] = [
    "Dazu finde ich gerade keine sichere Antwort. Frag lieber im Discord nach.",
    "Das kann ich nicht sicher belegen. Im Discord kann dir jemand weiterhelfen.",
    "Dazu habe ich keine verlässliche Antwort. Frag am besten im Discord nach.",
];
const UNUSABLE_REPLY: &str =
    "Die Antwort kann ich hier nicht sicher wiedergeben. Frag bitte im Discord nach.";

#[async_trait]
trait BrainAnswerPort: Send + Sync {
    async fn answer(
        &self,
        request_id: &str,
        conversation_id: &str,
        question: &str,
    ) -> Result<KnowledgeReply, BrainAdapterError>;
}

#[async_trait]
impl BrainAnswerPort for BrainKnowledgeAdapter {
    async fn answer(
        &self,
        request_id: &str,
        conversation_id: &str,
        question: &str,
    ) -> Result<KnowledgeReply, BrainAdapterError> {
        BrainKnowledgeAdapter::answer(self, request_id, conversation_id, question).await
    }
}

struct BrainQuestion<'a> {
    channel_id: &'a str,
    channel_login: &'a str,
    user_id: &'a str,
    message_id: &'a str,
    text: &'a str,
}

#[async_trait]
trait BrainLogPort: Send + Sync {
    async fn begin(&self, question: &BrainQuestion<'_>) -> Result<Option<i64>, String>;
    async fn finish(
        &self,
        id: i64,
        answer: &str,
        status: &str,
        duration_ms: i64,
        send_expected: bool,
    ) -> Result<(), String>;
    async fn delivery(&self, id: i64, sent: bool, duration_ms: i64) -> Result<(), String>;
}

struct PgBrainLog {
    pool: PgPool,
    options: BrainChatOptions,
}

#[async_trait]
impl BrainLogPort for PgBrainLog {
    async fn begin(&self, question: &BrainQuestion<'_>) -> Result<Option<i64>, String> {
        let mut tx = self.pool.begin().await.map_err(|error| error.to_string())?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('tb_chat_brain_answers'), 1)")
            .execute(&mut *tx)
            .await
            .map_err(|error| error.to_string())?;
        let (user_count, channel_count, daily_count): (i64, i64, i64) = sqlx::query_as(
            "SELECT \
             (SELECT COUNT(*) FROM public.tb_chat_brain_answers \
              WHERE chatter_user_id = $1 AND created_at > now() - ($2::bigint * INTERVAL '1 second')), \
             (SELECT COUNT(*) FROM public.tb_chat_brain_answers \
              WHERE broadcaster_user_id = $3 AND created_at > now() - INTERVAL '1 hour'), \
             (SELECT COUNT(*) FROM public.tb_chat_brain_answers \
              WHERE created_at >= date_trunc('day', now(), 'UTC'))",
        )
        .bind(question.user_id)
        .bind(self.options.user_cooldown_seconds as i64)
        .bind(question.channel_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
        if user_count > 0
            || channel_count >= i64::from(self.options.channel_hourly_limit)
            || daily_count >= i64::from(self.options.global_daily_limit)
        {
            return Ok(None);
        }
        let id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO public.tb_chat_brain_answers \
             (broadcaster_user_id, broadcaster_login, chatter_user_id, message_id, question, status) \
             VALUES ($1, $2, $3, $4, $5, 'Pending') \
             ON CONFLICT (broadcaster_user_id, message_id) DO NOTHING RETURNING id",
        )
        .bind(question.channel_id)
        .bind(question.channel_login)
        .bind(question.user_id)
        .bind(question.message_id)
        .bind(question.text)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
        tx.commit().await.map_err(|error| error.to_string())?;
        Ok(id)
    }

    async fn finish(
        &self,
        id: i64,
        answer: &str,
        status: &str,
        duration_ms: i64,
        send_expected: bool,
    ) -> Result<(), String> {
        let result = sqlx::query(
            "UPDATE public.tb_chat_brain_answers \
             SET answer = $2, status = $3, duration_ms = $4, finished_at = now(), \
                 delivery_status = CASE WHEN $5 THEN 'Pending' ELSE 'Fehler' END \
             WHERE id = $1 AND status = 'Pending'",
        )
        .bind(id)
        .bind(answer)
        .bind(status)
        .bind(duration_ms)
        .bind(send_expected)
        .execute(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        if result.rows_affected() != 1 {
            return Err("brain_log_update_missing".to_string());
        }
        Ok(())
    }

    async fn delivery(&self, id: i64, sent: bool, duration_ms: i64) -> Result<(), String> {
        let result = sqlx::query(
            "UPDATE public.tb_chat_brain_answers \
             SET status = CASE WHEN $2 THEN status ELSE 'Fehler' END, \
                 delivery_status = CASE WHEN $2 THEN 'Sent' ELSE 'Fehler' END, \
                 duration_ms = $3 \
             WHERE id = $1 AND status IN ('Answered', 'NoEvidence', 'Fehler') AND delivery_status = 'Pending'",
        )
        .bind(id)
        .bind(sent)
        .bind(duration_ms)
        .execute(&self.pool)
        .await
        .map_err(|error| error.to_string())?;
        if result.rows_affected() != 1 {
            return Err("brain_log_delivery_update_missing".to_string());
        }
        Ok(())
    }
}

#[derive(Default)]
struct RateState {
    users: HashMap<String, DateTime<Utc>>,
    channels: HashMap<String, VecDeque<DateTime<Utc>>>,
    day: Option<NaiveDate>,
    daily_count: u32,
}

struct BrainRateLimit {
    options: BrainChatOptions,
    state: Mutex<RateState>,
}

struct RateReservation<'a> {
    limit: &'a BrainRateLimit,
    user_id: &'a str,
    channel_id: &'a str,
    at: DateTime<Utc>,
    committed: bool,
}

impl RateReservation<'_> {
    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for RateReservation<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.limit.release(self.user_id, self.channel_id, self.at);
        }
    }
}

impl BrainRateLimit {
    fn new(options: BrainChatOptions) -> Self {
        Self {
            options,
            state: Mutex::new(RateState::default()),
        }
    }

    fn reserve<'a>(
        &'a self,
        user_id: &'a str,
        channel_id: &'a str,
        now: DateTime<Utc>,
    ) -> Option<RateReservation<'a>> {
        if user_id.is_empty() || channel_id.is_empty() {
            return None;
        }
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let cutoff = now - chrono::Duration::hours(1);
        state.users.retain(|_, last| {
            now.signed_duration_since(*last).num_seconds()
                < self.options.user_cooldown_seconds as i64
        });
        state.channels.retain(|_, times| {
            while times.front().is_some_and(|last| *last <= cutoff) {
                times.pop_front();
            }
            !times.is_empty()
        });
        if state.day != Some(now.date_naive()) {
            state.day = Some(now.date_naive());
            state.daily_count = 0;
        }
        if state.daily_count >= self.options.global_daily_limit
            || state.users.contains_key(user_id)
            || state
                .channels
                .get(channel_id)
                .is_some_and(|times| times.len() >= self.options.channel_hourly_limit as usize)
        {
            return None;
        }
        state.users.insert(user_id.to_string(), now);
        state
            .channels
            .entry(channel_id.to_string())
            .or_default()
            .push_back(now);
        state.daily_count += 1;
        Some(RateReservation {
            limit: self,
            user_id,
            channel_id,
            at: now,
            committed: false,
        })
    }

    fn release(&self, user_id: &str, channel_id: &str, at: DateTime<Utc>) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.users.get(user_id) == Some(&at) {
            state.users.remove(user_id);
        }
        if let Some(times) = state.channels.get_mut(channel_id) {
            if let Some(index) = times.iter().position(|time| *time == at) {
                times.remove(index);
            }
            if times.is_empty() {
                state.channels.remove(channel_id);
            }
        }
        if state.day == Some(at.date_naive()) {
            state.daily_count = state.daily_count.saturating_sub(1);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BackendState {
    Unknown,
    Available,
    Unavailable,
}

struct BrainChatService {
    answerer: Option<Arc<dyn BrainAnswerPort>>,
    log: Arc<dyn BrainLogPort>,
    api: Arc<dyn ChatApi>,
    timeout_guard: Arc<TimeoutGuard>,
    bot_user_id: String,
    bot_login: String,
    limit: BrainRateLimit,
    backend_state: Mutex<BackendState>,
    no_evidence_index: AtomicUsize,
}

impl BrainChatService {
    fn backend_state(&self, current: BackendState) {
        let mut previous = self
            .backend_state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if current == BackendState::Unavailable {
            tb_observability::warning_budget::warn(
                "brain_backend",
                "Brain-Antwortdienst nicht erreichbar, Chat-Antworten bleiben aus",
            );
        }
        *previous = current;
    }

    fn no_evidence_reply(&self) -> &'static str {
        NO_EVIDENCE_REPLIES
            [self.no_evidence_index.fetch_add(1, Ordering::Relaxed) % NO_EVIDENCE_REPLIES.len()]
    }

    async fn finish(
        &self,
        id: i64,
        answer: &str,
        status: &str,
        send_expected: bool,
        started: Instant,
    ) -> bool {
        for attempt in 0..3 {
            let duration_ms = started.elapsed().as_millis().min(i64::MAX as u128) as i64;
            match self
                .log
                .finish(id, answer, status, duration_ms, send_expected)
                .await
            {
                Ok(()) => return true,
                Err(_) if attempt == 2 => {
                    tb_observability::warning_budget::warn(
                        "brain_log_finish",
                        "Brain-Chat-Protokoll konnte nicht abgeschlossen werden",
                    );
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(100 * (1 << attempt))).await,
            }
        }
        false
    }

    async fn delivery(&self, id: i64, sent: bool, started: Instant) {
        for attempt in 0..3 {
            let duration_ms = started.elapsed().as_millis().min(i64::MAX as u128) as i64;
            match self.log.delivery(id, sent, duration_ms).await {
                Ok(()) => return,
                Err(_) if attempt == 2 => {
                    tb_observability::warning_budget::warn(
                        "brain_log_delivery",
                        "Brain-Chat-Zustellung bleibt ungeklärt",
                    );
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(100 * (1 << attempt))).await,
            }
        }
    }
}

#[async_trait]
impl BrainChatPort for BrainChatService {
    async fn maybe_respond(&self, event: &ChatMessageEvent) -> bool {
        if event.chatter_user_id == self.bot_user_id
            || event.message_id.is_empty()
            || event.text().starts_with('!')
        {
            return false;
        }
        let Some(question) = question_for_bot(event.text(), &self.bot_login) else {
            return false;
        };
        if self.timeout_guard.is_muted(&event.broadcaster_user_login) {
            return true;
        }
        let Some(answerer) = &self.answerer else {
            self.backend_state(BackendState::Unavailable);
            return true;
        };
        let Some(reservation) = self.limit.reserve(
            &event.chatter_user_id,
            &event.broadcaster_user_id,
            Utc::now(),
        ) else {
            return true;
        };
        let started = Instant::now();
        let record = BrainQuestion {
            channel_id: &event.broadcaster_user_id,
            channel_login: &event.broadcaster_user_login,
            user_id: &event.chatter_user_id,
            message_id: &event.message_id,
            text: &question,
        };
        let id = match self.log.begin(&record).await {
            Ok(Some(id)) => id,
            Ok(None) => return true,
            Err(_) => {
                tb_observability::warning_budget::warn(
                    "brain_log_begin",
                    "Brain-Chat-Protokoll nicht verfügbar, Antwort unterdrückt",
                );
                return true;
            }
        };
        reservation.commit();
        let conversation_id = format!("{}-{}", record.channel_id, record.user_id);
        let (text, status) = match answerer
            .answer(record.message_id, &conversation_id, &question)
            .await
        {
            Ok(KnowledgeReply::Answered { text, .. }) => {
                self.backend_state(BackendState::Available);
                match safe_chat_answer(&text) {
                    Some(text) => (text, "Answered"),
                    None => (UNUSABLE_REPLY.to_string(), "Fehler"),
                }
            }
            Ok(KnowledgeReply::NoEvidence) => {
                self.backend_state(BackendState::Available);
                (self.no_evidence_reply().to_string(), "NoEvidence")
            }
            Err(error) => {
                if error == BrainAdapterError::Backend {
                    self.backend_state(BackendState::Unavailable);
                } else {
                    tb_observability::warning_budget::warn(
                        "brain_question",
                        "Brain-Chat-Frage abgelehnt",
                    );
                }
                let _ = self.finish(id, "", "Fehler", false, started).await;
                return true;
            }
        };
        if !self.finish(id, &text, status, true, started).await {
            return true;
        }
        let send = self
            .api
            .send_thread_reply(record.channel_id, record.message_id, &text)
            .await;
        let delivered = matches!(send, Ok(SendOutcome::Sent));
        if !delivered {
            tb_observability::warning_budget::warn(
                "brain_send",
                "Brain-Chat-Antwort nicht zugestellt",
            );
        }
        self.delivery(id, delivered, started).await;
        true
    }
}

pub struct BrainChatBuild<'a> {
    pub client: &'a BrainClientOptions,
    pub options: &'a BrainChatOptions,
    pub token: &'a str,
    pub bot_login: &'a str,
    pub bot_user_id: &'a str,
    pub api: Arc<dyn ChatApi>,
    pub timeout_guard: Arc<TimeoutGuard>,
    pub pool: PgPool,
}

pub fn build(
    BrainChatBuild {
        client,
        options,
        token,
        bot_login,
        bot_user_id,
        api,
        timeout_guard,
        pool,
    }: BrainChatBuild<'_>,
) -> Option<Arc<dyn BrainChatPort>> {
    if !options.enabled || client.mode != BrainClientMode::Typed {
        return None;
    }
    let answerer = if token.trim().is_empty() {
        tb_observability::warning_budget::warn(
            "brain_backend",
            "Brain-Chat-Adapter ohne Dienstzugang, Antworten bleiben aus",
        );
        None
    } else {
        BrainKnowledgeAdapter::new(
            client.endpoint.as_deref().unwrap_or_default(),
            token,
            Duration::from_millis(client.timeout_ms.unwrap_or(8_000)),
            client.public_scopes.iter().cloned().collect(),
        )
        .map(|adapter| Arc::new(adapter) as Arc<dyn BrainAnswerPort>)
        .map_err(|_| {
            tb_observability::warning_budget::warn(
                "brain_backend",
                "Brain-Chat-Adapter konnte nicht gestartet werden",
            );
        })
        .ok()
    };
    let backend_state = if answerer.is_some() {
        BackendState::Unknown
    } else {
        BackendState::Unavailable
    };
    Some(Arc::new(BrainChatService {
        answerer,
        log: Arc::new(PgBrainLog {
            pool,
            options: options.clone(),
        }),
        api,
        timeout_guard,
        bot_user_id: bot_user_id.to_string(),
        bot_login: bot_login.to_string(),
        limit: BrainRateLimit::new(options.clone()),
        backend_state: Mutex::new(backend_state),
        no_evidence_index: AtomicUsize::new(0),
    }))
}

fn question_for_bot(text: &str, bot_login: &str) -> Option<String> {
    if bot_login.is_empty() {
        return None;
    }
    let mut question = String::with_capacity(text.len());
    let mut last = 0;
    let mut found = false;
    for (index, character) in text.char_indices() {
        if character != '@' {
            continue;
        }
        let after = index + character.len_utf8();
        let Some(candidate) = text.get(after..after + bot_login.len()) else {
            continue;
        };
        let before_ok = index == 0
            || !text[..index]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        let after_ok = !text[after + bot_login.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        if before_ok && after_ok && candidate.eq_ignore_ascii_case(bot_login) {
            question.push_str(&text[last..index]);
            question.push(' ');
            last = after + bot_login.len();
            found = true;
        }
    }
    if !found {
        return None;
    }
    question.push_str(&text[last..]);
    let question = question
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" ,", ",")
        .replace(" :", ":");
    (question.chars().any(char::is_alphanumeric) && question.chars().count() <= 500)
        .then_some(question)
}

fn looks_like_link(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    if lower.contains("://") || lower.contains("www.") {
        return true;
    }
    lower
        .split(['(', ')', '[', ']', '<', '>', '"', '\'', ',', ';'])
        .any(|part| {
            let authority = part
                .trim_matches(|c: char| !c.is_alphanumeric())
                .split(['/', '?', '#'])
                .next()
                .unwrap_or_default();
            let Ok(parsed) = url::Url::parse(&format!("https://{authority}")) else {
                return false;
            };
            match parsed.host() {
                Some(url::Host::Ipv4(_)) => {
                    authority
                        .rsplit('@')
                        .next()
                        .unwrap_or_default()
                        .split(':')
                        .next()
                        .unwrap_or_default()
                        .split('.')
                        .count()
                        == 4
                }
                Some(url::Host::Ipv6(_)) => true,
                Some(url::Host::Domain(domain)) => {
                    domain.rsplit_once('.').is_some_and(|(_, tld)| {
                        (2..=63).contains(&tld.len())
                            && (tld.bytes().all(|c| c.is_ascii_alphabetic())
                                || tld.starts_with("xn--"))
                    })
                }
                None => false,
            }
        })
}

fn has_substantive_answer(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let word =
            token.trim_matches(|character: char| !character.is_alphanumeric() && character != '＠');
        if word.is_empty() || word.starts_with('＠') {
            return false;
        }
        !matches!(
            word.to_lowercase().as_str(),
            "siehe"
                | "mehr"
                | "auf"
                | "unter"
                | "bei"
                | "im"
                | "in"
                | "und"
                | "oder"
                | "links"
                | "link"
                | "hier"
                | "dort"
                | "infos"
                | "informationen"
                | "weitere"
                | "weiteres"
                | "zu"
                | "zum"
                | "zur"
                | "quelle"
        )
    })
}

fn strip_markdown_links(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut offset = 0;
    while let Some(open) = input[offset..].find('[').map(|index| offset + index) {
        if let Some(label_end) = input[open + 1..].find("](").map(|index| open + 1 + index) {
            if let Some(close) = input[label_end + 2..]
                .find(')')
                .map(|index| label_end + 2 + index)
            {
                if looks_like_link(&input[label_end + 2..close]) {
                    output.push_str(&input[offset..open]);
                    output.push(' ');
                    offset = close + 1;
                    continue;
                }
            }
        }
        output.push_str(&input[offset..open + 1]);
        offset = open + 1;
    }
    output.push_str(&input[offset..]);
    output
}

fn safe_chat_answer(input: &str) -> Option<String> {
    // Erst normalisieren: Nach der Linkprüfung entfernte Steuerzeichen könnten
    // sonst aus einem ungültigen Token wieder eine gültige Domain machen.
    let normalized = input
        .chars()
        .filter(|character| !character.is_control() || character.is_whitespace())
        .collect::<String>();
    let input = strip_markdown_links(&normalized);
    let cleaned = input
        .split_whitespace()
        .filter(|token| !looks_like_link(token))
        .map(|token| {
            token
                .chars()
                .map(|character| if character == '@' { '＠' } else { character })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ");
    let text = cleaned.trim();
    if !has_substantive_answer(text) {
        return None;
    }
    if text.chars().count() <= 450 {
        return Some(text.to_string());
    }
    let prefix = text.chars().take(450).collect::<String>();
    if let Some((index, character)) = prefix.char_indices().rfind(|(index, character)| {
        matches!(character, '.' | '!' | '?')
            && text[index + character.len_utf8()..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace)
    }) {
        let answer = &prefix[..index + character.len_utf8()];
        return has_substantive_answer(answer).then(|| answer.to_string());
    }
    None
}

#[cfg(test)]
use crate::chat_wiring::invite_test_postgres as brain_test_postgres;

#[cfg(test)]
mod tests {
    use super::*;
    use tb_chat::types::ChatMessageBody;

    #[tokio::test]
    async fn postgres_protokoll_und_rollen_sind_wirksam() {
        let db = brain_test_postgres::TestPostgres::start().await;
        sqlx::raw_sql("CREATE ROLE twitchbot; CREATE ROLE twitchdash;")
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260929113000_chat_brain_answers.sql"
        ))
        .execute(&db.pool)
        .await
        .unwrap();
        let log = PgBrainLog {
            pool: db.pool.clone(),
            options: BrainChatOptions::default(),
        };
        let question = BrainQuestion {
            channel_id: "200",
            channel_login: "streamer",
            user_id: "100",
            message_id: "message-1",
            text: "Wie viele Fähigkeiten hat Warden?",
        };
        let id = log.begin(&question).await.unwrap().unwrap();
        sqlx::query(
            "UPDATE public.tb_chat_brain_answers \
             SET created_at = now() - INTERVAL '2 hours' WHERE id = $1",
        )
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
        assert!(log.begin(&question).await.unwrap().is_none());
        log.finish(id, "Vier Fähigkeiten.", "Answered", 24, true)
            .await
            .unwrap();
        let row: (String, String, String, i64, bool) = sqlx::query_as(
            "SELECT question, answer, delivery_status, duration_ms, finished_at IS NOT NULL \
             FROM public.tb_chat_brain_answers WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(
            row,
            (
                question.text.into(),
                "Vier Fähigkeiten.".into(),
                "Pending".into(),
                24,
                true
            )
        );
        log.delivery(id, true, 31).await.unwrap();
        let sent: (String, String, i64) = sqlx::query_as(
            "SELECT status, delivery_status, duration_ms FROM public.tb_chat_brain_answers WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(sent, ("Answered".into(), "Sent".into(), 31));
        let failed = BrainQuestion {
            message_id: "message-2",
            user_id: "101",
            ..question
        };
        let failed_id = log.begin(&failed).await.unwrap().unwrap();
        log.finish(failed_id, "Nicht sicher.", "NoEvidence", 10, true)
            .await
            .unwrap();
        log.delivery(failed_id, false, 20).await.unwrap();
        let undelivered: (String, String, String) = sqlx::query_as(
            "SELECT status, delivery_status, answer FROM public.tb_chat_brain_answers WHERE id = $1",
        )
        .bind(failed_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(
            undelivered,
            ("Fehler".into(), "Fehler".into(), "Nicht sicher.".into())
        );
        let unusable = BrainQuestion {
            message_id: "message-3",
            user_id: "102",
            ..question
        };
        let unusable_id = log.begin(&unusable).await.unwrap().unwrap();
        log.finish(unusable_id, UNUSABLE_REPLY, "Fehler", 10, true)
            .await
            .unwrap();
        log.delivery(unusable_id, true, 20).await.unwrap();
        let unusable_row: (String, String) = sqlx::query_as(
            "SELECT status, delivery_status FROM public.tb_chat_brain_answers WHERE id = $1",
        )
        .bind(unusable_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(unusable_row, ("Fehler".into(), "Sent".into()));
        let backend_error = BrainQuestion {
            message_id: "message-4",
            user_id: "103",
            ..question
        };
        let backend_id = log.begin(&backend_error).await.unwrap().unwrap();
        log.finish(backend_id, "", "Fehler", 10, false)
            .await
            .unwrap();
        let backend_row: (String, String) = sqlx::query_as(
            "SELECT status, delivery_status FROM public.tb_chat_brain_answers WHERE id = $1",
        )
        .bind(backend_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(backend_row, ("Fehler".into(), "Fehler".into()));
        let bot_can_insert: bool = sqlx::query_scalar(
            "SELECT has_table_privilege('twitchbot', 'public.tb_chat_brain_answers', 'INSERT')",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        let bot_can_use_sequence: bool = sqlx::query_scalar(
            "SELECT has_sequence_privilege('twitchbot', 'public.tb_chat_brain_answers_id_seq', 'USAGE')",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        let dashboard_can_update: bool = sqlx::query_scalar(
            "SELECT has_table_privilege('twitchdash', 'public.tb_chat_brain_answers', 'UPDATE')",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert!(bot_can_insert);
        assert!(bot_can_use_sequence);
        assert!(!dashboard_can_update);
    }

    #[tokio::test]
    async fn postgres_limits_ueberleben_einen_neuen_prozesszustand() {
        let db = brain_test_postgres::TestPostgres::start().await;
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260929113000_chat_brain_answers.sql"
        ))
        .execute(&db.pool)
        .await
        .unwrap();
        let options = BrainChatOptions {
            user_cooldown_seconds: 60,
            channel_hourly_limit: 2,
            global_daily_limit: 3,
            ..BrainChatOptions::default()
        };
        let first = PgBrainLog {
            pool: db.pool.clone(),
            options: options.clone(),
        };
        let first_question = BrainQuestion {
            channel_id: "200",
            channel_login: "streamer",
            user_id: "100",
            message_id: "message-1",
            text: "Frage eins?",
        };
        assert!(first.begin(&first_question).await.unwrap().is_some());
        let restarted = PgBrainLog {
            pool: db.pool.clone(),
            options,
        };
        for (channel_id, user_id, message_id, allowed) in [
            ("200", "100", "message-2", false),
            ("200", "101", "message-3", true),
            ("200", "102", "message-4", false),
            ("201", "102", "message-5", true),
            ("201", "103", "message-6", false),
        ] {
            let question = BrainQuestion {
                channel_id,
                channel_login: "streamer",
                user_id,
                message_id,
                text: "Noch eine Frage?",
            };
            assert_eq!(restarted.begin(&question).await.unwrap().is_some(), allowed);
        }
    }

    #[test]
    fn mention_braucht_eigenen_login_und_inhalt() {
        assert_eq!(
            question_for_bot("@DeadlockBot wie viele Fähigkeiten?", "deadlockbot"),
            Some("wie viele Fähigkeiten?".into())
        );
        assert_eq!(
            question_for_bot("Hi @deadlockbot, kannst du helfen?", "DeadlockBot"),
            Some("Hi, kannst du helfen?".into())
        );
        for text in [
            "@DeadlockBot",
            "@DeadlockBot ?!",
            "@andererBot Frage?",
            "@DeadlockBotExtra Frage?",
            "mail@DeadlockBot.com",
        ] {
            assert_eq!(question_for_bot(text, "deadlockbot"), None, "{text}");
        }
    }

    #[test]
    fn antwort_entfernt_links_mentions_und_kuerzt_am_satzende() {
        assert_eq!(
            safe_chat_answer("Siehe https://example.com/x und www.example.org @everyone."),
            None
        );
        assert_eq!(safe_chat_answer("Ja."), Some("Ja.".into()));
        assert_eq!(
            safe_chat_answer(
                "Links: example.com:443, 127.0.0.1/path, info@example.org und Warden."
            ),
            Some("Links: und Warden.".into())
        );
        assert_eq!(
            safe_chat_answer("Quelle [Guide](https://example.com) [Kurz](example.com) und Warden."),
            Some("Quelle und Warden.".into())
        );
        assert_eq!(safe_chat_answer("[The guide](https://example.com)"), None);
        assert_eq!(
            safe_chat_answer("[The guide](https://example.com) Warden hat vier Fähigkeiten."),
            Some("Warden hat vier Fähigkeiten.".into())
        );
        assert_eq!(
            safe_chat_answer("Mehr auf bücher.de, 例子.中国 und xn--bcher-kva.de."),
            None
        );
        assert_eq!(safe_chat_answer("Siehe example.com"), None);
        for control in ['\0', '\u{0007}', '\u{001b}', '\u{007f}', '\u{0080}'] {
            assert_eq!(
                safe_chat_answer(&format!("Siehe example{control}.com")),
                None
            );
            assert_eq!(
                safe_chat_answer(&format!(
                    "[Der Guide](example{control}.com) Warden hat vier Fähigkeiten."
                )),
                Some("Warden hat vier Fähigkeiten.".into())
            );
        }
        assert_eq!(
            safe_chat_answer("Warden\n hat\t vier Fähig\0keiten."),
            Some("Warden hat vier Fähigkeiten.".into())
        );
        let long = format!("{} Satzende. {}", "A".repeat(250), "B".repeat(240));
        assert_eq!(
            safe_chat_answer(&long),
            Some(format!("{} Satzende.", "A".repeat(250)))
        );
        let version = format!("{} Version 1.2.3. {}", "A".repeat(250), "B".repeat(230));
        let shortened = safe_chat_answer(&version).unwrap();
        assert!(shortened.ends_with("Version 1.2.3."));
        assert!(shortened.chars().count() <= 450);
        assert_eq!(safe_chat_answer(&"Wort ".repeat(100)), None);
        assert_eq!(safe_chat_answer(&"Ä".repeat(800)), None);
        assert_eq!(safe_chat_answer("https://example.com"), None);
    }

    #[test]
    fn limits_gelten_fuer_ids_und_utc_tag() {
        let options = BrainChatOptions {
            user_cooldown_seconds: 60,
            channel_hourly_limit: 2,
            global_daily_limit: 3,
            ..BrainChatOptions::default()
        };
        let limits = BrainRateLimit::new(options);
        let now = DateTime::parse_from_rfc3339("2026-09-29T23:59:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let accepted = |user, channel, at| {
            limits
                .reserve(user, channel, at)
                .map(|reservation| reservation.commit())
                .is_some()
        };
        assert!(accepted("101", "201", now));
        assert!(!accepted("101", "202", now + chrono::Duration::seconds(59)));
        assert!(accepted("102", "201", now));
        assert!(!accepted("103", "201", now));
        assert!(accepted("103", "202", now));
        assert!(!accepted("104", "202", now));
        assert!(accepted("104", "202", now + chrono::Duration::minutes(2)));
    }

    #[test]
    fn nicht_protokollierte_reservierung_gibt_quoten_frei() {
        let limits = BrainRateLimit::new(BrainChatOptions {
            user_cooldown_seconds: 60,
            channel_hourly_limit: 1,
            global_daily_limit: 1,
            ..BrainChatOptions::default()
        });
        let now = DateTime::parse_from_rfc3339("2026-09-29T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let abandoned = limits.reserve("100", "200", now).unwrap();
        assert!(limits.reserve("101", "200", now).is_none());
        drop(abandoned);
        let recovered = limits.reserve("100", "200", now).unwrap();
        recovered.commit();
        assert!(limits.reserve("101", "200", now).is_none());
    }

    #[test]
    fn no_evidence_wechselt_zwischen_ehrlichen_antworten() {
        assert_eq!(NO_EVIDENCE_REPLIES.len(), 3);
        assert!(NO_EVIDENCE_REPLIES
            .iter()
            .all(|reply| safe_chat_answer(reply).as_deref() == Some(*reply)));
    }

    struct FakeAnswer(Mutex<VecDeque<Result<KnowledgeReply, BrainAdapterError>>>);

    #[async_trait]
    impl BrainAnswerPort for FakeAnswer {
        async fn answer(
            &self,
            _request_id: &str,
            _conversation_id: &str,
            _question: &str,
        ) -> Result<KnowledgeReply, BrainAdapterError> {
            self.0.lock().unwrap().pop_front().unwrap()
        }
    }

    #[derive(Default)]
    struct FakeLog {
        questions: Mutex<Vec<(String, String)>>,
        results: Mutex<Vec<(String, String, i64)>>,
        deliveries: Mutex<Vec<(bool, i64)>>,
        fail_finishes: AtomicUsize,
    }

    #[async_trait]
    impl BrainLogPort for FakeLog {
        async fn begin(&self, question: &BrainQuestion<'_>) -> Result<Option<i64>, String> {
            let mut questions = self.questions.lock().unwrap();
            questions.push((question.user_id.to_string(), question.text.to_string()));
            Ok(Some(questions.len() as i64))
        }

        async fn finish(
            &self,
            _id: i64,
            answer: &str,
            status: &str,
            duration_ms: i64,
            _send_expected: bool,
        ) -> Result<(), String> {
            if self
                .fail_finishes
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                    (left > 0).then(|| left - 1)
                })
                .is_ok()
            {
                return Err("temporary_log_failure".into());
            }
            self.results
                .lock()
                .unwrap()
                .push((answer.into(), status.into(), duration_ms));
            Ok(())
        }

        async fn delivery(&self, _id: i64, sent: bool, duration_ms: i64) -> Result<(), String> {
            self.deliveries.lock().unwrap().push((sent, duration_ms));
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeChat {
        sends: Mutex<Vec<(String, String, String)>>,
        audit: Option<Arc<FakeLog>>,
    }

    #[async_trait]
    impl ChatApi for FakeChat {
        async fn send_message(&self, _: &str, _: &str) -> Result<SendOutcome, String> {
            Err("plain_send_forbidden".into())
        }
        async fn send_thread_reply(
            &self,
            channel: &str,
            parent: &str,
            text: &str,
        ) -> Result<SendOutcome, String> {
            if let Some(log) = &self.audit {
                assert_eq!(
                    log.results.lock().unwrap().len(),
                    self.sends.lock().unwrap().len() + 1
                );
            }
            self.sends
                .lock()
                .unwrap()
                .push((channel.into(), parent.into(), text.into()));
            Ok(SendOutcome::Sent)
        }
        async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
            Err("unused".into())
        }
        async fn ban_user(&self, _: &str, _: &str, _: &str) -> Result<tb_chat::BanOutcome, String> {
            Err("unused".into())
        }
        async fn timeout_user(
            &self,
            _: &str,
            _: &str,
            _: u32,
            _: &str,
        ) -> Result<tb_chat::BanOutcome, String> {
            Err("unused".into())
        }
        async fn unban_user(&self, _: &str, _: &str) -> Result<bool, String> {
            Err("unused".into())
        }
        async fn delete_message(&self, _: &str, _: &str) -> Result<bool, String> {
            Err("unused".into())
        }
        async fn user_created_at(&self, _: &str) -> Result<Option<DateTime<Utc>>, String> {
            Err("unused".into())
        }
        async fn resolve_user_id(&self, _: &str) -> Result<Option<String>, String> {
            Err("unused".into())
        }
        async fn bot_user_id(&self) -> String {
            "999".into()
        }
    }

    fn event(index: usize) -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "200".into(),
            broadcaster_user_login: "streamer".into(),
            chatter_user_id: (100 + index).to_string(),
            message_id: format!("message-{index}"),
            message: ChatMessageBody {
                text: "@DeadlockBot wie viele Fähigkeiten hat Warden?".into(),
                ..ChatMessageBody::default()
            },
            ..ChatMessageEvent::default()
        }
    }

    #[tokio::test]
    async fn fake_brain_antwortet_als_reply_und_protokolliert_jeden_status() {
        let answerer = Arc::new(FakeAnswer(Mutex::new(VecDeque::from([
            Ok(KnowledgeReply::Answered {
                text: "Vier Fähigkeiten. https://example.com @everyone".into(),
                sources: vec![],
            }),
            Ok(KnowledgeReply::Answered {
                text: "[The guide](https://example.com)".into(),
                sources: vec![],
            }),
            Ok(KnowledgeReply::Answered {
                text: "A".repeat(460),
                sources: vec![],
            }),
            Ok(KnowledgeReply::NoEvidence),
            Ok(KnowledgeReply::NoEvidence),
            Ok(KnowledgeReply::NoEvidence),
            Err(BrainAdapterError::Backend),
        ]))));
        let log = Arc::new(FakeLog::default());
        let chat = Arc::new(FakeChat {
            audit: Some(log.clone()),
            ..FakeChat::default()
        });
        let service = BrainChatService {
            answerer: Some(answerer),
            log: log.clone(),
            api: chat.clone(),
            timeout_guard: Arc::new(TimeoutGuard::new()),
            bot_user_id: "999".into(),
            bot_login: "deadlockbot".into(),
            limit: BrainRateLimit::new(BrainChatOptions::default()),
            backend_state: Mutex::new(BackendState::Unknown),
            no_evidence_index: AtomicUsize::new(0),
        };
        for index in 0..7 {
            assert!(service.maybe_respond(&event(index)).await);
        }
        let sends = chat.sends.lock().unwrap().clone();
        assert_eq!(sends.len(), 6);
        assert_eq!(
            sends[0],
            (
                "200".into(),
                "message-0".into(),
                "Vier Fähigkeiten. ＠everyone".into()
            )
        );
        assert_eq!(sends[1].2, UNUSABLE_REPLY);
        assert_eq!(sends[2].2, UNUSABLE_REPLY);
        assert_eq!(sends[3].2, NO_EVIDENCE_REPLIES[0]);
        assert_eq!(sends[4].2, NO_EVIDENCE_REPLIES[1]);
        assert_eq!(sends[5].2, NO_EVIDENCE_REPLIES[2]);
        let records = log.results.lock().unwrap().clone();
        assert_eq!(
            records.iter().map(|row| row.1.as_str()).collect::<Vec<_>>(),
            [
                "Answered",
                "Fehler",
                "Fehler",
                "NoEvidence",
                "NoEvidence",
                "NoEvidence",
                "Fehler"
            ]
        );
        assert!(records.iter().all(|row| row.2 >= 0));
        assert_eq!(log.deliveries.lock().unwrap().len(), 6);
        assert!(log.deliveries.lock().unwrap().iter().all(|row| row.0));
        assert_eq!(log.questions.lock().unwrap().len(), 7);
        assert!(matches!(
            *service.backend_state.lock().unwrap(),
            BackendState::Unavailable
        ));
        let mut self_message = event(8);
        self_message.chatter_user_id = "999".into();
        assert!(!service.maybe_respond(&self_message).await);
        let mut command = event(9);
        command.message.text = "!status @DeadlockBot".into();
        assert!(!service.maybe_respond(&command).await);
        assert_eq!(log.questions.lock().unwrap().len(), 7);
    }

    #[tokio::test]
    async fn antwort_bleibt_aus_wenn_audit_nicht_gespeichert_werden_kann() {
        let answerer = Arc::new(FakeAnswer(Mutex::new(VecDeque::from([Ok(
            KnowledgeReply::Answered {
                text: "Vier Fähigkeiten.".into(),
                sources: vec![],
            },
        )]))));
        let log = Arc::new(FakeLog::default());
        log.fail_finishes.store(3, Ordering::Relaxed);
        let chat = Arc::new(FakeChat::default());
        let service = BrainChatService {
            answerer: Some(answerer),
            log: log.clone(),
            api: chat.clone(),
            timeout_guard: Arc::new(TimeoutGuard::new()),
            bot_user_id: "999".into(),
            bot_login: "deadlockbot".into(),
            limit: BrainRateLimit::new(BrainChatOptions::default()),
            backend_state: Mutex::new(BackendState::Unknown),
            no_evidence_index: AtomicUsize::new(0),
        };
        assert!(service.maybe_respond(&event(0)).await);
        assert!(chat.sends.lock().unwrap().is_empty());
        assert!(log.results.lock().unwrap().is_empty());
        assert!(log.deliveries.lock().unwrap().is_empty());
        assert_eq!(log.fail_finishes.load(Ordering::Relaxed), 0);
    }
}
