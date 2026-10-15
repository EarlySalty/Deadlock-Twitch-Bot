use super::*;
use crate::auth::level::DashboardAuthLevel;
use axum::{body::Body, extract::FromRequestParts, http::Request};
use tower::ServiceExt;

pub(crate) async fn fixture() -> crate::test_database::Database {
    let db = crate::test_database::Database::new().await;
    sqlx::raw_sql(r#"
        CREATE TABLE twitch_partners (
            id bigserial PRIMARY KEY, twitch_user_id text NOT NULL, twitch_login text NOT NULL,
            require_discord_link integer DEFAULT 0, next_link_check_at text,
            manual_partner_opt_out integer DEFAULT 0, partnered_at text DEFAULT CURRENT_TIMESTAMP,
            admin_archived_at text, status text NOT NULL DEFAULT 'active', departnered_at text,
            raid_bot_enabled integer DEFAULT 0, silent_ban integer DEFAULT 0, silent_raid integer DEFAULT 0,
            live_ping_role_id bigint, live_ping_enabled integer DEFAULT 1, technical_pause_reason text,
            inactivity_flagged_at text, added_by text, manual_verified_permanent integer DEFAULT 0,
            manual_verified_until text, manual_verified_at text
        );
        CREATE TABLE twitch_streamer_identities (twitch_user_id text PRIMARY KEY, twitch_login text NOT NULL,
            discord_user_id text, discord_display_name text, is_on_discord integer DEFAULT 0);
        CREATE TABLE twitch_exclusions (twitch_user_id text PRIMARY KEY, reactivated_at timestamptz);
        CREATE TABLE twitch_partner_signup_denylist (twitch_user_id text, twitch_login text);
        CREATE TABLE twitch_channel_updates (twitch_user_id text, language text, recorded_at timestamptz);
        CREATE TABLE twitch_stream_sessions (twitch_user_id text, language text, started_at timestamptz);
        INSERT INTO twitch_streamer_identities (twitch_user_id,twitch_login) VALUES ('42','existing');
        INSERT INTO twitch_partners (twitch_user_id,twitch_login) VALUES ('42','existing');
    "#).execute(&db.pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20260623150000_drop_manual_verified_columns.sql"
    ))
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261015100000_guild_membership_onboarding.sql"
    ))
    .execute(&db.pool)
    .await
    .unwrap();
    db
}

fn headers(raw: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::HOST, HeaderValue::from_static("dead-lock.de"));
    headers.insert(
        header::ORIGIN,
        HeaderValue::from_static("https://dead-lock.de"),
    );
    headers.insert(header::COOKIE, format!("{COOKIE}={raw}").parse().unwrap());
    headers
}

async fn new_session(pool: &PgPool, raw: &str, user: &str, guilds: &[i64]) {
    sqlx::query("INSERT INTO guild_onboarding_sessions (session_hash, twitch_user_id, twitch_login, discord_user_id, guild_ids, discord_verified_at) VALUES ($1,$2,$3,'987',$4,now())")
        .bind(session_lookup_key(raw)).bind(user).bind(format!("user{user}")).bind(guilds).execute(pool).await.unwrap();
}

fn auth(pool: PgPool) -> DashboardAuthState {
    use base64::Engine;
    DashboardAuthState::new(
        pool,
        base64::engine::general_purpose::URL_SAFE.encode([0u8; 32]),
    )
}

#[tokio::test]
async fn migration_preserves_home_parity_and_cutover_requires_matching_shadow() {
    let db = fixture().await;
    let summary: (i64, i64, i64, i64) =
        sqlx::query_as("SELECT * FROM twitch_partner_shadow_summary()")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(summary, (1, 1, 1, 0));
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT enabled FROM guild_runtime")
            .fetch_one(&db.pool)
            .await
            .unwrap()
    );
    sqlx::query(
        "UPDATE streamer_guilds SET active=false,deactivated_at=now() WHERE twitch_user_id='42'",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    assert!(sqlx::query("SELECT enable_guild_runtime()")
        .execute(&db.pool)
        .await
        .is_err());
    sqlx::query(
        "UPDATE streamer_guilds SET active=true,deactivated_at=NULL WHERE twitch_user_id='42'",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query("SELECT enable_guild_runtime()")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE streamer_guilds SET active=false,deactivated_at=now() WHERE twitch_user_id='42'",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i32>(
            "SELECT is_partner_active FROM twitch_partners_all_state WHERE twitch_user_id='42'"
        )
        .fetch_one(&db.pool)
        .await
        .unwrap(),
        0
    );
    db.close().await;
}

#[tokio::test]
async fn per_guild_discord_requirement_and_language_observations_stay_current() {
    let db = fixture().await;
    sqlx::query("UPDATE twitch_partners SET require_discord_link=1 WHERE twitch_user_id='42'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM partner_guild_active")
            .fetch_one(&db.pool)
            .await
            .unwrap(),
        0
    );
    sqlx::query("UPDATE twitch_streamer_identities SET discord_user_id='987',is_on_discord=1 WHERE twitch_user_id='42'").execute(&db.pool).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM partner_guild_active")
            .fetch_one(&db.pool)
            .await
            .unwrap(),
        1
    );
    sqlx::query("INSERT INTO twitch_channel_updates VALUES ('42','EN',now()),('42','de',now()-interval '1 day'),('42','und',now()+interval '1 day')").execute(&db.pool).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT broadcaster_language FROM twitch_streamer_identities WHERE twitch_user_id='42'"
        )
        .fetch_one(&db.pool)
        .await
        .unwrap(),
        "en"
    );
    db.close().await;
}

#[tokio::test]
async fn relinking_discord_invalidates_existing_guild_proofs() {
    let db = fixture().await;
    sqlx::query("UPDATE twitch_streamer_identities SET discord_user_id='987',is_on_discord=1 WHERE twitch_user_id='42'").execute(&db.pool).await.unwrap();
    sqlx::query("INSERT INTO discord_guilds(guild_id,status) VALUES(700,'active')")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO streamer_guilds(twitch_user_id,guild_id,active,is_on_discord,require_discord_link,deactivated_at) VALUES('42',700,true,true,true,NULL)").execute(&db.pool).await.unwrap();
    sqlx::query(
        "UPDATE twitch_streamer_identities SET discord_user_id='988' WHERE twitch_user_id='42'",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    let membership: (bool,bool) = sqlx::query_as("SELECT active,is_on_discord FROM streamer_guilds WHERE twitch_user_id='42' AND guild_id=700").fetch_one(&db.pool).await.unwrap();
    assert_eq!(membership, (false, false));
    assert!(!sqlx::query_scalar::<_,bool>("SELECT is_on_discord FROM streamer_guilds WHERE twitch_user_id='42' AND guild_id=1289721245281292288").fetch_one(&db.pool).await.unwrap());
    db.close().await;
}

#[tokio::test]
async fn active_home_membership_with_invalidated_proof_can_be_verified_again() {
    let db = fixture().await;
    sqlx::raw_sql("UPDATE twitch_streamer_identities SET discord_user_id='987',is_on_discord=1 WHERE twitch_user_id='42';
        UPDATE streamer_guilds SET require_discord_link=true WHERE twitch_user_id='42';
        UPDATE twitch_streamer_identities SET discord_user_id='988' WHERE twitch_user_id='42';").execute(&db.pool).await.unwrap();
    let raw = "synthetic-home-reverification-session-000001";
    new_session(&db.pool, raw, "42", &[HOME]).await;
    sqlx::query("UPDATE guild_onboarding_sessions SET discord_user_id='988'")
        .execute(&db.pool)
        .await
        .unwrap();
    let response = status(Extension(auth(db.pool.clone())), headers(raw)).await;
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["memberships"][0]["active"], false);
    assert_eq!(body["memberships"][0]["can_rejoin"], true);
    assert_eq!(
        assign(&db.pool, &session_lookup_key(raw), HOME)
            .await
            .unwrap(),
        Some(true)
    );
    assert!(sqlx::query_scalar::<_, bool>(
        "SELECT is_on_discord FROM streamer_guilds WHERE twitch_user_id='42'"
    )
    .fetch_one(&db.pool)
    .await
    .unwrap());
    db.close().await;
}

#[tokio::test]
async fn membership_selection_enforces_proof_runtime_approval_and_admin_decisions() {
    let db = fixture().await;
    let raw = "synthetic-onboarding-session-0000000001";
    let hash = session_lookup_key(raw);
    sqlx::query("INSERT INTO discord_guilds(guild_id,name,status,auto_partner_default) VALUES (700,'Test server','active',true),(701,'Approval server','active',false)").execute(&db.pool).await.unwrap();
    new_session(&db.pool, raw, "100", &[700, 701]).await;
    assert_eq!(assign(&db.pool, &hash, 700).await.unwrap(), None);
    sqlx::query("SELECT enable_guild_runtime()")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(assign(&db.pool, &hash, 702).await.unwrap(), None);
    assert_eq!(assign(&db.pool, &hash, 701).await.unwrap(), Some(false));
    assert_eq!(assign(&db.pool, &hash, 700).await.unwrap(), Some(true));
    let guilds: Vec<i64> = sqlx::query_scalar(
        "SELECT guild_id FROM streamer_guilds WHERE twitch_user_id='100' ORDER BY guild_id",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(guilds, vec![700, 701]);
    sqlx::query("UPDATE streamer_guilds SET active=false,deactivated_by='admin',deactivated_at=now() WHERE twitch_user_id='100' AND guild_id=700").execute(&db.pool).await.unwrap();
    assert_eq!(assign(&db.pool, &hash, 700).await.unwrap(), None);
    sqlx::query(
        "UPDATE guild_onboarding_sessions SET discord_verified_at=now()-interval '3 minutes'",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(assign(&db.pool, &hash, 700).await.unwrap(), None);
    db.close().await;
}

#[tokio::test]
async fn session_uses_only_hash_expires_and_requires_same_origin_for_writes() {
    let db = fixture().await;
    let raw = "synthetic-onboarding-session-0000000002";
    new_session(&db.pool, raw, "100", &[HOME]).await;
    let stored: String = sqlx::query_scalar("SELECT session_hash FROM guild_onboarding_sessions")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(stored, session_lookup_key(raw));
    assert_ne!(stored, raw);
    assert!(load(&db.pool, &headers(raw)).await.is_ok());
    let response = cookie(COOKIE, raw, TTL);
    for attribute in [
        "HttpOnly",
        "Secure",
        "SameSite=Lax",
        "Max-Age=1200",
        "Path=/twitch/guild-onboarding",
    ] {
        assert!(response.contains(attribute));
    }
    assert!(same_origin(&headers(raw)));
    let mut foreign = headers(raw);
    foreign.insert(
        header::ORIGIN,
        HeaderValue::from_static("https://evil.example"),
    );
    assert!(!same_origin(&foreign));
    let response = select(
        Extension(auth(db.pool.clone())),
        foreign,
        Json(Selection {
            guild_id: HOME.to_string(),
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    sqlx::query("UPDATE guild_onboarding_sessions SET expires_at=now()-interval '1 second'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(load(&db.pool, &headers(raw)).await.is_err());
    db.close().await;
}

fn rust_sources(directory: &std::path::Path, sources: &mut Vec<String>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, sources);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(std::fs::read_to_string(path).unwrap());
        }
    }
}

#[tokio::test]
async fn onboarding_cookie_never_authenticates_any_dashboard_or_admin_route() {
    let db = fixture().await;
    let raw = "synthetic-onboarding-session-0000000003";
    new_session(&db.pool, raw, "42", &[HOME]).await;
    let auth = auth(db.pool.clone());
    let mut sources = Vec::new();
    rust_sources(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut sources,
    );
    let route = regex::Regex::new(r#"\.route\(\s*"([^"\n]+)""#).unwrap();
    let mut paths = std::collections::BTreeSet::new();
    for source in sources {
        for capture in route.captures_iter(&source) {
            let path = capture[1].to_string();
            if path.starts_with("/twitch/")
                || path.starts_with("/api/")
                || path.starts_with("/admin/")
            {
                paths.insert(path);
            }
        }
    }
    assert!(
        paths.len() > 250,
        "route coverage unexpectedly shrank: {}",
        paths.len()
    );
    for path in &paths {
        let mut request = Request::builder()
            .uri(path.replace(['{', '}'], ""))
            .body(Body::empty())
            .unwrap();
        *request.headers_mut() = headers(raw);
        request.extensions_mut().insert(auth.clone());
        let (mut parts, _) = request.into_parts();
        let resolved = DashboardAuthLevel::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert_eq!(resolved, DashboardAuthLevel::None, "{path}");
    }
    let app = crate::build_router(db.pool.clone(), "synthetic-internal-token".into())
        .layer(Extension(auth));
    for path in [
        "/twitch/api/v2/streamer/onboarding",
        "/twitch/api/v2/overview",
        "/twitch/api/v2/admin/config",
        "/twitch/dashboard",
        "/twitch/verwaltung",
        "/twitch/analyse",
    ] {
        let mut request = Request::builder().uri(path).body(Body::empty()).unwrap();
        *request.headers_mut() = headers(raw);
        let response = app.clone().oneshot(request).await.unwrap();
        assert!(
            !response.status().is_success(),
            "onboarding cookie reached {path}"
        );
    }
    println!(
        "Onboarding-only session rejected by shared auth on {} registered route paths",
        paths.len()
    );
    db.close().await;
}

#[test]
fn domain_allowlist_ignores_forwarded_hosts_and_rejects_unknown_origins() {
    let mut headers = headers("synthetic-onboarding-session-0000000004");
    headers.insert("x-forwarded-host", HeaderValue::from_static("evil.example"));
    assert_eq!(origin(&headers), Some("https://dead-lock.de"));
    headers.insert(header::HOST, HeaderValue::from_static("evil.example"));
    assert_eq!(origin(&headers), None);
}

#[tokio::test]
async fn cutover_preserves_legacy_signup_history_and_rollback_rejects_foreign_only_partners() {
    let db = fixture().await;
    sqlx::query("SELECT enable_guild_runtime()")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::raw_sql("INSERT INTO twitch_streamer_identities(twitch_user_id,twitch_login) VALUES('500','home_new');
        INSERT INTO twitch_partners(twitch_user_id,twitch_login) VALUES('500','home_new');
        INSERT INTO twitch_partners(twitch_user_id,twitch_login,status,admin_archived_at,manual_partner_opt_out) VALUES('500','home_old','archived',CURRENT_TIMESTAMP,1);
        UPDATE twitch_partners SET technical_pause_reason='maintenance' WHERE twitch_user_id='500' AND status='archived';")
        .execute(&db.pool).await.unwrap();
    assert!(sqlx::query_scalar::<_,bool>("SELECT active FROM streamer_guilds WHERE twitch_user_id='500' AND guild_id=1289721245281292288").fetch_one(&db.pool).await.unwrap());
    assert_eq!(sqlx::query_scalar::<_,i32>("SELECT is_partner_active FROM twitch_partners_all_state WHERE twitch_user_id='500' AND status='active'").fetch_one(&db.pool).await.unwrap(), 1);
    sqlx::query("INSERT INTO discord_guilds(guild_id,status) VALUES(700,'active')")
        .execute(&db.pool)
        .await
        .unwrap();
    let raw = "synthetic-cutover-foreign-session-000001";
    new_session(&db.pool, raw, "100", &[700]).await;
    assert_eq!(
        assign(&db.pool, &session_lookup_key(raw), 700)
            .await
            .unwrap(),
        Some(true)
    );
    sqlx::raw_sql("UPDATE streamer_guilds SET active=false,is_on_discord=false,deactivated_by='discord_leave',deactivated_at=now() WHERE twitch_user_id='100'; UPDATE guild_runtime SET enabled=false;").execute(&db.pool).await.unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT guild_partner_allowed('100')")
            .fetch_one(&db.pool)
            .await
            .unwrap()
    );
    assert!(auth(db.pool.clone())
        .find_partner_for_login("user100", "100")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i32>(
            "SELECT is_partner_active FROM twitch_partners_all_state WHERE twitch_user_id='100'"
        )
        .fetch_one(&db.pool)
        .await
        .unwrap(),
        0
    );
    db.close().await;
}

#[tokio::test]
async fn current_partner_state_and_independent_home_proof_survive_other_assignments() {
    let db = fixture().await;
    sqlx::query("SELECT enable_guild_runtime()")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::raw_sql("INSERT INTO twitch_partners(twitch_user_id,twitch_login,status,admin_archived_at,manual_partner_opt_out) VALUES('42','historical','archived',CURRENT_TIMESTAMP,1);
        INSERT INTO discord_guilds(guild_id,status,auto_partner_default) VALUES(700,'active',true),(701,'active',false);
        UPDATE streamer_guilds SET is_on_discord=true,require_discord_link=true WHERE twitch_user_id='42';").execute(&db.pool).await.unwrap();
    let raw = "synthetic-existing-proof-session-000001";
    let hash = session_lookup_key(raw);
    new_session(&db.pool, raw, "42", &[HOME, 700, 701]).await;
    assert_eq!(assign(&db.pool, &hash, 701).await.unwrap(), Some(false));
    assert!(sqlx::query_scalar::<_,bool>("SELECT is_on_discord FROM streamer_guilds WHERE twitch_user_id='42' AND guild_id=1289721245281292288").fetch_one(&db.pool).await.unwrap());
    assert_eq!(assign(&db.pool, &hash, 700).await.unwrap(), Some(true));
    sqlx::query("UPDATE twitch_partners SET technical_pause_reason='maintenance' WHERE twitch_user_id='42' AND status='archived'").execute(&db.pool).await.unwrap();
    assert!(sqlx::query_scalar::<_,bool>("SELECT is_on_discord FROM streamer_guilds WHERE twitch_user_id='42' AND guild_id=1289721245281292288").fetch_one(&db.pool).await.unwrap());
    assert_eq!(assign(&db.pool, &hash, HOME).await.unwrap(), Some(true));
    assert_eq!(
        sqlx::query_scalar::<_, i32>(
            "SELECT is_on_discord FROM twitch_streamer_identities WHERE twitch_user_id='42'"
        )
        .fetch_one(&db.pool)
        .await
        .unwrap(),
        1
    );
    db.close().await;
}

#[tokio::test]
async fn foreign_admission_preserves_inactive_home_membership_after_revocation() {
    for (partner_status, removal) in [
        ("departnered", "discord_leave"),
        ("archived", "guild_removed"),
    ] {
        let db = fixture().await;
        sqlx::query("SELECT enable_guild_runtime()")
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE twitch_partners SET status=$1 WHERE twitch_user_id='42'")
            .bind(partner_status)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO discord_guilds(guild_id,status) VALUES(700,'active')")
            .execute(&db.pool)
            .await
            .unwrap();
        let raw = "synthetic-cross-guild-admission-session-000001";
        new_session(&db.pool, raw, "42", &[HOME, 700]).await;
        assert_eq!(
            assign(&db.pool, &session_lookup_key(raw), 700)
                .await
                .unwrap(),
            Some(true)
        );
        let home: (bool, bool) = sqlx::query_as("SELECT active,require_discord_link FROM streamer_guilds WHERE twitch_user_id='42' AND guild_id=$1")
            .bind(HOME).fetch_one(&db.pool).await.unwrap();
        assert_eq!(home, (false, false));
        assert!(
            auth(db.pool.clone())
                .is_partner_active("existing", "42")
                .await
        );
        if removal == "guild_removed" {
            sqlx::query(
                "UPDATE discord_guilds SET status='removed',removed_at=now() WHERE guild_id=700",
            )
            .execute(&db.pool)
            .await
            .unwrap();
        }
        sqlx::query("UPDATE streamer_guilds SET active=false,is_on_discord=false,deactivated_at=now(),deactivated_by=$1 WHERE twitch_user_id='42' AND guild_id=700")
            .bind(removal).execute(&db.pool).await.unwrap();
        assert!(
            !auth(db.pool.clone())
                .is_partner_active("existing", "42")
                .await
        );
        assert!(auth(db.pool.clone())
            .find_partner_for_login("existing", "42")
            .await
            .unwrap()
            .is_none());
        sqlx::raw_sql("UPDATE twitch_partners SET status='departnered' WHERE twitch_user_id='42'; UPDATE twitch_partners SET status='active' WHERE twitch_user_id='42';")
            .execute(&db.pool).await.unwrap();
        assert!(sqlx::query_scalar::<_, bool>(
            "SELECT active FROM streamer_guilds WHERE twitch_user_id='42' AND guild_id=$1"
        )
        .bind(HOME)
        .fetch_one(&db.pool)
        .await
        .unwrap());
        db.close().await;
    }
}

#[tokio::test]
async fn automatic_admission_reactivates_unblocked_history_and_departed_members_can_rejoin() {
    let db = fixture().await;
    sqlx::query("SELECT enable_guild_runtime()")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::raw_sql("INSERT INTO twitch_streamer_identities(twitch_user_id,twitch_login) VALUES('100','former');
        INSERT INTO twitch_partners(twitch_user_id,twitch_login,status) VALUES('100','former','departnered');
        INSERT INTO discord_guilds(guild_id,status) VALUES(700,'active');").execute(&db.pool).await.unwrap();
    let raw = "synthetic-returning-partner-session-000001";
    let hash = session_lookup_key(raw);
    new_session(&db.pool, raw, "100", &[700]).await;
    assert_eq!(assign(&db.pool, &hash, 700).await.unwrap(), Some(true));
    assert!(
        auth(db.pool.clone())
            .is_partner_active("user100", "100")
            .await
    );
    sqlx::query("UPDATE streamer_guilds SET active=false,is_on_discord=false,deactivated_at=now(),deactivated_by='discord_leave' WHERE twitch_user_id='100' AND guild_id=700").execute(&db.pool).await.unwrap();
    let response = status(Extension(auth(db.pool.clone())), headers(raw)).await;
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert!(body["memberships"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["guild_id"] == "700" && m["can_rejoin"] == true));
    assert_eq!(assign(&db.pool, &hash, 700).await.unwrap(), None);
    sqlx::query("UPDATE guild_onboarding_sessions SET guild_ids=ARRAY[700]::bigint[],discord_verified_at=now()")
        .execute(&db.pool).await.unwrap();
    assert_eq!(assign(&db.pool, &hash, 700).await.unwrap(), Some(true));
    sqlx::query("UPDATE streamer_guilds SET active=false,deactivated_at=now(),deactivated_by='admin' WHERE twitch_user_id='100' AND guild_id=700").execute(&db.pool).await.unwrap();
    assert_eq!(assign(&db.pool, &hash, 700).await.unwrap(), None);
    db.close().await;
}

#[tokio::test]
async fn foreign_reactivation_metadata_and_disabled_runtime_never_restore_home_access() {
    for source in ["migration", "onboarding"] {
        let db = fixture().await;
        sqlx::query("SELECT enable_guild_runtime()")
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE streamer_guilds SET source=$1 WHERE twitch_user_id='42'")
            .bind(source)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::raw_sql("UPDATE twitch_partners SET status='departnered' WHERE twitch_user_id='42';
            INSERT INTO twitch_partners(twitch_user_id,twitch_login,status) VALUES('42','old','archived');
            INSERT INTO discord_guilds(guild_id,status) VALUES(700,'active');")
            .execute(&db.pool).await.unwrap();
        let raw = "synthetic-durable-home-provenance-000001";
        new_session(&db.pool, raw, "42", &[700]).await;
        assert_eq!(
            assign(&db.pool, &session_lookup_key(raw), 700)
                .await
                .unwrap(),
            Some(true)
        );
        for status in ["active", "archived"] {
            sqlx::query("UPDATE twitch_partners SET require_discord_link=0,technical_pause_reason=NULL WHERE twitch_user_id='42' AND status=$1")
                .bind(status).execute(&db.pool).await.unwrap();
            assert!(!sqlx::query_scalar::<_, bool>(
                "SELECT active FROM streamer_guilds WHERE twitch_user_id='42' AND guild_id=$1"
            )
            .bind(HOME)
            .fetch_one(&db.pool)
            .await
            .unwrap());
        }
        sqlx::query("UPDATE streamer_guilds SET active=false,is_on_discord=false,deactivated_at=now() WHERE twitch_user_id='42' AND guild_id=700")
            .execute(&db.pool).await.unwrap();
        for enabled in [true, false] {
            sqlx::query("UPDATE guild_runtime SET enabled=$1")
                .bind(enabled)
                .execute(&db.pool)
                .await
                .unwrap();
            assert!(
                !sqlx::query_scalar::<_, bool>("SELECT guild_partner_allowed('42')")
                    .fetch_one(&db.pool)
                    .await
                    .unwrap()
            );
            assert!(
                !auth(db.pool.clone())
                    .is_partner_active("existing", "42")
                    .await
            );
        }
        db.close().await;
    }
}

#[tokio::test]
async fn legacy_admission_creates_first_home_membership_after_foreign_onboarding() {
    let db = fixture().await;
    sqlx::raw_sql("SELECT enable_guild_runtime(); INSERT INTO discord_guilds(guild_id,status) VALUES(700,'active');")
        .execute(&db.pool).await.unwrap();
    let raw = "synthetic-first-home-after-foreign-000001";
    new_session(&db.pool, raw, "100", &[700]).await;
    assert_eq!(
        assign(&db.pool, &session_lookup_key(raw), 700)
            .await
            .unwrap(),
        Some(true)
    );
    assert!(!sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM streamer_guilds WHERE twitch_user_id='100' AND guild_id=$1)"
    )
    .bind(HOME)
    .fetch_one(&db.pool)
    .await
    .unwrap());
    sqlx::query("UPDATE twitch_partners SET status='active',added_by='community_signup' WHERE twitch_user_id='100'")
        .execute(&db.pool).await.unwrap();
    assert!(sqlx::query_scalar::<_, bool>(
        "SELECT active FROM streamer_guilds WHERE twitch_user_id='100' AND guild_id=$1"
    )
    .bind(HOME)
    .fetch_one(&db.pool)
    .await
    .unwrap());
    db.close().await;
}

#[tokio::test]
async fn legacy_departnership_deactivates_home_onboarding_memberships() {
    for change in [
        "UPDATE twitch_partners SET status='departnered' WHERE twitch_user_id='100'",
        "UPDATE twitch_partners SET status='archived' WHERE twitch_user_id='100'",
        "UPDATE twitch_partners SET admin_archived_at=CURRENT_TIMESTAMP WHERE twitch_user_id='100'",
    ] {
        let db = fixture().await;
        let raw = "synthetic-onboarded-home-departner-000001";
        new_session(&db.pool, raw, "100", &[HOME]).await;
        assert_eq!(
            assign(&db.pool, &session_lookup_key(raw), HOME)
                .await
                .unwrap(),
            Some(true)
        );
        sqlx::query(change).execute(&db.pool).await.unwrap();
        assert!(!sqlx::query_scalar::<_, bool>(
            "SELECT active FROM streamer_guilds WHERE twitch_user_id='100' AND guild_id=$1"
        )
        .bind(HOME)
        .fetch_one(&db.pool)
        .await
        .unwrap());
        assert!(
            !sqlx::query_scalar::<_, bool>("SELECT guild_partner_allowed('100')")
                .fetch_one(&db.pool)
                .await
                .unwrap()
        );
        db.close().await;
    }
}

#[tokio::test]
async fn cached_selection_cannot_undo_any_post_verification_revocation() {
    for revoke in [
        "UPDATE streamer_guilds SET active=false,is_on_discord=false,deactivated_by='discord_leave',deactivated_at=now() WHERE twitch_user_id='42' AND guild_id=700",
        "UPDATE discord_guilds SET status='removed',removed_at=now() WHERE guild_id=700; UPDATE discord_guilds SET status='active',removed_at=NULL WHERE guild_id=700",
        "UPDATE twitch_streamer_identities SET discord_user_id=NULL WHERE twitch_user_id='42'",
        "UPDATE twitch_streamer_identities SET is_on_discord=0 WHERE twitch_user_id='42'",
        "UPDATE streamer_guilds SET is_on_discord=false WHERE twitch_user_id='42' AND guild_id=1289721245281292288",
    ] {
        let db = fixture().await;
        sqlx::raw_sql("SELECT enable_guild_runtime();
            INSERT INTO discord_guilds(guild_id,status) VALUES(700,'active');
            UPDATE twitch_streamer_identities SET discord_user_id='987',is_on_discord=1 WHERE twitch_user_id='42';
            UPDATE streamer_guilds SET require_discord_link=true WHERE twitch_user_id='42';
            INSERT INTO streamer_guilds(twitch_user_id,guild_id,active,is_on_discord,require_discord_link,deactivated_at) VALUES('42',700,true,true,true,NULL);")
            .execute(&db.pool).await.unwrap();
        let raw = "synthetic-revocation-after-proof-000001";
        let hash = session_lookup_key(raw);
        new_session(&db.pool, raw, "42", &[HOME,700]).await;
        sqlx::raw_sql(revoke).execute(&db.pool).await.unwrap();
        let guild = if revoke.contains("1289721245281292288") { HOME } else { 700 };
        assert_eq!(assign(&db.pool, &hash, guild).await.unwrap(), None, "{revoke}");
        sqlx::query("UPDATE guild_onboarding_sessions SET guild_ids=ARRAY[$1]::bigint[],discord_verified_at=now()")
            .bind(guild).execute(&db.pool).await.unwrap();
        assert_eq!(assign(&db.pool, &hash, guild).await.unwrap(), Some(true), "{revoke}");
        db.close().await;
    }
}

#[tokio::test]
async fn discord_guild_pagination_omits_first_cursor_and_sends_next_cursor() {
    use wiremock::{
        matchers::{method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token":"synthetic"})))
        .mount(&server)
        .await;
    Mock::given(path("/users/@me"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id":"987","username":"test"})),
        )
        .mount(&server)
        .await;
    let guilds: Vec<_> = (1..=200).map(|id| json!({"id":id.to_string()})).collect();
    Mock::given(path("/users/@me/guilds"))
        .and(|request: &wiremock::Request| {
            !request.url.query_pairs().any(|(key, _)| key == "after")
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(guilds))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/users/@me/guilds"))
        .and(query_param("after", "200"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    let config = DiscordConfig {
        client_id: "synthetic".into(),
        client_secret: "synthetic".into(),
        http: reqwest::Client::new(),
        api_base: server.uri(),
    };
    let (_, _, guilds) = config
        .identity("synthetic", "https://dead-lock.de/callback")
        .await
        .unwrap();
    assert_eq!(guilds.len(), 200);
}

#[tokio::test]
async fn home_onboarding_keeps_required_proof_after_legacy_sync() {
    let db = fixture().await;
    let raw = "synthetic-home-onboarding-proof-000001";
    new_session(&db.pool, raw, "100", &[HOME]).await;
    assert_eq!(
        assign(&db.pool, &session_lookup_key(raw), HOME)
            .await
            .unwrap(),
        Some(true)
    );
    sqlx::query(
        "UPDATE twitch_streamer_identities SET discord_user_id=NULL WHERE twitch_user_id='100'",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT guild_partner_allowed('100')")
            .fetch_one(&db.pool)
            .await
            .unwrap()
    );
    assert_eq!(
        assign(&db.pool, &session_lookup_key(raw), HOME)
            .await
            .unwrap(),
        None
    );
    db.close().await;
}
