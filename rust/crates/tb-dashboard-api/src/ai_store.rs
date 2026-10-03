//! Persistenter Speicher ausschließlich für Analyse und Analyse-Folgechat.
//! Die Verbindung hält während des Anbieteraufrufs eine PostgreSQL-Session-Sperre.
//! Bei Abbruch wird sie geschlossen, niemals mit gehaltenem Lock in den Pool gelegt.
use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use sqlx::{pool::PoolConnection, postgres::PgPoolOptions, Connection, PgPool, Postgres};
use uuid::Uuid;
use std::sync::Arc;
use crate::ai_state::{ChatSession, AI_MODEL_LLM, AI_MODEL_OPUS,
    CHAT_SESSION_RETENTION_HOURS, LLM_HOURLY_FOLLOW_UP_LIMIT, OPUS_SESSION_FOLLOW_UP_LIMIT};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("Analyse-Speicher nicht verfügbar")]
    Database(#[from] sqlx::Error),
    #[error("Analyse-Unterhaltung nicht gefunden oder abgelaufen")]
    NotFound,
    #[error("Für diese Analyse läuft bereits eine Anfrage")]
    Busy,
    #[error("Kontingent ausgeschöpft")]
    Limit { reset: Option<i64> },
}

/// Höchstens zwei zusätzliche Verbindungen für lange Anbieteraufrufe.
/// Zugang und Ziel kommen unverändert aus dem vorhandenen Datenpool.
#[derive(Clone)]
pub struct AiStore {
    pub pool: PgPool,
    pub lock_pool: PgPool,
    _cleanup: Arc<CleanupTask>,
}
impl AiStore {
    pub fn new(pool: &PgPool) -> Self {
        let lock_pool = PgPoolOptions::new().max_connections(2).min_connections(0)
            .acquire_timeout(std::time::Duration::from_secs(2))
            .idle_timeout(std::time::Duration::from_secs(60))
            .connect_lazy_with(pool.connect_options().as_ref().clone());
        Self { pool: pool.clone(), lock_pool, _cleanup: Arc::new(CleanupTask::new(pool.clone())) }
    }
    pub async fn reserve(&self, user_id: &str, analysis_id: i64) -> Result<ReservedTurn, StoreError> {
        reserve(&self.pool, &self.lock_pool, user_id, analysis_id).await
    }
}

/// Eine Session-Sperre endet auch bei Abbruch des Rust-Futures mit der Verbindung.
pub struct SessionGuard {
    connection: PoolConnection<Postgres>,
    key: i64,
}
impl SessionGuard {
    pub async fn acquire(pool: &PgPool, name: &str) -> Result<Self, StoreError> {
        let mut connection = pool.acquire().await?;
        connection.close_on_drop();
        let key: i64 = sqlx::query_scalar("SELECT hashtextextended($1, 0)")
            .bind(name).fetch_one(&mut *connection).await?;
        let held: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(key).fetch_one(&mut *connection).await?;
        if !held { return Err(StoreError::Busy); }
        Ok(Self { connection, key })
    }
}

pub struct ReservedTurn {
    guard: SessionGuard,
    pub session: ChatSession,
    pub operation_id: Uuid,
    user_id: String,
}

/// Der letzte Storebesitzer beendet den Aufräumtask. Der Task selbst besitzt
/// nur seinen Poolclone und hält diesen Wächter nicht am Leben.
struct CleanupTask {
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for CleanupTask {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl CleanupTask {
    fn new(pool: PgPool) -> Self {
        let handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        let mut last_warning: Option<std::time::Instant> = None;
        let mut repetitions: u64 = 0;
        loop {
            interval.tick().await;
            if pool.is_closed() { break; }
            if let Err(error) = cleanup(&pool).await {
                if last_warning.is_none_or(|last| last.elapsed().as_secs() >= 345600) {
                    tracing::error!(%error, repetitions, "Abgelaufene Analyse-Unterhaltungen konnten nicht gelöscht werden");
                    last_warning = Some(std::time::Instant::now());
                    repetitions = 0;
                } else {
                    repetitions = repetitions.saturating_add(1);
                }
            }
        }
        });
        Self { handle }
    }
}

pub async fn cleanup(pool: &PgPool) -> Result<(), sqlx::Error> {
    // Taskabbruch darf weder eine laufende SQL-Anfrage noch deren Transaktion
    // im Pool zurücklassen. Das Schließen beendet beides auch serverseitig.
    let mut connection = pool.acquire().await?;
    connection.close_on_drop();
    let mut tx = connection.begin().await?;
    sqlx::query("DELETE FROM twitch_ai_chat_sessions WHERE expires_at <= clock_timestamp()")
        .execute(&mut *tx).await?;
    // Bei einem toten Backend ist das Anbieterergebnis unbekannt. Keine Erstattung.
    sqlx::query("UPDATE twitch_ai_chat_reservations r SET state = 'unknown' WHERE state = 'running' AND NOT EXISTS (
        SELECT 1 FROM pg_locks l WHERE l.locktype = 'advisory' AND l.granted AND l.pid = r.backend_pid
        AND l.database = (SELECT oid FROM pg_database WHERE datname = current_database())
        AND l.objsubid = 1 AND l.classid = ((r.lock_key >> 32) & 4294967295)::oid
        AND l.objid = (r.lock_key & 4294967295)::oid)")
        .execute(&mut *tx).await?;
    sqlx::query("DELETE FROM twitch_ai_chat_reservations WHERE state <> 'running' AND capacity_expires_at <= clock_timestamp()")
        .execute(&mut *tx).await?;
    sqlx::query("DELETE FROM twitch_ai_chat_hourly h WHERE window_start <= clock_timestamp() - interval '1 hour'
        AND NOT EXISTS (SELECT 1 FROM twitch_ai_chat_reservations r WHERE r.twitch_user_id = h.twitch_user_id AND r.model_kind = 'llm' AND r.state = 'running')")
        .execute(&mut *tx).await?;
    tx.commit().await
}

/// Die bereits über den vorhandenen Analyse-Schreibpfad erzeugte ID erhält
/// ihren vollständigen Folgechatkontext vor jeder HTTP-Erfolgsmeldung.
pub async fn save_session(
    guard: &mut SessionGuard, user_id: &str, session: &ChatSession,
) -> Result<(i64, Option<i64>), StoreError> {
    let mut tx = guard.connection.begin().await?;
    let encoded = serde_json::to_string(session)
        .map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
    sqlx::query("INSERT INTO twitch_ai_chat_sessions
        (twitch_user_id, analysis_id, session_json, created_at, expires_at)
        VALUES ($1, $2, $3::text::jsonb, $4, $4 + interval '24 hours')")
        .bind(user_id).bind(session.analysis_id).bind(encoded).bind(session.created_at)
        .execute(&mut *tx).await?;
    let remaining = remaining_in(&mut tx, user_id, session).await?;
    tx.commit().await?;
    Ok(remaining)
}

async fn load_in(connection: &mut sqlx::PgConnection, user_id: &str, analysis_id: i64) -> Result<ChatSession, StoreError> {
    let row: Option<(String, String, i64)> = sqlx::query_as("SELECT session_json::text, history::text, follow_up_count
        FROM twitch_ai_chat_sessions WHERE twitch_user_id = $1 AND analysis_id = $2 AND expires_at > clock_timestamp()")
        .bind(user_id).bind(analysis_id).fetch_optional(connection).await?;
    let Some((session_json, history_json, count)) = row else { return Err(StoreError::NotFound); };
    let mut session: ChatSession = serde_json::from_str(&session_json)
        .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
    session.history = serde_json::from_str(&history_json)
        .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
    session.follow_up_count = count;
    Ok(session)
}

async fn quota_lock(connection: &mut sqlx::PgConnection, user_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("ai-chat-quota:{user_id}")).execute(connection).await?;
    Ok(())
}

async fn remaining_in(connection: &mut sqlx::PgConnection, user_id: &str, session: &ChatSession) -> Result<(i64, Option<i64>), StoreError> {
    if session.model == AI_MODEL_OPUS {
        let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_ai_chat_reservations
            WHERE twitch_user_id = $1 AND analysis_id = $2 AND
            (state = 'running' OR (state = 'unknown' AND capacity_expires_at > clock_timestamp()))")
            .bind(user_id).bind(session.analysis_id).fetch_one(connection).await?;
        return Ok(((OPUS_SESSION_FOLLOW_UP_LIMIT - session.follow_up_count - pending).max(0), None));
    }
    let window: Option<(DateTime<Utc>, i64)> = sqlx::query_as("SELECT window_start, consumed FROM twitch_ai_chat_hourly
        WHERE twitch_user_id = $1 AND window_start > clock_timestamp() - interval '1 hour'")
        .bind(user_id).fetch_optional(&mut *connection).await?;
    let (start, used) = window.unwrap_or((Utc::now(), 0));
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_ai_chat_reservations
        WHERE twitch_user_id = $1 AND model_kind = 'llm' AND
        (state = 'running' OR (state = 'unknown' AND capacity_expires_at > clock_timestamp()))")
        .bind(user_id).fetch_one(connection).await?;
    Ok(((LLM_HOURLY_FOLLOW_UP_LIMIT - used - pending).max(0), Some((start + Duration::hours(1)).timestamp())))
}

pub async fn reserve(pool: &PgPool, lock_pool: &PgPool, user_id: &str, analysis_id: i64) -> Result<ReservedTurn, StoreError> {
    cleanup(pool).await?;
    let mut guard = SessionGuard::acquire(lock_pool, &format!("ai-chat-turn:{user_id}:{analysis_id}")).await?;
    let mut tx = guard.connection.begin().await?;
    quota_lock(&mut tx, user_id).await?;
    let session = load_in(&mut tx, user_id, analysis_id).await?;
    let (remaining, reset) = remaining_in(&mut tx, user_id, &session).await?;
    if remaining == 0 { return Err(StoreError::Limit { reset }); }
    if session.model != AI_MODEL_LLM && session.model != AI_MODEL_OPUS {
        return Err(sqlx::Error::Protocol("Unbekannter Analyse-Kontingenttyp".into()).into());
    }
    let capacity_expiry = if session.model == AI_MODEL_LLM {
        sqlx::query("INSERT INTO twitch_ai_chat_hourly (twitch_user_id, window_start, consumed)
            VALUES ($1, clock_timestamp(), 0) ON CONFLICT (twitch_user_id) DO UPDATE
            SET window_start = clock_timestamp(), consumed = 0
            WHERE twitch_ai_chat_hourly.window_start <= clock_timestamp() - interval '1 hour'")
            .bind(user_id).execute(&mut *tx).await?;
        sqlx::query_scalar::<_, DateTime<Utc>>("SELECT window_start + interval '1 hour' FROM twitch_ai_chat_hourly WHERE twitch_user_id = $1")
            .bind(user_id).fetch_one(&mut *tx).await?
    } else { session.created_at + Duration::hours(CHAT_SESSION_RETENTION_HOURS) };
    let operation_id = Uuid::new_v4();
    sqlx::query("INSERT INTO twitch_ai_chat_reservations
        (operation_id, twitch_user_id, analysis_id, model_kind, state, backend_pid, lock_key, capacity_expires_at)
        VALUES ($1, $2, $3, $4, 'running', pg_backend_pid(), $5, $6)")
        .bind(operation_id).bind(user_id).bind(analysis_id).bind(&session.model).bind(guard.key).bind(capacity_expiry)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(ReservedTurn { guard, session, operation_id, user_id: user_id.into() })
}

impl ReservedTurn {
    /// Bekannter Anbieterfehler: Der nicht erfolgreiche Turn verbraucht kein Kontingent.
    pub async fn fail(mut self, known_rejection: bool) -> Result<(), StoreError> {
        let mut tx = self.guard.connection.begin().await?;
        quota_lock(&mut tx, &self.user_id).await?;
        sqlx::query("UPDATE twitch_ai_chat_reservations SET state = $4 WHERE operation_id = $1
            AND twitch_user_id = $2 AND analysis_id = $3 AND state = 'running'")
            .bind(self.operation_id).bind(&self.user_id).bind(self.session.analysis_id)
            .bind(if known_rejection { "failed" } else { "unknown" })
            .execute(&mut *tx).await?;
        if known_rejection {
            sqlx::query("DELETE FROM twitch_ai_chat_hourly h WHERE twitch_user_id = $1 AND consumed = 0
                AND NOT EXISTS (SELECT 1 FROM twitch_ai_chat_reservations r WHERE r.twitch_user_id = h.twitch_user_id AND r.model_kind = 'llm'
                    AND (r.state = 'running' OR (r.state = 'unknown' AND r.capacity_expires_at > clock_timestamp())))")
                .bind(&self.user_id).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        self.guard.connection.close().await?;
        Ok(())
    }

    /// Derselbe Erfolgsabschluss darf nach einem unklaren Commit erneut geprüft werden.
    /// Die KI wird dabei niemals erneut aufgerufen.
    pub async fn complete(&mut self, message: &str, reply: &str) -> Result<(i64, Option<i64>), StoreError> {
        let mut tx = self.guard.connection.begin().await?;
        quota_lock(&mut tx, &self.user_id).await?;
        let state: Option<String> = sqlx::query_scalar("SELECT state FROM twitch_ai_chat_reservations
            WHERE operation_id = $1 AND twitch_user_id = $2 AND analysis_id = $3 FOR UPDATE")
            .bind(self.operation_id).bind(&self.user_id).bind(self.session.analysis_id).fetch_optional(&mut *tx).await?;
        let mut session = match load_in(&mut tx, &self.user_id, self.session.analysis_id).await {
            Ok(session) => session,
            Err(StoreError::NotFound) => {
                // Eine späte Antwort stellt weder die Unterhaltung wieder her
                // noch erstattet sie die noch gültige Reservierung.
                sqlx::query("UPDATE twitch_ai_chat_reservations SET state = 'unknown'
                    WHERE operation_id = $1 AND twitch_user_id = $2 AND analysis_id = $3 AND state = 'running'")
                    .bind(self.operation_id).bind(&self.user_id).bind(self.session.analysis_id)
                    .execute(&mut *tx).await?;
                tx.commit().await?;
                return Err(StoreError::NotFound);
            }
            Err(error) => return Err(error),
        };
        if state.as_deref() == Some("done") {
            let remaining = remaining_in(&mut tx, &self.user_id, &session).await?;
            tx.commit().await?;
            return Ok(remaining);
        }
        if state.as_deref() != Some("running") { return Err(StoreError::NotFound); }
        let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *tx).await?;
        let entries = json!([
            {"role":"user", "content":message, "timestamp":now.to_rfc3339(), "operation_id":self.operation_id},
            {"role":"assistant", "content":reply, "timestamp":now.to_rfc3339(), "operation_id":self.operation_id}
        ]);
        let updated = sqlx::query("UPDATE twitch_ai_chat_sessions SET history = history || $3::text::jsonb,
            follow_up_count = follow_up_count + 1 WHERE twitch_user_id = $1 AND analysis_id = $2 AND expires_at > clock_timestamp()")
            .bind(&self.user_id).bind(session.analysis_id).bind(entries.to_string()).execute(&mut *tx).await?;
        if updated.rows_affected() != 1 {
            sqlx::query("UPDATE twitch_ai_chat_reservations SET state = 'unknown'
                WHERE operation_id = $1 AND twitch_user_id = $2 AND analysis_id = $3 AND state = 'running'")
                .bind(self.operation_id).bind(&self.user_id).bind(session.analysis_id)
                .execute(&mut *tx).await?;
            tx.commit().await?;
            return Err(StoreError::NotFound);
        }
        if session.model == AI_MODEL_LLM {
            sqlx::query("INSERT INTO twitch_ai_chat_hourly (twitch_user_id, window_start, consumed) VALUES ($1, $2, 1)
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                window_start = CASE WHEN twitch_ai_chat_hourly.window_start <= $2 - interval '1 hour' OR
                    (twitch_ai_chat_hourly.consumed = 0 AND NOT EXISTS (SELECT 1 FROM twitch_ai_chat_reservations r
                        WHERE r.twitch_user_id = $1 AND r.model_kind = 'llm' AND r.state = 'unknown' AND r.capacity_expires_at > $2))
                    THEN $2 ELSE twitch_ai_chat_hourly.window_start END,
                consumed = CASE WHEN twitch_ai_chat_hourly.window_start <= $2 - interval '1 hour' THEN 1 ELSE twitch_ai_chat_hourly.consumed + 1 END")
                .bind(&self.user_id).bind(now).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE twitch_ai_chat_reservations SET state = 'done' WHERE operation_id = $1 AND twitch_user_id = $2 AND analysis_id = $3")
            .bind(self.operation_id).bind(&self.user_id).bind(session.analysis_id).execute(&mut *tx).await?;
        session.follow_up_count += 1;
        let remaining = remaining_in(&mut tx, &self.user_id, &session).await?;
        tx.commit().await?;
        Ok(remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_postgres::TestPostgres;

    async fn database() -> TestPostgres {
        let database = TestPostgres::start().await;
        sqlx::raw_sql("CREATE TABLE ai_analyses(id BIGINT PRIMARY KEY); INSERT INTO ai_analyses VALUES (7), (8);")
            .execute(&database.pool).await.unwrap();
        sqlx::raw_sql(include_str!("../../../migrations/20261003010000_ai_chat_persistenz.sql"))
            .execute(&database.pool).await.unwrap();
        database
    }

    async fn session(store: &AiStore, id: i64, model: &str) -> ChatSession {
        let session = ChatSession {
            model: model.into(), streamer: "kanal".into(), analysis_id: id,
            days: 30, game_filter: "all".into(), user_context: "Ursprünglicher Kontext".into(),
            ctx: json!({"original": [1, 2, 3]}), points: json!([{"text":"Analyse"}]),
            history: Vec::new(), follow_up_count: 0, created_at: Utc::now(),
        };
        let mut guard = SessionGuard::acquire(&store.lock_pool, "fixture").await.unwrap();
        save_session(&mut guard, "42", &session).await.unwrap();
        guard.connection.close().await.unwrap();
        session
    }

    #[tokio::test]
    async fn restart_erhaelt_kontext_verlauf_und_sessionlimit() {
        let db = database().await;
        let store = AiStore::new(&db.pool);
        let original = session(&store, 7, AI_MODEL_OPUS).await;
        let mut turn = store.reserve("42", 7).await.unwrap();
        assert_eq!(turn.complete("Frage", "Antwort").await.unwrap().0, 2);
        assert_eq!(turn.complete("Frage", "Antwort").await.unwrap().0, 2);
        turn.guard.connection.close().await.unwrap();
        drop(store);
        let store = AiStore::new(&db.pool);
        let turn = store.reserve("42", 7).await.unwrap();
        assert_eq!(turn.session.ctx, original.ctx);
        assert_eq!(turn.session.points, original.points);
        assert_eq!(turn.session.created_at, original.created_at);
        assert_eq!(turn.session.user_context, original.user_context);
        assert_eq!(turn.session.history.len(), 2);
        assert_eq!(turn.session.follow_up_count, 1);
        turn.fail(true).await.unwrap();
    }

    #[tokio::test]
    async fn parallele_tabs_verlieren_keinen_verlauf_und_unknown_sperrt_nicht_dauerhaft() {
        let db = database().await;
        let store = AiStore::new(&db.pool);
        session(&store, 7, AI_MODEL_OPUS).await;
        let turn = store.reserve("42", 7).await.unwrap();
        assert!(matches!(store.reserve("42", 7).await, Err(StoreError::Busy)));
        turn.fail(false).await.unwrap();
        let mut next = store.reserve("42", 7).await.unwrap();
        assert_eq!(next.complete("Zweite Frage", "Antwort").await.unwrap().0, 1);
        next.guard.connection.close().await.unwrap();
        let turn = store.reserve("42", 7).await.unwrap();
        turn.fail(false).await.unwrap();
        assert!(matches!(store.reserve("42", 7).await, Err(StoreError::Limit { .. })));
    }

    #[tokio::test]
    async fn stundenlimit_gilt_ueber_sessions_und_neue_storeinstanzen() {
        let db = database().await;
        let store = AiStore::new(&db.pool);
        session(&store, 7, AI_MODEL_LLM).await;
        session(&store, 8, AI_MODEL_LLM).await;
        for index in 0..10 {
            let store = AiStore::new(&db.pool);
            let mut turn = store.reserve("42", if index % 2 == 0 { 7 } else { 8 }).await.unwrap();
            assert_eq!(turn.complete("Frage", "Antwort").await.unwrap().0, 9 - index);
            turn.guard.connection.close().await.unwrap();
        }
        assert!(matches!(store.reserve("42", 7).await, Err(StoreError::Limit { .. })));
        assert!(matches!(store.reserve("43", 7).await, Err(StoreError::NotFound)));
    }

    #[tokio::test]
    async fn bekannter_erstfehler_startet_keine_stunde_und_abgelaufene_session_bleibt_abgelaufen() {
        let db = database().await;
        let store = AiStore::new(&db.pool);
        session(&store, 7, AI_MODEL_LLM).await;
        store.reserve("42", 7).await.unwrap().fail(true).await.unwrap();
        let windows: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_ai_chat_hourly")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(windows, 0);
        sqlx::query("UPDATE twitch_ai_chat_sessions SET created_at = statement_timestamp() - interval '25 hours', expires_at = statement_timestamp() - interval '1 hour'")
            .execute(&db.pool).await.unwrap();
        assert!(matches!(AiStore::new(&db.pool).reserve("42", 7).await, Err(StoreError::NotFound)));
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_ai_chat_sessions")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(rows, 0);
    }
    #[tokio::test]
    async fn abgebrochener_turn_wird_unknown_ohne_blinde_erstattung() {
        let db = database().await;
        let store = AiStore::new(&db.pool);
        session(&store, 7, AI_MODEL_OPUS).await;
        let turn = store.reserve("42", 7).await.unwrap();
        let operation_id = turn.operation_id;
        drop(turn);
        let mut recovered = false;
        for _ in 0..100 {
            cleanup(&db.pool).await.unwrap();
            let state: String = sqlx::query_scalar("SELECT state FROM twitch_ai_chat_reservations WHERE operation_id = $1")
                .bind(operation_id).fetch_one(&db.pool).await.unwrap();
            if state == "unknown" { recovered = true; break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(recovered, "Abgebrochene Backend-Sperre muss freigegeben werden");
        let mut next = store.reserve("42", 7).await.unwrap();
        assert_eq!(next.complete("Fortsetzung", "Antwort").await.unwrap().0, 1);
    }

    #[tokio::test]
    async fn fehlende_auth_id_wird_vor_datenbankzugriff_abgelehnt() {
        let db = database().await;
        let auth = crate::DashboardAuthLevel::Partner {
            twitch_login: "kanal".into(), twitch_user_id: String::new(), display_name: "Kanal".into(),
        };
        let response = crate::auth::streamer_scope::resolve_analysis_target(&db.pool, &auth, Some("kanal"))
            .await.unwrap_err();
        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn cleanup_endet_erst_mit_dem_letzten_storebesitzer() {
        let db = database().await;
        let store = AiStore::new(&db.pool);
        let task = store._cleanup.handle.abort_handle();
        let clone = store.clone();
        drop(store);
        tokio::task::yield_now().await;
        assert!(!task.is_finished(), "Ein Storeclone hält den Aufräumtask am Leben");
        drop(clone);
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !task.is_finished() { tokio::task::yield_now().await; }
        }).await.expect("Der letzte Besitzer beendet den Aufräumtask");
        assert!(!db.pool.is_closed(), "Ein fremder Poolbesitzer bleibt unverändert");
    }

    #[tokio::test]
    async fn cleanup_abbruch_schliesst_blockiertes_backend_und_rollt_zurueck() {
        let db = database().await;
        sqlx::query("INSERT INTO twitch_ai_chat_sessions
            (twitch_user_id, analysis_id, session_json, created_at, expires_at)
            VALUES ('42', 7, '{}'::jsonb, statement_timestamp() - interval '25 hours', statement_timestamp() - interval '1 hour')")
            .execute(&db.pool).await.unwrap();
        let mut blocker = db.pool.begin().await.unwrap();
        sqlx::query("SELECT * FROM twitch_ai_chat_sessions FOR UPDATE")
            .execute(&mut *blocker).await.unwrap();
        let writer = PgPoolOptions::new().max_connections(2).min_connections(0)
            .connect_lazy_with(db.pool.connect_options().as_ref().clone().application_name("ai-cleanup-cancel-test"));
        let store = AiStore::new(&writer);
        let cleanup_task = store._cleanup.handle.abort_handle();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let blocked: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity
                    WHERE application_name = 'ai-cleanup-cancel-test' AND wait_event_type = 'Lock')")
                    .fetch_one(&db.pool).await.unwrap();
                if blocked { break; }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.expect("Der Aufräumtask muss tatsächlich auf dem gesperrten DELETE warten");
        drop(store);
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while !cleanup_task.is_finished() { tokio::task::yield_now().await; }
        }).await.expect("Der Aufräumtask muss vor Freigabe des Blockers beendet sein");
        // PostgreSQL verarbeitet das Verbindungsende erst nach der blockierten
        // Anfrage. Der abgebrochene Task darf keinen DELETE bestätigen.
        blocker.rollback().await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let active: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity
                    WHERE application_name = 'ai-cleanup-cancel-test')")
                    .fetch_one(&db.pool).await.unwrap();
                if !active { break; }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.expect("Abbruch muss das wartende Backend schließen");
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_ai_chat_sessions")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(rows, 1, "Der abgebrochene DELETE wurde nicht bestätigt");
        writer.close().await;
    }

    async fn prepare_last_hourly_slot(store: &AiStore) {
        session(store, 7, AI_MODEL_LLM).await;
        session(store, 8, AI_MODEL_LLM).await;
        sqlx::query("INSERT INTO twitch_ai_chat_hourly (twitch_user_id, window_start, consumed)
            VALUES ('42', clock_timestamp(), 9)").execute(&store.pool).await.unwrap();
    }

    async fn expire_original_session(pool: &PgPool) {
        sqlx::query("UPDATE twitch_ai_chat_sessions SET created_at = statement_timestamp() - interval '25 hours',
            expires_at = statement_timestamp() - interval '1 hour' WHERE analysis_id = 7")
            .execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn sessionablauf_erstattet_keine_gueltige_unbekannte_stundenreserve() {
        let db = database().await;
        let store = AiStore::new(&db.pool);
        prepare_last_hourly_slot(&store).await;
        let turn = store.reserve("42", 7).await.unwrap();
        let operation_id = turn.operation_id;
        let expires: DateTime<Utc> = sqlx::query_scalar("SELECT capacity_expires_at FROM twitch_ai_chat_reservations WHERE operation_id = $1")
            .bind(operation_id).fetch_one(&db.pool).await.unwrap();
        turn.fail(false).await.unwrap();
        expire_original_session(&db.pool).await;
        cleanup(&db.pool).await.unwrap();
        let chats: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_ai_chat_sessions WHERE analysis_id = 7")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(chats, 0, "Originalkontext und Verlauf müssen gelöscht sein");
        let reservation: (String, DateTime<Utc>) = sqlx::query_as("SELECT state, capacity_expires_at FROM twitch_ai_chat_reservations WHERE operation_id = $1")
            .bind(operation_id).fetch_one(&db.pool).await.unwrap();
        assert_eq!(reservation, ("unknown".into(), expires));
        assert!(matches!(AiStore::new(&db.pool).reserve("42", 8).await, Err(StoreError::Limit { .. })));
        sqlx::query("UPDATE twitch_ai_chat_reservations SET capacity_expires_at = statement_timestamp() - interval '1 second'")
            .execute(&db.pool).await.unwrap();
        sqlx::query("UPDATE twitch_ai_chat_hourly SET window_start = statement_timestamp() - interval '2 hours'")
            .execute(&db.pool).await.unwrap();
        store.reserve("42", 8).await.unwrap().fail(true).await.unwrap();
    }

    #[tokio::test]
    async fn laufende_spaete_antwort_stellt_keinen_chat_wieder_her_und_zaehlt_nicht_doppelt() {
        let db = database().await;
        let store = AiStore::new(&db.pool);
        prepare_last_hourly_slot(&store).await;
        let mut turn = store.reserve("42", 7).await.unwrap();
        let operation_id = turn.operation_id;
        expire_original_session(&db.pool).await;
        cleanup(&db.pool).await.unwrap();
        let before: (String, DateTime<Utc>) = sqlx::query_as("SELECT state, capacity_expires_at FROM twitch_ai_chat_reservations WHERE operation_id = $1")
            .bind(operation_id).fetch_one(&db.pool).await.unwrap();
        assert_eq!(before.0, "running");
        assert!(matches!(turn.complete("Späte Frage", "Späte Antwort").await, Err(StoreError::NotFound)));
        assert!(matches!(turn.complete("Späte Frage", "Späte Antwort").await, Err(StoreError::NotFound)));
        let after: (String, DateTime<Utc>) = sqlx::query_as("SELECT state, capacity_expires_at FROM twitch_ai_chat_reservations WHERE operation_id = $1")
            .bind(operation_id).fetch_one(&db.pool).await.unwrap();
        assert_eq!(after, ("unknown".into(), before.1));
        let confirmed: i64 = sqlx::query_scalar("SELECT consumed FROM twitch_ai_chat_hourly WHERE twitch_user_id = '42'")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(confirmed, 9);
        let chats: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_ai_chat_sessions WHERE analysis_id = 7")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(chats, 0);
        assert!(matches!(store.reserve("42", 8).await, Err(StoreError::Limit { .. })));
        turn.fail(false).await.unwrap();
    }

    #[tokio::test]
    async fn abgeschlossene_und_abgelehnte_metadaten_werden_nach_eigener_frist_entfernt() {
        let db = database().await;
        let store = AiStore::new(&db.pool);
        session(&store, 7, AI_MODEL_LLM).await;
        let mut turn = store.reserve("42", 7).await.unwrap();
        turn.complete("Frage", "Antwort").await.unwrap();
        turn.guard.connection.close().await.unwrap();
        store.reserve("42", 7).await.unwrap().fail(true).await.unwrap();
        sqlx::query("UPDATE twitch_ai_chat_reservations SET capacity_expires_at = statement_timestamp() - interval '1 second'")
            .execute(&db.pool).await.unwrap();
        cleanup(&db.pool).await.unwrap();
        let metadata: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_ai_chat_reservations")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(metadata, 0);
        let count: i64 = sqlx::query_scalar("SELECT follow_up_count FROM twitch_ai_chat_sessions WHERE analysis_id = 7")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(count, 1);
    }

}
