//! Optionaler Rang-/Live-Kontext für Titelvorschläge in Chat und Dashboard.
//! Nutzt dieselbe ID-basierte Steam-HTTP-Quelle wie die Spielstatistikbefehle.

use crate::stats::{self, StatsError};
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct RankInfo {
    pub rank_display: String,
}

#[derive(Debug, Clone)]
pub struct LiveState {
    pub in_match: bool,
    pub hero: Option<String>,
    pub party_hint: Option<String>,
    pub stage: Option<String>,
}

// Pool-Parameter bleibt für die bestehenden Chat-/Dashboard-Aufrufer kompatibel.
// Steam-Tabellen werden ausdrücklich nicht im Twitch-Pool gesucht.
pub async fn get_rank_for_discord_user(_pool: &PgPool, user_id: i64) -> Option<RankInfo> {
    rank_context(stats::fetch_rank_checked(&user_id.to_string(), false).await)
}

fn rank_context(result: Result<stats::RankInfo, StatsError>) -> Option<RankInfo> {
    let info = match result {
        Ok(info) => info,
        Err(error) => {
            tracing::warn!(%error, "Titel wird ohne optionalen Rang erzeugt");
            return None;
        }
    };
    if !info.linked {
        return None;
    }
    let rank = info.rank_name.filter(|name| !name.trim().is_empty())?;
    let rank_display = match info.subrank {
        Some(subrank @ 1..=6) => format!("{rank} {subrank}"),
        _ => rank,
    };
    Some(RankInfo { rank_display })
}

pub async fn get_live_state_for_discord_user(
    _pool: &PgPool,
    user_id: i64,
) -> Result<Option<LiveState>, StatsError> {
    live_context(stats::fetch_live(&user_id.to_string()).await)
}

#[derive(Debug, sqlx::FromRow)]
struct CoStreamer {
    twitch_user_id: String,
    login: String,
}

async fn party_co_streamers(pool: &PgPool, discord_user_id: i64) -> Vec<CoStreamer> {
    let rows = sqlx::query_as::<_, CoStreamer>(
        "WITH me AS ( \
             SELECT pm.steam_id, pm.party_id \
               FROM core.steam_links sl \
               JOIN voice.deadlock_party_members pm ON pm.steam_id = sl.steam_id \
              WHERE sl.discord_id = $1 \
                AND NULLIF(BTRIM(pm.party_id), '') IS NOT NULL \
                AND pm.seen_at > now() - INTERVAL '10 minutes' \
              ORDER BY pm.seen_at DESC, pm.steam_id, pm.party_id \
              LIMIT 1 \
         ) \
         SELECT DISTINCT ls.twitch_user_id, LOWER(ls.streamer_login) AS login \
           FROM me \
           JOIN voice.deadlock_party_members pm2 \
             ON pm2.party_id = me.party_id \
            AND pm2.steam_id <> me.steam_id \
            AND pm2.seen_at > now() - INTERVAL '10 minutes' \
           JOIN core.steam_links sl2 \
             ON sl2.steam_id = pm2.steam_id AND sl2.discord_id <> $1 \
           JOIN twitch_streamer_identities tsi \
             ON tsi.discord_user_id::text = sl2.discord_id::text \
           JOIN twitch_live_state ls ON ls.twitch_user_id = tsi.twitch_user_id \
          WHERE COALESCE(ls.is_live, 0) = 1 \
            AND COALESCE(BTRIM(ls.streamer_login), '') <> '' \
          ORDER BY ls.twitch_user_id, login",
    )
    .bind(discord_user_id)
    .fetch_all(pool)
    .await;
    match rows {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(%error, "Co-Stream-Erkennung aus Steam-Präsenz fehlgeschlagen; andere Signale bleiben verfügbar");
            Vec::new()
        }
    }
}

async fn voice_co_streamers(pool: &PgPool, discord_user_id: i64) -> Vec<CoStreamer> {
    let rows = sqlx::query_as::<_, CoStreamer>(
        "WITH me AS ( \
             SELECT vw.channel_id, vw.guild_id \
               FROM core.steam_links sl \
               JOIN voice.deadlock_voice_watch vw ON vw.steam_id = sl.steam_id \
              WHERE sl.discord_id = $1 \
                AND vw.channel_id IS NOT NULL \
                AND vw.updated_at > now() - INTERVAL '10 minutes' \
              ORDER BY vw.updated_at DESC, vw.steam_id \
              LIMIT 1 \
         ) \
         SELECT DISTINCT ls.twitch_user_id, LOWER(ls.streamer_login) AS login \
           FROM me \
           JOIN voice.deadlock_voice_watch vw2 \
             ON vw2.channel_id = me.channel_id \
            AND vw2.guild_id = me.guild_id \
            AND vw2.updated_at > now() - INTERVAL '10 minutes' \
           JOIN core.steam_links sl2 \
             ON sl2.steam_id = vw2.steam_id AND sl2.discord_id <> $1 \
           JOIN twitch_streamer_identities tsi \
             ON tsi.discord_user_id::text = sl2.discord_id::text \
           JOIN twitch_live_state ls ON ls.twitch_user_id = tsi.twitch_user_id \
          WHERE COALESCE(ls.is_live, 0) = 1 \
            AND COALESCE(BTRIM(ls.streamer_login), '') <> '' \
          ORDER BY ls.twitch_user_id, login",
    )
    .bind(discord_user_id)
    .fetch_all(pool)
    .await;
    match rows {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(%error, "Co-Stream-Erkennung aus Discord-Voice fehlgeschlagen; andere Signale bleiben verfügbar");
            Vec::new()
        }
    }
}

pub async fn detect_co_streamers_all(
    pool: &PgPool,
    twitch_user_id: &str,
    discord_user_id: Option<i64>,
) -> Vec<String> {
    let shared = shared_chat_streamers(twitch_user_id).await;
    detect_co_streamers_with_shared(pool, twitch_user_id, discord_user_id, shared).await
}

async fn detect_co_streamers_with_shared(
    pool: &PgPool,
    twitch_user_id: &str,
    discord_user_id: Option<i64>,
    shared: Vec<CoStreamer>,
) -> Vec<String> {
    let (party, voice) = match discord_user_id {
        Some(id) => (
            party_co_streamers(pool, id).await,
            voice_co_streamers(pool, id).await,
        ),
        None => (Vec::new(), Vec::new()),
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for streamer in shared.into_iter().chain(party).chain(voice) {
        let id = streamer.twitch_user_id.trim();
        let login = streamer.login.trim().trim_start_matches('@').to_lowercase();
        if id.is_empty() || id == twitch_user_id || login.is_empty() || !seen.insert(id.to_string())
        {
            continue;
        }
        out.push(login);
        if out.len() >= 2 {
            break;
        }
    }
    out
}

async fn shared_chat_streamers(twitch_user_id: &str) -> Vec<CoStreamer> {
    let id = std::env::var("TWITCH_CLIENT_ID").unwrap_or_default();
    let secret = std::env::var("TWITCH_CLIENT_SECRET").unwrap_or_default();
    if id.trim().is_empty() || secret.trim().is_empty() {
        return Vec::new();
    }
    let helix = match tb_transport_twitch::HelixClient::new(tb_transport_twitch::HelixConfig::new(
        id.trim(),
        secret.trim(),
    )) {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(%error, "Shared-Chat: Helix-Client nicht baubar; Co-Stream aus Steam-Präsenz und Discord-Voice");
            return Vec::new();
        }
    };
    match helix.get_shared_chat_users(twitch_user_id).await {
        Ok(users) => users
            .into_iter()
            .map(|user| CoStreamer {
                twitch_user_id: user.id,
                login: user.login,
            })
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "Shared-Chat-Abfrage fehlgeschlagen; Co-Stream aus Steam-Präsenz und Discord-Voice");
            Vec::new()
        }
    }
}

pub async fn get_party_hint_for_discord_user(
    pool: &PgPool,
    discord_user_id: i64,
) -> Option<String> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT MAX(LEAST(GREATEST(pm.party_size, 1), 6))::bigint AS size \
           FROM core.steam_links sl \
           JOIN voice.deadlock_party_members pm ON pm.steam_id = sl.steam_id \
          WHERE sl.discord_id = $1 \
            AND pm.party_size IS NOT NULL \
            AND pm.seen_at > now() - INTERVAL '10 minutes'",
    )
    .bind(discord_user_id)
    .fetch_optional(pool)
    .await;
    let size = match row {
        Ok(Some(row)) => row.try_get::<Option<i64>, _>("size").ok().flatten(),
        Ok(None) => None,
        Err(error) => {
            tracing::warn!(%error, "Party-Größe nicht lesbar; Titel ohne Party-Hinweis");
            None
        }
    }?;
    Some(party_size_word(size).to_string())
}

fn party_size_word(size: i64) -> &'static str {
    match size {
        n if n <= 1 => "solo",
        2 => "Duo",
        3 => "Dreier",
        4 => "Vierer",
        5 => "Fünfer",
        _ => "Sechser",
    }
}

fn live_context(
    result: Result<stats::LiveStatus, StatsError>,
) -> Result<Option<LiveState>, StatsError> {
    let info = result?;
    // Der bestehende Steam-Endpunkt prüft die Aktualität und liefert live=false
    // bei altem Zustand. Alte Hero-Felder allein begründen keinen Live-Kontext.
    if !info.linked || !info.live || !info.in_deadlock {
        return Ok(None);
    }
    Ok(Some(LiveState {
        in_match: true,
        hero: info.hero,
        // Der vorhandene HTTP-Vertrag enthält keinen Partystatus.
        party_hint: None,
        stage: info.stage,
    }))
}

#[cfg(test)]
#[path = "../../../test-support/postgres.rs"]
mod test_postgres;

#[cfg(test)]
mod tests {
    use super::*;

    async fn detect_co_streamers(pool: &PgPool, discord_user_id: i64) -> Vec<String> {
        detect_co_streamers_with_shared(pool, "100", Some(discord_user_id), Vec::new()).await
    }

    fn co_streamer(id: &str, login: &str) -> CoStreamer {
        CoStreamer {
            twitch_user_id: id.to_string(),
            login: login.to_string(),
        }
    }

    #[tokio::test]
    async fn nachtrag3_shared_party_voice_dedupliziert_nach_ids() {
        let db = co_stream_pool().await;
        sqlx::raw_sql("INSERT INTO voice.deadlock_voice_watch VALUES ('own', 1, 7, now()), ('party', 1, 7, now()), ('voice1', 1, 7, now()), ('voice2', 1, 7, now());")
            .execute(&db.pool).await.unwrap();
        assert_eq!(
            detect_co_streamers_with_shared(
                &db.pool,
                "100",
                Some(42),
                vec![co_streamer("900", "shared")]
            )
            .await,
            ["shared", "party_live"]
        );
        assert_eq!(
            detect_co_streamers_with_shared(
                &db.pool,
                "100",
                Some(42),
                vec![
                    co_streamer("100", "other_self_name"),
                    co_streamer("", "invalid"),
                    co_streamer("900", "   "),
                    co_streamer("200", " @Renamed "),
                    co_streamer("200", "old_alias"),
                ]
            )
            .await,
            ["renamed", "voice_one"]
        );
    }

    #[tokio::test]
    async fn nachtrag3_signalfehler_lassen_andere_signale_stehen() {
        let db = co_stream_pool().await;
        sqlx::raw_sql("INSERT INTO voice.deadlock_voice_watch VALUES ('own', 1, 7, now()), ('voice1', 1, 7, now()); DROP TABLE voice.deadlock_party_members;")
            .execute(&db.pool).await.unwrap();
        assert_eq!(
            detect_co_streamers_with_shared(
                &db.pool,
                "100",
                Some(42),
                vec![co_streamer("900", "shared")]
            )
            .await,
            ["shared", "voice_one"]
        );
        assert_eq!(detect_co_streamers(&db.pool, 42).await, ["voice_one"]);
        sqlx::raw_sql("DROP TABLE voice.deadlock_voice_watch;")
            .execute(&db.pool)
            .await
            .unwrap();
        assert_eq!(
            detect_co_streamers_with_shared(
                &db.pool,
                "100",
                Some(42),
                vec![co_streamer("900", "shared")]
            )
            .await,
            ["shared"]
        );
    }

    #[tokio::test]
    async fn nachtrag3_ohne_discord_nur_shared_chat() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/unused")
            .unwrap();
        assert_eq!(
            detect_co_streamers_with_shared(
                &pool,
                "100",
                None,
                vec![
                    co_streamer("900", "shared"),
                    co_streamer("901", "second"),
                    co_streamer("902", "third")
                ]
            )
            .await,
            ["shared", "second"]
        );
    }

    async fn co_stream_pool() -> test_postgres::TestPostgres {
        let db = test_postgres::TestPostgres::start().await;
        sqlx::raw_sql(
            "CREATE SCHEMA core;
             CREATE SCHEMA voice;
             CREATE TABLE core.steam_links (
                 discord_id BIGINT NOT NULL, steam_id TEXT NOT NULL,
                 PRIMARY KEY (discord_id, steam_id)
             );
             CREATE TABLE voice.deadlock_party_members (
                 party_id TEXT NOT NULL, steam_id TEXT NOT NULL,
                 party_size INTEGER, seen_at TIMESTAMPTZ NOT NULL,
                 PRIMARY KEY (party_id, steam_id)
             );
             CREATE TABLE voice.deadlock_voice_watch (
                 steam_id TEXT PRIMARY KEY, guild_id BIGINT NOT NULL,
                 channel_id BIGINT, updated_at TIMESTAMPTZ NOT NULL
             );
             CREATE TABLE twitch_streamer_identities (
                 twitch_user_id TEXT PRIMARY KEY, discord_user_id TEXT
             );
             CREATE TABLE twitch_live_state (
                 twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT, is_live BIGINT
             );
             INSERT INTO core.steam_links VALUES
                 (42, 'own'), (43, 'party'), (44, 'voice1'), (45, 'voice2');
             INSERT INTO twitch_streamer_identities VALUES
                 ('100', '42'), ('200', '43'), ('300', '44'), ('400', '45');
             INSERT INTO twitch_live_state VALUES
                 ('100', 'myself', 1), ('200', 'Party_Live', 1),
                 ('300', 'voice_one', 1), ('400', 'voice_two', 1);
             INSERT INTO voice.deadlock_party_members VALUES
                 ('group', 'own', 2, now()), ('group', 'party', 2, now());",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        db
    }

    #[tokio::test]
    async fn nachtrag3_party_erkennung_prueft_frische_ids_und_live_status() {
        let db = co_stream_pool().await;
        assert_eq!(detect_co_streamers(&db.pool, 42).await, ["party_live"]);
        assert!(detect_co_streamers(&db.pool, 999).await.is_empty());

        for steam_id in ["own", "party"] {
            sqlx::query("UPDATE voice.deadlock_party_members SET seen_at = now() - INTERVAL '10 minutes' WHERE steam_id = $1")
                .bind(steam_id).execute(&db.pool).await.unwrap();
            assert!(
                detect_co_streamers(&db.pool, 42).await.is_empty(),
                "{steam_id}"
            );
            sqlx::query("UPDATE voice.deadlock_party_members SET seen_at = now() - INTERVAL '9 minutes' WHERE steam_id = $1")
                .bind(steam_id).execute(&db.pool).await.unwrap();
            assert_eq!(detect_co_streamers(&db.pool, 42).await, ["party_live"]);
        }
        for party_id in ["", "   "] {
            sqlx::query("UPDATE voice.deadlock_party_members SET party_id = $1")
                .bind(party_id)
                .execute(&db.pool)
                .await
                .unwrap();
            assert!(detect_co_streamers(&db.pool, 42).await.is_empty());
        }
        sqlx::raw_sql("UPDATE voice.deadlock_party_members SET party_id = steam_id;")
            .execute(&db.pool)
            .await
            .unwrap();
        assert!(detect_co_streamers(&db.pool, 42).await.is_empty());
        sqlx::raw_sql("UPDATE voice.deadlock_party_members SET party_id = 'group'; UPDATE twitch_live_state SET is_live = 0 WHERE twitch_user_id = '200';")
            .execute(&db.pool).await.unwrap();
        assert!(detect_co_streamers(&db.pool, 42).await.is_empty());
        sqlx::raw_sql("UPDATE twitch_live_state SET is_live = 1; DELETE FROM twitch_streamer_identities WHERE twitch_user_id = '200';")
            .execute(&db.pool).await.unwrap();
        assert!(detect_co_streamers(&db.pool, 42).await.is_empty());
    }

    #[tokio::test]
    async fn nachtrag3_party_hat_vorrang_vor_voice() {
        let db = co_stream_pool().await;
        sqlx::raw_sql("INSERT INTO voice.deadlock_voice_watch VALUES ('own', 1, 7, now()), ('voice1', 1, 7, now()), ('voice2', 1, 7, now());")
            .execute(&db.pool).await.unwrap();
        assert_eq!(
            detect_co_streamers(&db.pool, 42).await,
            ["party_live", "voice_one"]
        );
    }

    #[tokio::test]
    async fn nachtrag3_party_hinweis_liefert_steam_praesenz() {
        let db = co_stream_pool().await;
        assert_eq!(
            get_party_hint_for_discord_user(&db.pool, 42)
                .await
                .as_deref(),
            Some("Duo")
        );
        sqlx::raw_sql(
            "UPDATE voice.deadlock_party_members SET seen_at = now() - INTERVAL '10 minutes';",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        assert!(get_party_hint_for_discord_user(&db.pool, 42)
            .await
            .is_none());
    }
    use wiremock::{
        matchers::{method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn titel_kontext_aus_bestehender_http_quelle_ohne_fremde_db_tabellen() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rank"))
            .and(query_param("discord_id", "42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"linked":true,"rank_name":"Emissary","subrank":6}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let rank = rank_context(
            stats::fetch_rank_at(&format!("{}/rank", server.uri()), "42", false).await,
        )
        .unwrap();
        assert_eq!(rank.rank_display, "Emissary 6");
        Mock::given(method("GET"))
            .and(path("/player-live"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"linked":true,"live":true,"in_deadlock":true,"hero":"Haze"}),
            ))
            .mount(&server)
            .await;
        let live = live_context(
            stats::fetch_live_at(&format!("{}/player-live", server.uri()), "42").await,
        )
        .unwrap()
        .unwrap();
        assert_eq!(live.hero.as_deref(), Some("Haze"));
        assert!(live.party_hint.is_none());
    }

    #[test]
    fn titel_ohne_rang_bleibt_moeglich_und_alter_hero_ist_kein_live_kontext() {
        assert!(rank_context(Err(StatsError)).is_none());
        let prompt = crate::title_ai::build_title_prompt("ranked grind", &[], &[], None, 0.0, None);
        assert!(prompt.contains("ranked grind"));
        let old: stats::LiveStatus = serde_json::from_value(
            serde_json::json!({"linked":true,"live":false,"in_deadlock":true,"hero":"Haze"}),
        )
        .unwrap();
        assert!(live_context(Ok(old)).unwrap().is_none());
        assert!(live_context(Err(StatsError)).is_err());
    }
}
