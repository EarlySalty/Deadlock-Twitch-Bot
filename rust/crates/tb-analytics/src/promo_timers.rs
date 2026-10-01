//! Gemeinsame Promo-Timer und die unabhängige Policy des Community-Kanals.
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub const COMMUNITY_BROADCASTER_ID: &str = "1367527782";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PromoTimerPolicy {
    pub overall_cooldown_minutes: u64,
    pub activity_cooldown_min_minutes: u64,
    pub activity_cooldown_max_minutes: u64,
    pub min_messages: usize,
    pub new_chatters: usize,
    pub attempt_cooldown_minutes: u64,
    pub viewer_spike_cooldown_minutes: u64,
    pub pitch_cooldown_minutes: u64,
    pub pitch_max_per_stream: i64,
}

impl Default for PromoTimerPolicy {
    fn default() -> Self {
        Self {
            overall_cooldown_minutes: 90,
            activity_cooldown_min_minutes: 45,
            activity_cooldown_max_minutes: 180,
            min_messages: 16,
            new_chatters: 2,
            attempt_cooldown_minutes: 10,
            viewer_spike_cooldown_minutes: 60,
            pitch_cooldown_minutes: 10,
            pitch_max_per_stream: 3,
        }
    }
}

impl PromoTimerPolicy {
    fn validate(&self) -> Result<(), &'static str> {
        if [
            self.overall_cooldown_minutes,
            self.activity_cooldown_min_minutes,
            self.activity_cooldown_max_minutes,
            self.attempt_cooldown_minutes,
            self.viewer_spike_cooldown_minutes,
            self.pitch_cooldown_minutes,
        ]
        .iter()
        .any(|value| !(1..=1440).contains(value))
        {
            return Err("Zeitabstände müssen ganze Minuten zwischen 1 und 1440 sein.");
        }
        if self.activity_cooldown_min_minutes > self.activity_cooldown_max_minutes {
            return Err("Der kürzeste Aktivitätsabstand darf den längsten nicht überschreiten.");
        }
        if self.min_messages > 1000
            || self.new_chatters > 100
            || !(1..=100).contains(&self.pitch_max_per_stream)
        {
            return Err("Nachrichten müssen zwischen 0 und 1000, neue Chatter zwischen 0 und 100 und Hinweise je Stream zwischen 1 und 100 liegen.");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommunityTimerPolicy {
    pub broadcaster_id: String,
    #[serde(flatten)]
    pub timers: PromoTimerPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromoTimerSettings {
    pub defaults: PromoTimerPolicy,
    pub community: CommunityTimerPolicy,
}

impl Default for PromoTimerSettings {
    fn default() -> Self {
        Self {
            defaults: PromoTimerPolicy::default(),
            community: CommunityTimerPolicy {
                broadcaster_id: COMMUNITY_BROADCASTER_ID.into(),
                timers: PromoTimerPolicy {
                    overall_cooldown_minutes: 20,
                    activity_cooldown_min_minutes: 20,
                    activity_cooldown_max_minutes: 30,
                    min_messages: 8,
                    new_chatters: 0,
                    attempt_cooldown_minutes: 5,
                    viewer_spike_cooldown_minutes: 20,
                    pitch_cooldown_minutes: 5,
                    pitch_max_per_stream: 6,
                },
            },
        }
    }
}

impl PromoTimerSettings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.community.broadcaster_id != COMMUNITY_BROADCASTER_ID {
            return Err("Die Twitch-ID des Community-Kanals kann nicht geändert werden.");
        }
        self.defaults.validate()?;
        self.community.timers.validate()
    }

    pub fn for_broadcaster(&self, id: &str) -> PromoTimerPolicy {
        if id == COMMUNITY_BROADCASTER_ID {
            self.community.timers.clone()
        } else {
            self.defaults.clone()
        }
    }
}

pub async fn load(pool: &PgPool) -> Result<PromoTimerSettings, sqlx::Error> {
    let value: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT settings FROM twitch_promo_timer_settings WHERE singleton = true",
    )
    .fetch_optional(pool)
    .await?;
    let settings: PromoTimerSettings = value
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))?
        .unwrap_or_default();
    settings
        .validate()
        .map_err(|e| sqlx::Error::Protocol(e.into()))?;
    Ok(settings)
}

pub async fn save(pool: &PgPool, settings: &PromoTimerSettings) -> Result<(), sqlx::Error> {
    settings
        .validate()
        .map_err(|e| sqlx::Error::Protocol(e.into()))?;
    sqlx::query("INSERT INTO twitch_promo_timer_settings(singleton,settings,updated_at) VALUES(true,$1,now()) ON CONFLICT(singleton) DO UPDATE SET settings=EXCLUDED.settings,updated_at=now()")
        .bind(serde_json::to_value(settings).map_err(|e| sqlx::Error::Encode(Box::new(e)))?)
        .execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_roundtrip_and_channel_isolation() {
        let settings = PromoTimerSettings::default();
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            serde_json::from_value::<PromoTimerSettings>(json).unwrap(),
            settings
        );
        assert_eq!(settings.for_broadcaster("dach_lock"), settings.defaults);
        assert_eq!(settings.for_broadcaster(""), settings.defaults);
        assert_eq!(
            settings
                .for_broadcaster(COMMUNITY_BROADCASTER_ID)
                .overall_cooldown_minutes,
            20
        );
    }
    #[test]
    fn rejects_invalid_ranges_and_identity() {
        let mut settings = PromoTimerSettings::default();
        settings.defaults.activity_cooldown_min_minutes = 181;
        assert!(settings.validate().is_err());
        settings = PromoTimerSettings::default();
        settings.community.broadcaster_id = "another".into();
        assert!(settings.validate().is_err());
    }
}
