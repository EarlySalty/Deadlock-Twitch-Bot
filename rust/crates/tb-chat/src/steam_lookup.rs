//! Steam-/Rang-/Live-Lookup für den `!title`-Generator (B11).
//!
//! Steam-Links, Rang und Live-Daten kommen aus Central Postgres.

use sqlx::PgPool;

/// Deadlock-Rangnamen 0..11 (Python `_RANK_NAMES`; 10 und 11 = Eternus).
fn rank_name(rank_num: i64) -> &'static str {
    match rank_num {
        0 => "Obscurus",
        1 => "Seeker",
        2 => "Alchemist",
        3 => "Arcanist",
        4 => "Ritualist",
        5 => "Emissary",
        6 => "Archon",
        7 => "Oracle",
        8 => "Phantom",
        9 => "Ascendant",
        10 | 11 => "Eternus",
        _ => "Unknown",
    }
}

/// Rang-Info eines Discord-Users (Python `get_rank_for_discord_user`).
#[derive(Debug, Clone)]
pub struct RankInfo {
    pub rank_name: String,
    pub rank_num: i64,
    pub subrank: i64,
    pub rank_display: String,
}

/// Live-In-Game-Zustand (Python `get_live_state_for_discord_user`).
#[derive(Debug, Clone)]
pub struct LiveState {
    pub in_match: bool,
    pub hero: Option<String>,
    pub party_hint: Option<String>,
    pub stage: Option<String>,
}

/// Rang-Info für einen Discord-User; `None` wenn kein Link oder keine Rangdaten.
pub async fn get_rank_for_discord_user(pool: &PgPool, user_id: i64) -> Option<RankInfo> {
    let row = sqlx::query_as::<_, (Option<i32>, Option<i32>)>(
        "SELECT deadlock_rank, deadlock_subrank
         FROM core.steam_links
         WHERE discord_id = $1
         ORDER BY primary_account DESC, verified DESC, linked_at DESC NULLS LAST, steam_id ASC
         LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await;
    let (rank, subrank) = match row {
        Ok(Some(row)) => row,
        Ok(None) => return None,
        Err(error) => {
            tracing::warn!(
                %error,
                discord_id_tail = user_id.rem_euclid(10_000),
                "!title Steam-Rank-Abfrage fehlgeschlagen; der Titel wird ohne Rang erzeugt"
            );
            return None;
        }
    };
    let rank_num = i64::from(rank.unwrap_or(0));
    let subrank = i64::from(subrank.unwrap_or(0));
    let name = rank_name(rank_num);
    // Python: f"{name} {subrank or ''}".strip() — subrank 0/None → nur Name.
    let rank_display = if subrank != 0 {
        format!("{name} {subrank}")
    } else {
        name.to_string()
    };
    Some(RankInfo {
        rank_name: name.to_string(),
        rank_num,
        subrank,
        rank_display,
    })
}

/// Live-Zustand falls aktuell in Deadlock, sonst `None` (Python
/// `get_live_state_for_discord_user`: `not in_deadlock_now` → None).
pub async fn get_live_state_for_discord_user(
    pool: &PgPool,
    user_id: i64,
) -> Result<Option<LiveState>, sqlx::Error> {
    let row = sqlx::query_as::<
        _,
        (
            Option<bool>,
            Option<bool>,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    >(
        "SELECT lps.in_deadlock_now, lps.in_match_now_strict, lps.deadlock_hero,
                lps.deadlock_party_hint, lps.deadlock_stage
         FROM core.steam_links sl
         JOIN activity.live_player_state lps ON sl.steam_id = lps.steam_id
         WHERE sl.discord_id = $1
         ORDER BY sl.primary_account DESC, sl.verified DESC, sl.linked_at DESC NULLS LAST, sl.steam_id ASC
         LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    let Some((in_deadlock, in_match, hero, party_hint, stage)) = row else {
        return Ok(None);
    };
    if !in_deadlock.unwrap_or(false) {
        return Ok(None);
    }
    Ok(Some(LiveState {
        in_match: in_match.unwrap_or(false),
        hero,
        party_hint,
        stage,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;
    use sqlx::PgPool;

    async fn make_pg_pool() -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query("CREATE SCHEMA IF NOT EXISTS core")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE SCHEMA IF NOT EXISTS activity")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS core.users (
                discord_id BIGINT PRIMARY KEY,
                username TEXT,
                global_name TEXT,
                avatar TEXT,
                first_seen TIMESTAMPTZ NOT NULL DEFAULT now(),
                last_seen TIMESTAMPTZ NOT NULL DEFAULT now(),
                raw JSONB
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS core.steam_links (
                discord_id BIGINT NOT NULL,
                steam_id TEXT NOT NULL,
                steam_id64 BIGINT,
                verified BOOLEAN NOT NULL DEFAULT false,
                primary_account BOOLEAN NOT NULL DEFAULT false,
                linked_at TIMESTAMPTZ,
                deadlock_rank INTEGER,
                deadlock_subrank INTEGER,
                deadlock_rank_name TEXT,
                deadlock_rank_updated_at TIMESTAMPTZ,
                PRIMARY KEY (discord_id, steam_id)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS activity.live_player_state (
                steam_id TEXT PRIMARY KEY,
                in_deadlock_now BOOLEAN,
                in_match_now_strict BOOLEAN,
                deadlock_stage TEXT,
                deadlock_hero TEXT,
                deadlock_party_hint TEXT,
                deadlock_minutes INTEGER,
                deadlock_updated_at TIMESTAMPTZ,
                last_seen_at TIMESTAMPTZ
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn rank_lookup_namen_und_subrank() {
        let Some(pool) = make_pg_pool().await else {
            return;
        };
        let ranked_discord_id = 8_200_000_000_000_101_i64;
        let unrated_discord_id = 8_200_000_000_000_102_i64;
        sqlx::query("DELETE FROM core.steam_links WHERE discord_id = ANY($1)")
            .bind([ranked_discord_id, unrated_discord_id])
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO core.steam_links (
                discord_id, steam_id, deadlock_rank, deadlock_subrank
             )
             VALUES
                ($1, 'rank-contract', 6, 3),
                ($2, 'rank-null-contract', NULL, NULL)",
        )
        .bind(ranked_discord_id)
        .bind(unrated_discord_id)
        .execute(&pool)
        .await
        .unwrap();

        let rank = get_rank_for_discord_user(&pool, ranked_discord_id)
            .await
            .unwrap();
        assert_eq!(rank.rank_name, "Archon");
        assert_eq!(rank.subrank, 3);
        assert_eq!(rank.rank_display, "Archon 3");

        let unrated = get_rank_for_discord_user(&pool, unrated_discord_id)
            .await
            .unwrap();
        assert_eq!(unrated.rank_display, "Obscurus");
        assert!(get_rank_for_discord_user(&pool, 8_200_000_000_000_199_i64)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn live_state_nur_wenn_in_deadlock() {
        let Some(pool) = make_pg_pool().await else {
            return;
        };
        let discord_id = 9_223_372_036_854_774_000_i64;
        let verified_steam_id64 = 76_561_197_960_265_733_i64;
        let unverified_steam_id64 = 76_561_197_960_265_734_i64;
        sqlx::query("DELETE FROM core.steam_links WHERE discord_id = $1")
            .bind(discord_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM activity.live_player_state WHERE steam_id = ANY($1)")
            .bind([
                verified_steam_id64.to_string(),
                unverified_steam_id64.to_string(),
            ])
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO core.users (discord_id) VALUES ($1)
             ON CONFLICT (discord_id) DO NOTHING",
        )
        .bind(discord_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO core.steam_links (discord_id, steam_id, steam_id64, verified, linked_at)
             VALUES
                ($1, $2::text, $2, true, '2026-07-28T10:00:00Z'),
                ($1, $3::text, $3, false, '2026-07-28T11:00:00Z')
             ON CONFLICT (discord_id, steam_id) DO UPDATE
             SET verified = EXCLUDED.verified, linked_at = EXCLUDED.linked_at",
        )
        .bind(discord_id)
        .bind(verified_steam_id64)
        .bind(unverified_steam_id64)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO activity.live_player_state (
                steam_id, in_deadlock_now, in_match_now_strict,
                deadlock_hero, deadlock_party_hint, deadlock_stage
             )
             VALUES
                ($1, true, true, 'Haze', 'solo', 'laning'),
                ($2, true, false, 'Seven', 'duo', 'mid')
             ON CONFLICT (steam_id) DO UPDATE SET
                in_deadlock_now = EXCLUDED.in_deadlock_now,
                in_match_now_strict = EXCLUDED.in_match_now_strict,
                deadlock_hero = EXCLUDED.deadlock_hero,
                deadlock_party_hint = EXCLUDED.deadlock_party_hint,
                deadlock_stage = EXCLUDED.deadlock_stage",
        )
        .bind(verified_steam_id64.to_string())
        .bind(unverified_steam_id64.to_string())
        .execute(&pool)
        .await
        .unwrap();

        let ls = get_live_state_for_discord_user(&pool, discord_id)
            .await
            .unwrap()
            .unwrap();
        assert!(ls.in_match);
        assert_eq!(ls.hero.as_deref(), Some("Haze"));
        assert_eq!(ls.stage.as_deref(), Some("laning"));

        // in_deadlock_now = 0 → None.
        sqlx::query(
            "UPDATE activity.live_player_state
             SET in_deadlock_now = false
             WHERE steam_id = $1",
        )
        .bind(verified_steam_id64.to_string())
        .execute(&pool)
        .await
        .unwrap();
        assert!(get_live_state_for_discord_user(&pool, discord_id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn live_state_sortiert_linked_at_null_nach_hinten() {
        let Some(pool) = make_pg_pool().await else {
            return;
        };
        let discord_id = 8_200_000_000_000_001_i64;
        let dated_steam_id64 = 76_561_197_960_265_823_i64;
        let undated_steam_id64 = 76_561_197_960_265_824_i64;
        sqlx::query("DELETE FROM core.steam_links WHERE discord_id = $1")
            .bind(discord_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM activity.live_player_state WHERE steam_id = ANY($1)")
            .bind([dated_steam_id64.to_string(), undated_steam_id64.to_string()])
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO core.steam_links
                (discord_id, steam_id, steam_id64, verified, linked_at)
             VALUES
                ($1, $2::text, $2, true, '2026-07-28T10:00:00Z'),
                ($1, $3::text, $3, true, NULL)",
        )
        .bind(discord_id)
        .bind(dated_steam_id64)
        .bind(undated_steam_id64)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO activity.live_player_state (
                steam_id, in_deadlock_now, in_match_now_strict,
                deadlock_hero, deadlock_party_hint, deadlock_stage
             )
             VALUES
                ($1, true, true, 'Haze', 'solo', 'laning'),
                ($2, true, true, 'Seven', 'duo', 'mid')
             ON CONFLICT (steam_id) DO UPDATE SET
                in_deadlock_now = EXCLUDED.in_deadlock_now,
                in_match_now_strict = EXCLUDED.in_match_now_strict,
                deadlock_hero = EXCLUDED.deadlock_hero,
                deadlock_party_hint = EXCLUDED.deadlock_party_hint,
                deadlock_stage = EXCLUDED.deadlock_stage",
        )
        .bind(dated_steam_id64.to_string())
        .bind(undated_steam_id64.to_string())
        .execute(&pool)
        .await
        .unwrap();

        let live = get_live_state_for_discord_user(&pool, discord_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(live.hero.as_deref(), Some("Haze"));
    }

    #[tokio::test]
    async fn live_state_queryfehler_ist_fehler() {
        let Some(pool) = make_pg_pool().await else {
            return;
        };
        pool.close().await;

        assert!(get_live_state_for_discord_user(&pool, 200).await.is_err());
    }
}
