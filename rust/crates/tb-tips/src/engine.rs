use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use tb_knowledge::{rank_tip, KnowledgeBase};

use crate::repo::{self, TipSettings};

pub const MIN_GAP_HOURS: i64 = 12;

pub fn passes_gates(settings: &TipSettings, now: DateTime<Utc>, min_gap_hours: i64) -> bool {
    if settings.opt_out {
        return false;
    }

    match settings.last_tip_sent_at {
        Some(last) => now - last >= Duration::hours(min_gap_hours),
        None => true,
    }
}

pub async fn pick_tip(
    pool: &PgPool,
    kb: &KnowledgeBase,
    twitch_user_id: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    let eligible = kb.eligible_tips();
    if eligible.is_empty() {
        return Ok(None);
    }

    let slugs: Vec<String> = eligible.iter().map(|d| d.slug.clone()).collect();
    let state = repo::load_tip_state(pool, twitch_user_id, &slugs).await?;

    Ok(rank_tip(&eligible, &state).map(|d| (d.slug.clone(), d.tip_text.clone())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-21T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn opt_out_blockt() {
        let s = TipSettings {
            opt_out: true,
            last_tip_sent_at: None,
        };
        assert!(!passes_gates(&s, now(), MIN_GAP_HOURS));
    }

    #[test]
    fn nie_gesendet_passt() {
        let s = TipSettings {
            opt_out: false,
            last_tip_sent_at: None,
        };
        assert!(passes_gates(&s, now(), MIN_GAP_HOURS));
    }

    #[test]
    fn innerhalb_12h_blockt() {
        let last = now() - Duration::hours(5);
        let s = TipSettings {
            opt_out: false,
            last_tip_sent_at: Some(last),
        };
        assert!(!passes_gates(&s, now(), MIN_GAP_HOURS));
    }

    #[test]
    fn nach_12h_passt() {
        let last = now() - Duration::hours(13);
        let s = TipSettings {
            opt_out: false,
            last_tip_sent_at: Some(last),
        };
        assert!(passes_gates(&s, now(), MIN_GAP_HOURS));
    }
}
