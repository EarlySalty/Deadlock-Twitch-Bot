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

/// Erkennt Co-Streamer: verknüpfte, gerade auf Twitch live sitzende Streamer im
/// selben Discord-Voice-Kanal wie der Streamer. Reine ID-Auflösung über die
/// zentrale Postgres (`voice.deadlock_voice_watch`, `core.steam_links`,
/// `twitch_streamer_identities`, `twitch_live_state`), kein Helix-Poll. Ergebnis:
/// bis zu zwei Twitch-Logins.
pub async fn detect_co_streamers(pool: &PgPool, discord_user_id: i64) -> Vec<String> {
    use sqlx::Row;
    let rows = sqlx::query(
        "WITH me AS ( \
             SELECT vw.channel_id, vw.guild_id \
               FROM core.steam_links sl \
               JOIN voice.deadlock_voice_watch vw ON vw.steam_id = sl.steam_id \
              WHERE sl.discord_id = $1 \
                AND vw.channel_id IS NOT NULL \
                AND vw.updated_at > now() - INTERVAL '10 minutes' \
              ORDER BY vw.updated_at DESC \
              LIMIT 1 \
         ) \
         SELECT DISTINCT LOWER(ls.streamer_login) AS login \
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
            AND COALESCE(ls.streamer_login, '') <> '' \
          LIMIT 2",
    )
    .bind(discord_user_id)
    .fetch_all(pool)
    .await;
    match rows {
        Ok(rows) => rows
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>("login").ok())
            .filter(|login| !login.trim().is_empty())
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "Co-Stream-Erkennung fehlgeschlagen; Titel ohne Co-Streamer");
            Vec::new()
        }
    }
}

/// Co-Streamer aus allen Signalen: zuerst die Teilnehmer der Twitch-Shared-Chat-
/// Sitzung (Stream Together, stärkstes Signal, auch ohne Discord-Verknüpfung),
/// danach die verknüpften, live sitzenden Voice-Kanal-Nachbarn. Doppelte raus,
/// Shared-Chat zuerst, höchstens zwei Logins.
pub async fn detect_co_streamers_all(
    pool: &PgPool,
    twitch_user_id: &str,
    discord_user_id: Option<i64>,
) -> Vec<String> {
    let shared = shared_chat_logins(twitch_user_id).await;
    let voice = match discord_user_id {
        Some(id) => detect_co_streamers(pool, id).await,
        None => Vec::new(),
    };
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for login in shared.into_iter().chain(voice) {
        let key = login.trim().trim_start_matches('@').to_lowercase();
        if key.is_empty() || !seen.insert(key.clone()) {
            continue;
        }
        out.push(key);
        if out.len() >= 2 {
            break;
        }
    }
    out
}

/// Ein einzelner App-Token-Aufruf an Helix `GET /shared_chat/session` je "Titel
/// bauen", kein Dauer-Poll. Fehlt die Twitch-App-Konfiguration oder schlägt der
/// Aufruf fehl, bleibt die Liste leer und die Erkennung läuft still mit dem
/// Voice-Signal weiter.
async fn shared_chat_logins(twitch_user_id: &str) -> Vec<String> {
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
            tracing::warn!(%error, "Shared-Chat: Helix-Client nicht baubar; Co-Stream nur aus Voice");
            return Vec::new();
        }
    };
    match helix.get_shared_chat_logins(twitch_user_id).await {
        Ok(logins) => logins,
        Err(error) => {
            tracing::warn!(%error, "Shared-Chat-Abfrage fehlgeschlagen; Co-Stream nur aus Voice");
            Vec::new()
        }
    }
}

/// Größe der aktuellen Deadlock-Party des Streamers als Wort ("solo", "Duo",
/// "Dreier" ...), aus `voice.deadlock_party_members` über die verknüpfte
/// Steam-ID, gleiche Frische-Schranke wie die Voice-Belegung. Keine Namen. `None`,
/// wenn kein frischer Partystand vorliegt.
pub async fn get_party_hint_for_discord_user(
    pool: &PgPool,
    discord_user_id: i64,
) -> Option<String> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT MAX(LEAST(GREATEST(pm.party_size, 1), 6)) AS size \
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
mod tests {
    use super::*;
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
