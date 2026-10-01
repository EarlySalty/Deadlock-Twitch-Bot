use chrono::{DateTime, Duration, Utc};
use sqlx::{Postgres, Transaction};
use tb_domain::referral_window::RESERVATION_TTL;

pub async fn credit_first_activation(
    tx: &mut Transaction<'_, Postgres>,
    referred_user_id: &str,
    referred_login: &str,
    activated_at: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext(LOWER($1))::bigint)")
        .bind(referred_login)
        .execute(&mut **tx)
        .await?;
    let claims: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT streamer_user_id, streamer_login, claimed_at
         FROM twitch_referral_claim_candidates($1)",
    )
    .bind(referred_user_id)
    .fetch_all(&mut **tx)
    .await?;
    if claims.len() != 1 {
        return Ok(false);
    }
    let (streamer_user_id, streamer_login, claimed_at) = &claims[0];
    let Some(claimed_at) = qualifying_claim_time(claimed_at, activated_at) else {
        return Ok(false);
    };
    let valid_id = |id: &str| {
        !id.starts_with('0')
            && !id.is_empty()
            && id.bytes().all(|byte| byte.is_ascii_digit())
            && id.parse::<u64>().is_ok_and(|id| id > 0)
    };
    if !valid_id(streamer_user_id) || !valid_id(referred_user_id) {
        return Ok(false);
    }
    let result = sqlx::query(
        "INSERT INTO twitch_streamer_referral_credits
         (source_id, streamer_twitch_user_id, streamer_login, referred_twitch_user_id,
          referred_login, source_claimed_at, credited_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (referred_twitch_user_id) DO NOTHING",
    )
    .bind(format!("streamer_referral:{referred_user_id}"))
    .bind(streamer_user_id)
    .bind(streamer_login)
    .bind(referred_user_id)
    .bind(referred_login)
    .bind(claimed_at)
    .bind(activated_at)
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected() == 1)
}

fn qualifying_claim_time(raw: &str, activated_at: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let claimed_at = DateTime::parse_from_rfc3339(raw.trim())
        .ok()?
        .with_timezone(&Utc);
    (claimed_at <= activated_at
        && claimed_at >= activated_at - Duration::seconds(RESERVATION_TTL.seconds()))
    .then_some(claimed_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_existing_unexpired_pre_activation_claims_are_eligible() {
        let now = DateTime::parse_from_rfc3339("2026-01-10T12:00:00Z")
            .expect("activation fixture")
            .with_timezone(&Utc);
        for days in [0, 1, 4] {
            let timestamp = now - Duration::days(days);
            assert_eq!(
                qualifying_claim_time(&timestamp.to_rfc3339(), now),
                Some(timestamp)
            );
        }
        for timestamp in [
            now + Duration::seconds(1),
            now - Duration::days(4) - Duration::seconds(1),
        ] {
            assert!(qualifying_claim_time(&timestamp.to_rfc3339(), now).is_none());
        }
        assert!(qualifying_claim_time("invalid", now).is_none());
    }
}
