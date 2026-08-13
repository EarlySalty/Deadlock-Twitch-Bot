//! Ziel-Auflösung für einen einzelnen Raid-Versuch — reine Logik, kein I/O.
//! Port der Kandidaten-Filterung + Score-Join-Anteile aus
//! `raid/raid_pipeline.py` (`execute`, Z. 137–254) und
//! `raid/services/candidate_selection.py` (`select_partner_candidate_by_score`,
//! Score-Cache-Join Z. 218–258).
//!
//! Zwei Pfade in Prioritätsreihenfolge (der Outreach-Boost-Pfad ist Phase 6g,
//! Post-Cutover):
//! 1. **Partner**: Online-Partner → Blacklist-/Exclude-/raid_enabled-Filter →
//!    Score-Cache-Join (Cache-Miss + nicht-live fallen raus) → `select_by_score`.
//! 2. **Fallback**: DE-Deadlock-Streams → Blacklist-/Exclude-Filter →
//!    `select_fairest` (Cooldown + Fairness-Verteilung).
//!
//! Abweichung von Python: Kandidaten ohne `user_id`/`user_login` werden vor
//! der Auswahl gefiltert statt nach der Auswahl die ganze Pipeline mit
//! `invalid_target_identity` abzubrechen — ein korrupter Kandidat soll nicht
//! die übrigen Versuche verhindern.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::candidate_selection::{
    is_soft_avoided_fallback_login, select_by_score_matched, select_fairest, FairnessCandidate,
    ScoredCandidate, SelectionReason, SelectionResult, FOLLOWERS_UNKNOWN,
};
use crate::courtesy::CourtesyClass;
use crate::partner_roster::OnlineCandidate;
use crate::score_store::PartnerRaidScoreRow;

/// Startzeit-Sentinel für fehlende Werte (sortiert ans Ende, wie Python).
const STARTED_AT_SENTINEL: &str = "9999-99-99";

/// Das aufgelöste Raid-Ziel eines Versuchs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTarget {
    pub user_id: String,
    pub user_login: String,
    /// ISO-Startzeit des Ziel-Streams (für die Raid-History), falls bekannt.
    pub started_at: Option<String>,
    pub is_partner_raid: bool,
    /// Outreach-Boost-Ziel (Phase 6g): nach Erfolg als verbraucht markieren.
    pub is_outreach_boost: bool,
    /// Größe des Kandidaten-Pools, aus dem gewählt wurde (für History/Logs).
    pub candidates_count: i32,
    /// Eingefrorener Ziel-Stream-/Score-Snapshot für die spätere Arrival-Korrelation.
    pub target_stream_data: Option<Value>,
}

/// Diagnose-Zahlen des Partner-Pfads (Python-Log-Parität).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartnerResolutionStats {
    /// Kandidaten nach Blacklist-/Exclude-/raid_enabled-Filter.
    pub considered: usize,
    /// Ohne Score-Cache-Zeile übersprungen.
    pub cache_misses: usize,
    /// Score-Zeile vorhanden, aber nicht als live markiert.
    pub stale_not_live: usize,
}

/// Ergebnis des Partner-Pfads.
#[derive(Debug, Clone)]
pub struct PartnerResolution {
    pub target: Option<ResolvedTarget>,
    pub reason: Option<SelectionReason>,
    pub stats: PartnerResolutionStats,
}

fn is_filtered(
    user_id: &str,
    user_login: &str,
    blacklist_ids: &HashSet<String>,
    blacklist_logins: &HashSet<String>,
    exclude_ids: &HashSet<String>,
) -> bool {
    user_id.is_empty()
        || user_login.is_empty()
        || exclude_ids.contains(user_id)
        || blacklist_ids.contains(user_id)
        || blacklist_logins.contains(user_login)
}

fn normalized_followers_total(followers_total: i32) -> i32 {
    if followers_total > 0 {
        followers_total
    } else {
        FOLLOWERS_UNKNOWN
    }
}

fn target_stream_snapshot(
    user_id: &str,
    user_login: &str,
    viewer_count: i32,
    followers_total: i32,
    started_at: Option<&str>,
) -> Value {
    json!({
        "user_id": user_id,
        "user_login": user_login,
        "viewer_count": viewer_count,
        "followers_total": followers_total,
        "started_at": started_at,
    })
}

fn partner_score_snapshot(row: &PartnerRaidScoreRow) -> Value {
    json!({
        "twitch_user_id": row.twitch_user_id.as_str(),
        "twitch_login": row.twitch_login.as_str(),
        "is_live": row.is_live != 0,
        "final_score": row.final_score,
        "today_received_raids": row.today_received_raids,
        "duration_score": row.duration_score,
        "time_pattern_score": row.time_pattern_score,
        "readiness_score": row.readiness_score,
        "fairness_score": row.fairness_score,
        "base_score": row.base_score,
        "new_partner_multiplier": row.new_partner_multiplier,
        "raid_boost_multiplier": row.raid_boost_multiplier,
        "last_computed_at": row.last_computed_at.as_str(),
    })
}

fn partner_target_stream_data(candidate: &ScoredCandidate, row: &PartnerRaidScoreRow) -> Value {
    let started_at =
        (candidate.started_at != STARTED_AT_SENTINEL).then_some(candidate.started_at.as_str());
    let mut data = target_stream_snapshot(
        &candidate.user_id,
        &candidate.user_login,
        candidate.viewer_count,
        candidate.followers_total,
        started_at,
    );
    if let Value::Object(ref mut obj) = data {
        obj.insert("_partner_score".to_string(), partner_score_snapshot(row));
    }
    data
}

fn fairness_target_stream_data(candidate: &FairnessCandidate) -> Value {
    let started_at = (!candidate.started_at.trim().is_empty()
        && candidate.started_at != STARTED_AT_SENTINEL)
        .then_some(candidate.started_at.as_str());
    target_stream_snapshot(
        candidate.user_id.trim(),
        &candidate.user_login.trim().to_lowercase(),
        candidate.viewer_count,
        candidate.followers_total,
        started_at,
    )
}

/// Partner-Pfad: filtert + joint den Score-Cache und wählt per
/// `select_by_score_matched`.
///
/// `raider_class` ist die Courtesy-Klasse des Streamers, der gerade raidet
/// (aus seiner eigenen Score-Zeile). Sie bevorzugt Ziele, die sich nach
/// eigenen Raids genauso verhalten; ist keins verfügbar, entscheidet wie
/// bisher allein der Score. `None` = keine Historie, kein Vorfilter.
pub fn resolve_partner_target(
    partners: &[OnlineCandidate],
    scores: &HashMap<String, PartnerRaidScoreRow>,
    blacklist_ids: &HashSet<String>,
    blacklist_logins: &HashSet<String>,
    exclude_ids: &HashSet<String>,
    raider_class: Option<CourtesyClass>,
) -> PartnerResolution {
    let mut stats = PartnerResolutionStats::default();
    let mut scored: Vec<ScoredCandidate> = Vec::new();

    for candidate in partners {
        let user_id = candidate.twitch_user_id.trim();
        let user_login = candidate.twitch_login.trim().to_lowercase();
        // Pipeline-Filter (Python Z. 180–187): raid_enabled + Blacklist + Exclude.
        if !candidate.raid_enabled
            || is_filtered(
                user_id,
                &user_login,
                blacklist_ids,
                blacklist_logins,
                exclude_ids,
            )
        {
            continue;
        }
        stats.considered += 1;

        // Score-Cache-Join (Python Z. 226–258): Miss/nicht-live fallen raus.
        let Some(row) = scores.get(user_id) else {
            stats.cache_misses += 1;
            continue;
        };
        if row.is_live == 0 {
            stats.stale_not_live += 1;
            continue;
        }

        scored.push(ScoredCandidate {
            user_id: user_id.to_string(),
            user_login,
            final_score: row.final_score,
            today_received_raids: row.today_received_raids,
            is_live: true,
            viewer_count: candidate.stream.viewer_count,
            followers_total: normalized_followers_total(candidate.stream.followers_total),
            started_at: candidate
                .stream
                .started_at
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| STARTED_AT_SENTINEL.to_string()),
            raid_boost_multiplier: row.raid_boost_multiplier,
            new_partner_multiplier: row.new_partner_multiplier,
            courtesy_class: crate::courtesy_store::parse_class(row.courtesy_class.as_deref()),
        });
    }

    let candidates_count = stats.considered as i32;
    let selection = select_by_score_matched(&scored, raider_class).map(|matched| {
        if let Some(class) = matched.matched_class {
            tracing::debug!(
                ziel = %matched.candidate.user_login,
                klasse = %class.as_str(),
                "Raid-Ziel über die Courtesy-Klasse vorgefiltert"
            );
        }
        SelectionResult {
            candidate: matched.candidate,
            reason: matched.reason,
        }
    });
    PartnerResolution {
        reason: selection.as_ref().map(|s| s.reason.clone()),
        target: selection.map(|s| {
            let target_stream_data = scores
                .get(&s.candidate.user_id)
                .map(|row| partner_target_stream_data(s.candidate, row));
            ResolvedTarget {
                user_id: s.candidate.user_id.clone(),
                user_login: s.candidate.user_login.clone(),
                started_at: Some(s.candidate.started_at.clone())
                    .filter(|v| v != STARTED_AT_SENTINEL),
                is_partner_raid: true,
                is_outreach_boost: false,
                candidates_count,
                target_stream_data,
            }
        }),
        stats,
    }
}

/// Filtert die Fallback-Streams (Blacklist/Exclude) zum auswahlfähigen Pool.
///
/// Getrennt von der Auswahl, damit die Composition-Root den Pool **nach** dem
/// Filter mit echten Follower-Zahlen anreichern kann (Python
/// `attach_followers_totals(pool)`), bevor [`select_fallback_from_pool`] den
/// Tie-Break entscheidet.
pub fn filter_fallback_pool(
    streams: &[FairnessCandidate],
    blacklist_ids: &HashSet<String>,
    blacklist_logins: &HashSet<String>,
    exclude_ids: &HashSet<String>,
) -> Vec<FairnessCandidate> {
    streams
        .iter()
        .filter(|s| {
            !is_filtered(
                s.user_id.trim(),
                &s.user_login.trim().to_lowercase(),
                blacklist_ids,
                blacklist_logins,
                exclude_ids,
            )
        })
        .cloned()
        .collect()
}

/// Wählt aus einem bereits gefilterten (und ggf. follower-angereicherten) Pool
/// das fairste Ziel per `select_fairest`.
pub fn select_fallback_from_pool(
    pool: &[FairnessCandidate],
    recent_targets: &HashSet<String>,
    received_raids_by_id: &HashMap<String, i32>,
) -> Option<ResolvedTarget> {
    let candidates_count = pool.len() as i32;
    select_fairest(pool, recent_targets, received_raids_by_id).map(|chosen| ResolvedTarget {
        user_id: chosen.user_id.trim().to_string(),
        user_login: chosen.user_login.trim().to_lowercase(),
        started_at: Some(chosen.started_at.clone())
            .filter(|v| !v.trim().is_empty() && v != STARTED_AT_SENTINEL),
        is_partner_raid: false,
        is_outreach_boost: false,
        candidates_count,
        target_stream_data: Some(fairness_target_stream_data(chosen)),
    })
}

/// Fallback-Pfad (DE-Deadlock-Kategorie): filtert + wählt per `select_fairest`.
/// Convenience-Wrapper über [`filter_fallback_pool`] + [`select_fallback_from_pool`]
/// (ohne Follower-Anreicherung — die läuft im Pipeline-Pfad mit Helix-Zugang).
pub fn resolve_fallback_target(
    streams: &[FairnessCandidate],
    recent_targets: &HashSet<String>,
    received_raids_by_id: &HashMap<String, i32>,
    blacklist_ids: &HashSet<String>,
    blacklist_logins: &HashSet<String>,
    exclude_ids: &HashSet<String>,
) -> Option<ResolvedTarget> {
    let pool = filter_fallback_pool(streams, blacklist_ids, blacklist_logins, exclude_ids);
    select_fallback_from_pool(&pool, recent_targets, received_raids_by_id)
}

/// Boost-Pfad (Python `execute` Z. 144–178): Outreach-Empfänger unter den
/// Kategorie-Streams — kleinster Stream zuerst (der Boost soll gezielt
/// kleinen, frisch vorgemerkten Streamern helfen).
pub fn resolve_boost_target(
    streams: &[FairnessCandidate],
    boost_logins: &HashSet<String>,
    blacklist_ids: &HashSet<String>,
    blacklist_logins: &HashSet<String>,
    exclude_ids: &HashSet<String>,
) -> Option<ResolvedTarget> {
    if boost_logins.is_empty() {
        return None;
    }
    let mut matches: Vec<&FairnessCandidate> = streams
        .iter()
        .filter(|s| {
            let login = s.user_login.trim().to_lowercase();
            boost_logins.contains(&login)
                && !is_soft_avoided_fallback_login(&login)
                && !is_filtered(
                    s.user_id.trim(),
                    &login,
                    blacklist_ids,
                    blacklist_logins,
                    exclude_ids,
                )
        })
        .collect();
    if matches.is_empty() {
        return None;
    }
    matches.sort_by(|a, b| {
        (a.viewer_count, a.started_at.as_str()).cmp(&(b.viewer_count, b.started_at.as_str()))
    });
    let chosen = matches[0];
    Some(ResolvedTarget {
        user_id: chosen.user_id.trim().to_string(),
        user_login: chosen.user_login.trim().to_lowercase(),
        started_at: Some(chosen.started_at.clone())
            .filter(|v| !v.trim().is_empty() && v != STARTED_AT_SENTINEL),
        is_partner_raid: false,
        is_outreach_boost: true,
        candidates_count: matches.len() as i32,
        target_stream_data: Some(fairness_target_stream_data(chosen)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::partner_roster::StreamData;

    fn online(user_id: &str, login: &str, raid_enabled: bool, viewers: i32) -> OnlineCandidate {
        OnlineCandidate {
            twitch_user_id: user_id.to_string(),
            twitch_login: login.to_string(),
            raid_enabled,
            stream: StreamData {
                viewer_count: viewers,
                followers_total: 0,
                started_at: Some("2026-06-10T17:00:00+00:00".to_string()),
                game_name: Some("Deadlock".to_string()),
            },
        }
    }

    fn score_row(user_id: &str, final_score: f64, is_live: i32) -> PartnerRaidScoreRow {
        PartnerRaidScoreRow {
            twitch_user_id: user_id.to_string(),
            twitch_login: user_id.to_string(),
            avg_duration_sec: 0,
            time_pattern_score_base: 0.0,
            received_successful_raids_total: 0,
            is_new_partner_preferred: 0,
            new_partner_multiplier: 1.0,
            raid_boost_multiplier: 1.0,
            is_live,
            current_started_at: None,
            current_uptime_sec: 0,
            duration_score: 0.0,
            time_pattern_score: 0.0,
            base_score: 0.0,
            final_score,
            today_received_raids: 0,
            last_computed_at: "2026-06-10T17:30:00+00:00".to_string(),
            readiness_score: 0.0,
            fairness_score: 0.0,
            internal_sent_raids_30d: 0,
            internal_received_raids_7d: 0,
            internal_received_raids_30d: 0,
            courtesy_score: 1.0,
            courtesy_class: None,
            courtesy_observed: 0,
        }
    }

    fn sets() -> (HashSet<String>, HashSet<String>, HashSet<String>) {
        (HashSet::new(), HashSet::new(), HashSet::new())
    }

    #[test]
    fn partner_filter_blacklist_exclude_und_raid_enabled() {
        let partners = vec![
            online("1", "quelle", true, 5),
            online("2", "geblockt_id", true, 5),
            online("3", "geblockt_login", true, 5),
            online("4", "kein_raid", false, 5),
            online("5", "ziel", true, 5),
        ];
        let scores: HashMap<String, PartnerRaidScoreRow> = [
            ("2".to_string(), score_row("2", 0.9, 1)),
            ("3".to_string(), score_row("3", 0.9, 1)),
            ("4".to_string(), score_row("4", 0.9, 1)),
            ("5".to_string(), score_row("5", 0.5, 1)),
        ]
        .into();
        let blacklist_ids: HashSet<String> = ["2".to_string()].into();
        let blacklist_logins: HashSet<String> = ["geblockt_login".to_string()].into();
        let exclude: HashSet<String> = ["1".to_string()].into();

        let res = resolve_partner_target(
            &partners,
            &scores,
            &blacklist_ids,
            &blacklist_logins,
            &exclude,
            None,
        );
        let target = res.target.unwrap();
        assert_eq!(target.user_login, "ziel");
        assert!(target.is_partner_raid);
        assert_eq!(
            target.candidates_count, 1,
            "nur 'ziel' kam durch den Filter"
        );
    }

    #[test]
    fn score_join_zaehlt_cache_miss_und_nicht_live() {
        let partners = vec![
            online("1", "ohne_cache", true, 5),
            online("2", "nicht_live", true, 5),
            online("3", "gewinner", true, 5),
        ];
        let scores: HashMap<String, PartnerRaidScoreRow> = [
            ("2".to_string(), score_row("2", 0.9, 0)),
            ("3".to_string(), score_row("3", 0.4, 1)),
        ]
        .into();
        let (bl_ids, bl_logins, excl) = sets();

        let res = resolve_partner_target(&partners, &scores, &bl_ids, &bl_logins, &excl, None);
        assert_eq!(res.stats.cache_misses, 1);
        assert_eq!(res.stats.stale_not_live, 1);
        assert_eq!(res.stats.considered, 3);
        assert_eq!(res.target.unwrap().user_login, "gewinner");
    }

    #[test]
    fn partner_hoechster_score_gewinnt() {
        let partners = vec![online("1", "klein", true, 5), online("2", "gross", true, 5)];
        let scores: HashMap<String, PartnerRaidScoreRow> = [
            ("1".to_string(), score_row("1", 0.3, 1)),
            ("2".to_string(), score_row("2", 0.8, 1)),
        ]
        .into();
        let (bl_ids, bl_logins, excl) = sets();

        let res = resolve_partner_target(&partners, &scores, &bl_ids, &bl_logins, &excl, None);
        assert_eq!(res.target.unwrap().user_login, "gross");
        assert_eq!(res.reason, Some(SelectionReason::HighestFinalScore));
    }

    #[test]
    fn partner_unbekannte_follower_sortieren_ans_ende_und_snapshot_wird_gesetzt() {
        let unknown = online("1", "unbekannt", true, 5);
        let mut known = online("2", "bekannt", true, 5);
        known.stream.followers_total = 500;
        let partners = vec![unknown, known];
        let scores: HashMap<String, PartnerRaidScoreRow> = [
            ("1".to_string(), score_row("1", 0.9, 1)),
            ("2".to_string(), score_row("2", 0.9, 1)),
        ]
        .into();
        let (bl_ids, bl_logins, excl) = sets();

        let res = resolve_partner_target(&partners, &scores, &bl_ids, &bl_logins, &excl, None);
        let target = res.target.unwrap();

        assert_eq!(
            target.user_login, "bekannt",
            "bekannte Follower-Zahl gewinnt gegen unbekannte 0 aus StreamData"
        );
        let snapshot = target.target_stream_data.expect("target snapshot");
        assert_eq!(snapshot["followers_total"], serde_json::json!(500));
        assert_eq!(
            snapshot["_partner_score"]["final_score"],
            serde_json::json!(0.9)
        );
        assert_eq!(
            snapshot["_partner_score"]["last_computed_at"],
            serde_json::json!("2026-06-10T17:30:00+00:00")
        );
    }

    #[test]
    fn partner_leer_nach_filter_kein_ziel() {
        let partners = vec![online("", "korrupt", true, 5)];
        let scores = HashMap::new();
        let (bl_ids, bl_logins, excl) = sets();

        let res = resolve_partner_target(&partners, &scores, &bl_ids, &bl_logins, &excl, None);
        assert!(res.target.is_none());
        assert_eq!(res.stats.considered, 0, "korrupte Identität vorgefiltert");
    }

    fn fairness(user_id: &str, login: &str, viewers: i32) -> FairnessCandidate {
        FairnessCandidate {
            user_id: user_id.to_string(),
            user_login: login.to_string(),
            viewer_count: viewers,
            followers_total: 0,
            started_at: "2026-06-10T16:00:00+00:00".to_string(),
        }
    }

    #[test]
    fn fallback_filtert_und_bevorzugt_wenig_geraidete() {
        let streams = vec![
            fairness("1", "quelle", 2),
            fairness("2", "geblockt", 2),
            fairness("3", "oft_geraidet", 2),
            fairness("4", "frisch", 50),
        ];
        let blacklist_ids: HashSet<String> = ["2".to_string()].into();
        let exclude: HashSet<String> = ["1".to_string()].into();
        let received: HashMap<String, i32> = [("3".to_string(), 7), ("4".to_string(), 0)].into();

        let target = resolve_fallback_target(
            &streams,
            &HashSet::new(),
            &received,
            &blacklist_ids,
            &HashSet::new(),
            &exclude,
        )
        .unwrap();
        assert_eq!(target.user_login, "frisch", "0 erhaltene Raids schlägt 7");
        assert!(!target.is_partner_raid);
        assert_eq!(target.candidates_count, 2);
    }

    #[test]
    fn fallback_cooldown_greift_aber_nicht_wenn_pool_sonst_leer() {
        let streams = vec![fairness("3", "einziger", 2)];
        let recent: HashSet<String> = ["3".to_string()].into();
        let (bl_ids, bl_logins, excl) = sets();

        // Python: recent-Filter leert den Pool → voller Pool wird genommen.
        let target = resolve_fallback_target(
            &streams,
            &recent,
            &HashMap::new(),
            &bl_ids,
            &bl_logins,
            &excl,
        )
        .unwrap();
        assert_eq!(target.user_login, "einziger");
    }

    #[test]
    fn boost_waehlt_kleinsten_match_und_respektiert_filter() {
        let streams = vec![
            fairness("1", "Boost_Gross", 50),
            fairness("2", "boost_klein", 3),
            fairness("3", "geblockt", 1),
            fairness("4", "kein_boost", 1),
        ];
        let boost: HashSet<String> = ["boost_gross", "boost_klein", "geblockt"]
            .into_iter()
            .map(String::from)
            .collect();
        let blacklist_logins: HashSet<String> = ["geblockt".to_string()].into();
        let target = resolve_boost_target(
            &streams,
            &boost,
            &HashSet::new(),
            &blacklist_logins,
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(
            target.user_login, "boost_klein",
            "kleinster Viewer-Count gewinnt"
        );
        assert!(target.is_outreach_boost);
        assert!(!target.is_partner_raid);
        assert_eq!(target.candidates_count, 2);
    }

    #[test]
    fn boost_leer_oder_ohne_match_gibt_none() {
        let streams = vec![fairness("1", "x", 5)];
        assert!(resolve_boost_target(
            &streams,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new()
        )
        .is_none());
        let boost: HashSet<String> = ["anderer".to_string()].into();
        assert!(resolve_boost_target(
            &streams,
            &boost,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new()
        )
        .is_none());
    }

    #[test]
    fn boost_stellt_edoeasy_fuer_spaetesten_fallback_zurueck() {
        let streams = vec![fairness("1", "EdoEasy", 1)];
        let boost: HashSet<String> = ["edoeasy".to_string()].into();

        assert!(resolve_boost_target(
            &streams,
            &boost,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
        .is_none());
    }
}
