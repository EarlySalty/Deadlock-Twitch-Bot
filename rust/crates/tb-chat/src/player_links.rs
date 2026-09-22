//! Voluntary Twitch -> Steam identity. Never creates or modifies Discord links.
use sqlx::PgPool;

pub const CONNECT_PATH: &str = "/twitch/connect";
pub const CONNECT_URL: &str = "https://deutsche-deadlock-community.de/twitch/connect";
pub const DISCONNECTED_REPLY: &str = "Für dieses Twitch-Konto ist die Steam-Zuordnung deaktiviert. Nur die Person selbst kann sie mit !connect wieder aktivieren.";
pub const STEAM64_BASE: i64 = 76561197960265728;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PlayerLink {
    pub steam_id64: Option<i64>,
    pub lookup_enabled: bool,
    pub revision: i64,
}

impl PlayerLink {
    pub fn account_id(&self) -> Option<u32> {
        self.steam_id64
            .and_then(|id| id.checked_sub(STEAM64_BASE))
            .and_then(|id| u32::try_from(id).ok())
            .filter(|id| *id > 0)
    }
}

#[derive(Debug, Clone, sqlx::FromRow, PartialEq, Eq)]
pub struct PlayerSteamAccount {
    pub steam_id64: i64,
    pub is_primary: bool,
}

pub async fn load(pool: &PgPool, twitch_user_id: &str) -> Result<Option<PlayerLink>, sqlx::Error> {
    sqlx::query_as("SELECT steam_id64, lookup_enabled, revision FROM twitch_player_steam_links WHERE twitch_user_id = $1")
        .bind(twitch_user_id).fetch_optional(pool).await
}

pub async fn accounts(
    pool: &PgPool,
    twitch_user_id: &str,
) -> Result<Vec<PlayerSteamAccount>, sqlx::Error> {
    sqlx::query_as(
        "SELECT steam_id64, is_primary
         FROM twitch_player_steam_accounts
         WHERE twitch_user_id = $1
         ORDER BY is_primary DESC, linked_at ASC, steam_id64 ASC",
    )
    .bind(twitch_user_id)
    .fetch_all(pool)
    .await
}

/// Saves an opt-out even without a direct link, so legacy/name fallback stays off.
/// Revision invalidates Steam callbacks started before this command.
pub async fn disconnect(pool: &PgPool, twitch_user_id: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM twitch_player_steam_accounts WHERE twitch_user_id = $1")
        .bind(twitch_user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO twitch_player_steam_links (twitch_user_id, lookup_enabled, revision) VALUES ($1, FALSE, 1)
        ON CONFLICT (twitch_user_id) DO UPDATE SET lookup_enabled = FALSE,
        revision = twitch_player_steam_links.revision + 1, updated_at = NOW()")
        .bind(twitch_user_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// An authenticated browser may prepare a link, but only verified OpenID completes it.
pub async fn prepare(pool: &PgPool, twitch_user_id: &str) -> Result<i64, sqlx::Error> {
    sqlx::query(
        "INSERT INTO twitch_player_steam_links (twitch_user_id) VALUES ($1) ON CONFLICT DO NOTHING",
    )
    .bind(twitch_user_id)
    .execute(pool)
    .await?;
    Ok(load(pool, twitch_user_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?
        .revision)
}

/// CAS + nonce consumption in one transaction: replay, concurrent relink and
/// a callback arriving after !unconnect cannot silently overwrite current intent.
/// A newly verified account is retained and becomes the primary account.
pub async fn complete(
    pool: &PgPool,
    twitch_user_id: &str,
    expected_revision: i64,
    steam_id64: i64,
    nonce_hash: &str,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let changed = sqlx::query(
        "UPDATE twitch_player_steam_links SET steam_id64 = $3,
        lookup_enabled = TRUE, revision = revision + 1, updated_at = NOW()
        WHERE twitch_user_id = $1 AND revision = $2",
    )
    .bind(twitch_user_id)
    .bind(expected_revision)
    .bind(steam_id64)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed != 1 {
        return Ok(false);
    }
    let fresh = sqlx::query(
        "INSERT INTO twitch_steam_openid_nonces (nonce_hash, expires_at)
        VALUES ($1, NOW() + INTERVAL '20 minutes') ON CONFLICT DO NOTHING",
    )
    .bind(nonce_hash)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if fresh != 1 {
        return Ok(false);
    }
    sqlx::query(
        "UPDATE twitch_player_steam_accounts SET is_primary = FALSE, updated_at = NOW()
         WHERE twitch_user_id = $1 AND is_primary",
    )
    .bind(twitch_user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO twitch_player_steam_accounts (twitch_user_id, steam_id64, is_primary)
         VALUES ($1, $2, TRUE)
         ON CONFLICT (twitch_user_id, steam_id64) DO UPDATE
         SET is_primary = TRUE, updated_at = NOW()",
    )
    .bind(twitch_user_id)
    .bind(steam_id64)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM twitch_steam_openid_nonces WHERE expires_at < NOW()")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn set_primary(
    pool: &PgPool,
    twitch_user_id: &str,
    steam_id64: i64,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM twitch_player_steam_accounts
            WHERE twitch_user_id = $1 AND steam_id64 = $2
        )",
    )
    .bind(twitch_user_id)
    .bind(steam_id64)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Ok(false);
    }
    sqlx::query(
        "UPDATE twitch_player_steam_accounts SET is_primary = FALSE, updated_at = NOW()
         WHERE twitch_user_id = $1 AND is_primary",
    )
    .bind(twitch_user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE twitch_player_steam_accounts SET is_primary = TRUE, updated_at = NOW()
         WHERE twitch_user_id = $1 AND steam_id64 = $2",
    )
    .bind(twitch_user_id)
    .bind(steam_id64)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE twitch_player_steam_links
         SET steam_id64 = $2, lookup_enabled = TRUE, revision = revision + 1, updated_at = NOW()
         WHERE twitch_user_id = $1",
    )
    .bind(twitch_user_id)
    .bind(steam_id64)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn remove_account(
    pool: &PgPool,
    twitch_user_id: &str,
    steam_id64: i64,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let removed_primary: Option<bool> = sqlx::query_scalar(
        "DELETE FROM twitch_player_steam_accounts
         WHERE twitch_user_id = $1 AND steam_id64 = $2
         RETURNING is_primary",
    )
    .bind(twitch_user_id)
    .bind(steam_id64)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(removed_primary) = removed_primary else {
        return Ok(false);
    };
    if removed_primary {
        let replacement: Option<i64> = sqlx::query_scalar(
            "SELECT steam_id64 FROM twitch_player_steam_accounts
             WHERE twitch_user_id = $1
             ORDER BY linked_at ASC, steam_id64 ASC
             LIMIT 1",
        )
        .bind(twitch_user_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(replacement) = replacement {
            sqlx::query(
                "UPDATE twitch_player_steam_accounts SET is_primary = TRUE, updated_at = NOW()
                 WHERE twitch_user_id = $1 AND steam_id64 = $2",
            )
            .bind(twitch_user_id)
            .bind(replacement)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            "UPDATE twitch_player_steam_links
             SET steam_id64 = $2, revision = revision + 1, updated_at = NOW()
             WHERE twitch_user_id = $1",
        )
        .bind(twitch_user_id)
        .bind(replacement)
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query(
            "UPDATE twitch_player_steam_links
             SET revision = revision + 1, updated_at = NOW()
             WHERE twitch_user_id = $1",
        )
        .bind(twitch_user_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fixture() -> crate::test_postgres::TestPostgres {
        let db = crate::test_postgres::TestPostgres::start().await;
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260918100000_twitch_player_steam_links.sql"
        ))
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260920170000_twitch_player_multi_steam.sql"
        ))
        .execute(&db.pool)
        .await
        .unwrap();
        db
    }

    #[tokio::test]
    async fn player_link_keeps_multiple_accounts_and_switches_primary() {
        let db = fixture().await;
        let rev = prepare(&db.pool, "111").await.unwrap();
        assert!(complete(&db.pool, "111", rev, STEAM64_BASE + 42, "nonce1")
            .await
            .unwrap());
        let rev = prepare(&db.pool, "111").await.unwrap();
        assert!(complete(&db.pool, "111", rev, STEAM64_BASE + 84, "nonce2")
            .await
            .unwrap());

        let linked = accounts(&db.pool, "111").await.unwrap();
        assert_eq!(linked.len(), 2);
        assert_eq!(
            linked.iter().find(|a| a.is_primary).unwrap().steam_id64,
            STEAM64_BASE + 84
        );
        assert!(set_primary(&db.pool, "111", STEAM64_BASE + 42)
            .await
            .unwrap());
        assert_eq!(
            load(&db.pool, "111").await.unwrap().unwrap().account_id(),
            Some(42)
        );
    }

    #[tokio::test]
    async fn removing_primary_promotes_remaining_account_and_disconnect_clears_all() {
        let db = fixture().await;
        let rev = prepare(&db.pool, "111").await.unwrap();
        complete(&db.pool, "111", rev, STEAM64_BASE + 42, "n1")
            .await
            .unwrap();
        let rev = prepare(&db.pool, "111").await.unwrap();
        complete(&db.pool, "111", rev, STEAM64_BASE + 84, "n2")
            .await
            .unwrap();

        assert!(remove_account(&db.pool, "111", STEAM64_BASE + 84)
            .await
            .unwrap());
        assert_eq!(
            load(&db.pool, "111").await.unwrap().unwrap().account_id(),
            Some(42)
        );
        disconnect(&db.pool, "111").await.unwrap();
        assert!(accounts(&db.pool, "111").await.unwrap().is_empty());
        let link = load(&db.pool, "111").await.unwrap().unwrap();
        assert!(!link.lookup_enabled);
        assert_eq!(link.steam_id64, None);
    }

    #[tokio::test]
    async fn player_link_disconnect_invalidates_pending_callbacks_and_allows_fresh_reconnect() {
        let db = fixture().await;
        let rev = prepare(&db.pool, "111").await.unwrap();
        disconnect(&db.pool, "111").await.unwrap();
        assert!(!complete(&db.pool, "111", rev, STEAM64_BASE + 42, "old")
            .await
            .unwrap());
        let rev = prepare(&db.pool, "111").await.unwrap();
        assert!(complete(&db.pool, "111", rev, STEAM64_BASE + 42, "fresh")
            .await
            .unwrap());
        assert!(load(&db.pool, "111").await.unwrap().unwrap().lookup_enabled);
    }

    #[tokio::test]
    async fn player_link_nonce_replay_rolls_back_the_whole_change() {
        let db = fixture().await;
        let a = prepare(&db.pool, "111").await.unwrap();
        let b = prepare(&db.pool, "222").await.unwrap();
        assert!(complete(&db.pool, "111", a, STEAM64_BASE + 42, "same")
            .await
            .unwrap());
        assert!(!complete(&db.pool, "222", b, STEAM64_BASE + 42, "same")
            .await
            .unwrap());
        let second = load(&db.pool, "222").await.unwrap().unwrap();
        assert_eq!(second.steam_id64, None);
        assert_eq!(second.revision, b);
        assert!(accounts(&db.pool, "222").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn player_link_racing_completions_only_one_wins() {
        let db = fixture().await;
        let rev = prepare(&db.pool, "111").await.unwrap();
        let (a, b) = tokio::join!(
            complete(&db.pool, "111", rev, STEAM64_BASE + 42, "a"),
            complete(&db.pool, "111", rev, STEAM64_BASE + 43, "b")
        );
        assert_ne!(a.unwrap(), b.unwrap());
        assert_eq!(accounts(&db.pool, "111").await.unwrap().len(), 1);
    }
}
