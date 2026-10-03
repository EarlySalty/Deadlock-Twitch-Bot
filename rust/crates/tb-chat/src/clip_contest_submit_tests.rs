use super::*;
use crate::types::{ChatBadge, ChatMessageBody};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

// ─── Reine Regeln ───────────────────────────────────────────────────────────

#[test]
fn clip_urls_nur_twitch_clips() {
    for (raw, slug) in [
        ("https://clips.twitch.tv/AbcDef-123_x", "AbcDef-123_x"),
        ("clips.twitch.tv/AbcDef", "AbcDef"),
        ("http://clips.twitch.tv/AbcDef/", "AbcDef"),
        ("https://clips.twitch.tv/AbcDef?tt_medium=x#y", "AbcDef"),
        ("https://www.twitch.tv/name/clip/AbcDef", "AbcDef"),
        ("twitch.tv/name/clip/AbcDef?filter=clips", "AbcDef"),
        ("https://m.twitch.tv/Name_1/clip/AbcDef", "AbcDef"),
        ("<https://clips.twitch.tv/AbcDef>", "AbcDef"),
    ] {
        assert_eq!(parse_clip_url(raw).as_deref(), Some(slug), "{raw}");
    }
    for raw in [
        "",
        "AbcDef",
        "https://clips.twitch.tv/",
        "https://clips.twitch.tv/a b",
        "https://clips.twitch.tv/Abc/Def",
        "https://evil.example/clips.twitch.tv/AbcDef",
        "https://clips.twitch.tv.evil.example/AbcDef",
        "https://www.twitch.tv/name/clips/AbcDef",
        "https://www.twitch.tv/name/videos/123",
        "https://www.twitch.tv/na-me/clip/AbcDef",
        "https://user@clips.twitch.tv/AbcDef",
        "https://clips.twitch.tv:8443/AbcDef",
        "ftp://clips.twitch.tv/AbcDef",
        "https://clips.twitch.tv/Ab",
        "https://clips.twitch.tv/Abc%20Def",
        "https://clips.twitch.tv/Ab.cd",
    ] {
        assert_eq!(parse_clip_url(raw), None, "{raw}");
    }
    assert_eq!(
        canonical_clip_url("AbcDef"),
        "https://clips.twitch.tv/AbcDef"
    );
    assert_eq!(idempotency_key("AbcDef"), "twitch-clip-AbcDef");
}

fn event(chatter: &str, badges: &[&str]) -> ChatMessageEvent {
    ChatMessageEvent {
        broadcaster_user_id: "456".into(),
        broadcaster_user_login: "name".into(),
        chatter_user_id: chatter.into(),
        chatter_user_login: "jemand".into(),
        message_id: "m1".into(),
        message: ChatMessageBody {
            text: "!clipcontest".into(),
            ..Default::default()
        },
        badges: badges
            .iter()
            .map(|b| ChatBadge {
                set_id: (*b).into(),
                id: String::new(),
                info: String::new(),
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn nur_broadcaster_und_mods_des_eigenen_kanals() {
    assert_eq!(chat_recht(&event("456", &[])), ChatRecht::Erlaubt);
    assert_eq!(chat_recht(&event("9", &["moderator"])), ChatRecht::Erlaubt);
    assert_eq!(
        chat_recht(&event("9", &["broadcaster"])),
        ChatRecht::Erlaubt
    );
    assert_eq!(chat_recht(&event("9", &[])), ChatRecht::NichtErlaubt);
    assert_eq!(chat_recht(&event("9", &["vip"])), ChatRecht::NichtErlaubt);
    assert_eq!(
        chat_recht(&event("9", &["subscriber"])),
        ChatRecht::NichtErlaubt
    );
    let mut shared = event("9", &["moderator"]);
    shared.source_broadcaster_user_id = Some("777".into());
    assert_eq!(chat_recht(&shared), ChatRecht::FremderKanal);
    shared.source_broadcaster_user_id = Some("456".into());
    assert_eq!(chat_recht(&shared), ChatRecht::Erlaubt);
}

#[test]
fn tageslimit_und_bestand() {
    assert!(!limit_erreicht(0));
    assert!(!limit_erreicht(2));
    assert!(limit_erreicht(3));
    assert!(limit_erreicht(4));
    assert!(valid_twitch_id("456"));
    for id in ["", "0456", "45a", "123456789012345678901"] {
        assert!(!valid_twitch_id(id), "{id}");
    }
    let now = Utc::now();
    assert_eq!(bestand("accepted", now, now), Bestand::SchonDrin);
    assert_eq!(bestand("duplicate", now, now), Bestand::SchonDrin);
    assert_eq!(
        bestand("pending", now - Duration::seconds(5), now),
        Bestand::Laeuft
    );
    assert_eq!(
        bestand("pending", now - Duration::seconds(PENDING_TTL_SECS), now),
        Bestand::Frei
    );
    assert_eq!(bestand("failed", now, now), Bestand::Frei);
    assert_eq!(bestand("rejected", now, now), Bestand::Frei);
}

#[test]
fn titel_und_helix_und_envelope() {
    assert_eq!(broker_title("  "), None);
    assert_eq!(broker_title(" a\nb ").as_deref(), Some("a b"));
    assert_eq!(broker_title(&"ä".repeat(300)).unwrap().chars().count(), 200);

    let body = serde_json::json!({"data": [
        {"id": "Other", "broadcaster_id": "1"},
        {"id": "AbcDef", "broadcaster_id": "456", "title": " Titel "}
    ]});
    assert_eq!(
        clip_from_helix(&body, "AbcDef"),
        Some(ClipInfo {
            id: "AbcDef".into(),
            broadcaster_id: "456".into(),
            title: "Titel".into()
        })
    );
    assert_eq!(clip_from_helix(&body, "Fehlt"), None);
    assert_eq!(
        clip_from_helix(&serde_json::json!({"data": []}), "AbcDef"),
        None
    );

    let ok = serde_json::json!({"ok": true, "result":
        {"status": "duplicate", "submission_id": 7, "reason": "duplicate_clip_this_week"}});
    assert_eq!(
        parse_broker_envelope(&ok),
        Some(BrokerClipResponse {
            status: BrokerClipStatus::Duplicate,
            submission_id: Some(7),
            reason: Some("duplicate_clip_this_week".into())
        })
    );
    assert_eq!(
        parse_broker_envelope(&serde_json::json!({"ok": false, "result": {"status": "accepted"}})),
        None
    );
    assert_eq!(
        parse_broker_envelope(&serde_json::json!({"ok": true, "result": {"status": "irgendwas"}})),
        None
    );
}

#[test]
fn broker_payload_exakt_nach_plan() {
    let request = BrokerClipRequest {
        source: "twitch".into(),
        clip_url: "https://clips.twitch.tv/AbcDef".into(),
        streamer_twitch_user_id: "456".into(),
        streamer_login: "name".into(),
        submitted_by_twitch_user_id: Some("456".into()),
        submitted_at: None,
        title: Some("Titel".into()),
        idempotency_key: "twitch-clip-AbcDef".into(),
    };
    assert_eq!(
        serde_json::to_value(&request).unwrap(),
        serde_json::json!({"source":"twitch","clip_url":"https://clips.twitch.tv/AbcDef",
            "streamer_twitch_user_id":"456","streamer_login":"name",
            "submitted_by_twitch_user_id":"456","title":"Titel",
            "idempotency_key":"twitch-clip-AbcDef"})
    );
}

#[test]
fn antworten_deutsch_ohne_gedankenstriche() {
    let outcomes = [
        SubmitOutcome::Accepted {
            clip_url: String::new(),
            submission_id: None,
        },
        SubmitOutcome::AlreadyIn {
            clip_url: String::new(),
        },
        SubmitOutcome::Rejected {
            reason: Some("not_partner".into()),
        },
        SubmitOutcome::Rejected { reason: None },
        SubmitOutcome::BrokerUnavailable,
        SubmitOutcome::InvalidUrl,
        SubmitOutcome::NoRecentClip,
        SubmitOutcome::ClipNotFound,
        SubmitOutcome::ForeignClip,
        SubmitOutcome::NotPartner,
        SubmitOutcome::RateLimited,
        SubmitOutcome::TwitchUnavailable,
        SubmitOutcome::StoreUnavailable,
    ];
    for outcome in &outcomes {
        let text = outcome.reply().expect("Antwort");
        assert!(!text.contains('–') && !text.contains('—'), "{text}");
        assert!(text.len() < 400, "{text}");
    }
    assert_eq!(
        outcomes[0].reply().unwrap(),
        "Clip ist im Wochen-Contest, die Community stimmt im Discord ab."
    );
    assert!(outcomes[2].reply().unwrap().contains("Partner"));
    assert_eq!(SubmitOutcome::InFlight.reply(), None);
    assert!(!REPLY_NOT_ALLOWED.contains('–'));
}

// ─── Gegen Wegwerf-DB ───────────────────────────────────────────────────────

struct FakeLookup(Mutex<Vec<(String, Option<ClipInfo>)>>);

#[async_trait]
impl ClipLookup for FakeLookup {
    async fn clip(&self, clip_id: &str) -> Result<Option<ClipInfo>, String> {
        let known = self.0.lock().unwrap();
        match known.iter().find(|(id, _)| id == clip_id) {
            Some((_, clip)) => Ok(clip.clone()),
            None if clip_id == "Kaputt" => Err("offline".into()),
            None => Ok(None),
        }
    }
}

struct FakeBroker {
    calls: AtomicUsize,
    requests: Mutex<Vec<BrokerClipRequest>>,
    answer: Mutex<Result<BrokerClipResponse, String>>,
    delay_ms: u64,
}

impl FakeBroker {
    fn new(answer: Result<BrokerClipResponse, String>) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
            answer: Mutex::new(answer),
            delay_ms: 0,
        })
    }
}

#[async_trait]
impl ClipContestBroker for FakeBroker {
    async fn submit(&self, request: &BrokerClipRequest) -> Result<BrokerClipResponse, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push(request.clone());
        if self.delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
        }
        self.answer.lock().unwrap().clone()
    }
}

fn accepted(id: i64) -> Result<BrokerClipResponse, String> {
    Ok(BrokerClipResponse {
        status: BrokerClipStatus::Accepted,
        submission_id: Some(id),
        reason: None,
    })
}

fn clip(id: &str, broadcaster: &str) -> (String, Option<ClipInfo>) {
    (
        id.to_string(),
        Some(ClipInfo {
            id: id.to_string(),
            broadcaster_id: broadcaster.to_string(),
            title: format!("Titel {id}"),
        }),
    )
}

fn lookup() -> Arc<FakeLookup> {
    Arc::new(FakeLookup(Mutex::new(vec![
        clip("ClipEins", "456"),
        clip("ClipZwei", "456"),
        clip("ClipDrei", "456"),
        clip("ClipVier", "456"),
        clip("FremderClip", "999"),
        clip("PipelineClip", "456"),
        clip("BefehlClip", "456"),
    ])))
}

fn request(url: Option<&str>) -> SubmitRequest {
    SubmitRequest {
        broadcaster_id: "456".into(),
        broadcaster_login: "Name".into(),
        submitted_by: Some("4242".into()),
        clip_url: url.map(str::to_string),
        via: SubmitVia::Chat,
    }
}

async fn migrated_pool(_db_name: &str) -> crate::test_postgres::TestPostgres {
    let db = crate::test_postgres::TestPostgres::start_with_timescaledb().await;
    let pool = db.pool.clone();
    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&pool)
        .await
        .expect("TimescaleDB");
    static MIGRATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = MIGRATE_LOCK.lock().await;
    tb_db::migrate::MIGRATOR
        .run(&pool)
        .await
        .expect("Migrationen");
    sqlx::raw_sql(
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status)
             VALUES ('456', 'name', 'active'), ('457', 'pausiert', 'archived');",
    )
    .execute(&pool)
    .await
    .unwrap();
    db
}

async fn status_of(pool: &PgPool, clip_id: &str) -> Option<(String, String, Option<String>)> {
    sqlx::query_as(
        "SELECT status, via, submitted_by_twitch_id FROM twitch_clip_contest_forwards WHERE clip_id = $1",
    )
    .bind(clip_id)
    .fetch_optional(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn einreichung_angenommen_dann_schon_drin_ohne_zweiten_brokeraufruf() {
    let _db = migrated_pool("tb_clipcontest_flow").await;
    let pool = _db.pool.clone();
    let broker = FakeBroker::new(accepted(123));
    let submitter = ClipContestSubmitter::new(pool.clone(), lookup(), broker.clone());
    let outcome = submitter
        .submit(request(Some("https://www.twitch.tv/name/clip/ClipEins")))
        .await;
    assert_eq!(
        outcome,
        SubmitOutcome::Accepted {
            clip_url: "https://clips.twitch.tv/ClipEins".into(),
            submission_id: Some(123)
        }
    );
    let original_at: DateTime<Utc> = sqlx::query_scalar(
        "SELECT submitted_at FROM twitch_clip_contest_forwards WHERE clip_id = 'ClipEins'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        broker.requests.lock().unwrap()[0],
        BrokerClipRequest {
            source: "twitch".into(),
            clip_url: "https://clips.twitch.tv/ClipEins".into(),
            streamer_twitch_user_id: "456".into(),
            streamer_login: "name".into(),
            submitted_by_twitch_user_id: Some("4242".into()),
            submitted_at: Some(original_at.to_rfc3339_opts(SecondsFormat::Micros, true)),
            title: Some("Titel ClipEins".into()),
            idempotency_key: "twitch-clip-ClipEins".into(),
        }
    );
    assert_eq!(
        status_of(&pool, "ClipEins").await,
        Some(("accepted".into(), "chat".into(), Some("4242".into())))
    );
    let again = submitter
        .submit(request(Some("clips.twitch.tv/ClipEins")))
        .await;
    assert_eq!(
        again,
        SubmitOutcome::AlreadyIn {
            clip_url: "https://clips.twitch.tv/ClipEins".into()
        }
    );
    assert_eq!(broker.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn drift_duplikat_bleibt_persistiert_und_verhindert_weiteren_brokeraufruf() {
    let _db = migrated_pool("tb_clipcontest_drift").await;
    let pool = _db.pool.clone();
    let broker = FakeBroker::new(Ok(BrokerClipResponse {
        status: BrokerClipStatus::Duplicate,
        submission_id: Some(77),
        reason: Some("idempotency_metadata_drift".into()),
    }));
    let submitter = ClipContestSubmitter::new(pool.clone(), lookup(), broker.clone());
    let expected = SubmitOutcome::AlreadyIn {
        clip_url: "https://clips.twitch.tv/ClipEins".into(),
    };

    assert_eq!(
        submitter
            .submit(request(Some("https://clips.twitch.tv/ClipEins")))
            .await,
        expected
    );
    let persisted: (String, Option<i64>, Option<String>) = sqlx::query_as(
        "SELECT status, broker_submission_id, reason
           FROM twitch_clip_contest_forwards
          WHERE clip_id = $1",
    )
    .bind("ClipEins")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        persisted,
        (
            "duplicate".into(),
            Some(77),
            Some("idempotency_metadata_drift".into())
        )
    );

    assert_eq!(
        submitter
            .submit(request(Some("https://clips.twitch.tv/ClipEins")))
            .await,
        expected
    );
    assert_eq!(broker.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ablehnungen_vor_dem_broker() {
    let _db = migrated_pool("tb_clipcontest_reject").await;
    let pool = _db.pool.clone();
    let broker = FakeBroker::new(accepted(1));
    let submitter = ClipContestSubmitter::new(pool.clone(), lookup(), broker.clone());
    assert_eq!(
        submitter
            .submit(request(Some("https://youtube.com/x")))
            .await,
        SubmitOutcome::InvalidUrl
    );
    assert_eq!(
        submitter
            .submit(request(Some("https://clips.twitch.tv/GibtEsNicht")))
            .await,
        SubmitOutcome::ClipNotFound
    );
    assert_eq!(
        submitter
            .submit(request(Some("https://clips.twitch.tv/FremderClip")))
            .await,
        SubmitOutcome::ForeignClip
    );
    assert_eq!(
        submitter
            .submit(request(Some("https://clips.twitch.tv/Kaputt")))
            .await,
        SubmitOutcome::TwitchUnavailable
    );
    let mut fremd = request(Some("https://clips.twitch.tv/ClipEins"));
    fremd.broadcaster_id = "457".into();
    assert_eq!(submitter.submit(fremd).await, SubmitOutcome::NotPartner);
    assert_eq!(broker.calls.load(Ordering::SeqCst), 0);
    // Ohne laufende Session gibt es keinen jüngsten Clip.
    assert_eq!(
        submitter.submit(request(None)).await,
        SubmitOutcome::NoRecentClip
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clip_contest_forwards")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn tageslimit_drei_je_kanal_und_broker_status() {
    let _db = migrated_pool("tb_clipcontest_limit").await;
    let pool = _db.pool.clone();
    let broker = FakeBroker::new(accepted(1));
    let submitter = ClipContestSubmitter::new(pool.clone(), lookup(), broker.clone());
    // Duplikat aus dem Discord zählt nicht als neue Einreichung.
    *broker.answer.lock().unwrap() = Ok(BrokerClipResponse {
        status: BrokerClipStatus::Duplicate,
        submission_id: Some(9),
        reason: Some("duplicate_clip_this_week".into()),
    });
    assert!(matches!(
        submitter
            .submit(request(Some("https://clips.twitch.tv/ClipVier")))
            .await,
        SubmitOutcome::AlreadyIn { .. }
    ));
    *broker.answer.lock().unwrap() = accepted(1);
    for id in ["ClipEins", "ClipZwei", "ClipDrei"] {
        assert!(matches!(
            submitter
                .submit(request(Some(&format!("https://clips.twitch.tv/{id}"))))
                .await,
            SubmitOutcome::Accepted { .. }
        ));
    }
    assert_eq!(
        submitter
            .submit(request(Some("https://clips.twitch.tv/PipelineClip")))
            .await,
        SubmitOutcome::RateLimited
    );
    assert_eq!(broker.calls.load(Ordering::SeqCst), 4);
    // Einreichungen von gestern zählen nicht.
    sqlx::query(
        "UPDATE twitch_clip_contest_forwards SET created_at = created_at - INTERVAL '2 days'",
    )
    .execute(&pool)
    .await
    .unwrap();
    *broker.answer.lock().unwrap() = Ok(BrokerClipResponse {
        status: BrokerClipStatus::Rejected,
        submission_id: None,
        reason: Some("not_partner".into()),
    });
    assert_eq!(
        submitter
            .submit(request(Some("https://clips.twitch.tv/PipelineClip")))
            .await,
        SubmitOutcome::Rejected {
            reason: Some("not_partner".into())
        }
    );
    assert_eq!(
        status_of(&pool, "PipelineClip").await.unwrap().0,
        "rejected"
    );
}

#[tokio::test]
async fn broker_offline_dann_neuer_versuch_und_doppelsend_schutz() {
    let _db = migrated_pool("tb_clipcontest_retry").await;
    let pool = _db.pool.clone();
    let broker = FakeBroker::new(Err("connection refused".into()));
    let submitter = ClipContestSubmitter::new(pool.clone(), lookup(), broker.clone());
    let url = Some("https://clips.twitch.tv/ClipEins");
    assert_eq!(
        submitter.submit(request(url)).await,
        SubmitOutcome::BrokerUnavailable
    );
    assert_eq!(status_of(&pool, "ClipEins").await.unwrap().0, "failed");
    let original_at: DateTime<Utc> = sqlx::query_scalar(
        "SELECT submitted_at FROM twitch_clip_contest_forwards WHERE clip_id = 'ClipEins'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    // created_at trägt weiterhin den Tageslimitzeitpunkt, nicht die Herkunft.
    sqlx::query("UPDATE twitch_clip_contest_forwards SET created_at = created_at - INTERVAL '2 days' WHERE clip_id = 'ClipEins'")
        .execute(&pool).await.unwrap();
    *broker.answer.lock().unwrap() = accepted(5);
    let mut dashboard = request(url);
    dashboard.via = SubmitVia::Dashboard;
    dashboard.submitted_by = Some("9898".into());
    assert!(matches!(
        submitter.submit(dashboard).await,
        SubmitOutcome::Accepted { .. }
    ));
    assert_eq!(status_of(&pool, "ClipEins").await.unwrap().1, "dashboard");
    let (stored_at, created_at): (DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
        "SELECT submitted_at, created_at FROM twitch_clip_contest_forwards WHERE clip_id = 'ClipEins'",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(stored_at, original_at);
    assert!(created_at > original_at);
    let payloads = broker.requests.lock().unwrap().clone();
    assert_eq!(payloads.len(), 2);
    assert_eq!(payloads[0].submitted_at, payloads[1].submitted_at);
    assert_eq!(
        payloads[0].submitted_by_twitch_user_id,
        payloads[1].submitted_by_twitch_user_id
    );
    assert_eq!(
        status_of(&pool, "ClipEins").await.unwrap().2.as_deref(),
        Some("4242")
    );
    assert_eq!(
        payloads[0].submitted_at.as_deref(),
        Some(
            original_at
                .to_rfc3339_opts(SecondsFormat::Micros, true)
                .as_str()
        )
    );

    // Zwei gleichzeitige Einreichungen desselben Clips: genau ein Brokeraufruf.
    let slow = Arc::new(FakeBroker {
        calls: AtomicUsize::new(0),
        requests: Mutex::new(Vec::new()),
        answer: Mutex::new(accepted(6)),
        delay_ms: 300,
    });
    let submitter = Arc::new(ClipContestSubmitter::new(
        pool.clone(),
        lookup(),
        slow.clone(),
    ));
    let a = {
        let s = submitter.clone();
        tokio::spawn(async move {
            s.submit(request(Some("https://clips.twitch.tv/ClipZwei")))
                .await
        })
    };
    let b = {
        let s = submitter.clone();
        tokio::spawn(async move {
            s.submit(request(Some("https://clips.twitch.tv/ClipZwei")))
                .await
        })
    };
    let mut results = [a.await.unwrap(), b.await.unwrap()];
    results.sort_by_key(|o| o.code());
    assert_eq!(results[0].code(), "accepted");
    assert_eq!(results[1].code(), "in_flight");
    assert_eq!(slow.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn verspaeteter_alter_abschluss_erhaelt_neueren_erfolg() {
    struct RetryBroker {
        started: tokio::sync::Notify,
        release: tokio::sync::Notify,
        calls: AtomicUsize,
        requests: Mutex<Vec<BrokerClipRequest>>,
    }

    #[async_trait]
    impl ClipContestBroker for RetryBroker {
        async fn submit(&self, request: &BrokerClipRequest) -> Result<BrokerClipResponse, String> {
            self.requests.lock().unwrap().push(request.clone());
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                self.started.notify_one();
                self.release.notified().await;
                Err("Verspätete verlorene Antwort".into())
            } else {
                accepted(87)
            }
        }
    }

    let db = migrated_pool("tb_clipcontest_stale_completion").await;
    let pool = &db.pool;
    let broker = Arc::new(RetryBroker {
        started: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
        calls: AtomicUsize::new(0),
        requests: Mutex::new(Vec::new()),
    });
    let submitter = Arc::new(ClipContestSubmitter::new(
        pool.clone(),
        lookup(),
        broker.clone(),
    ));
    let old = {
        let submitter = submitter.clone();
        tokio::spawn(async move {
            submitter
                .submit(request(Some("https://clips.twitch.tv/ClipEins")))
                .await
        })
    };
    broker.started.notified().await;
    sqlx::query("UPDATE twitch_clip_contest_forwards SET updated_at = updated_at - INTERVAL '121 seconds' WHERE clip_id = 'ClipEins'")
        .execute(pool).await.unwrap();
    let mut retry = request(Some("https://clips.twitch.tv/ClipEins"));
    retry.submitted_by = Some("9898".into());
    assert!(matches!(
        submitter.submit(retry).await,
        SubmitOutcome::Accepted { .. }
    ));
    let success: (String, Option<i64>, DateTime<Utc>, Option<String>, DateTime<Utc>) = sqlx::query_as(
        "SELECT status, broker_submission_id, submitted_at, submitted_by_twitch_id, updated_at FROM twitch_clip_contest_forwards WHERE clip_id = 'ClipEins'",
    ).fetch_one(pool).await.unwrap();
    broker.release.notify_one();
    assert_eq!(old.await.unwrap(), SubmitOutcome::StoreUnavailable);
    let final_row: (String, Option<i64>, DateTime<Utc>, Option<String>, DateTime<Utc>) = sqlx::query_as(
        "SELECT status, broker_submission_id, submitted_at, submitted_by_twitch_id, updated_at FROM twitch_clip_contest_forwards WHERE clip_id = 'ClipEins'",
    ).fetch_one(pool).await.unwrap();
    assert_eq!(final_row, success);
    assert_eq!(final_row.0, "accepted");
    assert_eq!(final_row.1, Some(87));
    assert_eq!(final_row.3.as_deref(), Some("4242"));
    assert_eq!(
        ClipContestSubmitter::count_today(pool, "456", Utc::now())
            .await
            .unwrap(),
        1
    );
    let requests = broker.requests.lock().unwrap();
    assert_eq!(requests[0], requests[1]);
}

#[tokio::test]
async fn ohne_url_juengster_clip_der_laufenden_session() {
    let _db = migrated_pool("tb_clipcontest_latest").await;
    let pool = _db.pool.clone();
    sqlx::raw_sql(
        "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, twitch_user_id)
             VALUES (1, 'name', NOW() - INTERVAL '5 hours', '456');
         UPDATE twitch_stream_sessions SET ended_at = NOW() - INTERVAL '4 hours' WHERE id = 1;
         INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, twitch_user_id)
             VALUES (2, 'name', NOW() - INTERVAL '1 hour', '456');
         INSERT INTO twitch_clip_command_events
             (clip_id, streamer_login, twitch_user_id, requested_at, resolution_status)
             VALUES ('AlterClip', 'name', '456', NOW() - INTERVAL '3 hours', 'unavailable'),
                    ('BefehlClip', 'name', '456', NOW() - INTERVAL '30 minutes', 'unavailable');",
    )
    .execute(&pool)
    .await
    .unwrap();
    let broker = FakeBroker::new(accepted(1));
    let submitter = ClipContestSubmitter::new(pool.clone(), lookup(), broker.clone());
    assert_eq!(
        submitter.latest_session_clip("456", "name").await.unwrap(),
        Some("BefehlClip".into())
    );
    // Ein neuerer Clip aus der Clip-Pipeline gewinnt.
    sqlx::query(
        "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, twitch_user_id, created_at)
         VALUES ('PipelineClip', 'https://clips.twitch.tv/PipelineClip', 'name', '456', $1)",
    )
    .bind(Utc::now() - Duration::minutes(10))
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(
        submitter.submit(request(None)).await,
        SubmitOutcome::Accepted { clip_url, .. } if clip_url == "https://clips.twitch.tv/PipelineClip"
    ));
}

#[tokio::test]
async fn verlorene_brokerantwort_und_unbelegter_altbestand_behalten_herkunft() {
    let db = migrated_pool("tb_clipcontest_origin").await;
    let pool = db.pool.clone();
    let broker = FakeBroker::new(Err("Antwort nach Remotecommit verloren".into()));
    let lookup = Arc::new(FakeLookup(Mutex::new(vec![
        clip("LostResponse", "456"),
        clip("LegacyOrigin", "456"),
    ])));
    let submitter = ClipContestSubmitter::new(pool.clone(), lookup, broker.clone());
    let url = Some("https://clips.twitch.tv/LostResponse");
    assert_eq!(
        submitter.submit(request(url)).await,
        SubmitOutcome::BrokerUnavailable
    );
    let original = broker.requests.lock().unwrap()[0].clone();
    assert!(original.submitted_at.is_some());
    *broker.answer.lock().unwrap() = Ok(BrokerClipResponse {
        status: BrokerClipStatus::Duplicate,
        submission_id: Some(81),
        reason: Some("idempotency_metadata_drift".into()),
    });
    assert!(matches!(
        submitter.submit(request(url)).await,
        SubmitOutcome::AlreadyIn { .. }
    ));
    assert_eq!(broker.requests.lock().unwrap()[1], original);
    assert!(matches!(
        submitter.submit(request(url)).await,
        SubmitOutcome::AlreadyIn { .. }
    ));
    assert_eq!(broker.calls.load(Ordering::SeqCst), 2);

    // Ein Bestand ohne belegbaren ursprünglichen Claim erhält keine neue Herkunft.
    sqlx::query("INSERT INTO twitch_clip_contest_forwards (clip_id, clip_url, broadcaster_twitch_id, broadcaster_login, submitted_by_twitch_id, via, status, created_at, submitted_at) VALUES ('LegacyOrigin', 'https://clips.twitch.tv/LegacyOrigin', '456', 'name', '4242', 'chat', 'failed', clock_timestamp() - INTERVAL '2 days', NULL)")
        .execute(&pool).await.unwrap();
    *broker.answer.lock().unwrap() = accepted(82);
    assert!(matches!(
        submitter
            .submit(request(Some("https://clips.twitch.tv/LegacyOrigin")))
            .await,
        SubmitOutcome::Accepted { .. }
    ));
    let body = broker.requests.lock().unwrap().last().unwrap().clone();
    assert_eq!(body.submitted_at, None);
    assert!(serde_json::to_value(&body)
        .unwrap()
        .get("submitted_at")
        .is_none());
    let stored: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT submitted_at FROM twitch_clip_contest_forwards WHERE clip_id = 'LegacyOrigin'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored, None);
}
