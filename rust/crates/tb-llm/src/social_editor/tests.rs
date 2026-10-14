use super::*;

#[path = "admin_contract_tests.rs"]
mod admin_contract_tests;

fn config() -> EditorConfig {
    toml::from_str::<SocialAiSection>(fixture::SOCIAL_AI_SECTION)
        .expect("configuration")
        .into_editor_config()
        .expect("trusted configuration")
}

fn request() -> EditorRequest {
    EditorRequest {
        system: "Extract the clip facts".into(),
        text: "Describe the frame".into(),
        frames: vec![Frame {
            timestamp_ms: 1234,
            media_type: ImageMediaType::Png,
            bytes: STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC").expect("PNG fixture"),
        }],
        json_schema: json!({"type":"object","properties":{"title":{"type":"string"}},"required":["title"],"additionalProperties":false}),
    }
}

#[tokio::test]
async fn cumulative_usage_is_owner_scoped_and_unknown_attempts_never_form_partial_totals() {
    let database = DisposablePostgres::start().await;
    let pool = &database.pool;
    let empty = job_usage(pool, "101", "manual:1")
        .await
        .expect("empty usage");
    assert!(
        empty.input_tokens.is_none()
            && empty.output_tokens.is_none()
            && empty.estimated_provider_cost_usd.is_none()
            && empty.provider_latency_ms.is_none()
    );
    sqlx::raw_sql("INSERT INTO public.llm_usage (ts,purpose,provider,model,attempt_state,social_ai_twitch_user_id,social_ai_job_id,tokens_in,tokens_out,estimated_micro_usd,cost_basis,latency_ms) VALUES ('test','social_editor','anthropic','model','failed','101','manual:1',100,20,800,'usage_priced_estimate',12), ('test','social_editor','anthropic','model','succeeded','101','manual:1',150,30,1200,'usage_priced_estimate',18), ('test','social_editor','anthropic','model','succeeded','202','manual:1',9000,9000,9000,'usage_priced_estimate',9000), ('test','social_editor','anthropic','model','succeeded','101','production:1',9000,9000,9000,'usage_priced_estimate',9000)")
        .execute(pool).await.expect("durable attempts");
    let totals = job_usage(pool, "101", "manual:1")
        .await
        .expect("all attempts");
    assert_eq!(totals.input_tokens, Some(250));
    assert_eq!(totals.output_tokens, Some(50));
    assert_eq!(totals.estimated_provider_cost_usd, Some(0.002));
    assert_eq!(totals.provider_latency_ms, Some(30));
    let pending: i64 = sqlx::query_scalar("INSERT INTO public.llm_usage (ts,purpose,provider,model,attempt_state,social_ai_twitch_user_id,social_ai_job_id,latency_ms) VALUES ('test','social_editor','anthropic','model','started','101','manual:1',0) RETURNING id")
        .fetch_one(pool).await.expect("unresolved restart attempt");
    let unknown = job_usage(pool, "101", "manual:1")
        .await
        .expect("unknown totals");
    assert!(
        unknown.input_tokens.is_none()
            && unknown.output_tokens.is_none()
            && unknown.estimated_provider_cost_usd.is_none()
            && unknown.provider_latency_ms.is_none()
    );
    sqlx::query("UPDATE public.llm_usage SET attempt_state='failed',latency_ms=9 WHERE id=$1")
        .bind(pending)
        .execute(pool)
        .await
        .expect("finished unknown usage");
    let partial = job_usage(pool, "101", "manual:1")
        .await
        .expect("independent latency");
    assert!(
        partial.input_tokens.is_none()
            && partial.output_tokens.is_none()
            && partial.estimated_provider_cost_usd.is_none()
    );
    assert_eq!(partial.provider_latency_ms, Some(39));
    sqlx::query("UPDATE public.llm_usage SET tokens_in=50,tokens_out=10,estimated_micro_usd=400,cost_basis='usage_priced_estimate' WHERE id=$1").bind(pending).execute(pool).await.expect("recovery usage");
    let recovered = job_usage(pool, "101", "manual:1")
        .await
        .expect("recovered totals");
    assert_eq!(recovered.input_tokens, Some(300));
    assert_eq!(recovered.estimated_provider_cost_usd, Some(0.0024));
    sqlx::query("UPDATE public.llm_usage SET latency_ms=9007199254740991 WHERE id=$1")
        .bind(pending)
        .execute(pool)
        .await
        .expect("unsafe accumulated latency");
    assert!(job_usage(pool, "101", "manual:1")
        .await
        .expect("safe integer projection")
        .provider_latency_ms
        .is_none());
}

#[test]
fn native_vision_schema_request_has_no_tools_or_fallback() {
    let config = config();
    let body = build_body(&config.provider, &request()).expect("request");
    assert_eq!(
        body["messages"][0]["content"][0]["text"],
        "Frame bei 1234 ms"
    );
    assert_eq!(
        body["messages"][0]["content"][1]["source"]["media_type"],
        "image/png"
    );
    assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    assert!(body.get("tools").is_none());
    assert!(body.get("fallbacks").is_none());
}

#[test]
fn stop_reasons_and_non_text_are_not_success() {
    let schema = schema::compile(&request().json_schema).expect("schema");
    for (reason, expected) in [
        ("refusal", ErrorKind::Refusal),
        ("max_tokens", ErrorKind::Truncated),
        ("tool_use", ErrorKind::InvalidResponse),
    ] {
        let response = json!({"model":"configured-model","stop_reason":reason,"content":[{"type":"text","text":"{\"title\":\"a\"}"}]});
        assert_eq!(
            parse_reply(response.to_string().as_bytes(), &schema).err(),
            Some(expected)
        );
    }
    let response = json!({"model":"configured-model","stop_reason":"end_turn","content":[{"type":"thinking","thinking":""}]});
    assert_eq!(
        parse_reply(response.to_string().as_bytes(), &schema).err(),
        Some(ErrorKind::InvalidResponse)
    );
    let thinking = json!({"model":"configured-model","stop_reason":"end_turn","content":[{"type":"thinking","thinking":""},{"type":"text","text":"{\"title\":\"a\"}"}]});
    assert!(parse_reply(thinking.to_string().as_bytes(), &schema).is_ok());
    let malformed = json!({"model":"configured-model","stop_reason":"end_turn","content":[{"type":"text","text":"{"}]});
    assert_eq!(
        parse_reply(malformed.to_string().as_bytes(), &schema).err(),
        Some(ErrorKind::InvalidResponse)
    );
}

#[test]
fn compiled_schema_rejects_invalid_output_and_external_references() {
    let schema = json!({
        "type":"object",
        "properties":{
            "title":{"type":"string","minLength":3,"maxLength":12},
            "score":{"type":"integer","minimum":1,"maximum":5}
        },
        "required":["title","score"],
        "additionalProperties":false
    });
    let validator = schema::compile(&schema).expect("supported schema");
    let reply = |content: Value| {
        json!({"model":"configured-model","stop_reason":"end_turn","content":[{"type":"text","text":content.to_string()}]}).to_string()
    };
    assert!(parse_reply(
        reply(json!({"title":"clip","score":3})).as_bytes(),
        &validator
    )
    .is_ok());
    for content in [
        json!({"title":"clip"}),
        json!({"title":3,"score":3}),
        json!({"title":"clip","score":3,"extra":true}),
        json!({"title":"xy","score":3}),
        json!({"title":"clip","score":6}),
    ] {
        assert_eq!(
            parse_reply(reply(content).as_bytes(), &validator).err(),
            Some(ErrorKind::InvalidResponse)
        );
    }
    for reference in [
        "http://127.0.0.1/admin",
        "file:///etc/passwd",
        "#/$defs/local",
    ] {
        let unsafe_schema = json!({"type":"object","properties":{"title":{"$ref":reference}},"additionalProperties":false});
        assert!(schema::compile(&unsafe_schema).is_err());
    }
    assert!(schema::compile(&json!({"type":"object","properties":{"value":{"unknownConstraint":true}},"additionalProperties":false})).is_err());
    assert!(schema::compile(
        &json!({"type":"object","required":"title","additionalProperties":false})
    )
    .is_err());
}

#[test]
fn invalid_schema_is_blocked_before_database_or_network() {
    let config = config();
    let context = EditorContext {
        twitch_user_id: "person".into(),
        job_id: "job".into(),
        stage: "caption".into(),
    };
    let mut input = request();
    input.json_schema = json!({"type":"object","properties":{"title":{"$ref":"http://127.0.0.1/admin"}},"additionalProperties":false});
    assert_eq!(
        validate_request(&config.provider, &context, &input)
            .err()
            .map(|error| error.kind),
        Some(ErrorKind::InvalidRequest)
    );
}

#[test]
fn unknown_usage_does_not_become_zero_cost() {
    let usage = Usage::from_value(&json!({"input_tokens":100,"output_tokens":10})).expect("usage");
    assert_eq!(usage.cache_read_input_tokens, None);
    assert_eq!(usage.estimated_cost(&config().provider.pricing), None);
}

#[test]
fn bounded_input_reservation_is_enforced_before_transport() {
    let mut configuration = config();
    configuration.provider.max_input_tokens = 100;
    let context = EditorContext {
        twitch_user_id: "person".into(),
        job_id: "job".into(),
        stage: "stage".into(),
    };
    assert_eq!(
        validate_request(&configuration.provider, &context, &request())
            .err()
            .map(|e| e.kind),
        Some(ErrorKind::InvalidRequest)
    );
}

struct DisposablePostgres {
    pool: PgPool,
    data: tempfile::TempDir,
}

impl DisposablePostgres {
    async fn start() -> Self {
        Self::start_with_migration(true).await
    }

    async fn start_with_migration(migrate: bool) -> Self {
        use std::process::{Command, Stdio};
        let data = tempfile::Builder::new()
            .prefix("social-ai-db-")
            .tempdir()
            .expect("temporary database");
        let path = data.path().join("data");
        let init = Command::new("/usr/lib/postgresql/16/bin/initdb")
            .args(["--no-locale", "-A", "trust", "-U", "social_test", "-D"])
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("initdb");
        assert!(init.success());
        let opts = format!(
            "-h '' -k {} -c shared_buffers=16MB -c fsync=off",
            data.path().display()
        );
        let started = Command::new("/usr/lib/postgresql/16/bin/pg_ctl")
            .arg("-D")
            .arg(&path)
            .arg("-o")
            .arg(opts)
            .args(["-w", "start"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("pg_ctl start");
        assert!(started.success());
        let options = sqlx::postgres::PgConnectOptions::new()
            .host(data.path().to_str().expect("socket path"))
            .username("social_test")
            .database("postgres");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("isolated postgres");
        sqlx::raw_sql("CREATE TABLE public.llm_usage (id BIGSERIAL PRIMARY KEY, ts TEXT NOT NULL, source TEXT, purpose TEXT, model TEXT, tokens_in BIGINT, tokens_out BIGINT, total BIGINT, success BIGINT, project TEXT, service TEXT, provider TEXT, attempt_state TEXT NOT NULL DEFAULT 'legacy', request_id TEXT, error_code TEXT, http_status INT, latency_ms BIGINT, finished_at TIMESTAMPTZ); CREATE TABLE public.social_media_settings (key TEXT PRIMARY KEY, value JSONB NOT NULL)")
            .execute(&pool).await.expect("base schema");
        sqlx::query("CREATE TABLE public.twitch_streamers (twitch_user_id TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .expect("limit owner schema");
        if migrate {
            sqlx::raw_sql(include_str!(
                "../../../../migrations/20261011200001_social_ai_provider_usage.sql"
            ))
            .execute(&pool)
            .await
            .expect("provider migration");
            sqlx::raw_sql(include_str!(
                "../../../../migrations/20261011204000_social_ai_admin_limits.sql"
            ))
            .execute(&pool)
            .await
            .expect("admin limits migration");
            sqlx::raw_sql(include_str!(
                "../../../../migrations/20261014150000_social_ai_admin_console_limits.sql"
            ))
            .execute(&pool)
            .await
            .expect("admin console limits migration");
        }
        static LEDGER_IDENTITY: std::sync::Once = std::sync::Once::new();
        LEDGER_IDENTITY.call_once(|| {
            crate::ledger::initialize(pool.clone(), "social_test", "connector_test")
                .expect("ledger identity");
        });
        Self { pool, data }
    }
}

impl Drop for DisposablePostgres {
    fn drop(&mut self) {
        use std::process::{Command, Stdio};
        let _ = Command::new("/usr/lib/postgresql/16/bin/pg_ctl")
            .arg("-D")
            .arg(self.data.path().join("data"))
            .args(["-m", "immediate", "-w", "stop"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn apply_runtime_roles(database: &DisposablePostgres) {
    let output = std::process::Command::new("/usr/bin/psql")
        .args(["-X", "-v", "ON_ERROR_STOP=1", "-h"])
        .arg(database.data.path())
        .args(["-U", "social_test", "-d", "postgres", "-f"])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../ops/systemd/twitch-runtime-roles.sql"
        ))
        .output()
        .expect("run actual runtime role script");
    assert!(
        output.status.success(),
        "runtime role script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn assert_consent_roles(pool: &PgPool) {
    for (role, expected) in [
        ("twitchbot", (true, false, false)),
        ("twitchdash", (true, true, true)),
        ("twitchlegacy", (false, false, false)),
        ("twitchanalysis", (true, false, false)),
        ("twitchaudit", (true, false, false)),
    ] {
        let rights: (bool, bool, bool, bool) = sqlx::query_as(
            "SELECT has_table_privilege($1,'public.social_ai_user_consent','SELECT'), has_table_privilege($1,'public.social_ai_user_consent','INSERT'), has_table_privilege($1,'public.social_ai_user_consent','UPDATE'), has_table_privilege($1,'public.social_ai_user_consent','DELETE,TRUNCATE,REFERENCES,TRIGGER')",
        )
        .bind(role)
        .fetch_one(pool)
        .await
        .expect("effective consent grants");
        assert_eq!((rights.0, rights.1, rights.2), expected, "{role}");
        assert!(!rights.3, "{role} has surplus privileges");
    }
    let helper_exposed: bool = sqlx::query_scalar(
        "SELECT has_function_privilege('twitchbot','public.twitch_apply_social_ai_consent_roles()','EXECUTE')",
    )
    .fetch_one(pool)
    .await
    .expect("helper execution privilege");
    assert!(!helper_exposed);
}

#[tokio::test]
async fn consent_roles_survive_real_runtime_script_and_allow_only_dashboard_mutation() {
    let database = DisposablePostgres::start_with_migration(false).await;
    let pool = &database.pool;
    sqlx::query("CREATE DATABASE twitch_analytics")
        .execute(pool)
        .await
        .expect("role script database reference");
    sqlx::raw_sql(
        "CREATE ROLE postgres; \
         CREATE TABLE public.category_channels(id BIGINT); \
         CREATE TABLE public.category_collector_status(id BIGINT); \
         CREATE TABLE public.category_chat_rollup(id BIGINT); \
         CREATE TABLE public.category_media(id BIGINT); \
         CREATE TABLE public.category_media_jobs(id BIGINT); \
         CREATE TABLE public.category_collection_runs(id BIGINT); \
         CREATE TABLE public.category_chat_messages(id BIGINT); \
         CREATE TABLE public.category_chat_dirty(id BIGINT, hour_at TIMESTAMPTZ); \
         CREATE TABLE public.category_collector_config(id BIGINT); \
         CREATE TABLE public.category_chat_redactions(id BIGINT); \
         CREATE TABLE public.category_chat_user_redactions(id BIGINT); \
         CREATE FUNCTION public.category_prune_partitions(integer,bigint) RETURNS void LANGUAGE plpgsql AS $$ BEGIN END $$; \
         CREATE FUNCTION public.category_lock_chat_rooms(text[]) RETURNS void LANGUAGE plpgsql AS $$ BEGIN END $$; \
         CREATE FUNCTION public.category_redact_chat_event(text,text,text,timestamptz) RETURNS void LANGUAGE plpgsql AS $$ BEGIN END $$; \
         CREATE FUNCTION public.category_redact_chat_event_locked(text,text,text,timestamptz) RETURNS void LANGUAGE plpgsql AS $$ BEGIN END $$",
    )
    .execute(pool)
    .await
    .expect("minimal runtime role dependencies");
    apply_runtime_roles(&database);
    apply_runtime_roles(&database);
    let absent: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public.social_ai_user_consent')::text")
            .fetch_one(pool)
            .await
            .expect("consent table optional before migration");
    assert!(absent.is_none());
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261011200001_social_ai_provider_usage.sql"
    ))
    .execute(pool)
    .await
    .expect("provider migration after runtime role installation");
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261011204000_social_ai_admin_limits.sql"
    ))
    .execute(pool)
    .await
    .expect("limits migration after runtime role installation");
    admin_contract_tests::assert_limit_roles(pool).await;
    assert_consent_roles(pool).await;
    apply_runtime_roles(&database);
    apply_runtime_roles(&database);
    assert_consent_roles(pool).await;
    admin_contract_tests::assert_limit_roles(pool).await;

    let mut connection = pool.acquire().await.expect("role session");
    sqlx::query("SET ROLE twitchdash")
        .execute(&mut *connection)
        .await
        .expect("dashboard role");
    sqlx::query("INSERT INTO public.social_ai_user_consent(twitch_user_id,external_ai_consent) VALUES ('fixture',false)")
        .execute(&mut *connection)
        .await
        .expect("dashboard may store consent");
    sqlx::query("UPDATE public.social_ai_user_consent SET external_ai_consent=true WHERE twitch_user_id='fixture'")
        .execute(&mut *connection)
        .await
        .expect("dashboard may change consent");
    let allowed: bool = sqlx::query_scalar(
        "SELECT external_ai_consent FROM public.social_ai_user_consent WHERE twitch_user_id='fixture'",
    )
    .fetch_one(&mut *connection)
    .await
    .expect("dashboard may read consent");
    assert!(allowed);
    sqlx::query("RESET ROLE")
        .execute(&mut *connection)
        .await
        .expect("restore owner role");
    for (role, set_role) in [
        ("twitchbot", "SET ROLE twitchbot"),
        ("twitchlegacy", "SET ROLE twitchlegacy"),
        ("twitchanalysis", "SET ROLE twitchanalysis"),
        ("twitchaudit", "SET ROLE twitchaudit"),
    ] {
        sqlx::query(set_role)
            .execute(&mut *connection)
            .await
            .expect("read-only role");
        if role != "twitchlegacy" {
            let readable: bool = sqlx::query_scalar("SELECT external_ai_consent FROM public.social_ai_user_consent WHERE twitch_user_id='fixture'")
                .fetch_one(&mut *connection).await.expect("legitimate consent read");
            assert!(readable);
        }
        let denied = sqlx::query("UPDATE public.social_ai_user_consent SET external_ai_consent=false WHERE twitch_user_id='fixture'")
            .execute(&mut *connection).await.expect_err("read-only role cannot grant or revoke consent");
        assert_eq!(
            denied
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("42501")
        );
        sqlx::query("RESET ROLE")
            .execute(&mut *connection)
            .await
            .expect("restore owner role");
    }
    let persisted: bool = sqlx::query_scalar(
        "SELECT external_ai_consent FROM public.social_ai_user_consent WHERE twitch_user_id='fixture'",
    )
    .fetch_one(&mut *connection)
    .await
    .expect("unauthorized writes did not change consent");
    assert!(persisted);
    drop(connection);
    sqlx::query("DROP TABLE public.social_ai_user_consent")
        .execute(pool)
        .await
        .expect("remove optional fixture table");
    apply_runtime_roles(&database);
    apply_runtime_roles(&database);
}

#[tokio::test]
async fn invalid_reservation_data_blocks_new_attempts() {
    let database = DisposablePostgres::start().await;
    let pool = &database.pool;
    let context = EditorContext {
        twitch_user_id: "person".into(),
        job_id: "job".into(),
        stage: "caption".into(),
    };
    sqlx::query("INSERT INTO public.social_ai_user_consent(twitch_user_id, external_ai_consent) VALUES ($1, true)")
        .bind(&context.twitch_user_id).execute(pool).await.expect("user consent");
    sqlx::query("INSERT INTO public.social_media_settings(key,value) VALUES ('external_llm_consent','true'::jsonb)")
        .execute(pool).await.expect("global consent");
    sqlx::query("INSERT INTO public.llm_usage(ts,purpose,provider,attempt_state,reserved_micro_usd) VALUES ('test','social_editor','anthropic','legacy',NULL)")
        .execute(pool).await.expect("legacy row");
    assert_eq!(
        begin_attempt(pool, &config().provider, &context)
            .await
            .err()
            .map(|error| error.kind),
        Some(ErrorKind::Ledger)
    );
}

#[tokio::test]
async fn structured_provider_codes_are_bounded_and_saved_without_messages() {
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };
    let database = DisposablePostgres::start().await;
    let pool = &database.pool;
    let context = EditorContext {
        twitch_user_id: "person".into(),
        job_id: "job".into(),
        stage: "caption".into(),
    };
    sqlx::query("INSERT INTO public.social_ai_user_consent(twitch_user_id,external_ai_consent) VALUES ($1,true)")
        .bind(&context.twitch_user_id).execute(pool).await.expect("user consent");
    sqlx::query("INSERT INTO public.social_media_settings(key,value) VALUES ('external_llm_consent','true'::jsonb)")
        .execute(pool).await.expect("global consent");
    let mock = MockServer::start().await;
    let mut configuration = config();
    configuration.provider.endpoint = mock.uri();
    for (code, message) in [
        ("invalid_json_schema", "private input one"),
        ("token_limit_exceeded", "private input two"),
    ] {
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({"type":"error","error":{"type":"invalid_request_error","code":code,"message":message}})))
            .expect(1)
            .mount(&mock)
            .await;
        let error = complete_with_key(pool, &context, &configuration, &request(), "fake-key")
            .await
            .expect_err("structured provider failure");
        assert_eq!(error.kind, ErrorKind::Provider);
        assert_eq!(error.http_status, Some(400));
        assert_eq!(
            error.provider_error_code.as_deref(),
            Some(format!("invalid_request_error/{code}").as_str())
        );
        let saved: String =
            sqlx::query_scalar("SELECT error_code FROM public.llm_usage WHERE id=$1")
                .bind(error.ledger_id.expect("ledger id"))
                .fetch_one(pool)
                .await
                .expect("ledger code");
        assert_eq!(saved, format!("Provider/invalid_request_error/{code}"));
        assert!(!saved.contains(message));
        mock.reset().await;
    }
    configuration.provider.max_input_tokens = 16_000;
    let invalid = json!({"model":configuration.provider.model,"stop_reason":"end_turn","content":[{"type":"text","text":"{\"unexpected\":true}"}],"usage":{"input_tokens":100,"output_tokens":20,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}});
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(invalid))
        .expect(1)
        .mount(&mock)
        .await;
    let semantic = complete_with_key(pool, &context, &configuration, &request(), "fake-key")
        .await
        .expect_err("response missing required title");
    assert_eq!(semantic.kind, ErrorKind::InvalidResponse);
    assert_eq!(
        semantic.usage.as_ref().and_then(|usage| usage.input_tokens),
        Some(100)
    );
    assert!(semantic.estimated_micro_usd.is_some());
    let saved_estimate: Option<i64> =
        sqlx::query_scalar("SELECT estimated_micro_usd FROM public.llm_usage WHERE id=$1")
            .bind(semantic.ledger_id.expect("ledger id"))
            .fetch_one(pool)
            .await
            .expect("semantic failure ledger");
    assert_eq!(saved_estimate, semantic.estimated_micro_usd);
    mock.reset().await;
    let oversized_usage = json!({"model":configuration.provider.model,"stop_reason":"end_turn","content":[{"type":"text","text":"{\"title\":\"clip\"}"}],"usage":{"input_tokens":100,"output_tokens":1000000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}});
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(oversized_usage))
        .expect(1)
        .mount(&mock)
        .await;
    let overrun = complete_with_key(pool, &context, &configuration, &request(), "fake-key")
        .await
        .expect_err("reported cost exceeds reservation");
    assert_eq!(overrun.kind, ErrorKind::InvalidResponse);
    assert!(overrun.estimated_micro_usd > overrun.reserved_micro_usd);
    assert_eq!(overrun.cost_basis, Some("usage_priced_estimate"));
    assert_eq!(
        begin_attempt(pool, &configuration.provider, &context)
            .await
            .err()
            .map(|error| error.kind),
        Some(ErrorKind::Budget)
    );
    let malicious = json!({"error":{"type":"invalid_request_error","code":"invalid\nprivate input","message":"private input"}});
    assert_eq!(
        provider_error_code(malicious.to_string().as_bytes()).as_deref(),
        Some("invalid_request_error")
    );
    let unsafe_id = json!({"id":"private input\nsecond line"});
    assert!(response_request_id(unsafe_id.to_string().as_bytes(), None).is_none());
}

#[tokio::test]
async fn failed_finalization_blocks_reservations_until_exact_recovery() {
    let database = DisposablePostgres::start().await;
    let pool = &database.pool;
    let context = EditorContext {
        twitch_user_id: "person".into(),
        job_id: "job".into(),
        stage: "caption".into(),
    };
    sqlx::query("INSERT INTO public.social_ai_user_consent(twitch_user_id,external_ai_consent) VALUES ($1,true)")
        .bind(&context.twitch_user_id).execute(pool).await.expect("user consent");
    sqlx::query("INSERT INTO public.social_media_settings(key,value) VALUES ('external_llm_consent','true'::jsonb)")
        .execute(pool).await.expect("global consent");
    let mut config = config().provider;
    config.max_input_tokens = 16_000;
    let reservation = config
        .pricing
        .reservation(16_000, u64::from(config.max_output_tokens))
        .expect("reservation");
    let usage = Usage {
        input_tokens: Some(100),
        output_tokens: Some(10_000),
        cache_creation_input_tokens: Some(17),
        cache_read_input_tokens: Some(23),
    };
    let estimate = usage.estimated_cost(&config.pricing).expect("priced usage");
    assert!(estimate > reservation);
    config.initial_total_cap_micro_usd = estimate + reservation - 1;
    config.validate().expect("approved ceiling");
    let attempt = begin_attempt(pool, &config, &context)
        .await
        .expect("reservation before failure");
    let completed = completion(
        attempt.id,
        reservation,
        AttemptOutcome {
            status: Some(StatusCode::OK),
            request_id: Some("request-test"),
            usage: Some(&usage),
            estimated: Some(estimate),
            latency_ms: 11,
            kind: Some(ErrorKind::InvalidResponse),
            provider_code: None,
        },
    );
    sqlx::raw_sql("CREATE FUNCTION public.fail_social_finalization() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic finalization failure'; END $$; CREATE TRIGGER fail_social_finalization BEFORE UPDATE ON public.llm_usage FOR EACH ROW EXECUTE FUNCTION public.fail_social_finalization()")
        .execute(pool).await.expect("install database failure");
    assert_eq!(
        attempt
            .finalize(pool, &completed)
            .await
            .err()
            .map(|e| e.kind),
        Some(ErrorKind::Ledger)
    );
    crate::ledger::journal_completion(&completed);
    let record = crate::ledger::Recovery {
        project: "social_test".into(),
        service: "connector_test".into(),
        completion: completed.clone(),
    };
    let json = serde_json::to_string(&record).expect("journal payload");
    let replay: crate::ledger::Recovery = serde_json::from_str(&json).expect("journal recovery");
    assert_eq!(
        replay.completion.social_accounting,
        completed.social_accounting
    );
    assert!(!json.contains("private input"));
    let row: (String, i64, Option<i64>) = sqlx::query_as("SELECT attempt_state,reserved_micro_usd,estimated_micro_usd FROM public.llm_usage WHERE id=$1")
        .bind(completed.id).fetch_one(pool).await.expect("still unresolved");
    assert_eq!(row, ("started".into(), reservation, None));
    assert_eq!(
        begin_attempt(pool, &config, &context)
            .await
            .err()
            .map(|e| e.kind),
        Some(ErrorKind::Ledger)
    );
    sqlx::raw_sql("DROP TRIGGER fail_social_finalization ON public.llm_usage; DROP FUNCTION public.fail_social_finalization()")
        .execute(pool).await.expect("restore database finalization");
    let (project, service) = crate::ledger::identity().expect("ledger origin");
    let historical = format!(
        r#"{{"project":"{project}","service":"{service}","completion":{{"id":{},"tokens_in":100,"tokens_out":10000,"total":10100,"request_id":"request-test","success":false,"error_code":"InvalidResponse","http_status":200,"latency_ms":11}}}}"#,
        completed.id
    );
    let generic: crate::ledger::Recovery =
        serde_json::from_str(&historical).expect("historical generic Social payload");
    assert!(generic.completion.social_accounting.is_none());
    assert!(
        crate::ledger::recover_with_pool(pool, &generic.completion, project, service)
            .await
            .is_err()
    );
    let held: (String, i64, Option<i64>) = sqlx::query_as(
        "SELECT attempt_state,reserved_micro_usd,estimated_micro_usd FROM public.llm_usage WHERE id=$1",
    )
    .bind(completed.id)
    .fetch_one(pool)
    .await
    .expect("incomplete replay cannot finalize the row");
    assert_eq!(held, ("started".into(), reservation, None));
    assert_eq!(
        begin_attempt(pool, &config, &context)
            .await
            .err()
            .map(|e| e.kind),
        Some(ErrorKind::Ledger)
    );
    crate::ledger::recover_with_pool(pool, &replay.completion, project, service)
        .await
        .expect("recovery");
    crate::ledger::recover_with_pool(pool, &replay.completion, project, service)
        .await
        .expect("idempotent replay");
    let persisted: (String, i64, i64, String, i64, i64, i64, i64, String) = sqlx::query_as("SELECT attempt_state,reserved_micro_usd,estimated_micro_usd,cost_basis,cache_creation_input_tokens,cache_read_input_tokens,tokens_in,tokens_out,request_id FROM public.llm_usage WHERE id=$1")
        .bind(completed.id).fetch_one(pool).await.expect("recovered accounting");
    assert_eq!(
        persisted,
        (
            "failed".into(),
            reservation,
            estimate,
            "usage_priced_estimate".into(),
            17,
            23,
            100,
            10_000,
            "request-test".into()
        )
    );
    assert!(
        crate::ledger::recover_with_pool(pool, &generic.completion, project, service)
            .await
            .is_err()
    );
    assert_eq!(
        begin_attempt(pool, &config, &context)
            .await
            .err()
            .map(|e| e.kind),
        Some(ErrorKind::Budget)
    );
    config.initial_total_cap_micro_usd = estimate + reservation;
    config.validate().expect("test cap within approved ceiling");
    let next = begin_attempt(pool, &config, &context)
        .await
        .expect("exact remaining budget permits next reservation");
    assert_ne!(next.id, completed.id);
    drop(next);
    let mut conflicting = completed;
    conflicting
        .social_accounting
        .as_mut()
        .expect("social accounting")
        .estimated_micro_usd = Some(estimate + 1);
    assert!(
        crate::ledger::recover_with_pool(pool, &conflicting, project, service)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn orphaned_unknown_usage_stays_reserved_until_recovered() {
    let database = DisposablePostgres::start().await;
    let pool = &database.pool;
    let context = EditorContext {
        twitch_user_id: "person".into(),
        job_id: "job".into(),
        stage: "caption".into(),
    };
    sqlx::query("INSERT INTO public.social_ai_user_consent(twitch_user_id,external_ai_consent) VALUES ($1,true)")
        .bind(&context.twitch_user_id).execute(pool).await.expect("user consent");
    sqlx::query("INSERT INTO public.social_media_settings(key,value) VALUES ('external_llm_consent','true'::jsonb)")
        .execute(pool).await.expect("global consent");
    let config = config().provider;
    let reservation = config
        .pricing
        .reservation(
            u64::from(config.max_input_tokens),
            u64::from(config.max_output_tokens),
        )
        .expect("reservation");
    let attempt = begin_attempt(pool, &config, &context)
        .await
        .expect("reservation");
    let completed = completion(
        attempt.id,
        reservation,
        AttemptOutcome {
            status: None,
            request_id: None,
            usage: Some(&Usage {
                input_tokens: Some(12),
                output_tokens: Some(3),
                cache_creation_input_tokens: Some(4),
                cache_read_input_tokens: None,
            }),
            estimated: None,
            latency_ms: 12,
            kind: Some(ErrorKind::Transport),
            provider_code: None,
        },
    );
    drop(attempt);
    assert_eq!(
        begin_attempt(pool, &config, &context)
            .await
            .err()
            .map(|e| e.kind),
        Some(ErrorKind::Ledger)
    );
    let (project, service) = crate::ledger::identity().expect("ledger origin");
    crate::ledger::recover_with_pool(pool, &completed, project, service)
        .await
        .expect("reconcile unknown usage");
    let saved: (String, i64, Option<i64>, String, Option<i64>, Option<i64>) = sqlx::query_as("SELECT attempt_state,reserved_micro_usd,estimated_micro_usd,cost_basis,cache_creation_input_tokens,cache_read_input_tokens FROM public.llm_usage WHERE id=$1")
        .bind(completed.id).fetch_one(pool).await.expect("unknown usage remains reserved");
    assert_eq!(
        saved,
        (
            "failed".into(),
            reservation,
            None,
            "reserved_upper_bound".into(),
            Some(4),
            None
        )
    );
    let next = begin_attempt(pool, &config, &context)
        .await
        .expect("reconciled account permits reservation within cap");
    drop(next);
}

#[tokio::test]
async fn legacy_recovery_payload_remains_readable() {
    let database = DisposablePostgres::start().await;
    let pool = &database.pool;
    let (project, service) = crate::ledger::identity().expect("ledger origin");
    let id: i64 = sqlx::query_scalar("INSERT INTO public.llm_usage(ts,source,purpose,model,project,service,provider,attempt_state) VALUES ('test','twitch-bot','legacy','model',$1,$2,'fireworks','started') RETURNING id")
        .bind(project).bind(service).fetch_one(pool).await.expect("legacy attempt");
    let raw = format!(
        r#"{{"project":"{project}","service":"{service}","completion":{{"id":{id},"tokens_in":10,"tokens_out":5,"total":15,"request_id":null,"success":true,"error_code":null,"http_status":200,"latency_ms":3}}}}"#
    );
    let record: crate::ledger::Recovery = serde_json::from_str(&raw).expect("historical payload");
    assert!(record.completion.social_accounting.is_none());
    crate::ledger::recover_with_pool(pool, &record.completion, project, service)
        .await
        .expect("legacy recovery");
    crate::ledger::recover_with_pool(pool, &record.completion, project, service)
        .await
        .expect("legacy replay");
    let saved: (String, Option<i64>, i64) = sqlx::query_as(
        "SELECT attempt_state,estimated_micro_usd,total FROM public.llm_usage WHERE id=$1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("legacy accounting");
    assert_eq!(saved, ("succeeded".into(), None, 15));
}

#[tokio::test]
async fn consent_budget_and_native_mock_contract() {
    use wiremock::{
        matchers::{body_json, method, path},
        Mock, MockServer, ResponseTemplate,
    };
    let database = DisposablePostgres::start().await;
    let pool = &database.pool;
    let context = EditorContext {
        twitch_user_id: "person".into(),
        job_id: "job".into(),
        stage: "caption".into(),
    };
    let mut configuration = config();
    let mock = MockServer::start().await;
    configuration.provider.endpoint = mock.uri();
    let prompt = request();
    let response = json!({"model":configuration.provider.model,"stop_reason":"end_turn","content":[{"type":"text","text":"{\"title\":\"clip\"}"}],"usage":{"input_tokens":100,"output_tokens":20,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}});
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_json(
            build_body(&configuration.provider, &prompt).expect("body"),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
        .expect(1)
        .mount(&mock)
        .await;

    let denied = complete_with_key(pool, &context, &configuration, &prompt, "fake-key")
        .await
        .expect_err("missing consent");
    assert_eq!(denied.kind, ErrorKind::Consent);
    sqlx::query("INSERT INTO public.social_ai_user_consent(twitch_user_id,external_ai_consent) VALUES ($1,true)")
        .bind(&context.twitch_user_id).execute(pool).await.expect("person consent");
    let denied = complete_with_key(pool, &context, &configuration, &prompt, "fake-key")
        .await
        .expect_err("missing global consent");
    assert_eq!(denied.kind, ErrorKind::Consent);
    sqlx::query("INSERT INTO public.social_media_settings(key,value) VALUES ('external_llm_consent','false'::jsonb)")
        .execute(pool).await.expect("global consent disabled");
    let denied = complete_with_key(pool, &context, &configuration, &prompt, "fake-key")
        .await
        .expect_err("global consent false");
    assert_eq!(denied.kind, ErrorKind::Consent);
    sqlx::query("UPDATE public.social_media_settings SET value='true'::jsonb WHERE key='external_llm_consent'")
        .execute(pool).await.expect("global consent enabled");
    sqlx::query("UPDATE public.social_ai_user_consent SET external_ai_consent=false WHERE twitch_user_id=$1")
        .bind(&context.twitch_user_id).execute(pool).await.expect("user consent disabled");
    let denied = complete_with_key(pool, &context, &configuration, &prompt, "fake-key")
        .await
        .expect_err("user consent false");
    assert_eq!(denied.kind, ErrorKind::Consent);
    sqlx::query(
        "UPDATE public.social_ai_user_consent SET external_ai_consent=true WHERE twitch_user_id=$1",
    )
    .bind(&context.twitch_user_id)
    .execute(pool)
    .await
    .expect("user consent enabled");

    let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            begin_attempt(pool, &configuration.provider, &context),
            begin_attempt(pool, &configuration.provider, &context)
        )
    })
    .await
    .expect("one-connection concurrent reservations complete");
    let first = first.expect("first reservation");
    let second = second.expect("second reservation");
    let third = begin_attempt(pool, &configuration.provider, &context)
        .await
        .err()
        .expect("atomic cap");
    assert_eq!(third.kind, ErrorKind::Budget);
    let first_id = first.id;
    let second_id = second.id;
    let completed = completion(
        first_id,
        configuration
            .provider
            .pricing
            .reservation(
                u64::from(configuration.provider.max_input_tokens),
                u64::from(configuration.provider.max_output_tokens),
            )
            .expect("reservation"),
        AttemptOutcome {
            status: None,
            request_id: None,
            usage: None,
            estimated: None,
            latency_ms: 1,
            kind: Some(ErrorKind::Transport),
            provider_code: None,
        },
    );
    first
        .finalize(pool, &completed)
        .await
        .expect("ambiguous failure");
    let still_denied = begin_attempt(pool, &configuration.provider, &context)
        .await
        .err()
        .expect("ambiguous spend retained");
    assert_eq!(still_denied.kind, ErrorKind::Budget);
    drop(second);
    sqlx::query("DELETE FROM public.llm_usage WHERE id=$1 OR id=$2")
        .bind(first_id)
        .bind(second_id)
        .execute(pool)
        .await
        .expect("remove synthetic reservations");

    let success = complete_with_key(pool, &context, &configuration, &prompt, "fake-key")
        .await
        .expect("native response");
    assert_eq!(success.content["title"], "clip");
    assert!(success.estimated_micro_usd.is_some());
    assert_eq!(success.cost_basis, "usage_priced_estimate");
    let row = sqlx::query("SELECT attempt_state,reserved_micro_usd,estimated_micro_usd,request_id FROM public.llm_usage WHERE id=$1")
        .bind(success.ledger_id).fetch_one(pool).await.expect("ledger");
    assert_eq!(row.get::<String, _>("attempt_state"), "succeeded");
    assert!(row.get::<i64, _>("reserved_micro_usd") >= row.get::<i64, _>("estimated_micro_usd"));
    assert!(
        mock.received_requests()
            .await
            .expect("received requests")
            .len()
            == 1
    );

    let retry_server = MockServer::start().await;
    configuration.provider.endpoint = retry_server.uri();
    configuration.provider.retry_backoff_ms = 0;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .expect(1)
        .mount(&retry_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&retry_server)
        .await;
    let retried = complete_with_key(pool, &context, &configuration, &prompt, "fake-key")
        .await
        .expect("retry succeeds");
    let previous: (String, Option<i64>) = sqlx::query_as(
        "SELECT attempt_state,estimated_micro_usd FROM public.llm_usage WHERE id=$1",
    )
    .bind(retried.ledger_id - 1)
    .fetch_one(pool)
    .await
    .expect("failed attempt");
    assert_eq!(previous.0, "failed");
    assert_eq!(previous.1, None);
    assert_eq!(
        retry_server
            .received_requests()
            .await
            .expect("retry requests")
            .len(),
        2
    );
}
