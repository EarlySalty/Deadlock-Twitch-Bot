use std::future::Future;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use tb_monitoring::scout::ScoutRepository;
use tb_monitoring::streamer_login::rename_streamer_login;
use tracing_subscriber::fmt::MakeWriter;

mod support;

macro_rules! pool_or_skip {
    ($schema:expr) => {
        match support::pool_in_schema($schema).await {
            Some(pool) => pool,
            None => return,
        }
    };
}

#[derive(Clone, Default)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl<'a> MakeWriter<'a> for LogCapture {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter(Arc::clone(&self.0))
    }
}

impl Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

async fn capture_logs<T>(future: impl Future<Output = T>) -> (T, String) {
    let capture = LogCapture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(capture.clone())
        .finish();
    let dispatch = tracing::Dispatch::new(subscriber);
    let guard = tracing::dispatcher::set_default(&dispatch);
    let result = future.await;
    drop(guard);
    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    (result, logs)
}

async fn seed_operational_login_rows(pool: &sqlx::PgPool) {
    for statement in [
        "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('old_login', '520300019')",
        "INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_game) VALUES ('520300019', 'old_login', 'Deadlock')",
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, raid_bot_enabled) VALUES ('520300019', 'old_login', 'active', 0)",
        "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, needs_reauth) VALUES ('520300019', 'old_login', TRUE)",
        "INSERT INTO twitch_partner_raid_scores (twitch_user_id, twitch_login, final_score) VALUES ('520300019', 'old_login', 0.77)",
        "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_display_name) VALUES ('520300019', 'old_login', 'Identität bleibt')",
        "INSERT INTO twitch_engagement_settings (channel_login, enabled) VALUES ('old_login', FALSE)",
        "INSERT INTO twitch_engagement_channel_profile (channel_login, profile_text) VALUES ('old_login', 'alt')",
        "INSERT INTO twitch_stream_sessions (streamer_login, stream_id, started_at) VALUES ('old_login', 'open-stream', '2026-08-01T10:00:00Z')",
        "INSERT INTO twitch_stream_sessions (streamer_login, started_at, ended_at) VALUES ('old_login', '2026-07-01T10:00:00Z', '2026-07-01T11:00:00Z')",
        "INSERT INTO twitch_streamer_invites (streamer_login, invite_code, invite_url) VALUES ('old_login', 'old-code', 'https://example.invalid/old')",
        "INSERT INTO twitch_raw_chat_ingest_health (streamer_login, last_raw_chat_error) VALUES ('old_login', 'alt')",
    ] {
        sqlx::query(statement).execute(pool).await.unwrap();
    }
}

#[tokio::test]
async fn upsert_monitored_aktualisiert_bekannte_user_id_und_alle_betriebstabellen() {
    let pool = pool_or_skip!("t_rename_scout_existing_user");
    seed_operational_login_rows(&pool).await;
    sqlx::query(
        "INSERT INTO twitch_engagement_settings (channel_login, enabled)
         VALUES ('new_login', TRUE)",
    )
    .execute(&pool)
    .await
    .unwrap();
    for statement in [
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, raid_bot_enabled) VALUES ('520300019', 'new_login', 'active', 1)",
        "INSERT INTO twitch_engagement_channel_profile (channel_login, profile_text) VALUES ('new_login', 'neu gewinnt')",
        "INSERT INTO twitch_streamer_invites (streamer_login, invite_code, invite_url) VALUES ('new_login', 'new-code', 'https://example.invalid/new')",
        "INSERT INTO twitch_raw_chat_ingest_health (streamer_login, last_raw_chat_error) VALUES ('new_login', 'neu gewinnt')",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }

    let inserted = ScoutRepository::new(pool.clone())
        .upsert_monitored("new_login", "520300019")
        .await
        .expect("bekannte user_id muss als Rename statt Insert verarbeitet werden");

    assert!(!inserted, "Rename ist keine Neuentdeckung");
    for (table, column) in [
        ("twitch_streamers", "twitch_login"),
        ("twitch_live_state", "streamer_login"),
        ("twitch_partners", "twitch_login"),
        ("twitch_raid_auth", "twitch_login"),
        ("twitch_partner_raid_scores", "twitch_login"),
        ("twitch_streamer_identities", "twitch_login"),
    ] {
        let old_count: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM {table} WHERE LOWER({column}) = 'old_login'"
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(old_count, 0, "{table}.{column} behält alten Login");
    }
    let open_old: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_stream_sessions
         WHERE streamer_login = 'old_login' AND ended_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let closed_old: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_stream_sessions
         WHERE streamer_login = 'old_login' AND ended_at IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(open_old, 0, "offene Session wurde nicht umbenannt");
    assert_eq!(closed_old, 1, "geschlossene Session bleibt Betriebshistorie");

    let settings: Vec<(String, bool)> = sqlx::query_as(
        "SELECT channel_login, enabled FROM twitch_engagement_settings ORDER BY channel_login",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    // Unsere Einstellungen (enabled=false) folgen dem Kanal; die verwaiste
    // Fremdzeile bleibt inhaltlich erhalten, gibt aber den Login frei.
    assert_eq!(
        settings,
        vec![
            ("new_login".to_string(), false),
            ("stale:unbekannt:new_login".to_string(), true),
        ]
    );

    // Tabellen ohne ID-Spalte finden den Kanal nur über den Namen — bei
    // besetztem neuen Login bleibt ihre Zeile deshalb stehen.
    let ohne_id_spalte: (String, String) = sqlx::query_as(
        "SELECT invites.invite_code, health.last_raw_chat_error
           FROM twitch_streamer_invites invites
           JOIN twitch_raw_chat_ingest_health health
             ON health.streamer_login = invites.streamer_login
          WHERE invites.streamer_login = 'old_login'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        ohne_id_spalte,
        ("old-code".to_string(), "alt".to_string())
    );

    // Das Kanal-Profil trägt eine ID und folgt dem Kanal; die verwaiste
    // Fremdzeile behält ihren Inhalt unter einem Platzhalter-Login.
    let profile: Vec<(String, String)> = sqlx::query_as(
        "SELECT channel_login, profile_text
           FROM twitch_engagement_channel_profile ORDER BY channel_login",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        profile,
        vec![
            ("new_login".to_string(), "alt".to_string()),
            ("stale:unbekannt:new_login".to_string(), "neu gewinnt".to_string()),
        ]
    );

    let preserved: (String, f64, String, String, bool, String) = sqlx::query_as(
        "SELECT live.last_game, scores.final_score, identities.discord_display_name,
                sessions.stream_id, auth.needs_reauth, streamers.twitch_user_id
           FROM twitch_live_state live
           JOIN twitch_partner_raid_scores scores USING (twitch_user_id)
           JOIN twitch_streamer_identities identities USING (twitch_user_id)
           JOIN twitch_raid_auth auth USING (twitch_user_id)
           JOIN twitch_streamers streamers USING (twitch_user_id)
           JOIN twitch_stream_sessions sessions ON sessions.streamer_login = live.streamer_login
          WHERE live.twitch_user_id = '520300019' AND sessions.ended_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        preserved,
        (
            "Deadlock".to_string(),
            0.77,
            "Identität bleibt".to_string(),
            "open-stream".to_string(),
            true,
            "520300019".to_string(),
        )
    );
    // Unter dem neuen Login stehen jetzt: die aktive Partnerzeile dieses
    // Kanals und die login-gebundenen Zeilen, die dort schon lagen. Das Profil
    // kommt aus der ID-Zuordnung und trägt deshalb unseren Inhalt.
    let winning_rows: (i32, String, String, String) = sqlx::query_as(
        "SELECT partners.raid_bot_enabled, profile.profile_text,
                invites.invite_code, health.last_raw_chat_error
           FROM twitch_partners partners
           JOIN twitch_engagement_channel_profile profile ON profile.channel_login = partners.twitch_login
           JOIN twitch_streamer_invites invites ON invites.streamer_login = partners.twitch_login
           JOIN twitch_raw_chat_ingest_health health ON health.streamer_login = partners.twitch_login
          WHERE partners.twitch_user_id = '520300019'
            AND partners.twitch_login = 'new_login'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        winning_rows,
        (
            1,
            "alt".to_string(),
            "new-code".to_string(),
            "neu gewinnt".to_string(),
        )
    );

    let inserted = ScoutRepository::new(pool.clone())
        .upsert_monitored("brand_new", "999")
        .await
        .unwrap();
    assert!(inserted, "unbekannte user_id bleibt ein Insert");
    let inserted_login: String = sqlx::query_scalar(
        "SELECT twitch_login FROM twitch_streamers WHERE twitch_user_id = '999'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(inserted_login, "brand_new");
}

#[tokio::test(flavor = "current_thread")]
async fn login_keyed_konflikt_bewahrt_alte_zeile_und_warnt() {
    let pool = pool_or_skip!("t_rename_login_keyed_conflict");
    seed_operational_login_rows(&pool).await;
    sqlx::query(
        "INSERT INTO twitch_streamer_invites (streamer_login, invite_code, invite_url)
         VALUES ('new_login', 'new-code', 'https://example.invalid/new')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let (report, logs) = capture_logs(rename_streamer_login(
        &pool,
        "520300019",
        "old_login",
        "new_login",
    ))
    .await;
    let report = report.unwrap();

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT streamer_login, invite_code
           FROM twitch_streamer_invites
          ORDER BY streamer_login",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let warnings: Vec<_> = logs.lines().filter(|line| line.contains(" WARN ")).collect();
    assert_eq!(
        (
            rows,
            warnings.len(),
            warnings.first().is_some_and(|line| {
                line.contains("table=twitch_streamer_invites")
                    && line.contains("twitch_user_id=520300019")
                    && line.contains("old_login=old_login")
                    && line.contains("new_login=new_login")
            }),
            report.counts.for_table("twitch_streamer_invites"),
        ),
        (
            vec![
                ("new_login".to_string(), "new-code".to_string()),
                ("old_login".to_string(), "old-code".to_string()),
            ],
            1,
            true,
            tb_monitoring::RenameTableCounts {
                renamed: 0,
                stale_cleared: 0,
                skipped: 1,
            },
        )
    );
}

#[tokio::test]
async fn rename_schreibt_alias_historie_ohne_fruehere_logins_zu_verlieren() {
    let pool = pool_or_skip!("t_rename_alias_history");
    seed_operational_login_rows(&pool).await;

    rename_streamer_login(&pool, "520300019", "old_login", "new_login")
        .await
        .unwrap();
    let after_first: Vec<(String, bool)> = sqlx::query_as(
        "SELECT login, is_current FROM twitch_login_aliases ORDER BY login",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    rename_streamer_login(&pool, "520300019", "new_login", "third_login")
        .await
        .unwrap();
    let after_second: Vec<(String, bool)> = sqlx::query_as(
        "SELECT login, is_current FROM twitch_login_aliases ORDER BY login",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(
        (after_first, after_second),
        (
            vec![("new_login".to_string(), true), ("old_login".to_string(), false)],
            vec![
                ("new_login".to_string(), false),
                ("old_login".to_string(), false),
                ("third_login".to_string(), true),
            ],
        )
    );
}

#[tokio::test(flavor = "current_thread")]
async fn rename_behaelt_eigene_zeile_und_raeumt_veraltete_fremdzeile() {
    let pool = pool_or_skip!("t_rename_separate_counts");
    seed_operational_login_rows(&pool).await;
    sqlx::query(
        "INSERT INTO twitch_streamers (twitch_login, twitch_user_id)
         VALUES ('new_login', '999')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_engagement_settings (channel_login, enabled)
         VALUES ('new_login', TRUE)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let (report, logs) = capture_logs(rename_streamer_login(
        &pool,
        "520300019",
        "old_login",
        "new_login",
    ))
    .await;
    let report = report.unwrap();

    // Die eigene Zeile gewinnt: Twitch hat den Login gerade für 520300019
    // bestätigt, die Fremdzeile mit demselben Login ist der veraltete Stand.
    let eigener_login: String = sqlx::query_scalar(
        "SELECT twitch_login FROM twitch_streamers WHERE twitch_user_id = '520300019'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let fremder_login: String =
        sqlx::query_scalar("SELECT twitch_login FROM twitch_streamers WHERE twitch_user_id = '999'")
            .fetch_one(&pool)
            .await
            .unwrap();

    assert_eq!(
        (
            eigener_login.as_str(),
            fremder_login.as_str(),
            report.counts.for_table("twitch_streamers"),
            report.counts.for_table("twitch_engagement_settings"),
            logs.lines().any(|line| {
                line.contains("table=twitch_streamers")
                    && line.contains("renamed=1")
                    && line.contains("stale_cleared=1")
                    && line.contains("skipped=0")
            }),
            logs.lines().any(|line| {
                line.contains("table=twitch_engagement_settings")
                    && line.contains("renamed=1")
                    && line.contains("stale_cleared=1")
                    && line.contains("skipped=0")
            }),
        ),
        (
            "new_login",
            "stale:999:new_login",
            tb_monitoring::RenameTableCounts {
                renamed: 1,
                stale_cleared: 1,
                skipped: 0,
            },
            tb_monitoring::RenameTableCounts {
                renamed: 1,
                stale_cleared: 1,
                skipped: 0,
            },
            true,
            true,
        )
    );
}

#[tokio::test]
async fn rename_behaelt_raid_token_und_partner_konfiguration_bei_login_kollision() {
    let pool = pool_or_skip!("t_rename_keeps_tokens");
    seed_operational_login_rows(&pool).await;
    // Veraltete Fremdzeilen, die den neuen Login blockieren.
    for statement in [
        "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, needs_reauth) VALUES ('999', 'new_login', FALSE)",
        "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, raid_bot_enabled) VALUES ('999', 'new_login', 'active', 0)",
        "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_display_name) VALUES ('999', 'new_login', 'fremd')",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }

    rename_streamer_login(&pool, "520300019", "old_login", "new_login")
        .await
        .unwrap();

    // Weder unser Raid-Token noch unsere Partner-Konfiguration darf verschwinden.
    let eigene_zeilen: (i64, i64, i64) = sqlx::query_as(
        "SELECT
             (SELECT COUNT(*) FROM twitch_raid_auth WHERE twitch_user_id = '520300019' AND twitch_login = 'new_login'),
             (SELECT COUNT(*) FROM twitch_partners WHERE twitch_user_id = '520300019' AND twitch_login = 'new_login' AND status = 'active'),
             (SELECT COUNT(*) FROM twitch_streamer_identities WHERE twitch_user_id = '520300019' AND twitch_login = 'new_login')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    // Die Fremdzeilen bleiben inhaltlich erhalten, nur ihr Login gibt den Index frei.
    let fremde_zeilen: (i64, i64, i64) = sqlx::query_as(
        "SELECT
             (SELECT COUNT(*) FROM twitch_raid_auth WHERE twitch_user_id = '999'),
             (SELECT COUNT(*) FROM twitch_partners WHERE twitch_user_id = '999'),
             (SELECT COUNT(*) FROM twitch_streamer_identities WHERE twitch_user_id = '999')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!((eigene_zeilen, fremde_zeilen), ((1, 1, 1), (1, 1, 1)));
}

#[tokio::test]
async fn parallele_renames_halten_aktuellen_alias_und_betriebslogin_konsistent() {
    let pool = pool_or_skip!("t_rename_concurrent");
    seed_operational_login_rows(&pool).await;

    let first = rename_streamer_login(&pool, "520300019", "old_login", "new_login");
    let second = rename_streamer_login(&pool, "520300019", "old_login", "third_login");
    let (first, second) = tokio::join!(first, second);
    first.unwrap();
    second.unwrap();

    let operational_login: String = sqlx::query_scalar(
        "SELECT twitch_login FROM twitch_streamers WHERE twitch_user_id = '520300019'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let current_alias: String = sqlx::query_scalar(
        "SELECT login FROM twitch_login_aliases
          WHERE twitch_user_id = '520300019' AND is_current",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let aliases: Vec<String> = sqlx::query_scalar(
        "SELECT login FROM twitch_login_aliases
          WHERE twitch_user_id = '520300019' ORDER BY login",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(current_alias, operational_login);
    assert_eq!(
        aliases,
        vec![
            "new_login".to_string(),
            "old_login".to_string(),
            "third_login".to_string(),
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn parallele_identische_renames_zaehlen_den_zweiten_lauf_als_noop() {
    let pool = pool_or_skip!("t_rename_concurrent_same_target");
    seed_operational_login_rows(&pool).await;

    let ((first, second), logs) = capture_logs(async {
        tokio::join!(
            rename_streamer_login(&pool, "520300019", "old_login", "new_login"),
            rename_streamer_login(&pool, "520300019", "old_login", "new_login"),
        )
    })
    .await;
    let first = first.unwrap();
    let second = second.unwrap();
    let noop_reports = [&first, &second]
        .into_iter()
        .filter(|report| report.counts.total() == tb_monitoring::RenameTableCounts::default())
        .count();
    let warnings = logs.lines().filter(|line| line.contains(" WARN ")).count();

    assert_eq!((noop_reports, warnings), (1, 0));
}

#[tokio::test]
async fn rename_streamer_login_rollt_bei_spaetem_db_fehler_alles_zurueck() {
    let pool = pool_or_skip!("t_rename_transaction_rollback");
    seed_operational_login_rows(&pool).await;
    sqlx::query(
        "CREATE FUNCTION reject_ingest_health_rename() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             RAISE EXCEPTION 'simulierter später Schreibfehler';
         END
         $$",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER reject_ingest_health_rename
         BEFORE UPDATE ON twitch_raw_chat_ingest_health
         FOR EACH ROW EXECUTE FUNCTION reject_ingest_health_rename()",
    )
    .execute(&pool)
    .await
    .unwrap();

    let error = rename_streamer_login(&pool, "520300019", "old_login", "new_login")
        .await
        .expect_err("später DB-Fehler muss den gesamten Rename abbrechen");
    assert!(error.to_string().contains("simulierter später Schreibfehler"));

    for (table, column) in [
        ("twitch_streamers", "twitch_login"),
        ("twitch_live_state", "streamer_login"),
        ("twitch_engagement_settings", "channel_login"),
        ("twitch_stream_sessions", "streamer_login"),
        ("twitch_raw_chat_ingest_health", "streamer_login"),
    ] {
        let login: String = sqlx::query_scalar(&format!(
            "SELECT {column} FROM {table} WHERE LOWER({column}) = 'old_login' LIMIT 1"
        ))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(login, "old_login", "{table}.{column} wurde nicht zurückgerollt");
    }
}
