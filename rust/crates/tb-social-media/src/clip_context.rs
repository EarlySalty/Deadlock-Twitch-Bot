use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ContextSecond {
    pub vod_second: i32,
    pub lufs: Option<f64>,
    pub peak_dbfs: Option<f64>,
    pub speech: Option<String>,
    pub laughter: Option<bool>,
    pub exclamation: Option<bool>,
    pub kill_feed: Option<String>,
    pub souls: Option<i32>,
    pub soul_jump: Option<i32>,
    pub objective: Option<bool>,
    pub death_screen: Option<bool>,
    pub scene_change: Option<bool>,
    pub chat_messages: i32,
    pub ocr_sampled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CutWeights {
    pub audio: f64,
    pub speech: f64,
    pub laughter: f64,
    pub kill: f64,
    pub souls: f64,
    pub objective: f64,
    pub death: f64,
    pub scene: f64,
    pub chat: f64,
}

impl Default for CutWeights {
    fn default() -> Self {
        Self {
            audio: 0.29,
            speech: 0.06,
            laughter: 0.08,
            kill: 0.12,
            souls: 0.14,
            objective: 0.08,
            death: 0.08,
            scene: 0.04,
            chat: 0.11,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CutTemplate {
    pub weights: CutWeights,
    pub lead_seconds: i32,
    pub trail_seconds: i32,
    pub sample_count: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CutProposal {
    pub start_s: i32,
    pub peak_s: i32,
    pub end_s: i32,
}

pub fn clip_moment_from_start(vod_offset_s: i32, duration_s: f64) -> Option<i32> {
    if vod_offset_s < 0 || !duration_s.is_finite() || duration_s < 0.5 {
        return None;
    }
    let duration = duration_s.round();
    if duration > i32::MAX as f64 {
        return None;
    }
    vod_offset_s.checked_add(duration as i32)
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some(values[values.len() / 2])
}

fn confirmed_kill(text: &str) -> bool {
    text.split(|c: char| !c.is_alphabetic()).any(|word| {
        matches!(
            word.to_ascii_lowercase().as_str(),
            "killed" | "eliminated" | "defeated" | "assist"
        )
    })
}

fn signal_score(second: &ContextSecond, audio_floor: f64, chat_floor: f64, w: &CutWeights) -> f64 {
    let audio = second
        .lufs
        .map(|lufs| ((lufs - audio_floor - 3.0) / 20.0).clamp(0.0, 1.0))
        .unwrap_or_default();
    let transient = second
        .peak_dbfs
        .map(|peak| ((peak + 15.0) / 15.0).clamp(0.0, 1.0))
        .unwrap_or_default();
    let chat = ((second.chat_messages as f64 - chat_floor) / 6.0).clamp(0.0, 1.0);
    w.audio * audio.max(transient)
        + w.speech
            * (0.4 * f64::from(second.speech.as_ref().is_some_and(|s| !s.trim().is_empty()))
                + 0.6 * f64::from(second.exclamation == Some(true)))
        + w.laughter * f64::from(second.laughter == Some(true))
        + w.kill * f64::from(second.kill_feed.as_deref().is_some_and(confirmed_kill))
        + w.souls * (second.soul_jump.unwrap_or_default() as f64 / 600.0).clamp(0.0, 1.0)
        + w.objective * f64::from(second.objective == Some(true))
        + w.death * f64::from(second.death_screen == Some(true))
        + w.scene * f64::from(second.scene_change == Some(true))
        + w.chat * chat
}

pub fn suggest_cut(
    timeline: &[ContextSecond],
    moment_s: i32,
    template: &CutTemplate,
) -> Option<CutProposal> {
    let first = timeline.first()?.vod_second;
    let last = timeline.last()?.vod_second;
    if template.lead_seconds <= 0
        || template.trail_seconds <= 0
        || first > moment_s
        || last < moment_s
    {
        return None;
    }
    let audio_floor = median(timeline.iter().filter_map(|s| s.lufs).collect()).unwrap_or(-40.0);
    let chat_floor =
        median(timeline.iter().map(|s| s.chat_messages as f64).collect()).unwrap_or(0.0);
    let scores: Vec<_> = timeline
        .iter()
        .map(|s| signal_score(s, audio_floor, chat_floor, &template.weights))
        .collect();
    let peak_index = timeline
        .iter()
        .enumerate()
        .filter(|(_, s)| (moment_s - 40..=moment_s + 20).contains(&s.vod_second))
        .max_by(|(a, sa), (b, sb)| {
            scores[*a].total_cmp(&scores[*b]).then_with(|| {
                (moment_s - sb.vod_second)
                    .abs()
                    .cmp(&(moment_s - sa.vod_second).abs())
            })
        })?
        .0;
    let peak_s = timeline[peak_index].vod_second;
    let lead = template.lead_seconds.clamp(5, 90);
    let trail = template.trail_seconds.clamp(12, 60);
    let earliest = peak_s.saturating_sub(lead).max(first);
    let start_s = timeline
        .iter()
        .enumerate()
        .filter(|(_, s)| (earliest..=peak_s - 3).contains(&s.vod_second))
        .find(|(index, s)| {
            scores[*index] > 0.12
                || s.speech.as_ref().is_some_and(|text| !text.is_empty())
                || s.chat_messages as f64 > chat_floor + 2.0
        })
        .map(|(_, s)| s.vod_second.saturating_sub(2).max(first))
        .unwrap_or(earliest);
    let latest = peak_s.saturating_add(trail).min(last);
    let end_s = timeline
        .iter()
        .enumerate()
        .filter(|(_, s)| (peak_s + 12..=latest).contains(&s.vod_second))
        .find(|(index, s)| {
            scores[*index] < 0.08
                && !s.speech.as_ref().is_some_and(|text| !text.is_empty())
                && timeline
                    .get(index + 1)
                    .is_some_and(|next| next.vod_second == s.vod_second + 1)
                && scores.get(index + 1).is_some_and(|score| *score < 0.08)
        })
        .map(|(_, s)| (s.vod_second + 2).min(last + 1))
        .unwrap_or(latest + 1);
    let reaction_end = timeline
        .iter()
        .filter(|second| {
            (moment_s..=moment_s.saturating_add(30)).contains(&second.vod_second)
                && (second.chat_messages as f64 > chat_floor
                    || second.speech.as_ref().is_some_and(|text| !text.is_empty())
                    || second.laughter == Some(true)
                    || second.exclamation == Some(true))
        })
        .map(|second| second.vod_second.saturating_add(3))
        .max()
        .unwrap_or(moment_s.saturating_add(12));
    Some(CutProposal {
        start_s,
        peak_s,
        end_s: end_s
            .max(reaction_end)
            .max(moment_s.saturating_add(12))
            .min(last + 1),
    })
}

fn learn_weights(timelines: &[(i32, Vec<ContextSecond>)]) -> CutWeights {
    let mut counts = [[[0.0_f64; 2]; 9]; 2];
    for (moment, timeline) in timelines {
        let audio_floor = median(timeline.iter().filter_map(|s| s.lufs).collect()).unwrap_or(-40.0);
        let chat_floor =
            median(timeline.iter().map(|s| s.chat_messages as f64).collect()).unwrap_or(0.0);
        for second in timeline {
            let bucket = if (moment - 35..=moment + 10).contains(&second.vod_second) {
                0
            } else if (second.vod_second - moment).abs() >= 60 {
                1
            } else {
                continue;
            };
            let observations = [
                second.lufs.map(|v| v >= audio_floor + 6.0),
                second.exclamation,
                second.laughter,
                second
                    .ocr_sampled
                    .then_some(second.kill_feed.as_deref().is_some_and(confirmed_kill)),
                second
                    .ocr_sampled
                    .then_some(second.soul_jump.unwrap_or_default() >= 120),
                second.objective,
                second.death_screen,
                second.scene_change,
                Some(second.chat_messages as f64 >= chat_floor + 2.0),
            ];
            for (index, observation) in observations.into_iter().enumerate() {
                if let Some(value) = observation {
                    counts[bucket][index][0] += f64::from(value);
                    counts[bucket][index][1] += 1.0;
                }
            }
        }
    }
    let mut lift = [0.0; 9];
    for (index, value) in lift.iter_mut().enumerate() {
        let near = counts[0][index];
        let far = counts[1][index];
        if near[1] >= 3.0 && far[1] >= 3.0 && near[0] + far[0] > 0.0 {
            *value = (near[0] / near[1] - far[0] / far[1]).max(0.0);
        }
    }
    let sum: f64 = lift.iter().sum();
    if sum <= 0.0 {
        return CutWeights::default();
    }
    CutWeights {
        audio: lift[0] / sum,
        speech: lift[1] / sum,
        laughter: lift[2] / sum,
        kill: lift[3] / sum,
        souls: lift[4] / sum,
        objective: lift[5] / sum,
        death: lift[6] / sum,
        scene: lift[7] / sum,
        chat: lift[8] / sum,
    }
}

pub fn learn_template(timelines: &[(i32, Vec<ContextSecond>)]) -> Option<CutTemplate> {
    if timelines.is_empty() {
        return None;
    }
    let weights = learn_weights(timelines);
    let mut leads = Vec::new();
    let mut trails = Vec::new();
    for (moment, timeline) in timelines {
        let seed = CutTemplate {
            weights: weights.clone(),
            lead_seconds: 60,
            trail_seconds: 40,
            sample_count: 0,
        };
        if let Some(proposal) = suggest_cut(timeline, *moment, &seed) {
            leads.push((proposal.peak_s - proposal.start_s).clamp(5, 90));
            trails.push((proposal.end_s - proposal.peak_s).clamp(12, 60));
        }
    }
    if leads.is_empty() {
        return None;
    }
    leads.sort_unstable();
    trails.sort_unstable();
    Some(CutTemplate {
        weights,
        lead_seconds: leads[leads.len() / 2],
        trail_seconds: trails[trails.len() / 2],
        sample_count: leads.len() as i32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vod_offset_is_clip_start_not_command_moment() {
        assert_eq!(clip_moment_from_start(3650, 30.0), Some(3680));
        assert_eq!(clip_moment_from_start(0, 18.4), Some(18));
        assert_eq!(clip_moment_from_start(3650, f64::NAN), None);
        assert_eq!(clip_moment_from_start(i32::MAX, 30.0), None);
    }

    #[test]
    fn cuts_before_build_up_and_after_reaction() {
        let mut timeline: Vec<_> = (0..=180)
            .map(|vod_second| ContextSecond {
                vod_second,
                lufs: Some(-45.0),
                ..Default::default()
            })
            .collect();
        timeline[77].speech = Some("Pass auf".to_owned());
        timeline[100].lufs = Some(-8.0);
        timeline[100].soul_jump = Some(500);
        timeline[102].laughter = Some(true);
        timeline[103].speech = Some("Wow".to_owned());
        let template = CutTemplate {
            weights: CutWeights::default(),
            lead_seconds: 45,
            trail_seconds: 25,
            sample_count: 20,
        };
        let cut = suggest_cut(&timeline, 103, &template).unwrap();
        assert_eq!(cut.peak_s, 100);
        assert_eq!(cut.start_s, 75);
        assert!(cut.end_s > 103);
        assert!(cut.end_s <= 126);
    }

    #[test]
    fn clip_near_vod_start_stays_in_bounds() {
        let timeline: Vec<_> = (0..=91)
            .map(|vod_second| ContextSecond {
                vod_second,
                lufs: Some(if vod_second == 4 { -5.0 } else { -45.0 }),
                ..Default::default()
            })
            .collect();
        let template = CutTemplate {
            weights: CutWeights::default(),
            lead_seconds: 30,
            trail_seconds: 15,
            sample_count: 1,
        };
        let cut = suggest_cut(&timeline, 8, &template).unwrap();
        assert_eq!(cut.start_s, 0);
        assert!(cut.end_s <= 92);
    }

    #[test]
    fn early_peak_keeps_reaction_after_clip_request() {
        let mut timeline: Vec<_> = (0..=180)
            .map(|vod_second| ContextSecond {
                vod_second,
                lufs: Some(if vod_second == 80 { -4.0 } else { -45.0 }),
                ..Default::default()
            })
            .collect();
        let template = CutTemplate {
            weights: CutWeights::default(),
            lead_seconds: 30,
            trail_seconds: 12,
            sample_count: 1,
        };
        let cut = suggest_cut(&timeline, 90, &template).unwrap();
        assert_eq!(cut.peak_s, 80);
        assert!(cut.end_s >= 102);
        timeline[115].chat_messages = 1;
        let with_chat = suggest_cut(&timeline, 90, &template).unwrap();
        assert!(with_chat.end_s >= 118);
    }

    #[test]
    fn unconfirmed_kill_feed_ocr_does_not_shift_peak() {
        let mut timeline: Vec<_> = (0..=180)
            .map(|vod_second| ContextSecond {
                vod_second,
                ..Default::default()
            })
            .collect();
        timeline[80].kill_feed = Some("PomaauyBaeoo".to_owned());
        let template = CutTemplate {
            weights: CutWeights::default(),
            lead_seconds: 30,
            trail_seconds: 20,
            sample_count: 1,
        };
        assert_eq!(suggest_cut(&timeline, 90, &template).unwrap().peak_s, 90);
        assert!(confirmed_kill("Player eliminated opponent"));
        assert!(!confirmed_kill("PomaauyBaeoo"));
    }

    #[test]
    fn learning_uses_actual_timeline_and_counts_samples() {
        let timeline: Vec<_> = (0..=180)
            .map(|vod_second| ContextSecond {
                vod_second,
                lufs: Some(if vod_second == 89 { -4.0 } else { -39.0 }),
                ..Default::default()
            })
            .collect();
        let template = learn_template(&[(90, timeline.clone()), (90, timeline)]).unwrap();
        assert_eq!(template.sample_count, 2);
        assert!(template.weights.audio > 0.5);
        assert_eq!(template.weights.speech, 0.0);
        assert!(template.lead_seconds > 0);
        assert!(template.trail_seconds >= 5);
    }
}
