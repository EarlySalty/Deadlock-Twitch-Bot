//! 25-Sekunden-Worker des Twitch-Werbemanagers.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use tb_analytics::ad_manager::{
    decide, ActionKind, AdManagerStore, DecisionAction, DecisionInput, ManagedChannel,
    QueuedAction, COMMERCIAL_SCOPE, READ_SCOPE, SNOOZE_SCOPE,
};
use tb_raid::{RaidAuthStore, TokenProvider};
use tb_transport_twitch::{streams::normalize_ad_time, AdSchedule, HelixClient, HelixError};

use crate::task_supervisor::TaskSupervisor;

pub fn spawn(
    supervisor: &TaskSupervisor,
    pool: sqlx::PgPool,
    helix: HelixClient,
    tokens: Arc<TokenProvider>,
    auth: RaidAuthStore,
) {
    let cleanup_store = AdManagerStore::new(pool.clone());
    supervisor.spawn("twitch_ad_manager_retention", async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            match cleanup_store.cleanup_completed_actions().await {
                Ok(deleted) if deleted > 0 => {
                    tracing::info!(deleted, "Werbemanager: alte Aktionshistorie bereinigt")
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::error!(%error,"Werbemanager: Retention-Bereinigung fehlgeschlagen")
                }
            }
        }
    });
    supervisor.spawn("twitch_ad_manager", async move {
        let store = AdManagerStore::new(pool);
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(25));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let channels = match store.list_channels().await {
                Ok(value) => value,
                Err(error) => { tracing::error!(%error, "Werbemanager: Kanäle konnten nicht geladen werden"); continue; }
            };
            let limiter = Arc::new(tokio::sync::Semaphore::new(8));
            let mut tasks = tokio::task::JoinSet::new();
            for channel in channels {
                let store = store.clone();
                let helix = helix.clone();
                let tokens = tokens.clone();
                let auth = auth.clone();
                let limiter = limiter.clone();
                tasks.spawn(async move {
                    let Ok(_permit) = limiter.acquire_owned().await else { return };
                    let lease = match store.try_acquire_worker_lease(&channel.twitch_user_id).await {
                        Ok(Some(value)) => value,
                        Ok(None) => return,
                        Err(error) => { tracing::warn!(%error,"Werbemanager: Kanal-Lease konnte nicht gesetzt werden"); return; }
                    };
                    match process_channel(&store,&helix,&tokens,&auth,&channel).await {
                        Ok(RunHealth::Healthy) => {
                            if let Err(error) = store.touch_worker(&channel.twitch_user_id, &channel.twitch_login).await {
                                tracing::warn!(user=%channel.twitch_user_id,%error,"Werbemanager: erfolgreicher Worker-Lauf konnte nicht als gesund gespeichert werden");
                            }
                        }
                        Ok(RunHealth::Degraded) => {}
                        Err(error) => tracing::warn!(user=%channel.twitch_user_id,%error,"Werbemanager-Lauf fehlgeschlagen"),
                    }
                    match store.release_worker_lease(&channel.twitch_user_id,&lease).await {
                        Ok(true) => {}
                        Ok(false) => tracing::error!(user=%channel.twitch_user_id,"Werbemanager: Kanal-Lease gehörte beim Freigeben nicht mehr diesem Lauf"),
                        Err(error) => tracing::error!(user=%channel.twitch_user_id,%error,"Werbemanager: Kanal-Lease konnte nicht freigegeben werden"),
                    }
                });
            }
            while let Some(result) = tasks.join_next().await {
                if let Err(error) = result {
                    tracing::error!(%error, "Werbemanager: Kanal-Task ist unerwartet abgebrochen");
                }
            }
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunHealth {
    Healthy,
    Degraded,
}

async fn process_channel(
    store: &AdManagerStore,
    helix: &HelixClient,
    tokens: &TokenProvider,
    auth: &RaidAuthStore,
    channel: &ManagedChannel,
) -> Result<RunHealth, WorkerError> {
    let now = Utc::now();
    store
        .expire_unknown_actions(&channel.twitch_user_id, now)
        .await?;
    let live = store.live_state(&channel.twitch_user_id).await?;
    if !live.is_fresh_live(now) {
        let mut effective_live = live.clone();
        effective_live.is_live = false;
        store
            .upsert_state(
                &channel.twitch_user_id,
                &channel.twitch_login,
                &effective_live,
                None,
                None,
            )
            .await?;
        if let Some(action) = store.claim_due(&channel.twitch_user_id).await? {
            store
                .finish_action(
                    &action,
                    "failed",
                    Some("Der Kanal ist nicht sicher als live bestätigt."),
                    None,
                )
                .await?;
        }
        return Ok(RunHealth::Healthy);
    }
    let scopes = auth.get_scopes(&channel.twitch_user_id).await?;
    if !has(&scopes, READ_SCOPE) {
        store
            .upsert_state(
                &channel.twitch_user_id,
                &channel.twitch_login,
                &live,
                None,
                None,
            )
            .await?;
        if let Some(action) = store.claim_due(&channel.twitch_user_id).await? {
            store
                .finish_action(
                    &action,
                    "failed",
                    Some("Twitch-Berechtigung zum Lesen des Werbeplans fehlt."),
                    None,
                )
                .await?;
        }
        return Ok(RunHealth::Degraded);
    }
    let Some(token) = tokens
        .get_valid_token_unrestricted(&channel.twitch_user_id, now)
        .await?
    else {
        store
            .upsert_state(
                &channel.twitch_user_id,
                &channel.twitch_login,
                &live,
                None,
                None,
            )
            .await?;
        if let Some(action) = store.claim_due(&channel.twitch_user_id).await? {
            store
                .finish_action(
                    &action,
                    "failed",
                    Some("Twitch-Verbindung muss erneuert werden."),
                    None,
                )
                .await?;
        }
        return Ok(RunHealth::Degraded);
    };
    // Aktueller Helix-Stand ist für jede Entscheidung Pflicht.
    let schedule = helix
        .get_ad_schedule(&channel.twitch_user_id, &token)
        .await?;
    if let Some(current) = schedule.as_ref() {
        validate_schedule_times(current)?;
        reconcile_unknown(store, &channel.twitch_user_id, current).await?;
    }
    let write_history = match schedule.as_ref() {
        Some(value) => {
            store
                .should_write_history(&channel.twitch_user_id, value, now)
                .await?
        }
        None => false,
    };
    let decision = if channel.settings.enabled {
        if let Some(schedule) = schedule.as_ref() {
            let Some(session) = live.active_session_id else {
                return Ok(RunHealth::Healthy);
            };
            let chat_ingest_healthy = store
                .chat_ingest_healthy(
                    &channel.twitch_user_id,
                    &channel.twitch_login,
                    now,
                    channel.settings.quiet_window_minutes,
                )
                .await?;
            let quiet = store
                .quiet_messages(session, now, channel.settings.quiet_window_minutes)
                .await?;
            let input = DecisionInput {
                now,
                settings: channel.settings.clone(),
                stream_started_at: live.stream_started_at,
                next_ad_at: parse_time(schedule.next_ad_at.as_ref())?,
                last_ad_at: parse_time(schedule.last_ad_at.as_ref())?,
                snooze_count: schedule.snooze_count,
                quiet_chat_messages: quiet,
                chat_ingest_healthy,
            };
            Some(decide(&input))
        } else {
            None
        }
    } else {
        None
    };
    store
        .upsert_state(
            &channel.twitch_user_id,
            &channel.twitch_login,
            &live,
            schedule.as_ref(),
            decision.as_ref(),
        )
        .await?;
    if write_history {
        if let Some(schedule) = schedule.as_ref() {
            store
                .write_history_snapshot(&channel.twitch_user_id, &channel.twitch_login, schedule)
                .await?;
        }
    }

    if let (Some(decision), Some(schedule)) = (decision.as_ref(), schedule.as_ref()) {
        let key_time = schedule.next_ad_at.as_deref().unwrap_or("none");
        match decision.action {
            DecisionAction::Snooze => {
                store
                    .enqueue_automatic(
                        &channel.twitch_user_id,
                        &channel.twitch_login,
                        "snooze",
                        None,
                        &format!("auto:{}:snooze:{key_time}", channel.twitch_user_id),
                    )
                    .await?;
            }
            DecisionAction::Commercial { duration_seconds } => {
                store
                    .enqueue_automatic(
                        &channel.twitch_user_id,
                        &channel.twitch_login,
                        "commercial",
                        Some(duration_seconds),
                        &format!("auto:{}:commercial:{key_time}", channel.twitch_user_id),
                    )
                    .await?;
            }
            DecisionAction::None => {}
        }
    }
    if let Some(action) = store.claim_due(&channel.twitch_user_id).await? {
        if action.source == "automatic" && !automatic_matches(&action, decision.as_ref()) {
            store
                .finish_action(
                    &action,
                    "cancelled",
                    Some("Die aktuellen Automatikregeln verlangen diese Aktion nicht mehr."),
                    None,
                )
                .await?;
            return Ok(RunHealth::Healthy);
        }
        execute(
            store,
            helix,
            &token,
            &scopes,
            schedule.as_ref(),
            now,
            action,
        )
        .await?;
    }
    Ok(RunHealth::Healthy)
}

async fn execute(
    store: &AdManagerStore,
    helix: &HelixClient,
    token: &str,
    scopes: &[String],
    schedule: Option<&AdSchedule>,
    now: DateTime<Utc>,
    action: QueuedAction,
) -> Result<(), WorkerError> {
    let Some(schedule) = schedule else {
        store
            .finish_action(
                &action,
                "failed",
                Some("Twitch meldet keine nächste Werbung."),
                None,
            )
            .await?;
        return Ok(());
    };
    let required = match action.action {
        ActionKind::Snooze => SNOOZE_SCOPE,
        ActionKind::Commercial => COMMERCIAL_SCOPE,
    };
    let commercial_duration = match (action.action, action.duration_seconds) {
        (ActionKind::Snooze, None) => None,
        (ActionKind::Commercial, Some(value)) if [30, 60, 90, 120, 150, 180].contains(&value) => {
            Some(value)
        }
        _ => {
            store
                .finish_action(
                    &action,
                    "failed",
                    Some("Aktionsart und Commercial-Dauer sind ungültig."),
                    None,
                )
                .await?;
            return Ok(());
        }
    };
    if !has(scopes, required) {
        store
            .finish_action(
                &action,
                "failed",
                Some("Die erforderliche Twitch-Berechtigung fehlt."),
                None,
            )
            .await?;
        return Ok(());
    }
    if action.action == ActionKind::Snooze && schedule.snooze_count <= 0 {
        store
            .finish_action(
                &action,
                "failed",
                Some("Es ist keine Twitch-Werbepause verfügbar."),
                None,
            )
            .await?;
        return Ok(());
    }
    if action.action == ActionKind::Commercial {
        if let Some(last) = parse_time(schedule.last_ad_at.as_ref())? {
            if now < last + Duration::minutes(8) {
                store
                    .finish_action(
                        &action,
                        "failed",
                        Some("Twitch erlaubt Werbung frühestens nach acht Minuten."),
                        None,
                    )
                    .await?;
                return Ok(());
            }
        }
    }
    // Ab hier darf ein Prozessabbruch niemals zum zweiten POST führen.
    store
        .mark_unknown_before_send(&action, schedule, now)
        .await?;
    let result = match action.action {
        ActionKind::Snooze => helix
            .snooze_next_ad(&action.twitch_user_id, token)
            .await
            .map(|outcome| {
                (
                    format!(
                        "Nächste Werbung verschoben; {} Pausen verbleiben.",
                        outcome.snooze_count
                    ),
                    None,
                )
            }),
        ActionKind::Commercial => helix
            .start_commercial(
                &action.twitch_user_id,
                i64::from(commercial_duration.expect("vor dem POST validiert")),
                token,
            )
            .await
            .map(|outcome| {
                let retry_after = i32::try_from(outcome.retry_after)
                    .ok()
                    .filter(|value| *value >= 0);
                (outcome.message, retry_after)
            }),
    };
    match result {
        Ok((detail, retry_after)) => {
            store
                .finish_action(&action, "succeeded", Some(&detail), retry_after)
                .await?
        }
        Err(error) if helix_error_is_ambiguous(&error) => {
            tracing::warn!(%error,"Werbemanager: Twitch-Ergebnis bleibt unbekannt und wird per Schedule abgeglichen");
        }
        Err(error) => {
            store
                .finish_action(&action, "failed", Some(&error.to_string()), None)
                .await?
        }
    }
    Ok(())
}

fn automatic_matches(
    action: &QueuedAction,
    decision: Option<&tb_analytics::ad_manager::Decision>,
) -> bool {
    match (
        action.action,
        action.duration_seconds,
        decision.map(|value| value.action),
    ) {
        (ActionKind::Snooze, None, Some(DecisionAction::Snooze)) => true,
        (
            ActionKind::Commercial,
            Some(duration),
            Some(DecisionAction::Commercial { duration_seconds }),
        ) => duration == duration_seconds,
        _ => false,
    }
}

fn helix_error_is_ambiguous(error: &HelixError) -> bool {
    match error {
        HelixError::Http(_) | HelixError::AmbiguousOutcome { .. } => true,
        HelixError::Status { status } => *status >= 500,
        _ => false,
    }
}

async fn reconcile_unknown(
    store: &AdManagerStore,
    uid: &str,
    schedule: &AdSchedule,
) -> Result<(), WorkerError> {
    for action in store.unknown_actions(uid).await? {
        let confirmed = if action.action == ActionKind::Commercial {
            commercial_schedule_confirms(
                action.preflight_last_ad_at,
                parse_time(schedule.last_ad_at.as_ref())?,
            )
        } else {
            snooze_schedule_confirms(action.preflight_snooze_count, schedule.snooze_count)
        };
        if confirmed {
            store
                .finish_action(
                    &action,
                    "succeeded",
                    Some("Durch den aktuellen Twitch-Werbeplan bestätigt."),
                    None,
                )
                .await?;
            continue;
        }
    }
    Ok(())
}

fn snooze_schedule_confirms(preflight_count: Option<i32>, current_count: i64) -> bool {
    preflight_count
        .map(|before| current_count < i64::from(before))
        .unwrap_or(false)
}

fn commercial_schedule_confirms(
    _preflight_last: Option<DateTime<Utc>>,
    _current_last: Option<DateTime<Utc>>,
) -> bool {
    false
}

fn has(scopes: &[String], needle: &str) -> bool {
    scopes
        .iter()
        .any(|value| value.trim().eq_ignore_ascii_case(needle))
}
fn parse_time(value: Option<&String>) -> Result<Option<DateTime<Utc>>, WorkerError> {
    let Some(raw) = value else { return Ok(None) };
    let Some(normalized) = normalize_ad_time(raw) else {
        return Ok(None);
    };
    DateTime::parse_from_rfc3339(&normalized)
        .map(|value| Some(value.with_timezone(&Utc)))
        .map_err(|error| WorkerError::InvalidSchedule(format!("{raw}: {error}")))
}

fn validate_schedule_times(schedule: &AdSchedule) -> Result<(), WorkerError> {
    parse_time(schedule.next_ad_at.as_ref())?;
    parse_time(schedule.last_ad_at.as_ref())?;
    parse_time(schedule.snooze_refresh_at.as_ref())?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum WorkerError {
    #[error("DB: {0}")]
    Db(#[from] sqlx::Error),
    #[error("Helix: {0}")]
    Helix(#[from] HelixError),
    #[error("Ungültiger Twitch-Werbeplan: {0}")]
    InvalidSchedule(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn nur_explizite_4xx_sind_nach_dem_post_sicher_fehlgeschlagen() {
        assert!(!helix_error_is_ambiguous(&HelixError::Status {
            status: 400
        }));
        assert!(!helix_error_is_ambiguous(&HelixError::Status {
            status: 429
        }));
        assert!(helix_error_is_ambiguous(&HelixError::Status {
            status: 500
        }));
        assert!(helix_error_is_ambiguous(&HelixError::Status {
            status: 503
        }));
        assert!(helix_error_is_ambiguous(&HelixError::AmbiguousOutcome {
            reason: "leer"
        }));
    }

    #[test]
    fn spaeteres_last_ad_bestaetigt_keinen_ambigen_commercial() {
        let before = Utc.with_ymd_and_hms(2026, 9, 1, 14, 59, 0).unwrap();
        let twitch_second = Utc.with_ymd_and_hms(2026, 9, 1, 15, 0, 0).unwrap();
        let local_marker = twitch_second + Duration::milliseconds(700);

        assert!(twitch_second < local_marker);
        assert!(twitch_second > before);
        assert!(!commercial_schedule_confirms(
            Some(before),
            Some(twitch_second)
        ));
        assert!(!commercial_schedule_confirms(None, Some(twitch_second)));
    }

    #[test]
    fn unknown_expiry_liegt_vor_offline_stale_und_schedule_pfad() {
        assert!(!snooze_schedule_confirms(Some(2), 3));

        let source = include_str!("ad_manager_wiring.rs");
        let process = &source[source.find("async fn process_channel").unwrap()
            ..source.find("async fn execute").unwrap()];
        let expiry = process.find(".expire_unknown_actions").unwrap();
        assert!(expiry < process.find(".live_state").unwrap());
        assert!(expiry < process.find(".get_ad_schedule").unwrap());
    }

    #[test]
    fn spaeterer_next_ad_termin_ohne_gesunkenen_count_bestaetigt_keinen_snooze() {
        let before = Utc.with_ymd_and_hms(2026, 9, 1, 15, 0, 0).unwrap();
        let current = before + Duration::minutes(5);
        assert!(current > before);
        assert!(!snooze_schedule_confirms(Some(2), 2));
        assert!(snooze_schedule_confirms(Some(2), 1));
    }

    #[test]
    fn heartbeat_folgt_nur_einem_erfolgreichen_process_channel() {
        let source = include_str!("ad_manager_wiring.rs");
        let worker = &source[..source.find("async fn process_channel").unwrap()];
        let success = worker.find("Ok(RunHealth::Healthy) =>").unwrap();
        let heartbeat = worker.find(".touch_worker").unwrap();
        assert!(success < heartbeat);
    }

    #[test]
    fn fehlender_scope_und_token_sind_degraded_ohne_warnspam() {
        let source = include_str!("ad_manager_wiring.rs");
        let process = &source[source.find("async fn process_channel").unwrap()
            ..source.find("async fn execute").unwrap()];
        let scope_branch = &process[process
            .find("Twitch-Berechtigung zum Lesen des Werbeplans fehlt.")
            .unwrap()..];
        assert!(scope_branch
            .find("return Ok(RunHealth::Degraded)")
            .is_some());
        let token_branch = &process[process
            .find("Twitch-Verbindung muss erneuert werden.")
            .unwrap()..];
        assert!(token_branch
            .find("return Ok(RunHealth::Degraded)")
            .is_some());
    }
}
