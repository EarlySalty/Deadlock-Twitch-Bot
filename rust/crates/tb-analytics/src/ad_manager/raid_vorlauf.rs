//! Kurzer Empfänger-Schutz, bevor unser Bot einen Netzwerk-Raid einleitet.
//! RAM schützt sofort; der dauerhafte Zustand wird abgekoppelt geschrieben.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Instant;

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use tokio::sync::Notify;

pub const VORLAUF_TTL: Duration = Duration::minutes(2);

#[derive(Clone)]
pub struct Attempt {
    pub id: String,
    pub from_id: String,
    pub to_id: String,
    pub until: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub status: &'static str,
}

#[derive(Default)]
struct Warning {
    notices: VecDeque<Instant>,
    repeated: u64,
}

pub struct RaidAdVorlauf {
    pool: PgPool,
    attempts: Mutex<HashMap<String, Attempt>>,
    warnings: Mutex<HashMap<(String, &'static str), Warning>>,
    pub wake: Notify,
}

impl RaidAdVorlauf {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            attempts: Mutex::new(HashMap::new()),
            warnings: Mutex::new(HashMap::new()),
            wake: Notify::new(),
        }
    }

    pub fn announce(&self, id: &str, from_id: &str, to_id: &str, now: DateTime<Utc>) -> Attempt {
        let attempt = Attempt {
            id: id.to_owned(), from_id: from_id.to_owned(), to_id: to_id.to_owned(),
            until: now + VORLAUF_TTL, created_at: now, status: "pending",
        };
        let mut attempts = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        attempts.retain(|_, value| value.until > now);
        attempts.insert(id.to_owned(), attempt.clone());
        drop(attempts);
        self.wake.notify_one();
        attempt
    }

    pub fn complete(&self, id: &str, started: bool, now: DateTime<Utc>) -> Option<Attempt> {
        let mut attempts = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        let attempt = attempts.get_mut(id)?;
        // Ein verspäteter Erfolgswrite darf einen verworfenen Versuch nie beleben.
        if !matches!(attempt.status, "failed" | "arrived") {
            attempt.status = if started { "started" } else { "failed" };
            if started { attempt.until = now + VORLAUF_TTL; }
        }
        let value = attempt.clone();
        drop(attempts);
        self.wake.notify_one();
        Some(value)
    }

    pub fn source_attempt_ids(&self, from_id: &str, now: DateTime<Utc>) -> Vec<String> {
        self.attempts.lock().unwrap_or_else(|e| e.into_inner()).values()
            .filter(|attempt| attempt.from_id == from_id && attempt.until > now
                && matches!(attempt.status, "pending" | "started"))
            .map(|attempt| attempt.id.clone()).collect()
    }

    pub async fn cancel_source_before(&self, from_id: &str, cutoff: DateTime<Utc>) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '1500ms'").execute(&mut *tx).await?;
        sqlx::query("SET LOCAL statement_timeout = '1800ms'").execute(&mut *tx).await?;
        let targets: Vec<String> = sqlx::query_scalar("SELECT DISTINCT to_broadcaster_id FROM twitch_raid_ad_vorlauf WHERE from_broadcaster_id=$1 AND created_at <= $2 AND status IN ('pending','started') ORDER BY to_broadcaster_id")
            .bind(from_id).bind(cutoff).fetch_all(&mut *tx).await?;
        for target in &targets {
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('raid-ad-vorlauf:' || $1::text, 0))")
                .bind(target).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE twitch_raid_ad_vorlauf SET status='failed' WHERE from_broadcaster_id=$1 AND created_at <= $2 AND to_broadcaster_id=ANY($3) AND status IN ('pending','started')")
            .bind(from_id).bind(cutoff).bind(&targets).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub fn local_until(&self, to_id: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.attempts.lock().unwrap_or_else(|e| e.into_inner()).values()
            .filter(|attempt| attempt.to_id == to_id && matches!(attempt.status, "pending" | "started") && attempt.until > now)
            .map(|attempt| attempt.until).max()
    }

    pub async fn persist(&self, attempt: &Attempt) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '1500ms'").execute(&mut *tx).await?;
        sqlx::query("SET LOCAL statement_timeout = '1800ms'").execute(&mut *tx).await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('raid-ad-vorlauf:' || $1::text, 0))")
            .bind(&attempt.to_id).execute(&mut *tx).await?;
        if attempt.status == "pending" {
            sqlx::query("INSERT INTO twitch_raid_ad_vorlauf(attempt_id,from_broadcaster_id,to_broadcaster_id,status,protected_until,created_at) VALUES($1,$2,$3,CASE WHEN EXISTS(SELECT 1 FROM twitch_raid_arrival_tracking WHERE from_broadcaster_id=$2 AND to_broadcaster_id=$3 AND detected_at >= $5 AND detected_at <= $4) THEN 'arrived' ELSE 'pending' END,$4,$5) ON CONFLICT(attempt_id) DO NOTHING")
                .bind(&attempt.id).bind(&attempt.from_id).bind(&attempt.to_id).bind(attempt.until).bind(attempt.created_at)
                .execute(&mut *tx).await?;
        } else {
            sqlx::query("INSERT INTO twitch_raid_ad_vorlauf(attempt_id,from_broadcaster_id,to_broadcaster_id,status,protected_until,created_at) VALUES($1,$2,$3,CASE WHEN $4 <> 'failed' AND EXISTS(SELECT 1 FROM twitch_raid_arrival_tracking WHERE from_broadcaster_id=$2 AND to_broadcaster_id=$3 AND detected_at >= $6 AND detected_at <= $5) THEN 'arrived' ELSE $4 END,$5,$6) ON CONFLICT(attempt_id) DO UPDATE SET status=EXCLUDED.status,protected_until=EXCLUDED.protected_until WHERE twitch_raid_ad_vorlauf.status NOT IN ('failed','arrived')")
                .bind(&attempt.id).bind(&attempt.from_id).bind(&attempt.to_id).bind(attempt.status).bind(attempt.until).bind(attempt.created_at)
                .execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn active_until(&self, to_id: &str, now: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
        let rows: Vec<(String, DateTime<Utc>, bool)> = sqlx::query_as("SELECT v.attempt_id,v.protected_until,(v.status IN ('failed','arrived')) FROM twitch_raid_ad_vorlauf v WHERE v.to_broadcaster_id=$1 AND v.protected_until>$2")
            .bind(to_id).bind(now).fetch_all(&self.pool).await?;
        let mut attempts = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        for (id, _, terminal) in &rows {
            if *terminal {
                if let Some(attempt) = attempts.get_mut(id) {
                    attempt.status = "arrived";
                }
            }
        }
        // RAM ist neuer als abgekoppelte DB-Writes, insbesondere beim Fehlschlag.
        let persisted = rows.into_iter().filter(|(id, _, terminal)| {
            !terminal && !attempts.get(id).is_some_and(|attempt| matches!(attempt.status, "failed" | "arrived"))
        }).map(|(_, until, _)| until).max();
        let local = attempts.values().filter(|attempt| attempt.to_id == to_id && matches!(attempt.status, "pending" | "started") && attempt.until > now)
            .map(|attempt| attempt.until).max();
        Ok(local.into_iter().chain(persisted).max())
    }

    pub async fn cleanup(&self) -> Result<(), sqlx::Error> {
        // Ein Tag schützt gegen spät fertig werdende Writes; die fachliche TTL
        // wird beim Lesen geprüft und benötigt keinen reparierenden Poll.
        sqlx::query("DELETE FROM twitch_raid_ad_vorlauf WHERE protected_until < NOW()-INTERVAL '1 day'")
            .execute(&self.pool).await?;
        Ok(())
    }

    pub fn warn_limited(&self, target: &str, reason: &'static str) {
        let now = Instant::now();
        let mut warnings = self.warnings.lock().unwrap_or_else(|e| e.into_inner());
        let warning = warnings.entry((target.to_owned(), reason)).or_default();
        while warning.notices.front().is_some_and(|at| now.duration_since(*at).as_secs() >= 7 * 86400) {
            warning.notices.pop_front();
        }
        warning.repeated = warning.repeated.saturating_add(1);
        if warning.notices.len() >= 2 || warning.notices.back().is_some_and(|at| now.duration_since(*at).as_secs() < 86400) { return; }
        let repeated = warning.repeated.saturating_sub(1);
        warning.repeated = 0;
        warning.notices.push_back(now);
        tracing::warn!(twitch_user_id=target, grund=reason, wiederholungen=repeated,
            "Raid-Werbeschutz ist eingeschränkt; der Raid läuft unverändert weiter");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn registry() -> RaidAdVorlauf {
        RaidAdVorlauf::new(PgPoolOptions::new().connect_lazy("postgresql://localhost/unused").unwrap())
    }

    #[tokio::test]
    async fn vorlauf_weckt_worker_und_fehlschlag_betrifft_nur_eigenen_versuch() {
        let registry = registry();
        let now = Utc::now();
        registry.announce("a", "1", "2", now);
        tokio::time::timeout(std::time::Duration::from_millis(20), registry.wake.notified()).await.unwrap();
        registry.announce("b", "3", "2", now);
        registry.complete("a", false, now);
        assert_eq!(registry.local_until("2", now), Some(now + VORLAUF_TTL));
        registry.complete("a", true, now + Duration::seconds(1));
        assert_eq!(registry.attempts.lock().unwrap()["a"].status, "failed");
        let before_cancel = registry.source_attempt_ids("3", now);
        registry.announce("c", "3", "2", now);
        for id in before_cancel { registry.complete(&id, false, now); }
        assert_eq!(registry.attempts.lock().unwrap()["c"].status, "pending");
        registry.complete("c", false, now);
        registry.complete("b", false, now);
        assert_eq!(registry.local_until("2", now), None);
    }

    #[tokio::test]
    async fn unbekannter_ausgang_und_ausgebliebene_ankunft_laufen_kurz_ab() {
        let registry = registry();
        let now = Utc::now();
        let pending = registry.announce("a", "1", "2", now);
        assert_eq!(registry.local_until("2", now), Some(pending.until));
        assert_eq!(registry.local_until("2", pending.until), None);
        registry.announce("b", "1", "4", now);
        registry.complete("b", true, now + Duration::seconds(15));
        assert_eq!(registry.local_until("4", now), Some(now + Duration::seconds(15) + VORLAUF_TTL));
        assert_eq!(registry.local_until("2", now + VORLAUF_TTL), None);
    }
}
