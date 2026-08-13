use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::minimax_chat::TestModeRejectReason;
use crate::stream_transcripts::StreamTranscriptSegment;

pub const SESSION_DURATION: Duration = Duration::minutes(60);
pub const CHANNEL_COOLDOWN: Duration = Duration::hours(24);
const DISCORD_CLAIM_TTL: Duration = Duration::minutes(2);
const DISCORD_RETRY_DELAY: Duration = Duration::seconds(30);
const DISCORD_DELETE_RETRY_DELAY: Duration = Duration::hours(1);

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database")]
    Database(#[from] sqlx::Error),
    #[error("invalid_data")]
    InvalidData,
}

#[derive(Clone)]
pub struct SmalltalkLoopStore {
    pool: PgPool,
    /// Nur Log-Drosselung, kein fachlicher Zustand: zuletzt gemeldete Lage und
    /// wann. Der Loop teilt sich den Store per `clone`, deshalb `Arc`.
    last_no_candidate: NoCandidateLogState,
}

/// Zuletzt gemeldete Lage und Zeitpunkt, geteilt über alle Klone des Stores.
type NoCandidateLogState = Arc<Mutex<Option<(CandidateStats, DateTime<Utc>)>>>;

#[derive(Clone, Debug)]
pub struct SmalltalkSession {
    pub id: Uuid,
    pub channel_login: String,
    pub streamer_user_id: String,
    pub started_at: DateTime<Utc>,
    pub viewer_count: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneratedOutcome {
    WouldSend,
    Rejected(TestModeRejectReason),
}

impl GeneratedOutcome {
    fn values(self) -> (&'static str, Option<&'static str>) {
        match self {
            Self::WouldSend => ("would_send", None),
            Self::Rejected(reason) => ("rejected", Some(reason.as_str())),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SmalltalkMessage {
    pub generated_at: DateTime<Utc>,
    pub generated_text: String,
    pub trigger_text: String,
    pub outcome: String,
    pub reject_reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ReportSession {
    pub id: Uuid,
    pub channel_login: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub end_reason: String,
    pub viewer_count: Option<i32>,
    pub provider_error_count: i32,
    pub last_provider_error: Option<String>,
}

/// Ein lokal transkribierter Stream-Abschnitt aus der Sitzung.
#[derive(Clone, Debug)]
pub struct SmalltalkTranscript {
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub text: String,
    pub engine: String,
    pub model: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SmalltalkReport {
    pub session: ReportSession,
    pub messages: Vec<SmalltalkMessage>,
    /// Stream-Ton der Sitzung, chronologisch. Ohne ihn ist nicht zu beurteilen,
    /// ob eine Nachricht zum Moment gepasst hat oder nur zum Chat.
    pub transcripts: Vec<SmalltalkTranscript>,
}

#[derive(Clone, Debug)]
pub struct ClaimedReport {
    pub claim_id: Uuid,
    pub report: SmalltalkReport,
}

#[derive(Clone, Debug)]
pub struct ExpiredDiscordReport {
    pub session_id: Uuid,
    pub message_id: String,
}

#[derive(sqlx::FromRow)]
struct CandidateRow {
    channel_login: String,
    streamer_user_id: String,
    is_live: bool,
    game: Option<String>,
    viewer_count: Option<i32>,
    cooldown_until: Option<String>,
    partner_table: bool,
    partner_state: bool,
    /// Bot in diesem Kanal gebannt oder von Hand gesperrt
    /// (`twitch_raid_blacklist`, gesetzt u.a. von
    /// `token_lifecycle::mark_bot_banned_inner`).
    blacklisted: bool,
    last_observed_at: Option<DateTime<Utc>>,
    detected_at: String,
}

struct Candidate {
    channel_login: String,
    streamer_user_id: String,
    viewer_count: Option<i32>,
    last_observed_at: Option<DateTime<Utc>>,
    detected_at: String,
}

#[derive(sqlx::FromRow)]
struct SessionSettingsRow {
    id: Uuid,
    channel_login: String,
    streamer_user_id: String,
    started_at: DateTime<Utc>,
    viewer_count: Option<i32>,
    settings_existed: bool,
    previous_enabled: bool,
    previous_irc_read: bool,
    /// Ausgabemodus vor der Testsession — wird beim Schließen wieder
    /// gesetzt, sonst bliebe ein `shadow`/`live`-Kanal dauerhaft stumm.
    previous_output_mode: Option<String>,
}

impl SmalltalkLoopStore {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            last_no_candidate: Arc::new(Mutex::new(None)),
        }
    }

    /// Meldet, dass diese Runde keinen Kanal gefunden hat, und warum nicht.
    /// Ohne diese Zeile sieht "keine Kanaele uebrig" genauso aus wie "die
    /// Auswahl-Query ist kaputt": in beiden Faellen steht nichts im Journal.
    fn log_no_candidate(&self, stats: CandidateStats, now: DateTime<Utc>) {
        let mut last = self.last_no_candidate.lock().expect("log-drossel");
        if !should_log_no_candidate(*last, stats, now) {
            return;
        }
        *last = Some((stats, now));
        drop(last);
        tracing::info!(
            event = "smalltalk_loop.no_candidate",
            checked = stats.checked,
            offline = stats.offline,
            other_game = stats.other_game,
            partner = stats.partner,
            blacklisted = stats.blacklisted,
            cooldown = stats.cooldown,
            eligible = stats.eligible,
        );
    }

    pub async fn start_next_session(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<SmalltalkSession>, StoreError> {
        let mut tx = self.pool.begin().await?;
        lock_global(&mut tx).await?;
        sqlx::query("LOCK TABLE twitch_partners IN SHARE MODE")
            .execute(&mut *tx)
            .await?;
        let open: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT 1 FROM twitch_smalltalk_sessions WHERE ended_at IS NULL
            )",
        )
        .fetch_one(&mut *tx)
        .await?;
        if open {
            tx.commit().await?;
            return Ok(None);
        }

        let rows = sqlx::query_as::<_, CandidateRow>(
            "SELECT LOWER(BTRIM(o.streamer_login)) AS channel_login,
                    COALESCE(
                        NULLIF(BTRIM(o.streamer_user_id), ''),
                        ls.twitch_user_id
                    ) AS streamer_user_id,
                    COALESCE(ls.is_live, 0) <> 0 AS is_live,
                    ls.last_game AS game,
                    ls.last_viewer_count AS viewer_count,
                    NULLIF(BTRIM(o.cooldown_until), '') AS cooldown_until,
                    EXISTS (
                        SELECT 1
                        FROM twitch_partners p
                        WHERE p.status = 'active'
                          AND (
                            p.twitch_user_id = COALESCE(
                                NULLIF(BTRIM(o.streamer_user_id), ''),
                                ls.twitch_user_id
                            )
                            OR LOWER(p.twitch_login) = LOWER(o.streamer_login)
                          )
                    ) AS partner_table,
                    EXISTS (
                        SELECT 1
                        FROM twitch_streamers_partner_state ps
                        WHERE COALESCE(ps.is_partner_active, 0) <> 0
                          AND (
                            ps.twitch_user_id = COALESCE(
                                NULLIF(BTRIM(o.streamer_user_id), ''),
                                ls.twitch_user_id
                            )
                            OR LOWER(ps.twitch_login) = LOWER(o.streamer_login)
                          )
                    ) AS partner_state,
                    EXISTS (
                        SELECT 1
                        FROM twitch_raid_blacklist b
                        WHERE LOWER(b.target_login) = LOWER(o.streamer_login)
                           OR b.target_id = COALESCE(
                                NULLIF(BTRIM(o.streamer_user_id), ''),
                                ls.twitch_user_id
                              )
                    ) AS blacklisted,
                    (
                        SELECT MAX(s.started_at)
                        FROM twitch_smalltalk_sessions s
                        WHERE LOWER(s.channel_login) = LOWER(o.streamer_login)
                    ) AS last_observed_at,
                    o.detected_at
             FROM twitch_partner_outreach o
             JOIN twitch_live_state ls
               ON LOWER(ls.streamer_login) = LOWER(o.streamer_login)
             WHERE COALESCE(
                    NULLIF(BTRIM(o.streamer_user_id), ''),
                    NULLIF(BTRIM(ls.twitch_user_id), '')
               ) IS NOT NULL
             FOR UPDATE OF o SKIP LOCKED",
        )
        .fetch_all(&mut *tx)
        .await?;
        let mut stats = CandidateStats::default();
        let mut eligible = Vec::new();
        for row in rows {
            let cooldown_until = row.cooldown_until.as_deref().and_then(|raw| {
                let parsed = parse_text_timestamp(raw);
                if parsed.is_none() {
                    tracing::warn!(
                        event = "smalltalk_loop.cooldown_unparsbar",
                        channel = %row.channel_login,
                    );
                }
                parsed
            });
            let reason = exclusion_reason(&row, cooldown_until, now);
            stats.count(reason);
            if reason.is_none() {
                eligible.push(Candidate {
                    channel_login: row.channel_login,
                    streamer_user_id: row.streamer_user_id,
                    viewer_count: row.viewer_count,
                    last_observed_at: row.last_observed_at,
                    detected_at: row.detected_at,
                });
            }
        }
        let candidate = eligible.into_iter().min_by(|left, right| {
            left.last_observed_at
                .cmp(&right.last_observed_at)
                .then_with(|| left.detected_at.cmp(&right.detected_at))
                .then_with(|| left.channel_login.cmp(&right.channel_login))
        });
        let Some(candidate) = candidate else {
            tx.commit().await?;
            self.log_no_candidate(stats, now);
            return Ok(None);
        };

        // Den vorhandenen Primaerschluessel mitlesen, nicht nur die Werte.
        // `twitch_engagement_settings` wird laut Vertrag kleingeschrieben
        // befuellt und exakt gelesen (siehe `auto_off.rs` und
        // `gate::load_settings`). Eine abweichend geschriebene Zeile ist
        // deshalb ein Datenfehler, den dieser Loop weder erben noch
        // verschlimmern darf: schreibt er kleingeschrieben, entsteht eine
        // zweite Zeile, die nach Sitzungsende aktiv zurueckbliebe; schreibt er
        // in der vorhandenen Schreibweise, findet die Pipeline den Kanal zur
        // Laufzeit nicht und die Sitzung liefe leer. Beides waere still falsch,
        // also wird der Kandidat uebersprungen und bekommt Cooldown.
        let previous = sqlx::query_as::<_, (String, bool, bool, String)>(
            "SELECT channel_login, enabled, irc_read, output_mode
             FROM twitch_engagement_settings
             WHERE LOWER(channel_login) = LOWER($1)
             FOR UPDATE",
        )
        .bind(&candidate.channel_login)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((key, _, _, _)) = &previous {
            if key != &candidate.channel_login {
                set_cooldown(&mut tx, &candidate.channel_login, now).await?;
                tx.commit().await?;
                tracing::warn!(
                    event = "smalltalk_loop.candidate_skipped",
                    channel = %candidate.channel_login,
                    reason = "settings_case_mismatch",
                    "Settings-Zeile weicht in der Schreibweise ab, Kandidat uebersprungen"
                );
                return Ok(None);
            }
        }
        let (settings_existed, previous_enabled, previous_irc_read, previous_output_mode) =
            match previous {
                Some((_, enabled, irc_read, output_mode)) => (true, enabled, irc_read, output_mode),
                None => (false, false, false, "off".to_string()),
            };
        let session = SmalltalkSession {
            id: Uuid::new_v4(),
            channel_login: candidate.channel_login,
            streamer_user_id: candidate.streamer_user_id,
            started_at: now,
            viewer_count: candidate.viewer_count,
        };
        sqlx::query(
            "INSERT INTO twitch_smalltalk_sessions
                (id, channel_login, streamer_user_id, started_at, viewer_count,
                 settings_existed, previous_enabled, previous_irc_read,
                 previous_output_mode)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(session.id)
        .bind(&session.channel_login)
        .bind(&session.streamer_user_id)
        .bind(session.started_at)
        .bind(session.viewer_count)
        .bind(settings_existed)
        .bind(previous_enabled)
        .bind(previous_irc_read)
        .bind(&previous_output_mode)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO twitch_engagement_settings
                (channel_login, enabled, irc_read, output_mode)
             VALUES ($1, TRUE, TRUE, 'test')
             ON CONFLICT (channel_login) DO UPDATE
             SET enabled = TRUE, irc_read = TRUE, output_mode = 'test'",
        )
        .bind(&session.channel_login)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        tracing::info!(
            event = "smalltalk_loop.session_started",
            session_id = %session.id,
            channel = %session.channel_login,
            result = "started",
        );
        Ok(Some(session))
    }

    pub async fn active_session(&self) -> Result<Option<SmalltalkSession>, StoreError> {
        sqlx::query_as::<_, (Uuid, String, String, DateTime<Utc>, Option<i32>)>(
            "SELECT id, channel_login, streamer_user_id, started_at, viewer_count
             FROM twitch_smalltalk_sessions
             WHERE ended_at IS NULL
             ORDER BY started_at
             LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map(|row| {
            row.map(
                |(id, channel_login, streamer_user_id, started_at, viewer_count)| {
                    SmalltalkSession {
                        id,
                        channel_login,
                        streamer_user_id,
                        started_at,
                        viewer_count,
                    }
                },
            )
        })
        .map_err(StoreError::from)
    }

    pub async fn close_ineligible_session(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<&'static str>, StoreError> {
        let Some(session) = self.active_session().await? else {
            return Ok(None);
        };
        let live_deadlock: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT 1
                FROM twitch_live_state
                WHERE twitch_user_id = $1
                  AND COALESCE(is_live, 0) = 1
                  AND LOWER(COALESCE(last_game, '')) = 'deadlock'
            )",
        )
        .bind(&session.streamer_user_id)
        .fetch_one(&self.pool)
        .await?;
        let reason = if now - session.started_at >= SESSION_DURATION {
            Some("session_timeout")
        } else if !live_deadlock {
            Some("stream_ended")
        } else {
            None
        };
        if let Some(reason) = reason {
            self.close_session(session.id, reason, now).await?;
        }
        Ok(reason)
    }

    pub async fn close_active_session(
        &self,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        if let Some(session) = self.active_session().await? {
            self.close_session(session.id, reason, now).await?;
        }
        Ok(())
    }

    pub async fn close_all_open_sessions(
        &self,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        let mut tx = self.pool.begin().await?;
        lock_global(&mut tx).await?;
        let sessions = sqlx::query_as::<_, SessionSettingsRow>(
            "SELECT id, channel_login, streamer_user_id, started_at, viewer_count,
                    settings_existed, previous_enabled, previous_irc_read,
                    previous_output_mode
             FROM twitch_smalltalk_sessions
             WHERE ended_at IS NULL
             ORDER BY started_at
             FOR UPDATE",
        )
        .fetch_all(&mut *tx)
        .await?;
        for session in &sessions {
            close_locked(&mut tx, session, reason, now).await?;
        }
        tx.commit().await?;
        Ok(sessions.len() as u64)
    }

    async fn close_session(
        &self,
        session_id: Uuid,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        lock_global(&mut tx).await?;
        let session = sqlx::query_as::<_, SessionSettingsRow>(
            "SELECT id, channel_login, streamer_user_id, started_at, viewer_count,
                    settings_existed, previous_enabled, previous_irc_read,
                    previous_output_mode
             FROM twitch_smalltalk_sessions
             WHERE id = $1 AND ended_at IS NULL
             FOR UPDATE",
        )
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(session) = session {
            close_locked(&mut tx, &session, reason, now).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn record_generated(
        &self,
        channel_login: &str,
        triggered_by_msg_id: Option<&str>,
        generated_text: &str,
        trigger_text: &str,
        outcome: GeneratedOutcome,
        generated_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let (outcome, reject_reason) = outcome.values();
        let inserted_session = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO twitch_smalltalk_messages
                (session_id, channel_login, generated_at, generated_text,
                 trigger_text, triggered_by_msg_id, outcome, reject_reason)
             SELECT id, channel_login, $2, $3, $4, $5, $6, $7
             FROM twitch_smalltalk_sessions
             WHERE ended_at IS NULL AND LOWER(channel_login) = LOWER($1)
             ON CONFLICT (session_id, triggered_by_msg_id)
                 WHERE triggered_by_msg_id IS NOT NULL
             DO NOTHING
             RETURNING session_id",
        )
        .bind(channel_login)
        .bind(generated_at)
        .bind(generated_text)
        .bind(trigger_text)
        .bind(triggered_by_msg_id)
        .bind(outcome)
        .bind(reject_reason)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(session_id) = inserted_session {
            tracing::info!(
                event = "smalltalk_loop.message_recorded",
                %session_id,
                channel = channel_login,
                result = outcome,
                reason = reject_reason.unwrap_or("none"),
            );
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Legt einen lokal transkribierten Stream-Abschnitt an der offenen
    /// Sitzung dieses Kanals ab. `false`, wenn keine Sitzung offen ist: dann
    /// gehoert der Ton zu keinem Test und wird nicht aufbewahrt.
    ///
    /// Leerer Text wird verworfen statt gespeichert. Whisper liefert fuer
    /// stille oder reine Spielsound-Bloecke nichts, und eine Zeile ohne Text
    /// saehe in der Auswertung wie ein Abschnitt ohne Inhalt aus.
    pub async fn record_transcript(
        &self,
        channel_login: &str,
        segment: &StreamTranscriptSegment,
    ) -> Result<bool, StoreError> {
        let text = segment
            .text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            return Ok(false);
        }
        let session_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO twitch_smalltalk_transcripts
                (session_id, channel_login, started_at, ended_at, text, engine, model)
             SELECT id, channel_login, $2, $3, $4, $5, $6
             FROM twitch_smalltalk_sessions
             WHERE ended_at IS NULL AND LOWER(channel_login) = LOWER($1)
             RETURNING session_id",
        )
        .bind(channel_login)
        .bind(segment.started_at)
        .bind(segment.ended_at.max(segment.started_at))
        .bind(&text)
        .bind(&segment.engine)
        .bind(segment.model.as_deref())
        .fetch_optional(&self.pool)
        .await?;
        Ok(session_id.is_some())
    }

    pub async fn record_provider_error(
        &self,
        channel_login: &str,
        error_class: &str,
    ) -> Result<(), StoreError> {
        let session_id = sqlx::query_scalar::<_, Uuid>(
            "UPDATE twitch_smalltalk_sessions
             SET provider_error_count = provider_error_count + 1,
                 last_provider_error = LEFT($1, 64)
             WHERE ended_at IS NULL AND LOWER(channel_login) = LOWER($2)
             RETURNING id",
        )
        .bind(error_class)
        .bind(channel_login)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(session_id) = session_id {
            tracing::warn!(
                event = "smalltalk_loop.provider_error_recorded",
                %session_id,
                channel = channel_login,
                reason = error_class,
            );
        }
        Ok(())
    }

    pub async fn claim_reports(
        &self,
        limit: i64,
        now: DateTime<Utc>,
    ) -> Result<Vec<ClaimedReport>, StoreError> {
        let mut tx = self.pool.begin().await?;
        let claim_id = Uuid::new_v4();
        let ids = sqlx::query_scalar::<_, Uuid>(
            "SELECT id
             FROM twitch_smalltalk_sessions
             WHERE ended_at IS NOT NULL
               AND discord_message_id IS NULL
               AND expires_at > $1
               AND discord_attempts < 3
               AND discord_next_attempt_at <= $1
               AND (discord_claim_until IS NULL OR discord_claim_until <= $1)
             ORDER BY started_at
             FOR UPDATE SKIP LOCKED
             LIMIT $2",
        )
        .bind(now)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
        if ids.is_empty() {
            tx.commit().await?;
            return Ok(Vec::new());
        }
        sqlx::query(
            "UPDATE twitch_smalltalk_sessions
             SET discord_claim_id = $1, discord_claim_until = $2
             WHERE id = ANY($3)",
        )
        .bind(claim_id)
        .bind(now + DISCORD_CLAIM_TTL)
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
        let sessions = sqlx::query_as::<
            _,
            (
                Uuid,
                String,
                DateTime<Utc>,
                DateTime<Utc>,
                String,
                Option<i32>,
                i32,
                Option<String>,
            ),
        >(
            "SELECT id, channel_login, started_at, ended_at, end_reason,
                    viewer_count, provider_error_count, last_provider_error
             FROM twitch_smalltalk_sessions
             WHERE id = ANY($1)
             ORDER BY started_at",
        )
        .bind(&ids)
        .fetch_all(&mut *tx)
        .await?;
        let mut claimed = Vec::with_capacity(sessions.len());
        for (
            id,
            channel_login,
            started_at,
            ended_at,
            end_reason,
            viewer_count,
            provider_error_count,
            last_provider_error,
        ) in sessions
        {
            let messages =
                sqlx::query_as::<_, (DateTime<Utc>, String, String, String, Option<String>)>(
                    "SELECT generated_at, generated_text, trigger_text, outcome, reject_reason
                 FROM twitch_smalltalk_messages
                 WHERE session_id = $1
                 ORDER BY generated_at, id",
                )
                .bind(id)
                .fetch_all(&mut *tx)
                .await?
                .into_iter()
                .map(
                    |(generated_at, generated_text, trigger_text, outcome, reject_reason)| {
                        SmalltalkMessage {
                            generated_at,
                            generated_text,
                            trigger_text,
                            outcome,
                            reject_reason,
                        }
                    },
                )
                .collect();
            let transcripts = sqlx::query_as::<
                _,
                (DateTime<Utc>, DateTime<Utc>, String, String, Option<String>),
            >(
                "SELECT started_at, ended_at, text, engine, model
                 FROM twitch_smalltalk_transcripts
                 WHERE session_id = $1
                 ORDER BY ended_at, id",
            )
            .bind(id)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .map(
                |(started_at, ended_at, text, engine, model)| SmalltalkTranscript {
                    started_at,
                    ended_at,
                    text,
                    engine,
                    model: model.filter(|model| !model.is_empty()),
                },
            )
            .collect();
            claimed.push(ClaimedReport {
                claim_id,
                report: SmalltalkReport {
                    session: ReportSession {
                        id,
                        channel_login,
                        started_at,
                        ended_at,
                        end_reason,
                        viewer_count,
                        provider_error_count,
                        last_provider_error,
                    },
                    messages,
                    transcripts,
                },
            });
        }
        tx.commit().await?;
        Ok(claimed)
    }

    pub async fn mark_report_sent(
        &self,
        session_id: Uuid,
        claim_id: Uuid,
        message_id: &str,
    ) -> Result<(), StoreError> {
        let updated = sqlx::query(
            "UPDATE twitch_smalltalk_sessions
             SET discord_message_id = $1,
                 discord_claim_id = NULL,
                 discord_claim_until = NULL,
                 discord_last_error = NULL
             WHERE id = $2 AND discord_claim_id = $3",
        )
        .bind(message_id)
        .bind(session_id)
        .bind(claim_id)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 1 {
            Ok(())
        } else {
            Err(StoreError::InvalidData)
        }
    }

    pub async fn mark_report_failed(
        &self,
        session_id: Uuid,
        claim_id: Uuid,
        error_class: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE twitch_smalltalk_sessions
             SET discord_attempts = discord_attempts + 1,
                 discord_next_attempt_at = $1,
                 discord_claim_id = NULL,
                 discord_claim_until = NULL,
                 discord_last_error = LEFT($2, 64)
             WHERE id = $3 AND discord_claim_id = $4",
        )
        .bind(now + DISCORD_RETRY_DELAY)
        .bind(error_class)
        .bind(session_id)
        .bind(claim_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn expired_discord_reports(
        &self,
        limit: i64,
        now: DateTime<Utc>,
    ) -> Result<Vec<ExpiredDiscordReport>, StoreError> {
        let rows = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT id, discord_message_id
             FROM twitch_smalltalk_sessions
             WHERE expires_at <= $1
               AND discord_message_id IS NOT NULL
               AND discord_delete_next_attempt_at <= $1
             ORDER BY expires_at, id
             LIMIT $2",
        )
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(session_id, message_id)| ExpiredDiscordReport {
                session_id,
                message_id,
            })
            .collect())
    }

    pub async fn delete_expired_report(
        &self,
        session_id: Uuid,
        message_id: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "DELETE FROM twitch_smalltalk_sessions
             WHERE id = $1 AND discord_message_id = $2 AND expires_at <= $3",
        )
        .bind(session_id)
        .bind(message_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_report_delete_failed(
        &self,
        session_id: Uuid,
        error_class: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE twitch_smalltalk_sessions
             SET discord_delete_attempts = discord_delete_attempts + 1,
                 discord_delete_next_attempt_at = $1,
                 discord_last_delete_error = LEFT($2, 64)
             WHERE id = $3 AND expires_at <= $4 AND discord_message_id IS NOT NULL",
        )
        .bind(now + DISCORD_DELETE_RETRY_DELAY)
        .bind(error_class)
        .bind(session_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_expired_unposted(&self, now: DateTime<Utc>) -> Result<(), StoreError> {
        sqlx::query(
            "DELETE FROM twitch_smalltalk_sessions
             WHERE expires_at <= $1
               AND discord_message_id IS NULL
               AND (discord_claim_until IS NULL OR discord_claim_until <= $1)",
        )
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

async fn close_locked(
    tx: &mut Transaction<'_, Postgres>,
    session: &SessionSettingsRow,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE twitch_smalltalk_sessions
         SET ended_at = $1, end_reason = $2
         WHERE id = $3 AND ended_at IS NULL",
    )
    .bind(now)
    .bind(reason)
    .bind(session.id)
    .execute(&mut **tx)
    .await?;
    if session.settings_existed {
        sqlx::query(
            "UPDATE twitch_engagement_settings
             SET enabled = $1, irc_read = $2, output_mode = $3
             WHERE LOWER(channel_login) = LOWER($4)",
        )
        .bind(session.previous_enabled)
        .bind(session.previous_irc_read)
        .bind(
            session
                .previous_output_mode
                .as_deref()
                .filter(|mode| !mode.trim().is_empty())
                .unwrap_or("off"),
        )
        .bind(&session.channel_login)
        .execute(&mut **tx)
        .await?;
    } else {
        sqlx::query(
            "DELETE FROM twitch_engagement_settings
             WHERE LOWER(channel_login) = LOWER($1) AND output_mode = 'test'",
        )
        .bind(&session.channel_login)
        .execute(&mut **tx)
        .await?;
    }
    set_cooldown(tx, &session.channel_login, now).await?;
    tracing::info!(
        event = "smalltalk_loop.session_closed",
        session_id = %session.id,
        channel = %session.channel_login,
        result = reason,
        streamer_user_id = %session.streamer_user_id,
        started_at = %session.started_at,
        viewer_count = ?session.viewer_count,
    );
    Ok(())
}

async fn lock_global(tx: &mut Transaction<'_, Postgres>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
            hashtextextended('tb-engagement:smalltalk-loop-global', 0)
        )",
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Sperrt einen Kanal fuer [`CHANNEL_COOLDOWN`]. Wird sowohl nach einer
/// beendeten Sitzung als auch beim Ueberspringen eines Kandidaten benutzt,
/// damit ein uebersprungener Kanal nicht bei jedem Tick erneut auffaellt.
async fn set_cooldown(
    tx: &mut Transaction<'_, Postgres>,
    channel_login: &str,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE twitch_partner_outreach
         SET cooldown_until = $1
         WHERE LOWER(streamer_login) = LOWER($2)",
    )
    .bind((now + CHANNEL_COOLDOWN).to_rfc3339())
    .bind(channel_login)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Warum ein Kandidat nicht in Frage kam, oder `None` für "geeignet".
/// Die Reihenfolge ist die Prüfreihenfolge; gezählt wird der erste Grund, der
/// greift. Ein Kanal kann mehrere gleichzeitig erfüllen — die Zahlen sind
/// deshalb eine Aufteilung der geprüften Kanäle, keine Summe der Verstöße.
fn exclusion_reason(
    row: &CandidateRow,
    cooldown_until: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<&'static str> {
    if !row.is_live {
        return Some("offline");
    }
    if !row
        .game
        .as_deref()
        .is_some_and(|game| game.eq_ignore_ascii_case("deadlock"))
    {
        return Some("other_game");
    }
    if row.partner_table || row.partner_state {
        return Some("partner");
    }
    // Wo der Bot gebannt ist, koennte er ohnehin nie senden. Dort zu messen,
    // ob er mitreden koennte, misst nichts.
    if row.blacklisted {
        return Some("blacklisted");
    }
    if cooldown_until.is_some_and(|until| until > now) {
        return Some("cooldown");
    }
    None
}

/// Aufteilung der geprüften Kanäle einer Loop-Runde.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CandidateStats {
    checked: u32,
    offline: u32,
    other_game: u32,
    partner: u32,
    blacklisted: u32,
    cooldown: u32,
    eligible: u32,
}

impl CandidateStats {
    fn count(&mut self, reason: Option<&str>) {
        self.checked += 1;
        match reason {
            Some("offline") => self.offline += 1,
            Some("other_game") => self.other_game += 1,
            Some("partner") => self.partner += 1,
            Some("blacklisted") => self.blacklisted += 1,
            Some("cooldown") => self.cooldown += 1,
            Some(_) => {}
            None => self.eligible += 1,
        }
    }
}

/// Wiederholung derselben Lage, damit Stille nicht wieder mehrdeutig wird.
const NO_CANDIDATE_LOG_REPEAT: Duration = Duration::minutes(15);

/// Der Loop tickt alle 5 Sekunden. Jede Runde zu melden wären rund 17.000
/// Zeilen pro Tag; die liest niemand, und dann fällt auch nicht auf, wenn eine
/// davon wichtig ist. Gemeldet wird deshalb jede Änderung des Bildes, plus alle
/// 15 Minuten eine Wiederholung.
fn should_log_no_candidate(
    last: Option<(CandidateStats, DateTime<Utc>)>,
    stats: CandidateStats,
    now: DateTime<Utc>,
) -> bool {
    match last {
        None => true,
        Some((previous, logged_at)) => {
            previous != stats || now - logged_at >= NO_CANDIDATE_LOG_REPEAT
        }
    }
}

fn parse_text_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(raw) {
        return Some(parsed.with_timezone(&Utc));
    }
    for format in [
        "%Y-%m-%d %H:%M:%S%.f%#z",
        "%Y-%m-%dT%H:%M:%S%.f%#z",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
    ] {
        if let Ok(parsed) = DateTime::parse_from_str(raw, format) {
            return Some(parsed.with_timezone(&Utc));
        }
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, format) {
            return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    #[test]
    fn postgres_textzeitstempel_wird_gelesen() {
        let expected = Utc.with_ymd_and_hms(2026, 7, 27, 10, 34, 56).unwrap();
        assert_eq!(
            parse_text_timestamp("2026-07-27 12:34:56+02"),
            Some(expected)
        );
    }

    fn row() -> CandidateRow {
        CandidateRow {
            channel_login: "kanal".to_string(),
            streamer_user_id: "1".to_string(),
            is_live: true,
            game: Some("Deadlock".to_string()),
            viewer_count: Some(12),
            cooldown_until: None,
            partner_table: false,
            partner_state: false,
            blacklisted: false,
            last_observed_at: None,
            detected_at: "2026-07-27".to_string(),
        }
    }

    #[test]
    fn geeigneter_kandidat_hat_keinen_ausschlussgrund() {
        assert_eq!(exclusion_reason(&row(), None, Utc::now()), None);
    }

    #[test]
    fn jeder_ausschluss_hat_einen_eigenen_grund() {
        let now = Utc::now();
        let offline = CandidateRow {
            is_live: false,
            ..row()
        };
        assert_eq!(exclusion_reason(&offline, None, now), Some("offline"));

        let anderes_spiel = CandidateRow {
            game: Some("Dota 2".to_string()),
            ..row()
        };
        assert_eq!(
            exclusion_reason(&anderes_spiel, None, now),
            Some("other_game")
        );

        let partner = CandidateRow {
            partner_table: true,
            ..row()
        };
        assert_eq!(exclusion_reason(&partner, None, now), Some("partner"));

        let partner_state = CandidateRow {
            partner_state: true,
            ..row()
        };
        assert_eq!(exclusion_reason(&partner_state, None, now), Some("partner"));

        let blacklisted = CandidateRow {
            blacklisted: true,
            ..row()
        };
        assert_eq!(
            exclusion_reason(&blacklisted, None, now),
            Some("blacklisted")
        );

        assert_eq!(
            exclusion_reason(&row(), Some(now + Duration::hours(1)), now),
            Some("cooldown")
        );
        assert_eq!(
            exclusion_reason(&row(), Some(now - Duration::seconds(1)), now),
            None,
            "abgelaufener Cooldown sperrt nicht mehr"
        );
    }

    /// Der Loop tickt alle 5 Sekunden. Jede Runde zu melden waeren rund 17.000
    /// Zeilen pro Tag — die liest niemand, und dann faellt auch nicht auf, wenn
    /// eine davon wichtig ist.
    #[test]
    fn unveraendertes_bild_wird_nur_einmal_gemeldet() {
        let now = Utc::now();
        let stats = CandidateStats {
            checked: 3,
            cooldown: 3,
            ..CandidateStats::default()
        };
        assert!(
            should_log_no_candidate(None, stats, now),
            "die erste Runde meldet immer"
        );
        assert!(!should_log_no_candidate(
            Some((stats, now)),
            stats,
            now + Duration::seconds(5)
        ));
    }

    #[test]
    fn geaendertes_bild_wird_sofort_gemeldet() {
        let now = Utc::now();
        let vorher = CandidateStats {
            checked: 3,
            cooldown: 3,
            ..CandidateStats::default()
        };
        let nachher = CandidateStats {
            checked: 3,
            cooldown: 2,
            offline: 1,
            ..CandidateStats::default()
        };
        assert!(should_log_no_candidate(
            Some((vorher, now)),
            nachher,
            now + Duration::seconds(5)
        ));
    }

    /// Ohne Wiederholung waere nach der ersten Meldung wieder Stille — und
    /// Stille sieht aus wie "Prozess tot", nicht wie "Lage unveraendert".
    #[test]
    fn unveraendertes_bild_wird_nach_15_minuten_erneut_gemeldet() {
        let now = Utc::now();
        let stats = CandidateStats {
            checked: 1,
            partner: 1,
            ..CandidateStats::default()
        };
        assert!(!should_log_no_candidate(
            Some((stats, now)),
            stats,
            now + Duration::minutes(14)
        ));
        assert!(should_log_no_candidate(
            Some((stats, now)),
            stats,
            now + Duration::minutes(15)
        ));
    }

    #[test]
    fn statistik_zaehlt_jeden_kandidaten_genau_einmal() {
        let mut stats = CandidateStats::default();
        stats.count(Some("offline"));
        stats.count(Some("cooldown"));
        stats.count(Some("cooldown"));
        stats.count(None);
        assert_eq!(
            stats,
            CandidateStats {
                checked: 4,
                offline: 1,
                cooldown: 2,
                eligible: 1,
                ..CandidateStats::default()
            }
        );
    }
}
