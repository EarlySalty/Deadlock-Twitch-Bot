use crate::file::FileError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Challenges {
    pub enabled: bool,
    pub poll_seconds: u64,
    pub evidence_max_gap_seconds: i64,
    pub source_timeout_seconds: u64,
    pub source_batch_size: i64,
    pub community_guild_id: i64,
    pub points: Points,
    pub weekly_caps: WeeklyCaps,
    pub level_thresholds: Vec<i64>,
    pub stream_extra_minutes: i64,
    pub achievements: Achievements,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Points {
    pub qualified_invite: i32,
    pub streamer_referral: i32,
    pub co_stream: i32,
    pub party_play: i32,
    pub clip_submitted: i32,
    pub clip_top3: [i32; 3],
    pub quest_done: i32,
    pub quest_all_three_bonus: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeeklyCaps {
    pub party_play: i64,
    pub clip_submitted: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Achievements {
    pub recruiter: Vec<i64>,
    pub team_player: Vec<i64>,
    pub duo: Vec<i64>,
    pub stamina: Vec<i64>,
    pub talent_scout: Vec<i64>,
    pub clip_hunter: Vec<i64>,
}

impl Default for Challenges {
    fn default() -> Self {
        Self {
            enabled: true,
            poll_seconds: 60,
            evidence_max_gap_seconds: 150,
            source_timeout_seconds: 10,
            source_batch_size: 250,
            community_guild_id: 1289721245281292288,
            points: Points::default(),
            weekly_caps: WeeklyCaps::default(),
            level_thresholds: vec![0, 50, 150, 400, 1000, 2500],
            stream_extra_minutes: 30,
            achievements: Achievements::default(),
        }
    }
}

impl Default for Points {
    fn default() -> Self {
        Self {
            qualified_invite: 10,
            streamer_referral: 50,
            co_stream: 8,
            party_play: 5,
            clip_submitted: 2,
            clip_top3: [20, 15, 10],
            quest_done: 5,
            quest_all_three_bonus: 10,
        }
    }
}

impl Default for WeeklyCaps {
    fn default() -> Self {
        Self {
            party_play: 4,
            clip_submitted: 2,
        }
    }
}

impl Default for Achievements {
    fn default() -> Self {
        Self {
            recruiter: vec![5, 25, 100],
            team_player: vec![10, 50, 150],
            duo: vec![5, 25],
            stamina: vec![4, 12, 26],
            talent_scout: vec![1, 3],
            clip_hunter: vec![1, 5],
        }
    }
}

impl Challenges {
    pub fn validate(&self) -> Result<(), FileError> {
        let p = &self.points;
        let mut values = vec![
            p.qualified_invite,
            p.streamer_referral,
            p.co_stream,
            p.party_play,
            p.clip_submitted,
            p.quest_done,
            p.quest_all_three_bonus,
        ];
        values.extend(p.clip_top3);
        if values.iter().any(|v| !(1..=10000).contains(v)) {
            return Err(FileError::invalid("challenges.points"));
        }
        if self.level_thresholds.first() != Some(&0)
            || self.level_thresholds.len() < 2
            || self.level_thresholds.len() > 100
            || !self.level_thresholds.windows(2).all(|w| w[0] < w[1])
            || self.level_thresholds.iter().any(|v| *v > 1_000_000_000)
        {
            return Err(FileError::invalid("challenges.level_thresholds"));
        }
        let a = &self.achievements;
        for thresholds in [
            &a.recruiter,
            &a.team_player,
            &a.duo,
            &a.stamina,
            &a.talent_scout,
            &a.clip_hunter,
        ] {
            if thresholds.is_empty()
                || thresholds.len() > 20
                || thresholds.iter().any(|v| *v <= 0 || *v > 1_000_000_000)
                || !thresholds.windows(2).all(|w| w[0] < w[1])
            {
                return Err(FileError::invalid("challenges.achievements"));
            }
        }
        if !(15..=120).contains(&self.poll_seconds)
            || self.evidence_max_gap_seconds < self.poll_seconds as i64 * 2
            || self.evidence_max_gap_seconds > 300
            || !(1..=30).contains(&self.source_timeout_seconds)
            || !(1..=1000).contains(&self.source_batch_size)
            || !(1..=100).contains(&self.weekly_caps.party_play)
            || !(1..=100).contains(&self.weekly_caps.clip_submitted)
            || !(1..=600).contains(&self.stream_extra_minutes)
            || self.community_guild_id <= 0
        {
            return Err(FileError::invalid("challenges"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_config_deserializes_and_validates() {
        #[derive(Deserialize)]
        struct Example {
            challenges: Challenges,
        }
        let example: Example =
            toml::from_str(include_str!("../../../../docs/partner-effort-config.toml")).unwrap();
        example.challenges.validate().unwrap();
        assert_eq!(
            example.challenges.level_thresholds,
            Challenges::default().level_thresholds
        );
    }

    #[test]
    fn defaults_and_invalid_economy() {
        let mut c = Challenges::default();
        c.validate().unwrap();
        c.points.clip_top3[0] = -1;
        assert!(c.validate().is_err());
        c = Challenges::default();
        c.level_thresholds = vec![0, 50, 50];
        assert!(c.validate().is_err());
        c = Challenges::default();
        c.achievements.stamina = vec![4, 3];
        assert!(c.validate().is_err());
    }
}
