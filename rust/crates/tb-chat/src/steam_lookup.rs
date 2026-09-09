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
