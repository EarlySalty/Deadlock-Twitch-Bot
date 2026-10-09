use super::*;
use std::sync::{Arc, Mutex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

#[derive(Default)]
struct DiscordState {
    holders: BTreeSet<u64>,
    members: BTreeSet<u64>,
    membership_error: bool,
    membership_complete: bool,
    writes: Vec<(u64, bool, String)>,
    links: Vec<serde_json::Value>,
    role_error: bool,
    link_error: bool,
    write_error: bool,
    complete: bool,
}

#[derive(Clone)]
struct Discord(Arc<Mutex<DiscordState>>);
impl Respond for Discord {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let mut state = self.0.lock().unwrap();
        match request.url.path() {
            "/internal/master/v1/discord/role-members" => {
                assert!(request
                    .url
                    .query_pairs()
                    .any(|(key, value)| key == "live" && value == "true"));
                let membership = request
                    .url
                    .query_pairs()
                    .any(|(key, value)| key == "role_id" && value == "1");
                if membership && state.membership_error || !membership && state.role_error {
                    return ResponseTemplate::new(503);
                }
                let (role_id, complete, members) = if membership {
                    ("1", state.membership_complete, &state.members)
                } else {
                    ("2", state.complete, &state.holders)
                };
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "ok": true, "complete": complete, "guild_id": "1", "role_id": role_id,
                    "members": members.iter().map(|id| serde_json::json!({"id": id.to_string()})).collect::<Vec<_>>()
                }))
            }
            "/internal/master/v1/discord/twitch-links" => {
                if state.link_error {
                    return ResponseTemplate::new(503);
                }
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"ok": true, "links": state.links}))
            }
            path => {
                assert!(request.headers.contains_key("x-internal-token"));
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                assert_eq!(body["role_id"], 2);
                assert_eq!(body["guild_id"], 1);
                let id = body["user_id"].as_u64().unwrap();
                let enabled = path.ends_with("add-role");
                assert!(enabled || path.ends_with("remove-role"));
                state.writes.push((
                    id,
                    enabled,
                    request.headers["x-idempotency-key"]
                        .to_str()
                        .unwrap()
                        .into(),
                ));
                if state.write_error {
                    return ResponseTemplate::new(503);
                }
                if enabled {
                    state.holders.insert(id);
                } else {
                    state.holders.remove(&id);
                }
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"ok": true, "result": {"user_id": id}}))
            }
        }
    }
}

async fn fixture() -> (
    crate::role_test_postgres::TestPostgres,
    MockServer,
    BrokerRelay,
    Discord,
) {
    let db = crate::role_test_postgres::TestPostgres::start().await;
    sqlx::raw_sql("CREATE TABLE twitch_partners (twitch_user_id TEXT PRIMARY KEY, status TEXT, raid_admin_enabled BOOL, raid_bot_enabled INT, manual_partner_opt_out INT, technical_pause_reason TEXT, departnered_at TEXT, admin_archived_at TEXT);
        CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT PRIMARY KEY, discord_user_id TEXT);
        CREATE TABLE twitch_partner_signup_denylist (twitch_user_id TEXT);
        CREATE TABLE twitch_raid_auth (twitch_user_id TEXT PRIMARY KEY);
        INSERT INTO twitch_raid_auth VALUES ('100');
        INSERT INTO twitch_partners VALUES ('100','active',true,1,0,NULL,NULL,NULL);
        INSERT INTO twitch_streamer_identities VALUES ('100','10');")
        .execute(&db.pool).await.unwrap();
    let server = MockServer::start().await;
    let discord = Discord(Arc::new(Mutex::new(DiscordState {
        complete: true,
        membership_complete: true,
        members: BTreeSet::from([10, 11, 12, 13, 14, 15, 16, 17, 20, 99]),
        ..Default::default()
    })));
    Mock::given(wiremock::matchers::any())
        .respond_with(discord.clone())
        .mount(&server)
        .await;
    let relay = BrokerRelay::new(&tb_config::BrokerConfig {
        base_url: server.uri(),
        token: "test-only".into(),
    })
    .unwrap();
    (db, server, relay, discord)
}

#[tokio::test]
async fn activation_deactivation_reactivation_and_unchanged_state() {
    let (db, _server, relay, discord) = fixture().await;
    discord.0.lock().unwrap().members.remove(&10);
    reconcile(&db.pool, &relay, 1, 2, None).await.unwrap();
    assert_eq!(
        reconcile(&db.pool, &relay, 1, 2, Some(10)).await.unwrap(),
        None
    );
    assert!(discord.0.lock().unwrap().writes.is_empty());
    discord.0.lock().unwrap().members.insert(10);
    reconcile(&db.pool, &relay, 1, 2, None).await.unwrap();
    assert_eq!(discord.0.lock().unwrap().holders, BTreeSet::from([10]));
    sqlx::query(
        "UPDATE twitch_partners SET status=' ACTIVE ',departnered_at='',admin_archived_at='  '",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        reconcile(&db.pool, &relay, 1, 2, Some(10)).await.unwrap(),
        Some(true)
    );
    reconcile(&db.pool, &relay, 1, 2, None).await.unwrap();
    assert_eq!(discord.0.lock().unwrap().writes.len(), 1);
    sqlx::query("UPDATE twitch_partners SET raid_admin_enabled=false,raid_bot_enabled=0")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        reconcile(&db.pool, &relay, 1, 2, Some(10)).await.unwrap(),
        Some(true)
    );
    assert_eq!(discord.0.lock().unwrap().writes.len(), 1);
    sqlx::query("UPDATE twitch_partners SET status='departnered',manual_partner_opt_out=1,departnered_at='today'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        reconcile(&db.pool, &relay, 1, 2, Some(10)).await.unwrap(),
        Some(false)
    );
    sqlx::query(
        "UPDATE twitch_partners SET status='active',manual_partner_opt_out=0,departnered_at=NULL",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        reconcile(&db.pool, &relay, 1, 2, Some(10)).await.unwrap(),
        Some(true)
    );
    let state = discord.0.lock().unwrap();
    assert_eq!(
        state
            .writes
            .iter()
            .map(|(id, on, _)| (*id, *on))
            .collect::<Vec<_>>(),
        vec![(10, true), (10, false), (10, true)]
    );
    assert_ne!(state.writes[0].2, state.writes[2].2);
}

#[tokio::test]
async fn multiple_mappings_verified_fallback_and_legacy_drift() {
    let (db, _server, relay, discord) = fixture().await;
    sqlx::raw_sql("INSERT INTO twitch_partners VALUES ('101','active',true,1,0,NULL,NULL,NULL),('102','departnered',true,0,1,NULL,'today',NULL),('103','active',true,1,0,NULL,NULL,NULL);
        INSERT INTO twitch_streamer_identities VALUES ('101','10'),('102','11');
        INSERT INTO twitch_raid_auth VALUES ('101'),('103');
        UPDATE twitch_partners SET status='departnered',manual_partner_opt_out=1 WHERE twitch_user_id='100';")
        .execute(&db.pool).await.unwrap();
    {
        let mut state = discord.0.lock().unwrap();
        state.holders = BTreeSet::from([11, 12]);
        state.links =
            vec![serde_json::json!({"discord_id":"20","twitch_user_id":"103","verified":true})];
    }
    reconcile(&db.pool, &relay, 1, 2, None).await.unwrap();
    assert_eq!(discord.0.lock().unwrap().holders, BTreeSet::from([10, 20]));
    let count = discord.0.lock().unwrap().writes.len();
    reconcile(&db.pool, &relay, 1, 2, Some(10)).await.unwrap();
    assert_eq!(discord.0.lock().unwrap().writes.len(), count);
    {
        let mut state = discord.0.lock().unwrap();
        state.holders.remove(&10);
        state.holders.insert(99);
    }
    reconcile(&db.pool, &relay, 1, 2, None).await.unwrap();
    assert_eq!(discord.0.lock().unwrap().holders, BTreeSet::from([10, 20]));
}

#[tokio::test]
async fn redundant_unverified_links_do_not_override_reliable_inactivity() {
    let (db, _server, relay, discord) = fixture().await;
    sqlx::raw_sql(
        "UPDATE twitch_partners SET status='departnered',manual_partner_opt_out=1;
        INSERT INTO twitch_partners VALUES
            ('101','departnered',true,0,1,NULL,NULL,NULL),
            ('103','archived',true,0,1,NULL,NULL,NULL),
            ('104','blocked',true,0,1,NULL,NULL,NULL),
            ('105','departnered',true,0,1,NULL,NULL,NULL),
            ('106','departnered',true,0,1,NULL,NULL,NULL),
            ('107','active',true,1,0,NULL,NULL,NULL),
            ('200','active',true,1,0,NULL,NULL,NULL);
        INSERT INTO twitch_streamer_identities VALUES
            ('103','13'),('104','14'),('105','15'),('106','16'),('107','16');
        INSERT INTO twitch_raid_auth VALUES ('107'),('200');",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    {
        let mut state = discord.0.lock().unwrap();
        state.holders = BTreeSet::from([10, 11, 12, 13, 14, 15, 16]);
        state.links = vec![
            serde_json::json!({"discord_id":"10","twitch_user_id":"100","verified":false}),
            serde_json::json!({"discord_id":"11","twitch_user_id":"101","verified":false}),
            serde_json::json!({"discord_id":"12","twitch_user_id":"100","verified":false}),
            serde_json::json!({"discord_id":"13","twitch_user_id":"999","verified":false}),
            serde_json::json!({"discord_id":"14","twitch_user_id":"200","verified":false}),
            serde_json::json!({"discord_id":"15","twitch_user_id":"200","verified":true}),
            serde_json::json!({"discord_id":"16","twitch_user_id":"106","verified":false}),
            serde_json::json!({"discord_id":"11","twitch_user_id":"101","verified":true}),
        ];
    }
    reconcile(&db.pool, &relay, 1, 2, None).await.unwrap();
    {
        let state = discord.0.lock().unwrap();
        assert_eq!(state.holders, BTreeSet::from([12, 13, 14, 15, 16]));
        assert_eq!(
            state
                .writes
                .iter()
                .map(|(id, enabled, _)| (*id, *enabled))
                .collect::<Vec<_>>(),
            vec![(10, false), (11, false)]
        );
    }
    discord.0.lock().unwrap().links.reverse();
    for id in [10, 11] {
        assert_eq!(
            reconcile(&db.pool, &relay, 1, 2, Some(id)).await.unwrap(),
            Some(false)
        );
    }
    for id in [12, 13, 14] {
        assert_eq!(
            reconcile(&db.pool, &relay, 1, 2, Some(id)).await.unwrap(),
            None
        );
    }
    reconcile(&db.pool, &relay, 1, 2, None).await.unwrap();
    assert_eq!(discord.0.lock().unwrap().writes.len(), 2);
}

#[tokio::test]
async fn missing_state_temporary_pause_unverified_links_and_failures_do_not_revoke() {
    let (db, _server, relay, discord) = fixture().await;
    sqlx::raw_sql(
        "UPDATE twitch_partners SET raid_bot_enabled=0,technical_pause_reason='token_error';
        INSERT INTO twitch_streamer_identities VALUES ('404','14'),('405','17');
        INSERT INTO twitch_partners VALUES ('405','active',true,1,0,NULL,NULL,NULL);",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    {
        let mut state = discord.0.lock().unwrap();
        state.holders = BTreeSet::from([10, 14, 15, 16, 17]);
        state.links = vec![
            serde_json::json!({"discord_id":"15","twitch_user_id":"100","verified":false}),
            serde_json::json!({"discord_id":"16","twitch_user_id":"999","verified":true}),
        ];
    }
    reconcile(&db.pool, &relay, 1, 2, None).await.unwrap();
    assert!(discord.0.lock().unwrap().writes.is_empty());
    discord.0.lock().unwrap().holders.insert(99);
    for error in 0..5 {
        {
            let mut state = discord.0.lock().unwrap();
            state.role_error = error == 0;
            state.link_error = error == 1;
            state.complete = error != 2;
            state.membership_error = error == 3;
            state.membership_complete = error != 4;
        }
        assert!(reconcile(&db.pool, &relay, 1, 2, None).await.is_err());
        assert!(discord.0.lock().unwrap().writes.is_empty());
    }
    discord.0.lock().unwrap().complete = true;
    discord.0.lock().unwrap().membership_complete = true;
    sqlx::query("ALTER TABLE twitch_partners RENAME TO unavailable_partners")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(reconcile(&db.pool, &relay, 1, 2, None).await.is_err());
    assert!(discord.0.lock().unwrap().writes.is_empty());
}

#[tokio::test]
async fn failed_write_retries_on_next_sync_and_signup_denial_revokes() {
    let (db, _server, relay, discord) = fixture().await;
    discord.0.lock().unwrap().write_error = true;
    assert!(reconcile(&db.pool, &relay, 1, 2, None).await.is_err());
    assert!(discord.0.lock().unwrap().holders.is_empty());
    discord.0.lock().unwrap().write_error = false;
    reconcile(&db.pool, &relay, 1, 2, None).await.unwrap();
    assert_eq!(discord.0.lock().unwrap().holders, BTreeSet::from([10]));
    sqlx::query("INSERT INTO twitch_partner_signup_denylist VALUES ('100')")
        .execute(&db.pool)
        .await
        .unwrap();
    reconcile(&db.pool, &relay, 1, 2, None).await.unwrap();
    assert!(discord.0.lock().unwrap().holders.is_empty());
}

struct GraceNotifier {
    pool: PgPool,
    relay: BrokerRelay,
}

#[async_trait::async_trait]
impl tb_raid::token_lifecycle::TokenLifecycleNotifier for GraceNotifier {
    async fn send_admin_embed(&self, _: i64, _: &str, _: &str) -> bool {
        true
    }

    async fn send_user_dm(&self, _: &str, _: &str) -> bool {
        true
    }

    async fn revoke_streamer_role(&self, discord_user_id: &str, _: &str) -> bool {
        assert_eq!(
            reconcile(
                &self.pool,
                &self.relay,
                1,
                2,
                Some(discord_user_id.parse().unwrap())
            )
            .await
            .unwrap(),
            Some(false)
        );
        true
    }
}

#[tokio::test]
async fn grace_expiry_reconciles_after_persistent_deactivation() {
    let (db, _server, relay, discord) = fixture().await;
    reconcile(&db.pool, &relay, 1, 2, None).await.unwrap();
    sqlx::raw_sql("ALTER TABLE twitch_streamer_identities ADD twitch_login TEXT DEFAULT 'test', ADD updated_at TIMESTAMPTZ DEFAULT NOW();
        ALTER TABLE twitch_raid_auth ADD twitch_login TEXT DEFAULT 'test', ADD raid_enabled BOOL DEFAULT true, ADD needs_reauth BOOL DEFAULT false;
        CREATE TABLE twitch_token_blacklist (twitch_user_id TEXT, twitch_login TEXT, error_count INT, grace_expires_at TEXT, role_removed INT, reminder_sent INT);
        INSERT INTO twitch_token_blacklist VALUES ('100','test',3,'2000-01-01T00:00:00+00:00',0,0);")
        .execute(&db.pool).await.unwrap();
    let reactor = tb_raid::token_lifecycle::TokenLifecycleReactor::new(
        db.pool.clone(),
        GraceNotifier {
            pool: db.pool.clone(),
            relay,
        },
    );
    assert_eq!(reactor.check_grace_periods().await, 1);
    assert!(discord.0.lock().unwrap().holders.is_empty());
    assert_eq!(reactor.check_grace_periods().await, 0);
    assert_eq!(discord.0.lock().unwrap().writes.len(), 2);
}
