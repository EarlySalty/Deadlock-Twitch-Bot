mod sub_reminder_cases {
    use super::*;
    use crate::sub_reminder::{record_subscription_event, SubReminder, SubscriptionStatus};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Status {
        value: std::sync::Mutex<Option<bool>>,
        calls: AtomicUsize,
    }
    #[async_trait]
    impl SubscriptionStatus for Status {
        async fn active(&self, _: &str, _: &str) -> Option<bool> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.value.lock().unwrap()
        }
    }
    async fn setup() -> (
        crate::test_postgres::TestPostgres,
        Arc<MockApi>,
        Arc<Status>,
        Arc<SubReminder>,
    ) {
        let database = crate::test_postgres::TestPostgres::start().await;
        sqlx::raw_sql("CREATE TABLE streamer_plans(twitch_user_id text PRIMARY KEY,twitch_login text); CREATE TABLE twitch_subscription_events(id bigint);")
            .execute(&database.pool).await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260909220000_sub_reminders.sql"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        let api = MockApi::new();
        let status = Arc::new(Status {
            value: std::sync::Mutex::new(Some(false)),
            calls: AtomicUsize::new(0),
        });
        let reminder = Arc::new(SubReminder::new(
            database.pool.clone(),
            api.clone(),
            status.clone(),
        ));
        (database, api, status, reminder)
    }
    async fn enable(pool: &PgPool) {
        sqlx::query("INSERT INTO streamer_plans(twitch_user_id,twitch_login,sub_reminder_enabled,sub_reminder_enabled_at) VALUES ('bc123','alter_name',1,now()-interval '1 hour')")
            .execute(pool).await.unwrap();
    }
    async fn consent(reminder: &SubReminder) {
        reminder
            .command(
                &make_event("!sub erinnerung an", false, false),
                "erinnerung an",
            )
            .await;
    }
    async fn lifecycle(pool: &PgPool, id: &str, time: DateTime<Utc>, ended: bool) {
        let event = make_event("hallo", false, false);
        let mut connection = pool.acquire().await.unwrap();
        record_subscription_event(
            &mut connection,
            &event.broadcaster_user_id,
            &event.chatter_user_id,
            id,
            time,
            ended,
        )
        .await
        .unwrap();
    }
    async fn pending(pool: &PgPool) -> Option<String> {
        sqlx::query_scalar("SELECT end_message_id FROM twitch_sub_reminders")
            .fetch_one(pool)
            .await
            .unwrap()
    }
    async fn later(pool: &PgPool) -> DateTime<Utc> {
        sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn sub_defaults_link_kein_consent_scopefehler_und_abmelden() {
        let (db, api, status, reminder) = setup().await;
        reminder
            .command(&make_event("!sub", false, false), "")
            .await;
        assert!(api
            .last_message()
            .await
            .unwrap()
            .contains("https://www.twitch.tv/subs/testchannel"));
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_sub_reminders")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        consent(&reminder).await;
        assert_eq!(status.calls.load(Ordering::SeqCst), 0);
        enable(&db.pool).await;
        *status.value.lock().unwrap() = None;
        consent(&reminder).await;
        assert!(api
            .last_message()
            .await
            .unwrap()
            .contains("nicht verfügbar"));
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_sub_reminders")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        *status.value.lock().unwrap() = Some(false);
        consent(&reminder).await;
        sqlx::query("UPDATE streamer_plans SET sub_reminder_enabled=0")
            .execute(&db.pool)
            .await
            .unwrap();
        reminder
            .command(
                &make_event("!sub erinnerung aus", false, false),
                "erinnerung aus",
            )
            .await;
        let enabled: bool = sqlx::query_scalar("SELECT enabled FROM twitch_sub_reminders")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert!(!enabled);
    }

    #[tokio::test]
    async fn sub_ende_nur_nach_consent_reihenfolge_und_kanalgrenze() {
        let (db, _api, _status, reminder) = setup().await;
        enable(&db.pool).await;
        let old = later(&db.pool).await;
        lifecycle(&db.pool, "before-optin", old, true).await;
        consent(&reminder).await;
        lifecycle(&db.pool, "before-optin", old, true).await;
        assert_eq!(pending(&db.pool).await, None);
        let ended = later(&db.pool).await;
        lifecycle(&db.pool, "end", ended, true).await;
        assert_eq!(pending(&db.pool).await.as_deref(), Some("end"));
        // Wiederholtes an verwirft die legitime Fälligkeit nicht.
        consent(&reminder).await;
        assert_eq!(pending(&db.pool).await.as_deref(), Some("end"));
        let started = later(&db.pool).await;
        lifecycle(&db.pool, "renewal", started, false).await;
        lifecycle(&db.pool, "stale-end", ended, true).await;
        assert_eq!(pending(&db.pool).await, None);
        let disabled_end = later(&db.pool).await;
        sqlx::query("UPDATE streamer_plans SET sub_reminder_enabled_at=clock_timestamp()")
            .execute(&db.pool)
            .await
            .unwrap();
        lifecycle(&db.pool, "during-disabled", disabled_end, true).await;
        assert_eq!(pending(&db.pool).await, None);
        reminder
            .command(
                &make_event("!sub erinnerung aus", false, false),
                "erinnerung aus",
            )
            .await;
        let consent_before = later(&db.pool).await;
        consent(&reminder).await;
        lifecycle(&db.pool, "before-new-consent", consent_before, true).await;
        assert_eq!(pending(&db.pool).await, None);
    }

    #[tokio::test]
    async fn sub_parallel_neustart_und_helixfehler() {
        let (db, api, status, reminder) = setup().await;
        enable(&db.pool).await;
        consent(&reminder).await;
        let event = make_event("bin wieder da", false, false);
        let ended = later(&db.pool).await;
        lifecycle(&db.pool, "end", ended, true).await;
        let before = api.message_count().await;
        *status.value.lock().unwrap() = None;
        reminder.on_message(&event).await;
        assert_eq!(api.message_count().await, before);
        let consumed: Option<String> =
            sqlx::query_scalar("SELECT consumed_message_id FROM twitch_sub_reminders")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(consumed, None);
        sqlx::query("UPDATE twitch_sub_reminders SET retry_after=NULL")
            .execute(&db.pool)
            .await
            .unwrap();
        *status.value.lock().unwrap() = Some(false);
        let mut tasks = Vec::new();
        for _ in 0..12 {
            let reminder = reminder.clone();
            let event = event.clone();
            tasks.push(tokio::spawn(async move {
                reminder.on_message(&event).await;
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(api.message_count().await, before + 1);
        let restarted = SubReminder::new(db.pool.clone(), api.clone(), status.clone());
        restarted.on_message(&event).await;
        assert_eq!(api.message_count().await, before + 1);
    }

    #[tokio::test]
    async fn sub_erneuert_off_und_fremder_sharedchat_bleiben_still() {
        let (db, api, status, reminder) = setup().await;
        enable(&db.pool).await;
        consent(&reminder).await;
        lifecycle(&db.pool, "end", later(&db.pool).await, true).await;
        let before = api.message_count().await;
        let mut event = make_event("hallo", false, false);
        event.source_broadcaster_user_id = Some("fremd".into());
        let calls = status.calls.load(Ordering::SeqCst);
        reminder.on_message(&event).await;
        assert_eq!(status.calls.load(Ordering::SeqCst), calls);
        event.source_broadcaster_user_id = None;
        sqlx::query("UPDATE streamer_plans SET sub_reminder_enabled=0")
            .execute(&db.pool)
            .await
            .unwrap();
        reminder.on_message(&event).await;
        assert_eq!(status.calls.load(Ordering::SeqCst), calls);
        sqlx::query("UPDATE streamer_plans SET sub_reminder_enabled=1")
            .execute(&db.pool)
            .await
            .unwrap();
        *status.value.lock().unwrap() = Some(true);
        reminder.on_message(&event).await;
        assert_eq!(api.message_count().await, before);
    }
}
