use super::*;
use admin_limits::{GlobalControlChange, LimitError, PlanLimitChange, UserLimitChange};
use admin_usage::{UsageFilter, UsageGrouping};

pub(super) async fn assert_limit_roles(pool: &PgPool) {
    for table in [
        "social_ai_processing_control",
        "social_ai_user_limits",
        "social_ai_plan_limit_drafts",
    ] {
        for role in [
            "twitchdash",
            "twitchbot",
            "twitchanalysis",
            "twitchaudit",
            "twitchlegacy",
            "twitchcontest",
            "twitchcollector",
        ] {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT FROM pg_roles WHERE rolname=$1)")
                    .bind(role)
                    .fetch_one(pool)
                    .await
                    .expect("role presence");
            if !exists {
                continue;
            }
            let rights: (bool, bool, bool, bool) = sqlx::query_as("SELECT has_table_privilege($1,$2,'SELECT'),has_table_privilege($1,$2,'INSERT'),has_table_privilege($1,$2,'UPDATE'),has_table_privilege($1,$2,'DELETE,TRUNCATE,REFERENCES,TRIGGER')")
                .bind(role).bind(format!("public.{table}")).fetch_one(pool).await.expect("exact limits ACL");
            let dash = role == "twitchdash";
            let reader = table != "social_ai_plan_limit_drafts"
                && ["twitchbot", "twitchanalysis", "twitchaudit"].contains(&role);
            assert_eq!(
                rights,
                (dash || reader, dash, dash, false),
                "{table} {role}"
            );
        }
    }
    let exposed: bool = sqlx::query_scalar("SELECT has_function_privilege('twitchanalysis','public.twitch_apply_social_ai_limit_roles()','EXECUTE')").fetch_one(pool).await.expect("helper ACL");
    assert!(!exposed);
}

async fn consent(pool: &PgPool) {
    sqlx::raw_sql("INSERT INTO public.twitch_streamers VALUES ('101'); INSERT INTO public.social_ai_user_consent(twitch_user_id,external_ai_consent,updated_by) VALUES ('101',true,'fixture'); INSERT INTO public.social_media_settings VALUES ('external_llm_consent','true'::jsonb)")
        .execute(pool).await.expect("consented synthetic owner");
}

fn context() -> EditorContext {
    EditorContext {
        twitch_user_id: "101".into(),
        job_id: "manual:1".into(),
        stage: "caption".into(),
    }
}

#[tokio::test]
async fn m4_pauses_conflicts_and_inactive_plan_drafts_control_real_admission() {
    let Some(database) = authorized_test_fixture::start(
        "social_editor::tests::admin_contract_tests::m4_pauses_conflicts_and_inactive_plan_drafts_control_real_admission",
        "101",
    )
    .await
    else {
        return;
    };
    let pool = &database.pool;
    consent(pool).await;
    let global = GlobalControlChange {
        expected_revision: 1,
        paused: true,
        reason: "fixture".into(),
    };
    admin_limits::save_global_control(pool, &global, "discord:42")
        .await
        .expect("pause");
    assert_eq!(
        admin_limits::save_global_control(pool, &global, "discord:43").await,
        Err(LimitError::Conflict)
    );
    let mock = wiremock::MockServer::start().await;
    let mut configuration = config();
    configuration.provider.endpoint = mock.uri();
    assert_eq!(
        complete_with_key(pool, &context(), &configuration, &request(), "synthetic")
            .await
            .err()
            .map(|e| e.kind),
        Some(ErrorKind::Budget)
    );
    assert!(mock.received_requests().await.expect("requests").is_empty());
    admin_limits::save_global_control(
        pool,
        &GlobalControlChange {
            expected_revision: 2,
            paused: false,
            reason: "fixture".into(),
        },
        "discord:42",
    )
    .await
    .expect("resume");
    let mut user = UserLimitChange {
        twitch_user_id: "101".into(),
        expected_revision: 0,
        paused: true,
        monthly_cap_micro_usd: None,
        reason: "fixture".into(),
    };
    admin_limits::save_user_limit(pool, &user, "discord:42")
        .await
        .expect("user pause");
    assert_eq!(
        begin_attempt(pool, &configuration.provider, &context())
            .await
            .err()
            .map(|e| e.kind),
        Some(ErrorKind::Budget)
    );
    assert_eq!(
        admin_limits::save_user_limit(pool, &user, "discord:43").await,
        Err(LimitError::Conflict)
    );
    user.expected_revision = 1;
    user.paused = false;
    admin_limits::save_user_limit(pool, &user, "discord:42")
        .await
        .expect("user resume");
    let catalog = [("free", "Free")];
    let plan = PlanLimitChange {
        plan_id: "free".into(),
        expected_revision: 0,
        monthly_cap_micro_usd: Some(0),
        monthly_ai_jobs: Some(0),
        monthly_revisions: Some(0),
        reason: "fixture".into(),
    };
    admin_limits::save_plan_limit(pool, &plan, "discord:42", &catalog)
        .await
        .expect("inactive draft");
    assert_eq!(
        admin_limits::save_plan_limit(pool, &plan, "discord:43", &catalog).await,
        Err(LimitError::Conflict)
    );
    let report = admin_limits::processing_limits(pool, &catalog)
        .await
        .expect("stored policies");
    assert!(!report.plan_policies_active);
    assert_eq!(report.global.initial_total_cap_micro_usd, 2_000_000);
    assert_eq!(report.users[0].revision, 2);
    assert_eq!(report.global.updated_by, "discord:42");
    let admitted = begin_attempt(pool, &configuration.provider, &context())
        .await
        .expect("draft does not enforce");
    drop(admitted);
}

#[tokio::test]
async fn m4_monthly_reservations_are_atomic_and_keep_old_unknown_higher_debits() {
    let database = DisposablePostgres::start().await;
    let concurrent = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect_with(database.pool.connect_options().as_ref().clone())
        .await
        .expect("independent admission connections");
    let pool = &concurrent;
    consent(pool).await;
    let configuration = config().provider;
    let reservation = configuration
        .pricing
        .reservation(
            u64::from(configuration.max_input_tokens),
            u64::from(configuration.max_output_tokens),
        )
        .expect("reservation");
    let policy = UserLimitChange {
        twitch_user_id: "101".into(),
        expected_revision: 0,
        paused: false,
        monthly_cap_micro_usd: Some(reservation),
        reason: "fixture".into(),
    };
    admin_limits::save_user_limit(pool, &policy, "discord:42")
        .await
        .expect("monthly cap");
    let ctx = context();
    let (first, second) = tokio::join!(
        begin_attempt(pool, &configuration, &ctx),
        begin_attempt(pool, &configuration, &ctx)
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let (admitted, denied) = if first.is_ok() {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!(denied.err().map(|e| e.kind), Some(ErrorKind::Budget));
    let admitted = admitted.expect("single reservation");
    let held = completion(
        admitted.id,
        reservation,
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
    admitted
        .finalize(pool, &held)
        .await
        .expect("unknown held debit");
    sqlx::query("UPDATE public.llm_usage SET ts='2000-01-01T00:00:00Z'")
        .execute(pool)
        .await
        .expect("older hold");
    assert_eq!(
        begin_attempt(pool, &configuration, &ctx)
            .await
            .err()
            .map(|e| e.kind),
        Some(ErrorKind::Budget)
    );
    sqlx::query("UPDATE public.llm_usage SET cost_basis='usage_priced_estimate'")
        .execute(pool)
        .await
        .expect("unknown estimate with priced label");
    let mut connection = pool.acquire().await.expect("held accounting");
    assert_eq!(
        usage::consumption_on(&mut connection, Some("101"))
            .await
            .expect("unknown estimate stays held"),
        reservation
    );
    drop(connection);
    sqlx::query("UPDATE public.llm_usage SET estimated_micro_usd=reserved_micro_usd+1,cost_basis='usage_priced_estimate',ts=to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')").execute(pool).await.expect("higher priced failure");
    let mut connection = pool.acquire().await.expect("accounting");
    assert_eq!(
        usage::consumption_on(&mut connection, Some("101"))
            .await
            .expect("higher debit"),
        reservation + 1
    );
    drop(connection);
    sqlx::query("UPDATE public.llm_usage SET estimated_micro_usd=2000000")
        .execute(pool)
        .await
        .expect("fixed shared cap");
    let policy = UserLimitChange {
        expected_revision: 1,
        monthly_cap_micro_usd: None,
        ..policy
    };
    admin_limits::save_user_limit(pool, &policy, "discord:42")
        .await
        .expect("remove monthly override");
    assert_eq!(
        begin_attempt(pool, &configuration, &ctx)
            .await
            .err()
            .map(|e| e.kind),
        Some(ErrorKind::Budget)
    );
}

#[tokio::test]
async fn m4_usage_aggregates_all_attempts_and_binds_filters_with_safe_separate_metrics() {
    let database = DisposablePostgres::start().await;
    let pool = &database.pool;
    sqlx::query("CREATE TABLE public.twitch_clips_social_media(id BIGINT PRIMARY KEY)")
        .execute(pool)
        .await
        .expect("job dependency");
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261011200002_social_ai_jobs.sql"
    ))
    .execute(pool)
    .await
    .expect("actual jobs migration");
    sqlx::query("INSERT INTO public.social_ai_jobs(id,twitch_user_id,clip_db_id,source_twitch_clip_id,status,processing_ms,rendering_ms,completed_at) VALUES (1,101,1,'synthetic','failed',18400,6800,now()),(2,202,2,'synthetic','failed',9007199254740992,1,now())").execute(pool).await.expect("separate processing source");
    sqlx::raw_sql("INSERT INTO public.llm_usage(ts,purpose,provider,model,attempt_state,social_ai_twitch_user_id,social_ai_job_id,reserved_micro_usd,estimated_micro_usd,cost_basis,tokens_in,tokens_out,cache_creation_input_tokens,cache_read_input_tokens,latency_ms) VALUES ('2026-10-01T00:00:00Z','social_editor','anthropic','model-a','failed','101','manual:1',1000,500,'usage_priced_estimate',100,20,0,10,12),('2026-10-02T00:00:00Z','social_editor','other','model-b','succeeded','101','manual:1',1000,1200,'usage_priced_estimate',150,30,3,20,18),('2026-10-03T00:00:00Z','social_editor','other','model-b','failed','202','manual:2',2000,NULL,'reserved_upper_bound',50,NULL,NULL,NULL,9)").execute(pool).await.expect("all-provider attempts");
    let report = admin_usage::usage_report(
        pool,
        &UsageFilter {
            group_by: UsageGrouping::Job,
            limit: Some(1),
            ..UsageFilter::default()
        },
    )
    .await
    .expect("paged report");
    assert_eq!(report.totals.attempts, 3);
    assert_eq!(report.totals.charged_micro_usd, Some(4200));
    assert_eq!(report.totals.held_micro_usd, Some(2000));
    assert_eq!(report.totals.estimated_micro_usd, None);
    assert_eq!(report.totals.input_tokens, Some(300));
    assert_eq!(report.totals.output_tokens, None);
    assert_eq!(report.totals.provider_latency_ms, Some(39));
    assert!(
        report.totals.measured_micro_usd.is_none() && report.totals.invoiced_micro_usd.is_none()
    );
    assert!(report.has_more);
    let group = &report.groups[0];
    assert_eq!(group.totals.attempts, 2);
    assert_eq!(group.totals.estimated_micro_usd, Some(1700));
    assert_eq!(group.totals.charged_micro_usd, Some(2200));
    assert_eq!(group.totals.cache_read_input_tokens, Some(30));
    assert_eq!(group.processing_ms, Some(18400));
    assert_eq!(group.rendering_ms, Some(6800));
    assert_eq!(group.totals.provider_latency_ms, Some(30));
    assert!(group.provider.is_none());
    let next = admin_usage::usage_report(
        pool,
        &UsageFilter {
            group_by: UsageGrouping::Job,
            offset: 1,
            limit: Some(1),
            ..UsageFilter::default()
        },
    )
    .await
    .expect("next page");
    assert!(!next.has_more);
    assert_eq!(next.groups[0].processing_ms, None);
    let filtered = admin_usage::usage_report(
        pool,
        &UsageFilter {
            provider: Some("' OR TRUE --".into()),
            ..UsageFilter::default()
        },
    )
    .await
    .expect("bound filter");
    assert_eq!(filtered.totals.attempts, 0);
    sqlx::query("UPDATE public.llm_usage SET attempt_state='started',latency_ms=0 WHERE social_ai_twitch_user_id='202'").execute(pool).await.expect("unfinished attempt");
    let report = admin_usage::usage_report(pool, &UsageFilter::default())
        .await
        .expect("unknown projections");
    assert!(report.totals.input_tokens.is_none() && report.totals.provider_latency_ms.is_none());
    let serialized = serde_json::to_string(&report).expect("dto");
    for private in [
        "source_twitch_clip_id",
        "system",
        "request_id",
        "checkpoints",
        "payload",
        "error_code",
    ] {
        assert!(!serialized.contains(private));
    }
}
