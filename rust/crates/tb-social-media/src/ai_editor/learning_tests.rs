use super::*;

use crate::ai_editor::skill_snapshot;
use crate::test_support::postgres;

fn event(root: i64, job: i64, kind: &str, feedback: Option<&str>) -> FeedbackEvent {
    FeedbackEvent {
        root_job_id: root,
        job_id: job,
        kind: kind.into(),
        feedback: feedback.map(Into::into),
    }
}

fn smaller_subtitles(root: i64) -> Vec<FeedbackEvent> {
    vec![
        event(
            root,
            root + 1,
            "feedback",
            Some("Mach die Untertitel kleiner."),
        ),
        event(root, root + 1, "version_accepted", None),
    ]
}

#[test]
fn feedback_sequence_that_restores_the_original_learns_nothing() {
    let mut events = Vec::new();
    for root in [10, 20, 30] {
        events.push(event(
            root,
            root + 1,
            "feedback",
            Some("Die Untertitel bitte kleiner"),
        ));
        events.push(event(
            root,
            root + 2,
            "feedback",
            Some("Untertitel doch wieder wie vorher"),
        ));
        events.push(event(root, root + 2, "version_accepted", None));
    }
    let profile = personal_profile(&events);
    assert!(profile.preferences.is_empty());
    assert_eq!(profile.feedback_count, 6);
    assert_eq!(profile.accepted_versions, 3);
}

#[test]
fn single_instruction_is_not_a_permanent_preference() {
    assert!(personal_profile(&smaller_subtitles(10))
        .preferences
        .is_empty());
}

#[test]
fn repeated_confirmed_corrections_become_personal_preferences() {
    let events: Vec<_> = [10, 20].into_iter().flat_map(smaller_subtitles).collect();
    let profile = personal_profile(&events);
    assert_eq!(
        profile.preferences,
        vec![PersonalPreference {
            dimension: Dimension::SubtitleSize,
            direction: -1,
            supporting_reviews: 2,
            opposing_reviews: 0,
        }]
    );
    assert_eq!(
        planning_hints(&profile),
        vec!["Prefer a compact subtitle size that keeps gameplay visible.".to_owned()]
    );
}

#[test]
fn rejected_edits_do_not_count_as_preferences() {
    let events = vec![
        event(10, 11, "feedback", Some("Intro kürzer")),
        event(10, 11, "version_rejected", None),
        event(20, 21, "feedback", Some("Intro kürzer")),
        event(20, 21, "proposal_rejected", None),
    ];
    assert!(personal_profile(&events).preferences.is_empty());
}

#[test]
fn contradicting_reviews_block_a_preference() {
    let mut events = Vec::new();
    for (root, text) in [
        (10, "Untertitel kleiner"),
        (20, "Untertitel kleiner"),
        (30, "Untertitel größer"),
        (40, "Untertitel größer"),
    ] {
        events.push(event(root, root + 1, "feedback", Some(text)));
        events.push(event(root, root + 1, "version_accepted", None));
    }
    assert!(personal_profile(&events).preferences.is_empty());
}

#[test]
fn pending_feedback_is_not_evidence() {
    let events = vec![
        event(10, 11, "feedback", Some("Untertitel kleiner")),
        event(20, 21, "feedback", Some("Untertitel kleiner")),
        event(30, 31, "feedback", Some("Untertitel kleiner")),
    ];
    assert!(personal_profile(&events).preferences.is_empty());
}

#[test]
fn accepted_then_rejected_version_does_not_count() {
    let mut events: Vec<_> = [10, 20].into_iter().flat_map(smaller_subtitles).collect();
    events.push(event(20, 21, "version_rejected", None));
    assert!(personal_profile(&events).preferences.is_empty());
}

#[test]
fn negated_instructions_teach_nothing() {
    for text in [
        "Untertitel nicht kleiner",
        "Intro nicht kürzer",
        "Die Untertitel sind nicht zu groß",
        "Don't make the subtitles smaller",
        "Keine schnelleren Schnitte, nicht mehr Schnitte",
        "Die Facecam verdeckt nichts",
        "Teamfight nicht komplett zeigen",
        "Reaktion nicht drin lassen",
        "Die Fähigkeit wurde nicht falsch verstanden",
        "Don't cut the video shorter",
        "Das Intro nicht kürzen",
        "Die Untertitel nicht schneiden und kleiner machen",
        "Reaktion nicht schneiden und nicht behalten",
        "Untertitel keinesfalls kleiner",
        "Untertitel keineswegs größer",
        "Auf keinen Fall ein kürzeres Intro",
        "Untertitel niemals kleiner",
        "Bitte vermeide kleinere Untertitel",
        "Hör auf, die Untertitel kleiner zu machen",
        "Stop making the subtitles smaller",
        "Subtitles shouldn’t be smaller",
        "Never make the intro shorter",
    ] {
        assert!(classify(text).is_empty(), "{text}");
    }
    assert_eq!(
        classify("Die Reaktion bitte nicht rausschneiden"),
        vec![Signal::Toward(Dimension::KeepReaction, 1)]
    );
    assert!(classify("Untertitel kleiner, nicht größer").is_empty());
    assert_eq!(
        classify("Untertitel kleiner. Intro nicht kürzer"),
        vec![Signal::Toward(Dimension::SubtitleSize, -1)]
    );
}

#[test]
fn global_rules_need_several_agreeing_users() {
    let preference = |direction| PersonalProfile {
        preferences: vec![PersonalPreference {
            dimension: Dimension::IntroLength,
            direction,
            supporting_reviews: 2,
            opposing_reviews: 0,
        }],
        ..PersonalProfile::default()
    };
    assert!(global_rules(&[preference(-1), preference(-1)]).is_empty());
    assert_eq!(
        global_rules(&[
            preference(-1),
            preference(-1),
            preference(-1),
            preference(1)
        ]),
        vec![GlobalRule {
            dimension: Dimension::IntroLength,
            direction: -1,
            supporting_users: 3,
            opposing_users: 1,
        }]
    );
    assert!(global_rules(&[
        preference(-1),
        preference(-1),
        preference(-1),
        preference(1),
        preference(1)
    ])
    .is_empty());
}

#[test]
fn composed_skill_text_replaces_only_the_learned_section() {
    let base = "# Subtitles v1\n\nUse only supplied speech.\n";
    let rule = GlobalRule {
        dimension: Dimension::SubtitleSize,
        direction: -1,
        supporting_users: 3,
        opposing_users: 0,
    };
    let learned = compose_skill_text(base, &[&rule]);
    assert_eq!(
        learned,
        "# Subtitles v1\n\nUse only supplied speech.\n\n## Learned guidance\n\n- Prefer a compact subtitle size that keeps gameplay visible.\n"
    );
    assert_eq!(compose_skill_text(&learned, &[&rule]), learned);
    assert_eq!(compose_skill_text(&learned, &[]), base);
}

async fn database() -> postgres::TestPostgres {
    let db = postgres::TestPostgres::start().await;
    sqlx::raw_sql("CREATE TABLE public.social_ai_revision_events (id BIGSERIAL PRIMARY KEY, twitch_user_id BIGINT NOT NULL, root_job_id BIGINT NOT NULL, job_id BIGINT NOT NULL, produced_video_id BIGINT, event_kind TEXT NOT NULL, payload JSONB NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())")
        .execute(&db.pool)
        .await
        .expect("fixture revision events");
    for sql in [
        include_str!("../../../../migrations/20261011204100_social_ai_skill_versions.sql"),
        include_str!("../../../../migrations/20261011204200_social_ai_personal_preferences.sql"),
    ] {
        sqlx::raw_sql(sql)
            .execute(&db.pool)
            .await
            .expect("real learning migrations");
    }
    db
}

async fn insert(pool: &PgPool, user: i64, root: i64, job: i64, kind: &str, feedback: Option<&str>) {
    let payload = match feedback {
        Some(text) => serde_json::json!({ "feedback": text }),
        None => serde_json::json!({}),
    };
    sqlx::query("INSERT INTO public.social_ai_revision_events (twitch_user_id, root_job_id, job_id, event_kind, payload) VALUES ($1, $2, $3, $4, $5)")
        .bind(user)
        .bind(root)
        .bind(job)
        .bind(kind)
        .bind(payload)
        .execute(pool)
        .await
        .expect("fixture event");
}

async fn confirmed(pool: &PgPool, user: i64, feedback: &str) {
    for root in [user * 100, user * 100 + 50] {
        insert(pool, user, root, root + 1, "feedback", Some(feedback)).await;
        insert(pool, user, root, root + 1, "version_accepted", None).await;
    }
}

#[tokio::test]
async fn recurring_feedback_creates_versioned_skill_change_and_respects_rollback() {
    let db = database().await;
    let pool = &db.pool;
    for user in [101, 202] {
        confirmed(pool, user, "Untertitel kleiner bitte").await;
    }
    let mut holder = pool.begin().await.expect("lock holder");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(PASS_LOCK)
        .execute(&mut *holder)
        .await
        .expect("hold pass lock");
    assert_eq!(
        run_pass(pool, 4096).await.expect("concurrent pass"),
        PassReport::default()
    );
    holder.rollback().await.expect("release pass lock");
    let first = run_pass(pool, 4096).await.expect("first pass");
    assert_eq!(first.profiles_updated, 2);
    assert!(first.skills_changed.is_empty());
    assert_eq!(
        personal_hints(pool, "101").await.expect("hints"),
        vec!["Prefer a compact subtitle size that keeps gameplay visible.".to_owned()]
    );
    assert!(personal_hints(pool, "303")
        .await
        .expect("no hints")
        .is_empty());

    confirmed(pool, 303, "Die Untertitel sind zu groß").await;
    for user in [101, 202, 303] {
        confirmed(pool, user, "Die Facecam verdeckt den Teamfight").await;
    }
    let second = run_pass(pool, 4096).await.expect("second pass");
    assert_eq!(second.profiles_updated, 3);
    assert_eq!(
        second.skills_changed,
        vec![(SkillId::VideoLayout, 2), (SkillId::Subtitles, 2)]
    );
    assert_eq!(second.proposals_recorded, 1);

    let active = skill_registry::active_skill(pool, SkillId::Subtitles, 4096)
        .await
        .expect("active subtitles");
    assert_eq!(active.version, 2);
    assert!(active
        .text
        .starts_with(skill_snapshot(SkillId::Subtitles).text.trim_end()));
    assert!(active.text.ends_with(
        "## Learned guidance\n\n- Prefer a compact subtitle size that keeps gameplay visible.\n"
    ));

    let history = skill_registry::history(pool).await.expect("history");
    let subtitles = history
        .iter()
        .find(|skill| skill.skill_id == "subtitles")
        .expect("subtitle history");
    let learned = &subtitles.versions[0];
    assert_eq!(learned.previous_version.as_deref(), Some("1"));
    assert_eq!(learned.summary, "Kompaktere Untertitel bevorzugt");
    let evidence = learned.evidence.as_ref().expect("evidence");
    assert_eq!(evidence.sample_count, Some(3));
    assert!(!evidence.summary.contains("101"));
    assert!(learned
        .diff
        .iter()
        .any(|line| line.kind == skill_registry::DiffKind::Added
            && line.text == "- Prefer a compact subtitle size that keeps gameplay visible."));

    let third = run_pass(pool, 4096).await.expect("idempotent pass");
    assert_eq!(third, PassReport::default());

    skill_registry::rollback(pool, SkillId::Subtitles, 2, 1, "Admin prüft", "admin")
        .await
        .expect("rollback");
    insert(pool, 404, 1, 2, "feedback", Some("Intro kürzer")).await;
    let after_rollback = run_pass(pool, 4096).await.expect("pass after rollback");
    assert_eq!(after_rollback.profiles_updated, 1);
    assert!(after_rollback.skills_changed.is_empty());
    let restored = skill_registry::active_skill(pool, SkillId::Subtitles, 4096)
        .await
        .expect("restored subtitles");
    assert_eq!(restored.version, 1);
    let stale = skill_registry::activate_learned(
        pool,
        SkillId::Subtitles,
        skill_registry::LearnedChange {
            text: &format!("{}\n\nStale.\n", restored.text.trim_end()),
            summary: "Veraltet",
            reason: "Veraltet",
            evidence: None,
            actor: "ai-learning",
            max_bytes: 4096,
            expected_version: Some(2),
        },
    )
    .await;
    assert_eq!(stale, Err(SkillRegistryError::Conflict));

    let proposals = skill_registry::proposals(pool).await.expect("proposals");
    assert_eq!(proposals.len(), 1);
    assert_eq!(
        proposals[0].title,
        "Facecam-Überdeckung automatisch erkennen"
    );
    assert_eq!(proposals[0].status, "proposed");
}

#[test]
fn planning_input_carries_personal_preferences_only_when_present() {
    let empty = super::super::pipeline::planning_text(serde_json::json!({"task": "x"}), &[]);
    assert_eq!(empty, r#"{"task":"x"}"#);
    let with = super::super::pipeline::planning_text(
        serde_json::json!({"task": "x"}),
        &["Prefer fewer, calmer cuts.".to_owned()],
    );
    let value: Value = serde_json::from_str(&with).expect("json");
    assert_eq!(
        value["personal_preferences"]["preferences"][0],
        "Prefer fewer, calmer cuts."
    );
}
