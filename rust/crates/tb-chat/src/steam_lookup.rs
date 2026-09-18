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
    last_seen_at: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct CentralTitleContext {
    captured_at: i64,
    party_size: Option<i32>,
    party_discord_ids: Vec<String>,
    voice_discord_ids: Vec<String>,
}

#[derive(Debug, Default)]
pub struct CoStreamContext {
    pub co_streamers: Vec<String>,
    pub party_hint: Option<String>,
}

pub async fn detect_co_streamers_all(
    pool: &PgPool,
    twitch_user_id: &str,
    discord_user_id: Option<i64>,
) -> CoStreamContext {
    let (shared, central) = tokio::join!(
        shared_chat_streamers(twitch_user_id),
        central_title_context(discord_user_id)
    );
    detect_co_streamers_with_shared(pool, twitch_user_id, shared, &central).await
}

async fn detect_co_streamers_with_shared(
    pool: &PgPool,
    twitch_user_id: &str,
    shared: Vec<CoStreamer>,
    central: &CentralTitleContext,
) -> CoStreamContext {
    let central_fresh = chrono::Utc::now()
        .timestamp()
        .checked_sub(central.captured_at)
        .is_some_and(|age| (0..600).contains(&age));
    let party_hint = central
        .party_size
        .filter(|size| central_fresh && (1..=6).contains(size))
        .map(|size| party_size_word(i64::from(size)).to_string());
    let own_live = if shared.is_empty() {
        let stamp = sqlx::query_scalar::<_, Option<String>>(
            "SELECT last_seen_at FROM twitch_live_state WHERE twitch_user_id = $1 AND COALESCE(is_live, 0) = 1"
        ).bind(twitch_user_id).fetch_optional(pool).await.ok().flatten().flatten();
        fresh_live_stamp(stamp.as_deref())
    } else {
        true
    };
    let mut local = Vec::new();
    if central_fresh && own_live {
        let ids = central
            .party_discord_ids
            .iter()
            .chain(&central.voice_discord_ids)
            .cloned()
            .collect::<Vec<_>>();
        if !ids.is_empty() {
            let rows = sqlx::query_as::<_, (String, String, String, Option<String>)>(
                "SELECT tsi.discord_user_id::text, ls.twitch_user_id, ls.streamer_login, ls.last_seen_at \
                   FROM twitch_streamer_identities tsi \
                   JOIN twitch_live_state ls ON ls.twitch_user_id = tsi.twitch_user_id \
                  WHERE tsi.discord_user_id::text = ANY($1) AND COALESCE(ls.is_live, 0) = 1 \
                  ORDER BY ls.twitch_user_id"
            ).bind(&ids).fetch_all(pool).await;
            source_state(4, rows.is_err());
            let rows = rows.unwrap_or_default();
            for members in [&central.party_discord_ids, &central.voice_discord_ids] {
                local.extend(
                    rows.iter()
                        .filter(|row| members.contains(&row.0))
                        .map(|row| CoStreamer {
                            twitch_user_id: row.1.clone(),
                            login: row.2.clone(),
                            last_seen_at: row.3.clone(),
                        }),
                );
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for streamer in shared.into_iter().chain(local) {
        let id = streamer.twitch_user_id.trim();
        let login = streamer
            .login
            .trim()
            .trim_start_matches('@')
            .to_ascii_lowercase();
        if id.is_empty()
            || id == twitch_user_id
            || !crate::title_ai::valid_co_streamer_login(&login)
            || !fresh_live_stamp(streamer.last_seen_at.as_deref())
            || !seen.insert(id.to_string())
        {
            continue;
        }
        out.push(login);
        if out.len() == 2 {
            break;
        }
    }
    CoStreamContext {
        co_streamers: out,
        party_hint,
    }
}

fn fresh_live_stamp(stamp: Option<&str>) -> bool {
    stamp
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .is_some_and(|stamp| {
            let age = chrono::Utc::now().signed_duration_since(stamp);
            age >= chrono::Duration::zero() && age < chrono::Duration::minutes(10)
        })
}

fn source_transition(state: &std::sync::atomic::AtomicU8, source: u8, failed: bool) -> bool {
    use std::sync::atomic::Ordering;
    if failed {
        state.fetch_or(source, Ordering::Relaxed) & source == 0
    } else {
        state.fetch_and(!source, Ordering::Relaxed);
        false
    }
}

fn source_state(source: u8, failed: bool) {
    static FAILURES: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
    if source_transition(&FAILURES, source, failed) {
        tracing::warn!(
            source,
            "title_costream_source_unavailable: Titel nutzt die übrigen verfügbaren Angaben"
        );
    }
}

async fn central_title_context(discord_id: Option<i64>) -> CentralTitleContext {
    let Some(discord_id) = discord_id else {
        return CentralTitleContext::default();
    };
    let token = [
        "TWITCH_INTERNAL_API_TOKEN",
        "STEAM_INTERNAL_API_TOKEN",
        "INTERNAL_API_TOKEN",
    ]
    .iter()
    .find_map(|name| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    });
    let result = match token {
        Some(token) => {
            fetch_central_title_context(
                "http://127.0.0.1:8783/internal/title-context",
                &token,
                discord_id,
            )
            .await
        }
        None => Err(()),
    };
    source_state(2, result.is_err());
    result.unwrap_or_default()
}

async fn fetch_central_title_context(
    url: &str,
    token: &str,
    discord_id: i64,
) -> Result<CentralTitleContext, ()> {
    let url = reqwest::Url::parse(url).map_err(|_| ())?;
    if url.scheme() != "http"
        || !url
            .host_str()
            .and_then(|host| host.parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback())
        || !url.username().is_empty()
        || url.password().is_some()
        || token.trim().is_empty()
    {
        return Err(());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ())?;
    client
        .get(url)
        .header("X-Internal-Token", token)
        .query(&[("discord_id", discord_id)])
        .send()
        .await
        .map_err(|_| ())?
        .error_for_status()
        .map_err(|_| ())?
        .json()
        .await
        .map_err(|_| ())
}

async fn shared_chat_streamers(twitch_user_id: &str) -> Vec<CoStreamer> {
    static HELIX: tokio::sync::OnceCell<tb_transport_twitch::HelixClient> =
        tokio::sync::OnceCell::const_new();
    let helix = HELIX
        .get_or_try_init(|| async {
            let id = std::env::var("TWITCH_CLIENT_ID")
                .or_else(|_| std::env::var("TWITCH_BOT_CLIENT_ID"))
                .unwrap_or_default();
            let secret = std::env::var("TWITCH_CLIENT_SECRET")
                .or_else(|_| std::env::var("TWITCH_BOT_CLIENT_SECRET"))
                .unwrap_or_default();
            if id.trim().is_empty() || secret.trim().is_empty() {
                return Err(());
            }
            tb_transport_twitch::HelixClient::new(tb_transport_twitch::HelixConfig::new(
                id.trim(),
                secret.trim(),
            ))
            .map_err(|_| ())
        })
        .await;
    let result = match helix {
        Ok(helix) => helix
            .get_shared_chat_users(twitch_user_id)
            .await
            .map_err(|_| ()),
        Err(()) => Err(()),
    };
    source_state(1, result.is_err());
    result
        .unwrap_or_default()
        .into_iter()
        .map(|user| CoStreamer {
            twitch_user_id: user.id,
            login: user.login,
            last_seen_at: Some(chrono::Utc::now().to_rfc3339()),
        })
        .collect()
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
mod tests {
    use super::*;
    use crate::test_postgres;

    fn co_streamer(id: &str, login: &str) -> CoStreamer {
        CoStreamer {
            twitch_user_id: id.into(),
            login: login.into(),
            last_seen_at: Some(chrono::Utc::now().to_rfc3339()),
        }
    }

    fn central() -> CentralTitleContext {
        CentralTitleContext {
            captured_at: chrono::Utc::now().timestamp(),
            party_size: Some(2),
            party_discord_ids: vec!["43".into()],
            voice_discord_ids: vec!["43".into(), "44".into(), "45".into()],
        }
    }

    async fn co_stream_pool() -> test_postgres::TestPostgres {
        let db = test_postgres::TestPostgres::start().await;
        sqlx::raw_sql("CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT PRIMARY KEY, discord_user_id TEXT);
            CREATE TABLE twitch_live_state (twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT, is_live INTEGER, last_seen_at TEXT DEFAULT (to_json(now()) #>> '{}'));
            INSERT INTO twitch_streamer_identities VALUES ('100','42'),('200','43'),('300','44'),('400','45');
            INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live) VALUES ('100','myself',1),('200','party_live',1),('300','voice_one',1),('400','voice_two',1);")
            .execute(&db.pool).await.unwrap();
        db
    }

    #[tokio::test]
    async fn review_getrennte_datenbanken_shared_party_voice_und_ids() {
        let db = co_stream_pool().await;
        let result = detect_co_streamers_with_shared(
            &db.pool,
            "100",
            vec![co_streamer("900", "shared")],
            &central(),
        )
        .await;
        assert_eq!(result.co_streamers, ["shared", "party_live"]);
        assert_eq!(result.party_hint.as_deref(), Some("Duo"));
        let result = detect_co_streamers_with_shared(
            &db.pool,
            "100",
            vec![
                co_streamer("100", "self_renamed"),
                co_streamer("200", "Renamed"),
                co_streamer("200", "old_alias"),
            ],
            &central(),
        )
        .await;
        assert_eq!(result.co_streamers, ["renamed", "voice_one"]);
        let result = detect_co_streamers_with_shared(&db.pool, "100", Vec::new(), &central()).await;
        assert_eq!(result.co_streamers, ["party_live", "voice_one"]);
    }

    #[tokio::test]
    async fn review_veraltete_twitch_daten_sind_nicht_live() {
        let db = co_stream_pool().await;
        for stamp in [
            None,
            Some(""),
            Some("kaputt"),
            Some("2020-01-01T00:00:00Z"),
            Some("2099-01-01T00:00:00Z"),
        ] {
            sqlx::query(
                "UPDATE twitch_live_state SET last_seen_at = $1 WHERE twitch_user_id <> '100'",
            )
            .bind(stamp)
            .execute(&db.pool)
            .await
            .unwrap();
            assert!(
                detect_co_streamers_with_shared(&db.pool, "100", Vec::new(), &central())
                    .await
                    .co_streamers
                    .is_empty(),
                "{stamp:?}"
            );
        }
    }

    #[tokio::test]
    async fn review_eigener_offline_kanal_hat_keine_co_streamer() {
        let db = co_stream_pool().await;
        sqlx::query("UPDATE twitch_live_state SET is_live = 0 WHERE twitch_user_id = '100'")
            .execute(&db.pool)
            .await
            .unwrap();
        assert!(
            detect_co_streamers_with_shared(&db.pool, "100", Vec::new(), &central())
                .await
                .co_streamers
                .is_empty()
        );
    }

    #[tokio::test]
    async fn review_quellausfall_und_alte_praesenz_lassen_shared_stehen() {
        let db = co_stream_pool().await;
        for age in [600, -60] {
            let mut context = central();
            context.captured_at -= age;
            let result = detect_co_streamers_with_shared(
                &db.pool,
                "100",
                vec![co_streamer("900", "shared")],
                &context,
            )
            .await;
            assert_eq!(result.co_streamers, ["shared"]);
            assert!(result.party_hint.is_none());
        }
        sqlx::query("DROP TABLE twitch_streamer_identities")
            .execute(&db.pool)
            .await
            .unwrap();
        assert_eq!(
            detect_co_streamers_with_shared(
                &db.pool,
                "100",
                vec![co_streamer("900", "shared")],
                &central()
            )
            .await
            .co_streamers,
            ["shared"]
        );
    }

    #[tokio::test]
    async fn review_fehlende_ids_offline_und_falsche_logins_werden_verworfen() {
        let db = co_stream_pool().await;
        sqlx::raw_sql("DELETE FROM twitch_streamer_identities WHERE twitch_user_id='200'; UPDATE twitch_live_state SET is_live=0 WHERE twitch_user_id='300'; UPDATE twitch_live_state SET streamer_login='fremdä' WHERE twitch_user_id='400';")
            .execute(&db.pool).await.unwrap();
        assert!(
            detect_co_streamers_with_shared(&db.pool, "100", Vec::new(), &central())
                .await
                .co_streamers
                .is_empty()
        );
    }

    #[test]
    fn review_warnung_einmal_je_stoerung_mit_erholung() {
        let state = std::sync::atomic::AtomicU8::new(0);
        assert!(source_transition(&state, 1, true));
        assert!(!source_transition(&state, 1, true));
        assert!(source_transition(&state, 2, true));
        assert!(!source_transition(&state, 1, false));
        assert!(source_transition(&state, 1, true));
        assert!(!source_transition(&state, 2, true));
    }

    #[tokio::test]
    async fn review_bestehender_steam_dienst_auth_und_keine_token_weitergabe() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/internal/title-context"))
            .and(query_param("discord_id", "42"))
            .and(wiremock::matchers::header("x-internal-token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "captured_at":chrono::Utc::now().timestamp(), "party_size":2, "party_discord_ids":["43"], "voice_discord_ids":["44"]
            }))).expect(1).mount(&server).await;
        let url = format!("{}/internal/title-context", server.uri());
        let context = fetch_central_title_context(&url, "test-token", 42)
            .await
            .unwrap();
        assert_eq!(context.party_discord_ids, ["43"]);
        assert_eq!(context.voice_discord_ids, ["44"]);
        assert!(fetch_central_title_context(&url, "", 42).await.is_err());
        assert!(fetch_central_title_context(
            "http://example.invalid/internal/title-context",
            "test-token",
            42
        )
        .await
        .is_err());
        Mock::given(path("/redirect"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", url.as_str()))
            .expect(1)
            .mount(&server)
            .await;
        assert!(fetch_central_title_context(
            &format!("{}/redirect", server.uri()),
            "test-token",
            42
        )
        .await
        .is_err());
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
