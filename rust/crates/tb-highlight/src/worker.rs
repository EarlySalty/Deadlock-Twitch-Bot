//! Orchestrierung des Highlight-Clippers (Poll-Loop über aktive Partner).
//!
//! Port von `bot/highlight_clipper/worker.py`. Aufgebaut in Teil-Slices:
//! - 9a (hier): reine Entscheidungs-Helfer ([`filter_recent_matches`],
//!   [`get_hero_id`], [`compute_clip_window`]) — ohne I/O, voll testbar.
//! - 9b/9c (folgt): Partner-Datenschicht (Postgres + Steam-SQLite + manuelle
//!   steamids.json) und der eigentliche Poll-Loop inkl. Twitch-API.
//!
//! Das ungenutzte `_score_events_with_demo` (kein Caller in Python) wird nicht
//! portiert.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlx::PgPool;

use crate::config::{
    CLIPS_DIR, CLIP_POST_ROLL_SECONDS, CLIP_PRE_ROLL_SECONDS, MAX_CLIP_SECONDS, STATE_PATH,
};
use crate::event_detector::{detect_events, HighlightEvent};
use crate::state::{is_match_processed, mark_match_processed, HighlightState};
use crate::twitch_vod::TwitchVodApi;
use crate::{deadlock_client, demo_analyzer, demo_downloader, highlight_sender, partners, twitch_vod};

/// Ein zu verarbeitendes Match (gefiltert + normalisiert aus der Match-History).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentMatch {
    pub match_id: i64,
    pub start_time: i64,
    pub match_duration_s: i64,
}

/// Filtert die Match-History auf verarbeitbare Matches: Objekt-Form, gültige
/// `match_id`/`start_time`, jünger als 24h und noch nicht verarbeitet. Sortiert
/// aufsteigend nach `start_time` (Python `_filter_recent_matches`).
pub fn filter_recent_matches(
    matches: &[serde_json::Value],
    state: &HighlightState,
    login: &str,
    now: i64,
) -> Vec<RecentMatch> {
    let min_start = now - 86400;
    let mut filtered: Vec<RecentMatch> = Vec::new();
    for m in matches {
        let Some(obj) = m.as_object() else { continue };
        let (Some(match_id), Some(start_time)) = (
            as_int(obj.get("match_id")),
            as_int(obj.get("start_time")),
        ) else {
            continue;
        };
        if start_time <= min_start || is_match_processed(state, login, match_id) {
            continue;
        }
        filtered.push(RecentMatch {
            match_id,
            start_time,
            match_duration_s: as_int(obj.get("match_duration_s")).unwrap_or(0),
        });
    }
    filtered.sort_by_key(|m| m.start_time);
    filtered
}

/// Sucht die `hero_id` des Spielers (per `account_id == steam_id`) in den
/// Match-Metadaten (Python `_get_hero_id`).
pub fn get_hero_id(steam_id: i64, match_info: &serde_json::Value) -> Option<i64> {
    let players = match_info
        .get("players")
        .and_then(serde_json::Value::as_array)?;
    for player in players {
        if as_int(player.get("account_id")) == Some(steam_id) {
            return as_int(player.get("hero_id"));
        }
    }
    None
}

/// Berechnet das Clip-Fenster (start, end) im VOD aus dem VOD-Offset und dem
/// Event: Pre-Roll vor dem Event, Post-Roll danach, gedeckelt auf
/// `MAX_CLIP_SECONDS` (Python-Logik in `_process_match`).
pub fn compute_clip_window(vod_offset_s: i64, event: &HighlightEvent) -> (i64, i64) {
    let clip_start_s = (vod_offset_s + event.game_time_s - CLIP_PRE_ROLL_SECONDS).max(0);
    let clip_end_s = (clip_start_s + 1)
        .max(vod_offset_s + event.game_time_s + event.duration_s + CLIP_POST_ROLL_SECONDS);
    let clip_end_s = clip_end_s.min(clip_start_s + MAX_CLIP_SECONDS);
    (clip_start_s, clip_end_s)
}

/// `int(value)`-Semantik aus Python (`_as_int`): Zahlen gegen 0 gekürzt, Strings
/// nur als reine Ganzzahl, Bools → 0/1, sonst None.
fn as_int(value: Option<&serde_json::Value>) -> Option<i64> {
    match value {
        Some(serde_json::Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(serde_json::Value::String(s)) => s.trim().parse::<i64>().ok(),
        Some(serde_json::Value::Bool(b)) => Some(i64::from(*b)),
        _ => None,
    }
}

/// Laufzeit-Konfiguration des Highlight-Clippers (Pfade + Endpoints, alle
/// injizierbar). [`Self::new`] füllt alles außer den extern aufzulösenden
/// `boon`- und `yt-dlp`-Binärpfaden aus den Modul-Konstanten.
#[derive(Debug, Clone)]
pub struct HighlightClipperConfig {
    pub boon_path: PathBuf,
    pub yt_dlp_path: PathBuf,
    pub steamids_json_path: PathBuf,
    pub clips_dir: PathBuf,
    pub demo_cache_dir: PathBuf,
    pub state_path: PathBuf,
    pub highlight_api_url: String,
    pub deadlock_api_base: String,
}

impl HighlightClipperConfig {
    /// Baut die Konfiguration mit Default-Pfaden; nur `boon`/`yt-dlp` müssen
    /// (vom Aufrufer aufgelöst) übergeben werden.
    pub fn new(boon_path: PathBuf, yt_dlp_path: PathBuf) -> Self {
        Self {
            boon_path,
            yt_dlp_path,
            steamids_json_path: PathBuf::from(partners::STEAMIDS_JSON_DEFAULT),
            clips_dir: PathBuf::from(CLIPS_DIR),
            demo_cache_dir: PathBuf::from(demo_downloader::DEMO_CACHE_DIR),
            state_path: PathBuf::from(STATE_PATH),
            highlight_api_url: highlight_sender::HIGHLIGHT_API_URL.to_string(),
            deadlock_api_base: deadlock_client::DEADLOCK_API_BASE.to_string(),
        }
    }
}

/// Der Highlight-Clipper-Worker: hält Postgres-Pool, Twitch-API-Implementierung
/// und Konfiguration. Ein Durchlauf = [`Self::run_once`] (Python `_run_once`);
/// der Poll-Loop-Lifecycle wird beim Wiring in tb-bot aufgesetzt.
pub struct HighlightClipperWorker {
    pool: PgPool,
    api: Arc<dyn TwitchVodApi>,
    config: HighlightClipperConfig,
}

impl HighlightClipperWorker {
    pub fn new(pool: PgPool, api: Arc<dyn TwitchVodApi>, config: HighlightClipperConfig) -> Self {
        Self { pool, api, config }
    }

    /// Ein vollständiger Durchlauf über alle aktiven Partner.
    pub async fn run_once(&self) {
        let streamers = partners::get_partner_streamers(
            &self.pool,
            &self.config.steamids_json_path,
        )
        .await;
        if streamers.is_empty() {
            tracing::info!("HighlightClipper: Keine aktiven Partner mit Steam-ID gefunden");
            return;
        }
        tracing::info!(count = streamers.len(), "HighlightClipper: Partner werden verarbeitet");

        let mut state = crate::state::load_state(&self.config.state_path);
        let now = chrono::Utc::now().timestamp();

        for (login, account_id_str) in streamers {
            let Ok(account_id) = account_id_str.parse::<i64>() else {
                continue;
            };
            self.process_streamer(&mut state, &login, account_id, now).await;
        }
    }

    async fn process_streamer(
        &self,
        state: &mut HighlightState,
        login: &str,
        account_id: i64,
        now: i64,
    ) {
        let matches =
            deadlock_client::get_match_history(&self.config.deadlock_api_base, account_id, 10)
                .await
                .unwrap_or_default();
        let recent = filter_recent_matches(&matches, state, login, now);
        if recent.is_empty() {
            return;
        }

        let channel_id = match twitch_vod::get_channel_id(self.api.as_ref(), login).await {
            Some(c) => c,
            None => {
                tracing::warn!(login, "HighlightClipper: Kein Twitch-Channel");
                return;
            }
        };

        for m in &recent {
            let clip_dir = self
                .config
                .clips_dir
                .join(login)
                .join(m.match_id.to_string());
            if std::fs::create_dir_all(&clip_dir).is_err() {
                continue;
            }
            self.process_match(state, login, account_id, m, &channel_id, &clip_dir)
                .await;
            if let Err(error) = std::fs::remove_dir_all(&clip_dir) {
                tracing::warn!(
                    %error,
                    path = %clip_dir.display(),
                    "HighlightClipper: temp Clip-Verzeichnis konnte nicht geloescht werden"
                );
            }
        }
    }

    async fn process_match(
        &self,
        state: &mut HighlightState,
        login: &str,
        account_id: i64,
        m: &RecentMatch,
        channel_id: &str,
        clip_dir: &Path,
    ) {
        let match_id = m.match_id;
        let match_info = match deadlock_client::get_match_metadata(
            &self.config.deadlock_api_base,
            match_id,
        )
        .await
        {
            Ok(match_info) => match_info,
            Err(error) => {
                tracing::warn!(
                    %error,
                    login,
                    match_id,
                    "HighlightClipper: Match-Metadaten nicht ladbar"
                );
                serde_json::json!({})
            }
        };
        let hero_id = get_hero_id(account_id, &match_info);
        let mut events: Vec<HighlightEvent> = Vec::new();

        // Demo-First: Events direkt aus dem Replay lesen.
        if let Some(demo_path) = demo_downloader::get_demo_path(
            &self.config.deadlock_api_base,
            &self.config.demo_cache_dir,
            match_id,
        )
        .await
        {
            let moments = demo_analyzer::detect_all_events(
                &self.config.boon_path,
                &demo_path,
                hero_id.unwrap_or(0),
                login,
            )
            .await;
            let clutch = moments.iter().filter(|mm| mm.is_clutch).count();
            tracing::info!(
                login,
                match_id,
                kills = moments.len(),
                clutch,
                "HighlightClipper: Demo analysiert"
            );
            events = demo_analyzer::moments_to_events(&moments, 2);
            demo_downloader::cleanup_demo(&self.config.demo_cache_dir, match_id);
        }

        // Fallback auf API-Erkennung, wenn die Demo nichts liefert.
        if events.is_empty() {
            let api_events = detect_events(account_id, &match_info);
            if !api_events.is_empty() {
                tracing::info!(login, "HighlightClipper: Demo-Analyse leer, nutze API-Fallback");
                events = api_events;
            }
        }

        let vod = match twitch_vod::find_vod_for_match(
            self.api.as_ref(),
            channel_id,
            m.start_time,
            m.match_duration_s,
        )
        .await
        {
            Some(v) => v,
            None => {
                tracing::warn!(login, match_id, "HighlightClipper: Kein VOD gefunden");
                if let Err(error) =
                    mark_match_processed(state, &self.config.state_path, login, match_id)
                {
                    tracing::warn!(
                        %error,
                        login,
                        match_id,
                        "HighlightClipper: Processed-Marker konnte nicht gespeichert werden"
                    );
                }
                return;
            }
        };

        let vod_offset_s = m.start_time - vod.vod_started_at;
        let mut clip_paths: Vec<String> = Vec::new();
        let mut clip_events: Vec<HighlightEvent> = Vec::new();

        for (idx, event) in events.iter().enumerate() {
            let (clip_start, clip_end) = compute_clip_window(vod_offset_s, event);
            let output_path = clip_dir.join(format!(
                "{:02}_{}_{}.mp4",
                idx + 1,
                event.event_type.as_str(),
                event.game_time_s
            ));
            let ok = twitch_vod::download_clip(
                &self.config.yt_dlp_path,
                &vod.vod_id,
                clip_start,
                clip_end,
                &output_path,
            )
            .await;
            if !ok {
                continue;
            }
            clip_paths.push(output_path.to_string_lossy().into_owned());
            clip_events.push(event.clone());
        }

        if clip_paths.is_empty() {
            tracing::warn!(login, match_id, "HighlightClipper: Keine Clips erstellt");
        } else {
            highlight_sender::send_highlight_to_channel(
                &self.config.highlight_api_url,
                login,
                match_id,
                &clip_events,
                &clip_paths,
            )
            .await;
        }

        if let Err(error) = mark_match_processed(state, &self.config.state_path, login, match_id) {
            tracing::warn!(
                %error,
                login,
                match_id,
                "HighlightClipper: Processed-Marker konnte nicht gespeichert werden"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_detector::EventType;
    use serde_json::json;

    fn ev(game_time_s: i64, duration_s: i64) -> HighlightEvent {
        HighlightEvent {
            event_type: EventType::Multikill,
            game_time_s,
            duration_s,
            kill_count: 2,
            label: "x".into(),
            pre_roll_s: 0,
        }
    }

    #[test]
    fn filter_recent_fenster_und_sortierung() {
        let now = 1_000_000;
        let state = HighlightState::new();
        let matches = vec![
            json!({"match_id": 1, "start_time": now - 100, "match_duration_s": 1800}), // frisch
            json!({"match_id": 2, "start_time": now - 90000}),                          // > 24h alt
            json!({"match_id": 3, "start_time": now - 50}),                             // frisch, neuer
            json!("kein-objekt"),                                                       // übersprungen
            json!({"start_time": now - 10}),                                            // ohne match_id
        ];
        let out = filter_recent_matches(&matches, &state, "nani", now);
        // 1 und 3 bleiben, sortiert nach start_time (1 vor 3).
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].match_id, 1);
        assert_eq!(out[1].match_id, 3);
        assert_eq!(out[0].match_duration_s, 1800);
        assert_eq!(out[1].match_duration_s, 0); // Default ohne Feld
    }

    #[test]
    fn filter_recent_ueberspringt_verarbeitete() {
        let now = 1_000_000;
        let mut state = HighlightState::new();
        state.insert(
            "nani".into(),
            crate::state::StreamerState { processed_matches: vec![5], last_checked: 0 },
        );
        let matches = vec![json!({"match_id": 5, "start_time": now - 100})];
        assert!(filter_recent_matches(&matches, &state, "nani", now).is_empty());
    }

    #[test]
    fn hero_id_per_account() {
        let mi = json!({"players": [
            {"account_id": 111, "hero_id": 7},
            {"account_id": 222, "hero_id": 9},
        ]});
        assert_eq!(get_hero_id(222, &mi), Some(9));
        assert_eq!(get_hero_id(999, &mi), None);
        assert_eq!(get_hero_id(1, &json!({})), None);
    }

    #[test]
    fn clip_window_clamping() {
        // vod_offset 1000, event @ 50s, dur 5s. PRE=6, POST=4, MAX=40.
        // start = 1000+50-6 = 1044; end = max(1045, 1000+50+5+4=1059)=1059; cap 1044+40=1084 → 1059.
        assert_eq!(compute_clip_window(1000, &ev(50, 5)), (1044, 1059));
        // Lange Dauer wird auf MAX_CLIP gedeckelt.
        let (s, e) = compute_clip_window(0, &ev(100, 1000));
        assert_eq!(s, 94);
        assert_eq!(e, 94 + MAX_CLIP_SECONDS);
        // Negativer Start wird auf 0 geklemmt.
        let (s2, _) = compute_clip_window(0, &ev(0, 0));
        assert_eq!(s2, 0);
    }
}
