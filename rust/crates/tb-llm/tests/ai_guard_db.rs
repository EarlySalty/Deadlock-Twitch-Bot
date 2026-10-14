use sqlx::PgPool;
use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

#[path = "../../../test-support/postgres.rs"]
mod test_postgres;

const POLICY: &str = include_str!("../../../migrations/20261014170000_central_ai_policy.sql");
const DE: i64 = 1_289_721_245_281_292_288;
const FOREIGN: i64 = 900_000_000_000_000_001;

async fn install_production_schema(pool: &PgPool) {
    sqlx::raw_sql("CREATE DATABASE twitch_analytics; CREATE EXTENSION timescaledb")
        .execute(pool)
        .await
        .expect("disposable production prerequisites");
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    let mut files: Vec<_> = std::fs::read_dir(directory)
        .expect("production migrations")
        .map(|entry| entry.expect("migration entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
        .collect();
    files.sort();
    for path in files {
        if path.file_name().expect("migration name") == "20261014170000_central_ai_policy.sql" {
            continue;
        }
        let sql = std::fs::read_to_string(&path).expect("production SQL");
        let result = if sql.starts_with("-- no-transaction") {
            sqlx::raw_sql(&sql).execute(pool).await.map(|_| ())
        } else {
            let mut tx = pool.begin().await.expect("migration transaction");
            match sqlx::raw_sql(&sql).execute(&mut *tx).await {
                Ok(_) => tx.commit().await,
                Err(error) => Err(error),
            }
        };
        result.unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    }
}

fn request(server: &MockServer) -> tb_llm::Request {
    tb_llm::Request::prompt("synthetic input")
        .endpoint(tb_llm::LlmEndpoint {
            provider: "fireworks",
            base_url: server.uri(),
            model: tb_llm::selection::configured_fireworks_model(),
            api_key: Some("synthetic-key".into()),
        })
        .timeout_secs(5)
}

async fn denied(pool: &PgPool, server: &MockServer, request: tb_llm::Request, unavailable: bool) {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM public.llm_usage")
        .fetch_one(pool)
        .await
        .expect("ledger before");
    let sent = server.received_requests().await.expect("requests").len();
    let result = tb_llm::complete("engagement", request).await;
    assert!(
        matches!(
            (&result, unavailable),
            (Err(tb_llm::LlmError::Unavailable(_)), true)
                | (Err(tb_llm::LlmError::Disabled(_)), false)
        ),
        "{result:?}"
    );
    assert_eq!(server.received_requests().await.expect("requests").len(), sent);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM public.llm_usage")
            .fetch_one(pool)
            .await
            .expect("ledger after"),
        count
    );
}

#[tokio::test]
async fn production_policy_enforces_every_hub_route_without_denied_effects() {
    let database = test_postgres::TestPostgres::start_with_timescaledb().await;
    let pool = &database.pool;
    install_production_schema(pool).await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": "ok"}}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 1}
        })))
        .expect(3)
        .mount(&server)
        .await;
    denied(pool, &server, request(&server).twitch_broadcaster("100"), true).await;
    tb_llm::ledger::initialize(pool.clone(), "twitch-test", "production-ai-guard")
        .expect("isolated ledger");
    denied(pool, &server, request(&server).twitch_broadcaster("100"), true).await;
    sqlx::raw_sql(POLICY).execute(pool).await.expect("new production policy");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO public.twitch_streamer_identities (twitch_user_id,twitch_login) VALUES
             ('100','legacy_de'),('200','foreign_channel'),('300','mixed_channel'),
             ('400','inactive_channel'),('500','not_partner');
         INSERT INTO public.twitch_partners (twitch_user_id,twitch_login,status) VALUES
             ('100','legacy_de','active'),('200','foreign_channel','active'),
             ('300','mixed_channel','active'),('400','inactive_channel','active');
         INSERT INTO public.discord_guilds (guild_id) VALUES ({FOREIGN});
         INSERT INTO public.streamer_guilds (twitch_user_id,guild_id,active) VALUES
             ('100',{DE},true),('200',{FOREIGN},false),('300',{DE},true),
             ('300',{FOREIGN},false),('400',{DE},false),('500',{DE},true)"
    )))
    .execute(pool).await.expect("synthetic production rows");
    for id in ["200", "300", "400", "500", "600", "unknown"] {
        denied(pool, &server, request(&server).twitch_broadcaster(id), false).await;
    }
    for unscoped in [request(&server), request(&server).twitch_channel("unknown"), request(&server).no_ledger()] {
        denied(pool, &server, unscoped, false).await;
    }
    denied(pool, &server, request(&server).twitch_broadcaster("100").twitch_channel("foreign_channel"), false).await;
    for purpose in ["engagement", "social-media-fireworks"] {
        tb_llm::complete("social_media", request(&server).twitch_broadcaster("100").ledger_purpose(purpose))
            .await.expect("legacy German dispatch");
    }
    sqlx::query("UPDATE public.twitch_streamer_identities SET twitch_login='renamed_de' WHERE twitch_user_id='100'")
        .execute(pool).await.expect("rename");
    tb_llm::complete("engagement", request(&server).twitch_broadcaster("100"))
        .await.expect("stable original UID after rename");
    denied(pool, &server, request(&server).twitch_channel("legacy_de"), false).await;
    for (revoke, restore) in [
        ("UPDATE public.discord_guilds SET ai_features=false WHERE guild_id=1289721245281292288", "UPDATE public.discord_guilds SET ai_features=true WHERE guild_id=1289721245281292288"),
        ("UPDATE public.streamer_guilds SET active=false WHERE twitch_user_id='100'", "UPDATE public.streamer_guilds SET active=true WHERE twitch_user_id='100'"),
        ("UPDATE public.twitch_partners SET manual_partner_opt_out=1 WHERE twitch_user_id='100'", "UPDATE public.twitch_partners SET manual_partner_opt_out=0 WHERE twitch_user_id='100'"),
        ("INSERT INTO public.twitch_exclusions (twitch_user_id,kind) VALUES ('100','banned')", "DELETE FROM public.twitch_exclusions WHERE twitch_user_id='100'"),
        ("INSERT INTO public.twitch_partner_signup_denylist (twitch_user_id,twitch_login,reason,added_by) VALUES ('100','renamed_de','fixture','fixture')", "DELETE FROM public.twitch_partner_signup_denylist WHERE twitch_user_id='100'"),
        ("UPDATE public.twitch_partners SET technical_pause_reason='bot_banned' WHERE twitch_user_id='100'", "UPDATE public.twitch_partners SET technical_pause_reason=NULL WHERE twitch_user_id='100'"),
    ] {
        assert_eq!(tb_llm::streamer_ai_allowed(Some("100"), None).await, Ok(true));
        sqlx::raw_sql(revoke).execute(pool).await.expect("revoke");
        for denied_request in [request(&server).twitch_broadcaster("100"), request(&server).twitch_channel("renamed_de")] {
            denied(pool, &server, denied_request, false).await;
        }
        sqlx::raw_sql(restore).execute(pool).await.expect("restore");
    }
    sqlx::query("DROP FUNCTION public.twitch_streamer_ai_allowed(text,text)").execute(pool).await.expect("missing policy");
    denied(pool, &server, request(&server).twitch_broadcaster("100"), true).await;
    sqlx::raw_sql(POLICY).execute(pool).await.expect("restore production policy");
    sqlx::query("ALTER TABLE public.discord_guilds RENAME TO hidden_guilds").execute(pool).await.expect("missing schema");
    denied(pool, &server, request(&server).twitch_broadcaster("100"), true).await;
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM public.llm_usage").fetch_one(pool).await.expect("ledger"), 3);
    server.verify().await;
}
