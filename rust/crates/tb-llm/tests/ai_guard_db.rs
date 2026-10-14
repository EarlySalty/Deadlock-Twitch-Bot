use std::time::Duration;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[path = "../../../test-support/postgres.rs"]
mod test_postgres;

const LLM_USAGE_SCHEMA: &str = include_str!("../sqlx/schema.sql");
const PAKET_K_REGEL: &str = include_str!("../../../migrations/20261012140000_ai_features_rule.sql");
const MULTISERVER_REGEL: &str =
    include_str!("../../../migrations/20261013150200_multiserver_ai_contract.sql");
const PAKET_K_LEDGER: &str =
    include_str!("../../../migrations/20261012140100_llm_usage_streamer.sql");

const DE: i64 = 1_289_721_245_281_292_288;
const EN: i64 = 900_000_000_000_000_001;
const EN_ENTFERNT: i64 = 900_000_000_000_000_002;

const GUILD_SCHEMA: &str = include_str!("../../../test-support/multiserver_ai_schema.sql");

fn endpoint(server: &MockServer) -> tb_llm::LlmEndpoint {
    tb_llm::LlmEndpoint {
        provider: "fireworks",
        base_url: server.uri(),
        model: tb_llm::selection::configured_fireworks_model(),
        api_key: Some("synthetic-key".into()),
    }
}

async fn assert_denied_without_effects(
    pool: &sqlx::PgPool,
    server: &MockServer,
    request: tb_llm::Request,
    unavailable: bool,
) {
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public.llm_usage")
        .fetch_one(pool)
        .await
        .expect("ledger count");
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
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public.llm_usage")
        .fetch_one(pool)
        .await
        .expect("ledger count");
    assert_eq!(after, before);
    assert_eq!(
        server.received_requests().await.expect("requests").len(),
        sent
    );
}

#[tokio::test]
async fn ki_schalter_sperrt_fremde_server_und_belegt_im_ledger() {
    let database = test_postgres::TestPostgres::start().await;
    let pool = database.pool.clone();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": "ok"}}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 1}
        })))
        .expect(2)
        .mount(&server)
        .await;
    let request = || {
        tb_llm::Request::prompt("Fixture")
            .timeout(Duration::from_secs(5))
            .endpoint(endpoint(&server))
    };

    sqlx::raw_sql(LLM_USAGE_SCHEMA)
        .execute(&pool)
        .await
        .expect("llm_usage");
    sqlx::raw_sql(PAKET_K_LEDGER)
        .execute(&pool)
        .await
        .expect("ledger migration");
    assert_eq!(
        tb_llm::streamer_ai_allowed(Some("100"), None).await,
        Ok(false)
    );
    assert_denied_without_effects(&pool, &server, request().twitch_broadcaster("100"), true).await;
    tb_llm::ledger::initialize(pool.clone(), "twitch-test", "ai-guard").unwrap();
    assert_denied_without_effects(&pool, &server, request().twitch_broadcaster("100"), true).await;
    assert!(matches!(
        tb_llm::streamer_ai_allowed(Some("100"), None).await,
        Err(tb_llm::LlmError::Unavailable(_))
    ));
    sqlx::raw_sql(GUILD_SCHEMA)
        .execute(&pool)
        .await
        .expect("guild schema");
    assert_denied_without_effects(&pool, &server, request().twitch_broadcaster("100"), true).await;
    assert!(matches!(
        tb_llm::streamer_ai_allowed(Some("100"), None).await,
        Err(tb_llm::LlmError::Unavailable(_))
    ));
    for _ in 0..2 {
        sqlx::raw_sql(PAKET_K_REGEL)
            .execute(&pool)
            .await
            .expect("Migration Regel");
        sqlx::raw_sql(PAKET_K_LEDGER)
            .execute(&pool)
            .await
            .expect("Migration Ledger");
    }

    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO discord_guilds (guild_id, ai_features) VALUES ({DE}, true), ({EN}, false);
         INSERT INTO discord_guilds (guild_id, ai_features, status, removed_at)
              VALUES ({EN_ENTFERNT}, true, 'removed', now());
         INSERT INTO streamer_guilds (twitch_user_id, guild_id, active) VALUES
             ('100', {DE}, true),
             ('200', {EN}, true),
             ('300', {EN}, true), ('300', {DE}, true),
             ('500', {EN}, false),
             ('600', {DE}, false),
             ('700', {EN_ENTFERNT}, true), ('700', {EN}, true),
             ('800', {DE}, true), ('900', {DE}, true), ('1000', {DE}, true),
             ('1100', {DE}, true), ('1200', {DE}, true), ('1300', {DE}, true),
             ('1400', {DE}, true);
         INSERT INTO twitch_streamer_identities VALUES
             ('100', 'de_kanal'), ('200', 'fremd_kanal'), ('300', 'gemischt_kanal'),
             ('500', 'inaktiv_fremd'), ('600', 'inaktiv_de'), ('700', 'entfernt_kanal'),
             ('800', 'kein_partner'), ('900', 'ausgeschlossen'), ('1000', 'gesperrte_id'),
             ('1100', 'gesperrter_login'), ('1200', 'blockiert'), ('1300', 'bot_gebannt'),
             ('1400', 'reaktiviert');
         INSERT INTO twitch_partners_all_state VALUES
             ('100', 1), ('200', 1), ('300', 1), ('500', 1), ('600', 1), ('700', 1),
             ('800', 0), ('900', 1), ('1000', 1), ('1100', 1), ('1200', 1), ('1300', 1),
             ('1400', 1);
         INSERT INTO twitch_exclusions VALUES ('900', NULL), ('1400', now());
         INSERT INTO twitch_partner_signup_denylist VALUES
             ('1000', 'anderer_login'), ('9999', 'GESPERRTER_LOGIN');
         INSERT INTO twitch_partners VALUES ('1200', ' blocked '), ('1300', 'BOT_BANNED');"
    )))
    .execute(&pool)
    .await
    .expect("seed");

    for migration in [PAKET_K_REGEL, MULTISERVER_REGEL] {
        sqlx::raw_sql(migration)
            .execute(&pool)
            .await
            .expect("source AI migration");
        for (id, login, erwartet) in [
            (Some("100"), None, true),
            (Some("100"), Some("FREMD_KANAL"), false),
            (Some("200"), None, false),
            (Some("300"), None, false),
            (Some("400"), None, false),
            (Some("500"), None, false),
            (Some("600"), None, false),
            (Some("700"), None, false),
            (Some("800"), None, false),
            (Some("900"), None, false),
            (Some("1000"), None, false),
            (Some("1100"), None, false),
            (Some("1200"), None, false),
            (Some("1300"), None, false),
            (Some("1400"), None, true),
            (None, Some("FREMD_KANAL"), false),
            (None, Some("gemischt_kanal"), false),
            (None, Some("de_kanal"), true),
            (None, Some("unbekannt"), false),
            (None, None, false),
        ] {
            let erlaubt: bool =
                sqlx::query_scalar("SELECT public.twitch_streamer_ai_allowed($1, $2)")
                    .bind(id)
                    .bind(login)
                    .fetch_one(&pool)
                    .await
                    .expect("Funktion");
            assert_eq!(erlaubt, erwartet, "id={id:?} login={login:?}");
        }
    }

    assert_eq!(tb_llm::streamer_ai_allowed(None, None).await, Ok(false));
    assert_eq!(
        tb_llm::streamer_ai_allowed(Some(" "), Some("\t")).await,
        Ok(false)
    );

    assert_eq!(
        tb_llm::streamer_ai_allowed(Some("200"), None).await,
        Ok(false)
    );
    assert_eq!(
        tb_llm::streamer_ai_allowed(None, Some("de_kanal")).await,
        Ok(true)
    );

    assert_denied_without_effects(
        &pool,
        &server,
        request()
            .twitch_broadcaster("100")
            .twitch_channel("FREMD_KANAL"),
        false,
    )
    .await;

    for id in [
        "200", "300", "400", "500", "600", "700", "800", "900", "1000", "1100", "1200", "1300",
    ] {
        let gesperrt = tb_llm::complete("promo_pitch", request().twitch_broadcaster(id)).await;
        assert!(
            matches!(gesperrt, Err(tb_llm::LlmError::Disabled(_))),
            "id={id} {gesperrt:?}"
        );
    }
    let gesperrt = tb_llm::complete("lfg_pitch", request().twitch_channel("Fremd_Kanal")).await;
    assert!(
        matches!(gesperrt, Err(tb_llm::LlmError::Disabled(_))),
        "{gesperrt:?}"
    );
    let gesperrt = tb_llm::complete("promo_pitch", request()).await;
    assert!(
        matches!(gesperrt, Err(tb_llm::LlmError::Disabled(_))),
        "{gesperrt:?}"
    );

    tb_llm::complete("engagement", request().twitch_channel("de_kanal"))
        .await
        .expect("deutscher Kanal");
    for purpose in [
        "community_sentiment",
        "conversation_scam_guidance",
        "social-media-fireworks",
        "unbekannter_zweck",
    ] {
        assert_denied_without_effects(&pool, &server, request().ledger_purpose(purpose), false)
            .await;
    }
    tb_llm::complete("community_sentiment", request().twitch_broadcaster("100"))
        .await
        .expect("zugeordneter Aufruf");
    assert_denied_without_effects(
        &pool,
        &server,
        request()
            .twitch_broadcaster("100")
            .twitch_channel("fremd_kanal"),
        false,
    )
    .await;
    assert_eq!(
        tb_llm::streamer_ai_allowed(Some("100"), Some("fremd_kanal")).await,
        Ok(false)
    );
    for (revoke, restore) in [
        (
            "UPDATE public.discord_guilds SET ai_features=false WHERE guild_id=1289721245281292288",
            "UPDATE public.discord_guilds SET ai_features=true WHERE guild_id=1289721245281292288",
        ),
        (
            "UPDATE public.streamer_guilds SET active=false WHERE twitch_user_id='100'",
            "UPDATE public.streamer_guilds SET active=true WHERE twitch_user_id='100'",
        ),
        (
            "INSERT INTO public.streamer_guilds(twitch_user_id,guild_id) VALUES ('100',900000000000000001)",
            "DELETE FROM public.streamer_guilds WHERE twitch_user_id='100' AND guild_id=900000000000000001",
        ),
        (
            "UPDATE public.twitch_partners_all_state SET is_partner_active=0 WHERE twitch_user_id='100'",
            "UPDATE public.twitch_partners_all_state SET is_partner_active=1 WHERE twitch_user_id='100'",
        ),
        (
            "INSERT INTO public.twitch_exclusions VALUES ('100',NULL)",
            "DELETE FROM public.twitch_exclusions WHERE twitch_user_id='100'",
        ),
    ] {
        assert_eq!(tb_llm::streamer_ai_allowed(Some("100"), None).await, Ok(true));
        assert_eq!(tb_llm::streamer_ai_allowed(None, Some("de_kanal")).await, Ok(true));
        sqlx::raw_sql(revoke).execute(&pool).await.expect("revoke");
        assert_eq!(tb_llm::streamer_ai_allowed(Some("100"), None).await, Ok(false));
        assert_eq!(tb_llm::streamer_ai_allowed(None, Some("de_kanal")).await, Ok(false));
        for scoped in [
            request().twitch_broadcaster("100"),
            request().twitch_channel("de_kanal"),
        ] {
            assert_denied_without_effects(&pool, &server, scoped, false).await;
        }
        sqlx::raw_sql(restore).execute(&pool).await.expect("restore");
        assert_eq!(tb_llm::streamer_ai_allowed(Some("100"), None).await, Ok(true));
        assert_eq!(tb_llm::streamer_ai_allowed(None, Some("de_kanal")).await, Ok(true));
    }
    sqlx::query("UPDATE public.discord_guilds SET ai_features=true")
        .execute(&pool)
        .await
        .expect("all guilds on");
    for unscoped in [
        request(),
        request().twitch_broadcaster(" ").twitch_channel("\t"),
        request().ledger_purpose("community_sentiment"),
        request().ledger_purpose("unbekannter_zweck"),
        request().twitch_channel("unbekannt"),
    ] {
        assert_denied_without_effects(&pool, &server, unscoped, false).await;
    }
    sqlx::query("ALTER TABLE public.discord_guilds RENAME TO hidden_guilds")
        .execute(&pool)
        .await
        .expect("missing table");
    assert_denied_without_effects(&pool, &server, request().twitch_broadcaster("100"), true).await;
    assert!(matches!(
        tb_llm::streamer_ai_allowed(Some("100"), None).await,
        Err(tb_llm::LlmError::Unavailable(_))
    ));
    sqlx::query("ALTER TABLE public.hidden_guilds RENAME TO discord_guilds")
        .execute(&pool)
        .await
        .expect("restore table");
    assert_eq!(
        tb_llm::streamer_ai_allowed(Some("100"), None).await,
        Ok(true)
    );
    sqlx::query("DROP FUNCTION public.twitch_streamer_ai_allowed(text,text)")
        .execute(&pool)
        .await
        .expect("missing function");
    assert_denied_without_effects(&pool, &server, request().twitch_channel("de_kanal"), true).await;
    assert!(matches!(
        tb_llm::streamer_ai_allowed(None, Some("de_kanal")).await,
        Err(tb_llm::LlmError::Unavailable(_))
    ));
    sqlx::raw_sql(MULTISERVER_REGEL)
        .execute(&pool)
        .await
        .expect("restore actual function");
    assert_eq!(
        tb_llm::streamer_ai_allowed(Some("100"), None).await,
        Ok(true)
    );
    server.verify().await;

    let zeilen: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT purpose, twitch_user_id FROM llm_usage ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("Ledger lesen");
    assert_eq!(
        zeilen,
        vec![
            ("engagement".to_string(), Some("100".to_string())),
            ("community_sentiment".to_string(), Some("100".to_string())),
        ]
    );

    pool.close().await;
    assert!(matches!(
        tb_llm::complete("engagement", request().twitch_broadcaster("100")).await,
        Err(tb_llm::LlmError::Unavailable(_))
    ));
    assert!(matches!(
        tb_llm::streamer_ai_allowed(Some("100"), None).await,
        Err(tb_llm::LlmError::Unavailable(_))
    ));
    assert_eq!(server.received_requests().await.expect("requests").len(), 2);
}
