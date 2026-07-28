//! Titelgenerator, Insights und Twitch-Kanaltitel-Update.

use std::sync::{Arc, OnceLock};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tb_chat::steam_lookup;
use tb_chat::title_ai::{
    generate_title, GenerateTitleError, PromptHistoryItem, PromptKnowledgeItem, PromptLiveState,
    TitleRateLimiter,
};
use tb_chat::title_db;
use tb_crypto::FieldCipher;
use tb_raid::RaidAuthStore;

use crate::auth::level::DashboardAuthLevel;

static TITLE_RATE_LIMITER: OnceLock<TitleRateLimiter> = OnceLock::new();

#[derive(Deserialize)]
pub struct TitleSuggestBody {
    #[serde(default)]
    pub keywords: Option<String>,
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub include_live: bool,
}

#[derive(Deserialize, Default)]
pub struct TitleQuery {
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateTitleBody {
    pub title: String,
}

fn requested_login(
    auth: &DashboardAuthLevel,
    requested: Option<&str>,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let requested = requested.unwrap_or("").trim().to_lowercase();
    match auth {
        DashboardAuthLevel::None => Err(crate::auth::unauthorized_v2_json()),
        DashboardAuthLevel::Partner { twitch_login, .. } => {
            let own = twitch_login.trim().to_lowercase();
            if !requested.is_empty() && requested != own {
                Err((
                    StatusCode::FORBIDDEN,
                    Json(
                        json!({"error":"Du kannst nur auf deinen eigenen Twitch-Account zugreifen."}),
                    ),
                ))
            } else {
                Ok(own)
            }
        }
        DashboardAuthLevel::Admin { actor } => {
            let actor_login = actor
                .as_ref()
                .map(|a| a.twitch_login.trim().to_lowercase())
                .unwrap_or_default();
            let login = if requested.is_empty() {
                actor_login
            } else {
                requested
            };
            if login.is_empty() {
                Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"streamer required"})),
                ))
            } else {
                Ok(login)
            }
        }
    }
}

async fn resolve_user_id(pool: &PgPool, login: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT twitch_user_id AS \"twitch_user_id!\" FROM twitch_streamers \
         WHERE LOWER(twitch_login) = $1 AND COALESCE(twitch_user_id, '') <> '' LIMIT 1",
        login
    )
    .fetch_optional(pool)
    .await
}

/// Aufgelöster Deadlock-Kontext für den Titel-Prompt.
#[derive(Default)]
struct TitleContext {
    rank_display: Option<String>,
    live_state: Option<PromptLiveState>,
    /// `true`, wenn `include_live` gesetzt war UND ein Live-State wirklich
    /// geladen wurde — nur dann floss Live-Kontext in den Prompt (P2.102).
    live_context_used: bool,
}

/// Löst die Discord-ID des Streamers auf (für die Steam-Lookup-DB).
async fn resolve_discord_user_id(pool: &PgPool, twitch_user_id: &str) -> Option<i64> {
    let row = sqlx::query_scalar!(
        "SELECT discord_user_id::text AS \"discord_user_id?\" \
         FROM twitch_streamer_identities \
         WHERE twitch_user_id = $1 \
         LIMIT 1",
        twitch_user_id
    )
    .fetch_optional(pool)
    .await;
    match row {
        Ok(row) => row.flatten().and_then(|s| s.trim().parse::<i64>().ok()),
        Err(error) => {
            tracing::warn!(
                %error,
                twitch_user_id = %twitch_user_id.chars().take(16).collect::<String>(),
                "Title-Kontext: Discord-ID-Abfrage fehlgeschlagen; der Titel wird ohne Rang und Live-Daten erzeugt"
            );
            None
        }
    }
}

/// Holt Rang (+ optional Live-State) für den Streamer — Parität zu
/// `tb-chat/commands.rs` (P2.101/P2.102). Rang bleibt synchron auf SQLite;
/// Live-State kommt async aus Central Postgres.
async fn resolve_title_context(
    pool: &PgPool,
    twitch_user_id: &str,
    include_live: bool,
) -> TitleContext {
    let Some(discord_id) = resolve_discord_user_id(pool, twitch_user_id).await else {
        return TitleContext::default();
    };

    let db_path = steam_lookup::steam_db_path();
    let rank_path = db_path.clone();
    let rank_display = match tokio::task::spawn_blocking(move || {
        steam_lookup::get_rank_for_discord_user(&rank_path, discord_id)
    })
    .await
    {
        Ok(rank) => rank.map(|r| r.rank_display),
        Err(error) => {
            tracing::warn!(
                %error,
                discord_id_tail = discord_id.rem_euclid(10_000),
                "Title-Kontext: Steam-Rank-Task fehlgeschlagen; der Titel wird ohne Rang erzeugt"
            );
            None
        }
    };

    let mut live_state = None;
    let mut live_context_used = false;
    if include_live {
        live_state = match steam_lookup::get_live_state_for_discord_user(pool, discord_id).await {
            Ok(live) => live.map(|l| PromptLiveState {
                hero: l.hero,
                party_hint: l.party_hint,
            }),
            Err(error) => {
                tracing::warn!(
                    %error,
                    discord_id_tail = discord_id.rem_euclid(10_000),
                    "Title-Kontext: Steam-Live-Abfrage fehlgeschlagen; der Titel wird ohne Live-Daten erzeugt"
                );
                None
            }
        };
        // Live-Kontext wurde nur dann genutzt, wenn auch ein State vorlag.
        live_context_used = live_state.is_some();
    }

    TitleContext {
        rank_display,
        live_state,
        live_context_used,
    }
}

pub async fn suggest_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<TitleQuery>,
    body: String,
) -> impl IntoResponse {
    let body: TitleSuggestBody = match serde_json::from_str(&body) {
        Ok(body) => body,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"invalid json"})),
            )
                .into_response()
        }
    };
    let keywords = body.keywords.as_deref().unwrap_or("").trim();
    if keywords.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"keywords required"})),
        )
            .into_response();
    }
    let requested_streamer = body
        .streamer
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            query
                .streamer
                .as_deref()
                .filter(|value| !value.trim().is_empty())
        });
    let login = match requested_login(&auth, requested_streamer) {
        Ok(login) => login,
        Err(resp) => return resp.into_response(),
    };
    let user_id = match resolve_user_id(&pool, &login).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error":"streamer not found"})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("title user-id lookup fehlgeschlagen: {e}");
            return crate::auth::analytics_request_failed_json().into_response();
        }
    };

    let history = title_db::get_streamer_title_history(&pool, &user_id, 30).await;
    let own_avg = title_db::get_streamer_avg_viewers(&pool, &user_id).await;
    let analysis: Vec<serde_json::Value> = history
        .iter()
        .map(|item| {
            let avg = item.avg_viewers.unwrap_or(0.0);
            let followers = item.followers_start.unwrap_or(1).max(1) as f64;
            json!({
                "title": item.title,
                "avg_viewers": avg,
                "peak_viewers": item.peak_viewers.unwrap_or(0),
                "followers_start": item.followers_start,
                "started_at": item.started_at.map(|ts| ts.to_rfc3339()),
                "relative_perf": if own_avg > 0.0 { avg / own_avg } else { 0.0 },
                "engagement_rate": avg / followers,
            })
        })
        .collect();
    let prompt_history: Vec<PromptHistoryItem> = analysis
        .iter()
        .map(|item| PromptHistoryItem {
            title: item["title"].as_str().unwrap_or_default().to_string(),
            relative_perf: item["relative_perf"].as_f64(),
            engagement_rate: item["engagement_rate"].as_f64(),
        })
        .collect();
    let knowledge = title_db::get_top_knowledge_titles(&pool, 30).await;
    let prompt_knowledge: Vec<PromptKnowledgeItem> = knowledge
        .into_iter()
        .map(|item| PromptKnowledgeItem {
            title: item.title,
            normalized_score: item.normalized_score,
        })
        .collect();

    // P2.101/P2.102: Deadlock-Rang + (optional) Live-State auflösen und an den
    // Generator durchreichen — wie der !title-Chat-Command.
    let context = resolve_title_context(&pool, &user_id, body.include_live).await;

    let limiter = TITLE_RATE_LIMITER.get_or_init(TitleRateLimiter::default);
    match generate_title(
        limiter,
        &user_id,
        keywords,
        &prompt_history,
        &prompt_knowledge,
        context.rank_display.as_deref(),
        context.live_state.as_ref(),
        "dashboard",
    )
    .await
    {
        Ok(result) => Json(json!({
            "primary": result.primary,
            "alternatives": result.alternatives,
            "title_analysis": analysis.into_iter().take(20).collect::<Vec<_>>(),
            // P2.102: nur true, wenn Live-Kontext tatsächlich angewandt wurde.
            "live_context_used": context.live_context_used,
        }))
        .into_response(),
        Err(GenerateTitleError::RateLimit(rate)) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error":"rate_limit","retry_after":rate.retry_after})),
        )
            .into_response(),
        Err(GenerateTitleError::NoApiKey) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"title_ai_unavailable"})),
        )
            .into_response(),
        Err(GenerateTitleError::Http(e)) => {
            tracing::error!("title generation fehlgeschlagen: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":"title_generation_failed"})),
            )
                .into_response()
        }
    }
}

pub async fn insights_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<TitleQuery>,
) -> impl IntoResponse {
    let login = match requested_login(&auth, query.streamer.as_deref()) {
        Ok(login) => login,
        Err(resp) => return resp.into_response(),
    };
    let Some(user_id) = resolve_user_id(&pool, &login).await.ok().flatten() else {
        return Json(json!({"insight": null})).into_response();
    };
    match title_db::get_latest_insight(&pool, &user_id).await {
        Ok(insight) => Json(json!({"insight": insight})).into_response(),
        Err(e) => {
            tracing::error!("title insight lookup fehlgeschlagen: {e}");
            crate::auth::analytics_request_failed_json().into_response()
        }
    }
}

pub async fn update_channel_title_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<TitleQuery>,
    Json(body): Json<UpdateTitleBody>,
) -> impl IntoResponse {
    let title = body.title.trim();
    if title.is_empty() || title.chars().count() > 140 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"title must contain 1 to 140 characters"})),
        )
            .into_response();
    }
    let login = match requested_login(&auth, query.streamer.as_deref()) {
        Ok(login) => login,
        Err(resp) => return resp.into_response(),
    };
    let user_id = match resolve_user_id(&pool, &login).await {
        Ok(Some(id)) => id,
        _ => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error":"streamer not found"})),
            )
                .into_response()
        }
    };

    let Ok(cipher) = FieldCipher::from_env() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"token_store_unavailable"})),
        )
            .into_response();
    };
    let store = RaidAuthStore::new(pool.clone(), Arc::new(cipher));
    let scopes = store.get_scopes(&user_id).await.unwrap_or_default();
    if !scopes
        .iter()
        .any(|scope| scope == "channel:manage:broadcast")
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"scope_missing","required_scope":"channel:manage:broadcast"})),
        )
            .into_response();
    }
    let token = match store.load_decrypted_unrestricted(&user_id).await {
        Ok(Some(tokens)) if !tokens.needs_reauth => tokens.access_token,
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error":"reauth_required"})),
            )
                .into_response()
        }
    };
    let client_id = std::env::var("TWITCH_CLIENT_ID")
        .or_else(|_| std::env::var("TWITCH_BOT_CLIENT_ID"))
        .unwrap_or_default();
    if client_id.trim().is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"twitch_client_unavailable"})),
        )
            .into_response();
    }
    let base = std::env::var("TWITCH_HELIX_BASE_URL")
        .unwrap_or_else(|_| "https://api.twitch.tv/helix".to_string());
    let response = reqwest::Client::new()
        .patch(format!("{}/channels", base.trim_end_matches('/')))
        .query(&[("broadcaster_id", user_id.as_str())])
        .header("Client-Id", client_id)
        .bearer_auth(token)
        .json(&json!({"title": title}))
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            Json(json!({"ok":true,"title":title})).into_response()
        }
        Ok(resp) if resp.status() == reqwest::StatusCode::UNAUTHORIZED => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"reauth_required"})),
        )
            .into_response(),
        Ok(resp) if resp.status() == reqwest::StatusCode::FORBIDDEN => (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"scope_missing","required_scope":"channel:manage:broadcast"})),
        )
            .into_response(),
        Ok(resp) => {
            tracing::error!(status = %resp.status(), "Twitch-Titelupdate abgelehnt");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":"twitch_update_failed"})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Twitch-Titelupdate fehlgeschlagen: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":"twitch_unavailable"})),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partner() -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: "nani".into(),
            twitch_user_id: "1".into(),
            display_name: "Nani".into(),
        }
    }

    #[test]
    fn partner_darf_nur_eigenen_login_nutzen() {
        assert_eq!(requested_login(&partner(), None).unwrap(), "nani");
        assert!(requested_login(&partner(), Some("other")).is_err());
    }

    #[test]
    fn admin_braucht_auswahl_oder_actor() {
        assert!(requested_login(&DashboardAuthLevel::admin(), None).is_err());
        assert_eq!(
            requested_login(&DashboardAuthLevel::admin(), Some("Nani")).unwrap(),
            "nani"
        );
    }

    // ── DB-gestützte Kontext-Auflösung (P2.101/P2.102) ──────────────────────
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_streamer_identities \
             (twitch_user_id TEXT, discord_user_id TEXT)",
        )
        .execute(&pool)
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
                discord_id BIGINT NOT NULL REFERENCES core.users(discord_id) ON DELETE CASCADE,
                steam_id64 BIGINT NOT NULL,
                verified BOOLEAN NOT NULL DEFAULT false,
                primary_account BOOLEAN NOT NULL DEFAULT false,
                linked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                PRIMARY KEY (discord_id, steam_id64)
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

    /// P2.101: Ohne Discord-Verknüpfung bleibt der Kontext leer (kein Rang/Live),
    /// und der Titel kann trotzdem erzeugt werden.
    #[tokio::test]
    async fn kontext_leer_ohne_discord_link() {
        let Some(pool) = make_pool("t_title_nolink").await else {
            return;
        };
        // Kein twitch_streamer_identities-Eintrag → discord_user_id nicht auflösbar.
        let ctx = resolve_title_context(&pool, "999", true).await;
        assert!(ctx.rank_display.is_none());
        assert!(ctx.live_state.is_none());
        assert!(
            !ctx.live_context_used,
            "ohne Live-State darf live_context_used nicht true sein"
        );
    }

    /// P2.102: `include_live=true` allein macht `live_context_used` NICHT true,
    /// wenn kein Live-State geladen werden kann. Der Flag-Echo-Bug
    /// (Rust gab vorher body.include_live unverändert zurück) ist damit weg.
    #[tokio::test]
    async fn include_live_ohne_live_state_kein_kontext() {
        let Some(pool) = make_pool("t_title_live_noop").await else {
            return;
        };
        // Discord-Link vorhanden, aber kein Live-State → Live-Kontext bleibt leer.
        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, discord_user_id) \
             VALUES ('1', '123456789')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let prev = std::env::var("STEAM_BOT_DB_PATH").ok();
        std::env::set_var(
            "STEAM_BOT_DB_PATH",
            "/tmp/tb_nonexistent_steam_db_for_title_test.sqlite3",
        );

        let ctx = resolve_title_context(&pool, "1", true).await;

        match prev {
            Some(v) => std::env::set_var("STEAM_BOT_DB_PATH", v),
            None => std::env::remove_var("STEAM_BOT_DB_PATH"),
        }

        assert!(ctx.rank_display.is_none());
        assert!(ctx.live_state.is_none());
        assert!(
            !ctx.live_context_used,
            "include_live=true ohne geladenen State => live_context_used=false"
        );
    }

    #[tokio::test]
    async fn live_kontext_kommt_aus_postgres() {
        let Some(pool) = make_pool("t_title_live_postgres").await else {
            return;
        };
        let discord_id = 9_223_372_036_854_775_000_i64;
        let steam_id64 = 76_561_197_960_265_733_i64;
        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, discord_user_id)
             VALUES ('pg-live-user', $1)",
        )
        .bind(discord_id.to_string())
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
            "INSERT INTO core.steam_links (discord_id, steam_id64, verified)
             VALUES ($1, $2, true)
             ON CONFLICT (discord_id, steam_id64) DO UPDATE SET verified = true",
        )
        .bind(discord_id)
        .bind(steam_id64)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO activity.live_player_state (
                steam_id, in_deadlock_now, in_match_now_strict,
                deadlock_stage, deadlock_hero, deadlock_party_hint
             )
             VALUES ($1, true, true, 'laning', 'Haze', 'solo')
             ON CONFLICT (steam_id) DO UPDATE SET
                in_deadlock_now = EXCLUDED.in_deadlock_now,
                in_match_now_strict = EXCLUDED.in_match_now_strict,
                deadlock_stage = EXCLUDED.deadlock_stage,
                deadlock_hero = EXCLUDED.deadlock_hero,
                deadlock_party_hint = EXCLUDED.deadlock_party_hint",
        )
        .bind(steam_id64.to_string())
        .execute(&pool)
        .await
        .unwrap();

        let ctx = resolve_title_context(&pool, "pg-live-user", true).await;

        assert_eq!(
            ctx.live_state
                .as_ref()
                .and_then(|state| state.hero.as_deref()),
            Some("Haze")
        );
        assert!(ctx.live_context_used);
    }
}
