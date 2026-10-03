use super::*;

fn ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

fn day(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn msg(at: &str, content: &str) -> ChatSample {
    ChatSample {
        at: ts(at),
        content: content.to_string(),
        is_command: false,
        moderated: false,
    }
}

// ─── Tagesgrenze Berlin ─────────────────────────────────────────────────────

#[test]
fn berliner_tagesgrenze_sommer_und_winter() {
    // Sommerzeit: UTC+2
    assert_eq!(berlin_day(ts("2026-07-01T21:59:59Z")), day("2026-07-01"));
    assert_eq!(berlin_day(ts("2026-07-01T22:00:00Z")), day("2026-07-02"));
    // Winterzeit: UTC+1
    assert_eq!(berlin_day(ts("2026-12-01T22:59:59Z")), day("2026-12-01"));
    assert_eq!(berlin_day(ts("2026-12-01T23:00:00Z")), day("2026-12-02"));
    let (s, e) = berlin_day_bounds(day("2026-10-01"));
    assert_eq!(s, ts("2026-09-30T22:00:00Z"));
    assert_eq!(e, ts("2026-10-01T22:00:00Z"));
}

#[test]
fn umstellungstage_haben_23_und_25_stunden() {
    let (s, e) = berlin_day_bounds(day("2026-03-29"));
    assert_eq!((e - s).num_hours(), 23);
    let (s, e) = berlin_day_bounds(day("2026-10-25"));
    assert_eq!((e - s).num_hours(), 25);
}

#[test]
fn vortag_nur_beim_start_oder_kurz_nach_mitternacht() {
    // 00:30 Berlin (Sommerzeit) → Vortag mitrechnen
    assert_eq!(
        days_to_aggregate(ts("2026-09-30T22:30:00Z"), false),
        vec![day("2026-09-30"), day("2026-10-01")]
    );
    // 12:00 Berlin → nur heute
    assert_eq!(
        days_to_aggregate(ts("2026-10-01T10:00:00Z"), false),
        vec![day("2026-10-01")]
    );
    // erster Lauf → immer Vortag dazu
    assert_eq!(
        days_to_aggregate(ts("2026-10-01T10:00:00Z"), true),
        vec![day("2026-09-30"), day("2026-10-01")]
    );
}

// ─── Ausschlüsse ────────────────────────────────────────────────────────────

#[test]
fn broadcaster_bots_und_fehlende_ids_sind_ausgeschlossen() {
    assert!(is_excluded_viewer("456", "streamer", "456"));
    assert!(is_excluded_viewer("1", "Nightbot", "456"));
    assert!(is_excluded_viewer("2", "streamelements", "456"));
    assert!(is_excluded_viewer("3", "deutschedeadlockcommunity", "456"));
    assert!(is_excluded_viewer("4", "justinfan12345", "456"));
    assert!(is_excluded_viewer("", "viewer", "456"));
    assert!(is_excluded_viewer("abc", "viewer", "456"));
    assert!(!is_excluded_viewer("123", "viewer", "456"));
    for bot in crate::bekannte_bots::KNOWN_CHAT_BOTS {
        assert!(is_excluded_viewer("9", bot, "456"), "{bot}");
    }
}

// ─── Watchtime-Deckel ───────────────────────────────────────────────────────

#[test]
fn watchtime_volle_fuenf_minuten_und_kanaldeckel() {
    assert_eq!(watch_points_for_minutes(0), 0);
    assert_eq!(watch_points_for_minutes(4), 0);
    assert_eq!(watch_points_for_minutes(5), 1);
    assert_eq!(watch_points_for_minutes(95), 19);
    assert_eq!(watch_points_for_minutes(359), 71);
    assert_eq!(watch_points_for_minutes(360), 72);
    assert_eq!(watch_points_for_minutes(10_000), 72);
    assert_eq!(watch_points_for_minutes(-5), 0);
}

#[test]
fn watchtime_tagesdeckel_ueber_alle_kanaele() {
    assert_eq!(cap_viewer_watch_points(&[72, 72, 72]), vec![72, 72, 0]);
    assert_eq!(cap_viewer_watch_points(&[50, 72, 72]), vec![50, 72, 22]);
    assert_eq!(cap_viewer_watch_points(&[10, 20]), vec![10, 20]);
    assert_eq!(cap_viewer_watch_points(&[]), Vec::<i32>::new());
}

// ─── Chat-Regeln ────────────────────────────────────────────────────────────

#[test]
fn befehle_zaehlen_nicht() {
    let mut m = msg("2026-10-01T10:00:00Z", "!watchtime bitte jetzt");
    assert_eq!(countable_flags(&[m.clone()]), vec![false]);
    m.content = "  !discord ist wo eigentlich".into();
    assert_eq!(countable_flags(&[m.clone()]), vec![false]);
    m.content = "normale lange Nachricht".into();
    m.is_command = true;
    assert_eq!(countable_flags(&[m]), vec![false]);
}

#[test]
fn mindestlaenge_nach_trimmen() {
    assert!(!meets_min_length("   kurz    "));
    assert!(!meets_min_length("123456789"));
    assert!(meets_min_length("1234567890"));
    assert!(meets_min_length("  äöüßäöüßäö  "));
    // Zeichen, nicht Bytes: 9 Umlaute sind 18 Bytes, aber zu kurz.
    assert!(!meets_min_length("äöüäöüäöü"));
}

#[test]
fn duplikat_der_vorherigen_nachricht_zaehlt_nicht() {
    let msgs = vec![
        msg("2026-10-01T10:00:00Z", "Was für ein Play!"),
        msg("2026-10-01T10:02:00Z", "was  für ein play!"),
        msg("2026-10-01T10:04:00Z", "Was für ein Play!"),
        msg("2026-10-01T10:06:00Z", "Ganz andere Nachricht"),
        msg("2026-10-01T10:08:00Z", "Was für ein Play!"),
    ];
    assert_eq!(countable_flags(&msgs), vec![true, false, false, true, true]);
}

#[test]
fn cooldown_sechzig_sekunden_nach_letzter_zaehlender() {
    let msgs = vec![
        msg("2026-10-01T10:00:00Z", "erste lange Nachricht"),
        msg("2026-10-01T10:00:30Z", "zweite lange Nachricht"),
        msg("2026-10-01T10:00:59Z", "dritte lange Nachricht"),
        msg("2026-10-01T10:01:00Z", "vierte lange Nachricht"),
        msg("2026-10-01T10:01:30Z", "!befehl zählt nicht mit"),
        msg("2026-10-01T10:02:00Z", "fünfte lange Nachricht"),
    ];
    assert_eq!(
        countable_flags(&msgs),
        vec![true, false, false, true, false, true]
    );
}

#[test]
fn moderierte_nachrichten_zaehlen_nicht() {
    let mut m = msg("2026-10-01T10:00:00Z", "eigentlich gute Nachricht");
    m.moderated = true;
    assert_eq!(countable_flags(&[m]), vec![false]);
}

#[test]
fn chat_deckel_und_fenster_mit_vorgeschichte() {
    assert_eq!(chat_points(12), 12);
    assert_eq!(chat_points(30), 30);
    assert_eq!(chat_points(31), 30);
    assert_eq!(chat_points(-1), 0);

    // Vorgeschichte am Vortag sperrt Cooldown und Duplikat über Mitternacht.
    let (start, end) = berlin_day_bounds(day("2026-10-01"));
    let msgs = vec![
        msg("2026-09-30T21:59:30Z", "gute Nacht zusammen"),
        msg("2026-09-30T22:00:10Z", "noch eine Nachricht hier"),
        msg("2026-09-30T22:01:00Z", "noch eine Nachricht hier"),
        msg("2026-09-30T22:02:00Z", "und noch eine andere"),
    ];
    assert_eq!(countable_in_window(&msgs, start, end), 1);
    // ohne Vorgeschichte wären es 2 (die Nachricht um 00:00:10 zählte dann)
    assert_eq!(countable_in_window(&msgs[1..], start, end), 2);
    // Ohne Vorgeschichte zählt die erste, mit Vorgeschichte nicht:
    assert_eq!(countable_flags(&msgs), vec![true, false, false, true]);
}

// ─── Entdecker-Bonus ────────────────────────────────────────────────────────

fn cand(channel: &str, at: &str, seen_before: bool) -> DiscoveryCandidate {
    DiscoveryCandidate {
        channel_id: channel.into(),
        first_seen_at: ts(at),
        seen_before,
    }
}

#[test]
fn entdecker_bonus_hoechstens_drei_je_tag_in_reihenfolge() {
    let c = vec![
        cand("5", "2026-10-01T14:00:00Z", false),
        cand("1", "2026-10-01T10:00:00Z", false),
        cand("2", "2026-10-01T11:00:00Z", true),
        cand("3", "2026-10-01T12:00:00Z", false),
        cand("4", "2026-10-01T13:00:00Z", false),
    ];
    assert_eq!(
        select_discovery_bonuses(&c, 0),
        vec![false, true, false, true, true]
    );
    assert_eq!(
        select_discovery_bonuses(&c, 2),
        vec![false, true, false, false, false]
    );
    assert_eq!(select_discovery_bonuses(&c, 3), vec![false; 5]);
    assert_eq!(select_discovery_bonuses(&c, 7), vec![false; 5]);
}

// ─── Tagesrechnung ──────────────────────────────────────────────────────────

fn partner(id: &str, login: &str) -> Partner {
    Partner {
        twitch_user_id: id.into(),
        login: login.into(),
        discord_user_id: Some(format!("d{id}")),
    }
}

fn presence(channel: &str, viewer: &str, login: &str, minutes: f64, first: &str) -> PresenceRow {
    PresenceRow {
        channel_id: channel.into(),
        viewer_id: viewer.into(),
        viewer_login: login.into(),
        seconds: minutes * 60.0,
        first_seen_at: ts(first),
    }
}

#[test]
fn tagesrechnung_deckel_ausschluesse_und_streamerwerte() {
    let mut input = DayInput {
        day: day("2026-10-01"),
        partners: vec![
            partner("100", "a"),
            partner("200", "b"),
            partner("300", "c"),
            partner("400", "d"),
        ],
        ..DayInput::default()
    };
    input.presence = vec![
        presence("100", "1", "viewer", 400.0, "2026-10-01T08:00:00Z"),
        presence("200", "1", "viewer", 400.0, "2026-10-01T09:00:00Z"),
        presence("300", "1", "viewer", 95.5, "2026-10-01T10:00:00Z"),
        presence("400", "1", "viewer", 10.0, "2026-10-01T11:00:00Z"),
        // Broadcaster im eigenen Kanal
        presence("100", "100", "a", 300.0, "2026-10-01T08:00:00Z"),
        // Bot
        presence("100", "7", "nightbot", 300.0, "2026-10-01T08:00:00Z"),
        // Kanal-Bann
        presence("100", "8", "banned", 300.0, "2026-10-01T08:00:00Z"),
        // globaler Bann per ID
        presence("100", "9", "global", 300.0, "2026-10-01T08:00:00Z"),
        // kein Partnerkanal
        presence("999", "1", "viewer", 300.0, "2026-10-01T08:00:00Z"),
        // zweiter Zuschauer, war früher schon da
        presence("100", "2", "zwei", 30.0, "2026-10-01T12:00:00Z"),
    ];
    input
        .channel_bans
        .insert(("100".to_string(), "8".to_string()));
    input.global_ban_ids.insert("9".to_string());
    input
        .seen_before
        .insert(("100".to_string(), "2".to_string()));
    input.chat.insert(
        ("100".into(), "1".into()),
        (0..40)
            .map(|i| ChatSample {
                at: ts("2026-10-01T08:00:00Z") + Duration::minutes(i),
                content: format!("Nachricht Nummer {i}"),
                is_command: false,
                moderated: false,
            })
            .collect(),
    );
    input.raids_to_partners.insert("200".into(), 1);

    let out = compute_day(&input);
    let row = |v: &str, c: &str| {
        out.viewers
            .iter()
            .find(|r| r.twitch_user_id == v && r.channel_twitch_user_id == c)
            .cloned()
    };
    assert!(row("100", "100").is_none());
    assert!(row("7", "100").is_none());
    assert!(row("8", "100").is_none());
    assert!(row("9", "100").is_none());
    assert!(row("1", "999").is_none());

    let r = row("1", "100").unwrap();
    assert_eq!((r.watch_minutes, r.points_watch), (400, 72));
    assert_eq!((r.chat_messages, r.points_chat), (40, 30));
    assert_eq!(r.points_discovery, 10);
    let r = row("1", "200").unwrap();
    assert_eq!(r.points_watch, 72);
    assert_eq!(r.points_discovery, 10);
    let r = row("1", "300").unwrap();
    assert_eq!(r.watch_minutes, 95);
    assert_eq!(r.points_watch, 0, "Tagesdeckel 144 bereits erreicht");
    assert_eq!(r.points_discovery, 10);
    let r = row("1", "400").unwrap();
    assert_eq!(r.points_discovery, 0, "höchstens 3 Boni je Tag");
    let r = row("2", "100").unwrap();
    assert_eq!((r.points_watch, r.points_discovery), (6, 0));

    assert_eq!(out.new_discoveries.len(), 5);
    assert_eq!(
        out.new_discoveries
            .iter()
            .filter(|d| d.bonus_awarded)
            .count(),
        3
    );

    let s100 = out
        .streamers
        .iter()
        .find(|s| s.streamer_twitch_user_id == "100")
        .unwrap();
    assert_eq!((s100.viewer_minutes, s100.unique_viewers), (430, 2));
    assert_eq!(s100.discord_user_id.as_deref(), Some("d100"));
    let s200 = out
        .streamers
        .iter()
        .find(|s| s.streamer_twitch_user_id == "200")
        .unwrap();
    assert_eq!(s200.raids_to_partners, 1);
}

#[test]
fn bestehende_entdeckungen_werden_nicht_neu_entschieden() {
    let mut input = DayInput {
        day: day("2026-10-01"),
        partners: vec![partner("100", "a"), partner("200", "b")],
        ..DayInput::default()
    };
    input.presence = vec![
        presence("100", "1", "viewer", 10.0, "2026-10-01T08:00:00Z"),
        presence("200", "1", "viewer", 10.0, "2026-10-01T09:00:00Z"),
    ];
    input.discoveries.insert(
        ("1".into(), "100".into()),
        KnownDiscovery {
            day: day("2026-10-01"),
            bonus_awarded: true,
        },
    );
    input.discoveries.insert(
        ("1".into(), "200".into()),
        KnownDiscovery {
            day: day("2026-09-01"),
            bonus_awarded: true,
        },
    );
    let out = compute_day(&input);
    assert!(out.new_discoveries.is_empty());
    let pd: Vec<i32> = out.viewers.iter().map(|r| r.points_discovery).collect();
    assert_eq!(pd, vec![10, 0]);
}

#[test]
fn cursor_format_ist_rfc3339_mit_mikrosekunden_wenn_noetig() {
    assert_eq!(
        format_cursor(ts("2026-10-01T20:00:00Z")),
        "2026-10-01T20:00:00Z"
    );
    assert_eq!(
        format_cursor(ts("2026-10-01T20:00:00.000123Z")),
        "2026-10-01T20:00:00.000123Z"
    );
}

// ─── DB-Tests (Wegwerf-Timescale, alle Migrationen) ─────────────────────────

pub(crate) mod db {
    use sqlx::postgres::PgPoolOptions;
    use sqlx::PgPool;

    /// Isolierte Datenbank mit allen Migrationen; fehlende Infrastruktur ist ein Fehler.
    pub async fn migrated_pool(db_name: &str) -> crate::test_postgres::TestPostgres {
        migrated_pool_with_max_connections(db_name, 3).await
    }

    pub async fn migrated_pool_with_max_connections(
        _db_name: &str,
        max_connections: u32,
    ) -> crate::test_postgres::TestPostgres {
        let mut db = crate::test_postgres::TestPostgres::start_with_timescaledb().await;
        db.pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect_with((*db.pool.connect_options()).clone())
            .await
            .expect("Isolierter Pool");
        let pool = db.pool.clone();
        sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
            .execute(&pool)
            .await
            .expect("TimescaleDB");
        // Migrationen legen globale Rollen an: parallel laufende Tests würden
        // sich dabei gegenseitig stören ("tuple concurrently updated").
        static MIGRATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _guard = MIGRATE_LOCK.lock().await;
        tb_db::migrate::MIGRATOR
            .run(&pool)
            .await
            .expect("Migrationen");
        db
    }

    /// Partner 100 (alpha) und 200 (beta), beide aktiv; Session je Kanal.
    pub async fn seed_partners(pool: &PgPool) {
        sqlx::raw_sql(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES
                 ('alpha', '100'), ('beta', '200');
             INSERT INTO twitch_partners (twitch_user_id, twitch_login, status)
                 VALUES ('100', 'alpha', 'active'), ('200', 'beta', 'active');
             UPDATE twitch_streamer_identities SET discord_user_id = '789'
              WHERE twitch_user_id = '100';
             INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, twitch_user_id)
                 VALUES (1, 'alpha', '2026-10-01 06:00Z', '100'),
                        (2, 'beta', '2026-10-01 06:00Z', '200');",
        )
        .execute(pool)
        .await
        .expect("seed partners");
    }
}

async fn ticks(pool: &PgPool, session: i64, login: &str, id: &str, from: &str, n: i32) {
    sqlx::query(
        "INSERT INTO twitch_session_chatters
             (session_id, streamer_login, chatter_login, chatter_id, first_message_at)
         SELECT $1, s.streamer_login, $2, $3, $4::timestamptz
           FROM twitch_stream_sessions s WHERE s.id = $1
         ON CONFLICT DO NOTHING",
    )
    .bind(session)
    .bind(login)
    .bind(id)
    .bind(ts(from))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_viewer_presence_ticks (session_id, streamer_login, viewer_login, tick_at, viewer_twitch_user_id)
         SELECT $1, s.streamer_login, $2, $3::timestamptz + g * INTERVAL '30 seconds', $5
           FROM twitch_stream_sessions s, generate_series(0, $4 - 1) g WHERE s.id = $1",
    )
    .bind(session)
    .bind(login)
    .bind(ts(from))
    .bind(n)
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn aggregation_aus_rohdaten_ist_idempotent() {
    let _db = db::migrated_pool("tb_cp_aggregation").await;
    let pool = _db.pool.clone();
    db::seed_partners(&pool).await;
    let partners = load_active_partners(&pool).await.unwrap();
    assert_eq!(partners.len(), 2, "{partners:?}");

    // Zuschauer 1: 60 min durchgehend bei alpha (121 Ticks à 30 s = 60 min + 60 s).
    ticks(&pool, 1, "viewer", "1", "2026-10-01T08:00:00Z", 121).await;
    // Zuschauer 1 bei beta mit Lücke von 10 min: 2 × 10 min.
    ticks(&pool, 2, "viewer", "1", "2026-10-01T09:00:00Z", 20).await;
    ticks(&pool, 2, "viewer", "1", "2026-10-01T09:20:00Z", 20).await;
    // Broadcaster alpha bei sich selbst: ausgeschlossen.
    ticks(&pool, 1, "alpha", "100", "2026-10-01T08:00:00Z", 50).await;
    // Bot: ausgeschlossen.
    ticks(&pool, 1, "nightbot", "55", "2026-10-01T08:00:00Z", 50).await;
    // Zuschauer 2 war vor dem Tag schon bei alpha (Rollup), chattet nur.
    sqlx::raw_sql(
        "INSERT INTO twitch_chatter_rollup (streamer_login, chatter_login, chatter_id, first_seen_at, last_seen_at)
             VALUES ('alpha', 'zwei', '2', '2026-09-01 10:00Z', '2026-09-01 10:00Z');
         INSERT INTO twitch_chat_messages (session_id, streamer_login, chatter_login, chatter_id, message_ts, is_command, content) VALUES
             (1, 'alpha', 'zwei', '2', '2026-10-01 10:00:00Z', FALSE, 'hallo zusammen alle'),
             (1, 'alpha', 'zwei', '2', '2026-10-01 10:00:30Z', FALSE, 'zu schnell hinterher'),
             (1, 'alpha', 'zwei', '2', '2026-10-01 10:02:00Z', TRUE,  '!discord bitte posten'),
             (1, 'alpha', 'zwei', '2', '2026-10-01 10:03:00Z', FALSE, 'kurz'),
             (1, 'alpha', 'zwei', '2', '2026-10-01 10:04:00Z', FALSE, 'hallo zusammen alle'),
             (1, 'alpha', 'zwei', '2', '2026-10-01 10:05:00Z', FALSE, 'schöner Stream heute');
         INSERT INTO twitch_raid_history (from_broadcaster_id, from_broadcaster_login, to_broadcaster_id, to_broadcaster_login, executed_at, success) VALUES
             ('100', 'alpha', '200', 'beta', '2026-10-01 12:00Z', TRUE),
             ('100', 'alpha', '200', 'beta', '2026-10-01 13:00Z', FALSE),
             ('100', 'alpha', '999', 'fremd', '2026-10-01 14:00Z', TRUE);",
    )
    .execute(&pool)
    .await
    .unwrap();

    let now = ts("2026-10-01T20:00:00Z");
    let d = day("2026-10-01");
    let first = aggregate_day(&pool, d, now).await.unwrap();
    assert_eq!(first.viewer_rows_changed, 3);
    assert_eq!(first.streamer_rows_changed, 2);
    assert_eq!(first.discoveries_inserted, 3);

    let page = list_viewer_points(&pool, None, 100).await.unwrap();
    assert!(!page.has_more);
    let get = |v: &str, c: &str| {
        page.rows
            .iter()
            .find(|r| r.twitch_user_id == v && r.channel_twitch_user_id == c)
            .cloned()
            .unwrap()
    };
    let r = get("1", "100");
    assert_eq!(r.watch_minutes, 61);
    assert_eq!(r.points_watch, 12);
    assert_eq!(r.points_discovery, 10);
    assert_eq!(r.day, "2026-10-01");
    assert_eq!(r.twitch_login, "viewer");
    let r = get("1", "200");
    // je Block 20 Ticks: 9:30 min plus 60 s Abdeckung des letzten Ticks
    assert_eq!(
        r.watch_minutes, 21,
        "zwei Blöcke à 10,5 min, Lücke zählt nicht"
    );
    assert_eq!((r.points_watch, r.points_discovery), (4, 10));
    let r = get("2", "100");
    // zählt: 10:00, 10:04 (Vorgänger war "kurz", also kein Duplikat), 10:05
    assert_eq!((r.chat_messages, r.points_chat), (3, 3));
    assert_eq!(r.points_discovery, 0, "Rollup belegt früheren Besuch");

    let streamers = list_streamer_points(&pool, None, 100).await.unwrap();
    let alpha = streamers
        .rows
        .iter()
        .find(|s| s.streamer_twitch_user_id == "100")
        .unwrap();
    assert_eq!(alpha.raids_to_partners, 1);
    assert_eq!(alpha.unique_viewers, 2);
    assert_eq!(alpha.discord_user_id.as_deref(), Some("789"));
    assert_eq!(alpha.streamer_login, "alpha");

    // Zweiter Lauf ohne neue Daten: nichts ändert sich, Cursor bleibt.
    let second = aggregate_day(&pool, d, now).await.unwrap();
    assert_eq!(second, DayWriteStats::default());
    let since = DateTime::parse_from_rfc3339(page.next_updated_since.as_deref().unwrap())
        .unwrap()
        .with_timezone(&Utc);
    assert!(list_viewer_points(&pool, Some(since), 100)
        .await
        .unwrap()
        .rows
        .is_empty());

    // Bann nachträglich: Zeile wird genullt und taucht hinter dem Cursor auf.
    sqlx::query(
        "INSERT INTO twitch_ban_events (twitch_user_id, event_type, target_login, target_id, received_at)
         VALUES ('200', 'ban', 'viewer', '1', '2026-10-01 15:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let third = aggregate_day(&pool, d, now).await.unwrap();
    assert_eq!(third.viewer_rows_zeroed, 1);
    let delta = list_viewer_points(&pool, Some(since), 100).await.unwrap();
    assert_eq!(delta.rows.len(), 1);
    assert_eq!(delta.rows[0].channel_twitch_user_id, "200");
    assert_eq!(delta.rows[0].points_watch, 0);
    // Entdeckung bleibt festgehalten, Bonus wird nicht neu vergeben.
    let discoveries: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_community_points_discoveries")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(discoveries, 3);
}

#[tokio::test]
async fn beendete_session_begrenzt_tick_abdeckung() {
    let _db = db::migrated_pool("tb_cp_ended_session").await;
    let pool = _db.pool.clone();
    db::seed_partners(&pool).await;
    ticks(&pool, 1, "viewer", "3", "2026-10-01T08:00:00Z", 10).await;
    sqlx::query("UPDATE twitch_stream_sessions SET ended_at = $2 WHERE id = $1")
        .bind(1_i64)
        .bind(ts("2026-10-01T08:04:40Z"))
        .execute(&pool)
        .await
        .unwrap();

    let input = load_day_input(&pool, day("2026-10-01"), ts("2026-10-01T20:00:00Z"))
        .await
        .unwrap();
    let presence = input
        .presence
        .iter()
        .find(|row| row.viewer_id == "3" && row.channel_id == "100")
        .unwrap();
    assert_eq!(presence.seconds, 280.0);

    let output = compute_day(&input);
    let viewer = output
        .viewers
        .iter()
        .find(|row| row.twitch_user_id == "3" && row.channel_twitch_user_id == "100")
        .unwrap();
    assert_eq!(viewer.watch_minutes, 4);
    assert_eq!(viewer.points_watch, 0);
}

#[tokio::test]
async fn sessiongrenzen_sperren_watchtime_chat_und_entdeckerbonus() {
    let db = db::migrated_pool("tb_cp_session_boundaries").await;
    let pool = &db.pool;
    db::seed_partners(pool).await;
    sqlx::query("UPDATE twitch_stream_sessions SET started_at='2026-10-01 08:00Z', ended_at='2026-10-01 08:04:30Z' WHERE id=1")
        .execute(pool).await.unwrap();
    ticks(pool, 1, "vor_start", "901", "2026-10-01T07:59:30Z", 1).await;
    ticks(pool, 1, "ab_ende", "902", "2026-10-01T08:04:30Z", 2).await;
    ticks(pool, 1, "gueltig", "903", "2026-10-01T08:00:00Z", 9).await;
    ticks(pool, 2, "laufend", "904", "2026-10-01T10:00:00Z", 10).await;
    sqlx::raw_sql("INSERT INTO twitch_chat_messages(session_id,streamer_login,chatter_login,chatter_id,message_ts,is_command,content) VALUES
        (1,'alpha','vor_start','901','2026-10-01 07:59:30Z',FALSE,'Nachricht vor dem Start'),
        (1,'alpha','ab_ende','902','2026-10-01 08:04:30Z',FALSE,'Nachricht genau am Ende'),
        (1,'alpha','nur_chat_danach','906','2026-10-01 08:05Z',FALSE,'Nur Chat nach dem Ende'),
        (1,'alpha','gueltig','903','2026-10-01 07:59:30Z',FALSE,'Hallo genau am Anfang'),
        (1,'alpha','gueltig','903','2026-10-01 08:00Z',FALSE,'Hallo genau am Anfang'),
        (1,'alpha','gueltig','903','2026-10-01 08:01Z',FALSE,'Zweite gültige Nachricht'),
        (1,'alpha','gueltig','903','2026-10-01 08:04Z',FALSE,'Letzte gültige Nachricht'),
        (1,'alpha','gueltig','903','2026-10-01 08:04:30Z',FALSE,'Ab Ende nicht mehr zählen'),
        (2,'beta','laufend','904','2026-10-01 10:01Z',FALSE,'Laufende Session zählt'),
        (2,'beta','nur_chat_laufend','905','2026-10-01 10:06Z',FALSE,'Auch Chat ohne Helix-Tick');")
        .execute(pool).await.unwrap();
    let d = day("2026-10-01");
    let now = ts("2026-10-01T12:00:00Z");
    let input = load_day_input(pool, d, now).await.unwrap();
    assert_eq!(input.presence.len(), 3);
    for id in ["901", "902", "906"] {
        assert!(!input.presence.iter().any(|row| row.viewer_id == id));
        assert!(!input.chat.keys().any(|(_, viewer)| viewer == id));
    }
    let closed = input
        .presence
        .iter()
        .find(|row| row.viewer_id == "903")
        .unwrap();
    assert_eq!(closed.first_seen_at, ts("2026-10-01T08:00:00Z"));
    assert_eq!(closed.seconds, 270.0);
    assert_eq!(input.chat[&("100".into(), "903".into())].len(), 3);
    aggregate_day(pool, d, now).await.unwrap();
    let page = list_viewer_points(pool, None, 100).await.unwrap();
    assert_eq!(page.rows.len(), 3);
    let row = |id: &str| {
        page.rows
            .iter()
            .find(|row| row.twitch_user_id == id)
            .unwrap()
    };
    assert_eq!(
        (
            row("903").watch_minutes,
            row("903").points_watch,
            row("903").points_chat,
            row("903").points_discovery
        ),
        (4, 0, 3, 10)
    );
    assert_eq!(
        (
            row("904").watch_minutes,
            row("904").points_watch,
            row("904").points_chat,
            row("904").points_discovery
        ),
        (5, 1, 1, 10)
    );
    assert_eq!(
        (row("905").points_chat, row("905").points_discovery),
        (1, 10)
    );
    let excluded_discoveries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_community_points_discoveries WHERE twitch_user_id IN ('901','902','906')")
        .fetch_one(pool).await.unwrap();
    assert_eq!(excluded_discoveries, 0);
}

#[tokio::test]
async fn consent_grenze_liefert_nur_neue_rohaktivitaet_ohne_alten_tagesbonus() {
    let db = db::migrated_pool("tb_cp_consent").await;
    let pool = &db.pool;
    db::seed_partners(pool).await;
    ticks(pool, 1, "viewer", "111", "2026-10-01T08:00:00Z", 12).await;
    ticks(pool, 1, "viewer", "111", "2026-10-01T09:59:30Z", 1).await;
    sqlx::query("INSERT INTO twitch_chat_messages(session_id,streamer_login,chatter_login,chatter_id,message_ts,content) VALUES (1,'alpha','viewer','111','2026-10-01 08:00Z','Alte Nachricht bleibt gelöscht'),(1,'alpha','viewer','111','2026-10-01 09:59:30Z','Neue Nachricht nach Consent')")
        .execute(pool).await.unwrap();
    let d = day("2026-10-01");
    let consent = ts("2026-10-01T10:00:00Z");
    let now = ts("2026-10-01T20:00:00Z");
    aggregate_day(pool, d, now).await.unwrap();
    let empty = viewer_activity_since(pool, "111", d, consent, now)
        .await
        .unwrap();
    assert!(
        empty.rows.is_empty(),
        "Ein alter Tick über die Consent-Grenze zählt nicht erneut"
    );
    ticks(pool, 1, "viewer", "111", "2026-10-01T10:00:00Z", 10).await;
    ticks(pool, 1, "andere_person", "222", "2026-10-01T10:00:00Z", 10).await;
    sqlx::raw_sql("INSERT INTO twitch_chat_messages(session_id,streamer_login,chatter_login,chatter_id,message_ts,content) VALUES
        (1,'alpha','viewer','111','2026-10-01 10:00Z','Neue Nachricht nach Consent'),
        (1,'alpha','viewer','111','2026-10-01 10:00:30Z','Noch innerhalb des Cooldowns'),
        (1,'alpha','viewer','111','2026-10-01 10:01Z','Neue Nachricht nach Consent'),
        (1,'alpha','viewer','111','2026-10-01 10:02Z','Zweite Nachricht nach Consent');")
        .execute(pool).await.unwrap();
    aggregate_day(pool, d, now).await.unwrap();
    let stored_before = list_viewer_points(pool, None, 100).await.unwrap();
    let page = viewer_activity_since(pool, "111", d, consent, now)
        .await
        .unwrap();
    assert_eq!(page.twitch_user_id, "111");
    assert_eq!(page.day, "2026-10-01");
    assert_eq!(page.activity_since, "2026-10-01T10:00:00Z");
    assert_eq!(page.computed_at, "2026-10-01T20:00:00Z");
    assert_eq!(page.rows.len(), 1);
    let row = &page.rows[0];
    assert_eq!(
        (
            row.watch_minutes,
            row.points_watch,
            row.chat_messages,
            row.points_chat,
            row.points_discovery
        ),
        (5, 1, 3, 3, 0)
    );
    assert_eq!(
        list_viewer_points(pool, None, 100).await.unwrap(),
        stored_before,
        "Consent-Leseweg verändert keine Tageswerte"
    );
    let fresh = viewer_activity_since(pool, "222", d, consent, now)
        .await
        .unwrap();
    assert_eq!(
        fresh.rows[0].points_discovery, 10,
        "Eine tatsächlich neue Entdeckung ab Consent bleibt gültig"
    );
    assert!(
        viewer_activity_since(pool, "111", day("2026-09-30"), consent, now)
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    assert!(viewer_activity_since(pool, "111", d, now, now)
        .await
        .unwrap()
        .rows
        .is_empty());
}

#[tokio::test]
async fn historische_rohänderung_bleibt_bei_punktedeckel_hinter_cursor_sichtbar() {
    let db = db::migrated_pool("tb_cp_dirty_day").await;
    let pool = &db.pool;
    db::seed_partners(pool).await;
    ticks(pool, 1, "viewer", "111", "2026-10-01T08:00:00Z", 720).await;
    let d = day("2026-10-01");
    aggregate_day(pool, d, ts("2026-10-01T20:00:00Z"))
        .await
        .unwrap();
    let before = list_viewer_points(pool, None, 100).await.unwrap();
    assert_eq!(before.rows[0].points_watch, 72);
    assert_eq!(before.rows[0].watch_minutes, 360);
    let since = ts(before.next_updated_since.as_deref().unwrap());
    ticks(pool, 1, "viewer", "111", "2026-10-01T14:00:00Z", 10).await;
    let days = run_aggregation(pool, ts("2026-10-03T20:00:00Z"), false)
        .await
        .unwrap();
    assert!(
        days.iter().any(|(day, _)| *day == d),
        "Auch alte markierte Tage werden neu berechnet"
    );
    let after = list_viewer_points(pool, Some(since), 100).await.unwrap();
    assert_eq!(after.rows.len(), 1);
    assert_eq!(after.rows[0].points_watch, 72);
    assert_eq!(after.rows[0].watch_minutes, 365);
    let new = viewer_activity_since(
        pool,
        "111",
        d,
        ts("2026-10-01T14:00:00Z"),
        ts("2026-10-03T20:00:00Z"),
    )
    .await
    .unwrap();
    assert_eq!(new.rows[0].watch_minutes, 5);
    assert_eq!(
        aggregate_day(pool, d, ts("2026-10-03T20:00:00Z"))
            .await
            .unwrap(),
        DayWriteStats::default()
    );
    sqlx::query("DELETE FROM twitch_viewer_presence_ticks WHERE viewer_twitch_user_id='111' AND tick_at >= '2026-10-01 14:00Z'")
        .execute(pool).await.unwrap();
    run_aggregation(pool, ts("2026-10-03T20:00:00Z"), false)
        .await
        .unwrap();
    assert!(viewer_activity_since(
        pool,
        "111",
        d,
        ts("2026-10-01T14:00:00Z"),
        ts("2026-10-03T20:00:00Z")
    )
    .await
    .unwrap()
    .rows
    .is_empty());
}

#[tokio::test]
async fn historische_mitternachtsänderungen_korrigieren_chat_und_präsenz_im_folgetag() {
    let db = db::migrated_pool("tb_cp_dirty_midnight").await;
    let pool = &db.pool;
    db::seed_partners(pool).await;
    sqlx::query("UPDATE twitch_stream_sessions SET ended_at='2026-10-01 22:01:15Z' WHERE id=1")
        .execute(pool)
        .await
        .unwrap();
    ticks(pool, 1, "viewer", "111", "2026-10-01T22:00:10Z", 1).await;
    ticks(pool, 1, "zweite_person", "222", "2026-10-01T21:59:30Z", 1).await;
    ticks(pool, 1, "zweite_person", "222", "2026-10-01T22:00:45Z", 1).await;
    sqlx::raw_sql("INSERT INTO twitch_chat_messages(session_id,streamer_login,chatter_login,chatter_id,message_ts,content) VALUES
        (1,'alpha','viewer','111','2026-10-01 21:59:30Z','Nachricht über die Tagesgrenze hinweg'),
        (1,'alpha','viewer','111','2026-10-01 22:01:00Z','Nachricht über die Tagesgrenze hinweg');")
        .execute(pool).await.unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT bool_and(tableoid <> 'public.twitch_chat_messages'::regclass)
             FROM twitch_chat_messages",
        )
        .fetch_one(pool)
        .await
        .unwrap()
    );
    let previous = day("2026-10-01");
    let following = day("2026-10-02");
    let now = ts("2026-10-04T12:00:00Z");
    aggregate_day(pool, previous, now).await.unwrap();
    aggregate_day(pool, following, now).await.unwrap();
    let before = list_viewer_points(pool, None, 100).await.unwrap();
    let get = |rows: &[ViewerPointsRow], id: &str| {
        rows.iter()
            .find(|row| row.day == "2026-10-02" && row.twitch_user_id == id)
            .unwrap()
            .clone()
    };
    assert_eq!(get(&before.rows, "111").chat_messages, 0);
    assert_eq!(get(&before.rows, "222").watch_minutes, 1);
    let consent = berlin_day_bounds(following).0;
    assert_eq!(
        viewer_activity_since(pool, "222", following, consent, now)
            .await
            .unwrap()
            .rows[0]
            .watch_minutes,
        0,
        "Alte Intervalle vor Consent dürfen nicht erneut zählen"
    );
    let mut cursor = ts(before.next_updated_since.as_deref().unwrap());

    // Erst löschen, dann erneut einfügen und schließlich den Zeitstempel verschieben.
    for operation in ["delete", "insert", "update"] {
        match operation {
            "delete" => {
                sqlx::raw_sql(
                    "DELETE FROM twitch_chat_messages WHERE message_ts='2026-10-01 21:59:30Z';
                    DELETE FROM twitch_viewer_presence_ticks WHERE tick_at='2026-10-01 21:59:30Z';",
                )
                .execute(pool)
                .await
                .unwrap();
            }
            "insert" => {
                sqlx::query("INSERT INTO twitch_chat_messages(session_id,streamer_login,chatter_login,chatter_id,message_ts,content)
                    VALUES (1,'alpha','viewer','111','2026-10-01 21:59:30Z','Nachricht über die Tagesgrenze hinweg')")
                    .execute(pool).await.unwrap();
                ticks(pool, 1, "zweite_person", "222", "2026-10-01T21:59:30Z", 1).await;
            }
            "update" => {
                // Timescale erlaubt UPDATEs der Zeitspalte innerhalb eines Chunks.
                // Die Korrektur überschreitet Berliner Mitternacht und das Sessionende,
                // bleibt aber im selben UTC-Tageschunk der echten Chat-Hypertable.
                sqlx::raw_sql("UPDATE twitch_chat_messages SET message_ts='2026-10-01 22:02:30Z' WHERE message_ts='2026-10-01 21:59:30Z';
                    UPDATE twitch_viewer_presence_ticks SET tick_at='2026-10-01 22:02:30Z' WHERE tick_at='2026-10-01 21:59:30Z';")
                    .execute(pool).await.unwrap();
            }
            _ => unreachable!(),
        }
        let marked: Vec<NaiveDate> =
            sqlx::query_scalar("SELECT day FROM twitch_community_points_dirty_days ORDER BY day")
                .fetch_all(pool)
                .await
                .unwrap();
        assert!(marked.contains(&previous));
        assert!(
            marked.contains(&following),
            "Der Folgetag fehlt bei {operation}"
        );
        let days = run_aggregation(pool, now, false).await.unwrap();
        assert!(days.iter().any(|(marked, _)| *marked == following));
        let after = list_viewer_points(pool, Some(cursor), 100).await.unwrap();
        let restored = operation == "insert";
        assert_eq!(
            get(&after.rows, "111").chat_messages,
            if restored { 0 } else { 1 }
        );
        assert_eq!(
            get(&after.rows, "222").watch_minutes,
            if restored { 1 } else { 0 }
        );
        assert_eq!(
            viewer_activity_since(pool, "222", following, consent, now)
                .await
                .unwrap()
                .rows[0]
                .watch_minutes,
            0
        );
        cursor = ts(after.next_updated_since.as_deref().unwrap());
    }
}

async fn wait_for_dirty_day_fixture_lock(pool: &PgPool) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let waiting: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_locks
             WHERE locktype='advisory' AND objid::bigint=74103 AND NOT granted)",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        if waiting {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Erwarteter echter Dirty-Day-Wartezustand fehlt"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

const DIRTY_DAY_RACE_INSERT: &str = "INSERT INTO twitch_viewer_presence_ticks
    (session_id,streamer_login,viewer_login,tick_at,viewer_twitch_user_id)
    VALUES (1,'alpha','viewer','2026-10-01 08:05Z','111')";

async fn dirty_day_write_race(aggregation_first: bool) {
    let db = db::migrated_pool_with_max_connections("tb_cp_dirty_race", 5).await;
    let pool = &db.pool;
    db::seed_partners(pool).await;
    ticks(pool, 1, "viewer", "111", "2026-10-01T08:00:00Z", 10).await;
    let d = day("2026-10-01");
    let now = ts("2026-10-03T12:00:00Z");
    aggregate_day(pool, d, now).await.unwrap();
    let before = list_viewer_points(pool, None, 100).await.unwrap();
    assert_eq!(before.rows[0].watch_minutes, 5);
    let cursor = ts(before.next_updated_since.as_deref().unwrap());
    sqlx::query("INSERT INTO twitch_community_points_dirty_days(day) VALUES ($1)")
        .bind(d)
        .execute(pool)
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE FUNCTION dirty_day_fixture_pause() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN PERFORM pg_advisory_xact_lock(74103); RETURN NULL; END $$;",
    )
    .execute(pool)
    .await
    .unwrap();
    let trigger = if aggregation_first {
        "CREATE TRIGGER fixture_pause_dirty AFTER DELETE ON twitch_community_points_dirty_days
         FOR EACH ROW EXECUTE FUNCTION dirty_day_fixture_pause()"
    } else {
        // Die neue Generation ist geschrieben, aber noch nicht committed.
        "CREATE TRIGGER z_fixture_pause_dirty AFTER INSERT ON twitch_viewer_presence_ticks
         FOR EACH ROW EXECUTE FUNCTION dirty_day_fixture_pause()"
    };
    sqlx::raw_sql(trigger).execute(pool).await.unwrap();
    let mut pause = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(74103)")
        .execute(&mut *pause)
        .await
        .unwrap();
    let first_pool = (*pool).clone();
    let first = tokio::spawn(async move {
        if aggregation_first {
            aggregate_day(&first_pool, d, now).await.unwrap();
        } else {
            sqlx::query(DIRTY_DAY_RACE_INSERT)
                .execute(&first_pool)
                .await
                .unwrap();
        }
    });
    wait_for_dirty_day_fixture_lock(pool).await;
    let second_pool = (*pool).clone();
    let second = tokio::spawn(async move {
        if aggregation_first {
            sqlx::query(DIRTY_DAY_RACE_INSERT)
                .execute(&second_pool)
                .await
                .unwrap();
        } else {
            aggregate_day(&second_pool, d, now).await.unwrap();
        }
    });
    // Der zweite Pfad muss fertig werden, während der erste noch pausiert ist.
    // So belegt die DB sowohl nicht blockierte Erfassung als auch nicht verlorene Marker.
    tokio::time::timeout(std::time::Duration::from_secs(5), second)
        .await
        .expect("Rohschreiber und Aggregation blockieren einander")
        .unwrap();
    pause.commit().await.unwrap();
    first.await.unwrap();
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM twitch_community_points_dirty_days WHERE day=$1)",
    )
    .bind(d)
    .fetch_one(pool)
    .await
    .unwrap();
    let intermediate = list_viewer_points(pool, Some(cursor), 100).await.unwrap();
    assert_eq!(intermediate.rows.len(), 1);
    assert!(pending, "Die spätere Rohgeneration muss erhalten bleiben");
    assert_eq!(intermediate.rows[0].watch_minutes, 5);
    run_aggregation(pool, now, false).await.unwrap();
    let after = list_viewer_points(pool, Some(cursor), 100).await.unwrap();
    assert_eq!(after.rows.len(), 1);
    assert_eq!(after.rows[0].watch_minutes, 6);
    assert_eq!(after.rows[0].points_watch, 1);
    assert_eq!(
        aggregate_day(pool, d, now).await.unwrap(),
        DayWriteStats::default()
    );
}

#[tokio::test]
async fn historische_rohänderung_vor_aggregation_geht_nicht_verloren() {
    dirty_day_write_race(false).await;
}

#[tokio::test]
async fn historische_rohänderung_nach_aggregation_bleibt_markiert() {
    dirty_day_write_race(true).await;
}

#[tokio::test]
async fn historische_sessionkorrektur_markiert_grenzen_und_identität_nach_aggregation() {
    let db = db::migrated_pool("tb_cp_session_dirty").await;
    let pool = &db.pool;
    db::seed_partners(pool).await;
    sqlx::query("UPDATE twitch_stream_sessions SET ended_at='2026-10-01 09:00Z' WHERE id=1")
        .execute(pool)
        .await
        .unwrap();
    ticks(pool, 1, "viewer", "111", "2026-10-01T08:00:00Z", 10).await;
    sqlx::query("INSERT INTO twitch_chat_messages(session_id,streamer_login,chatter_login,chatter_id,message_ts,content) VALUES (1,'alpha','viewer','111','2026-10-01 08:06Z','Spätere Nachricht im Stream')")
        .execute(pool).await.unwrap();
    let d = day("2026-10-01");
    let now = ts("2026-10-03T12:00:00Z");
    run_aggregation(pool, now, false).await.unwrap();
    let before = list_viewer_points(pool, None, 100).await.unwrap();
    assert_eq!(before.rows.len(), 1);
    assert_eq!(
        (before.rows[0].points_watch, before.rows[0].points_chat),
        (1, 1)
    );
    let since = ts(before.next_updated_since.as_deref().unwrap());

    // Die historischen Rohzeilen bleiben unverändert; nur die Session wird korrigiert.
    sqlx::query("UPDATE twitch_stream_sessions SET started_at='2026-10-01 08:03Z',ended_at='2026-10-01 08:04:30Z' WHERE id=1")
        .execute(pool).await.unwrap();
    let days = run_aggregation(pool, now, false).await.unwrap();
    assert!(days.iter().any(|(marked, _)| *marked == d));
    let after = list_viewer_points(pool, Some(since), 100).await.unwrap();
    assert_eq!(after.rows.len(), 1);
    assert_eq!(
        (
            after.rows[0].watch_minutes,
            after.rows[0].points_watch,
            after.rows[0].points_chat
        ),
        (1, 0, 0)
    );
    let since = ts(after.next_updated_since.as_deref().unwrap());

    sqlx::query("UPDATE twitch_stream_sessions SET twitch_user_id='200' WHERE id=1")
        .execute(pool)
        .await
        .unwrap();
    run_aggregation(pool, now, false).await.unwrap();
    let moved = list_viewer_points(pool, Some(since), 100).await.unwrap();
    assert_eq!(moved.rows.len(), 2);
    let old = moved
        .rows
        .iter()
        .find(|r| r.channel_twitch_user_id == "100")
        .unwrap();
    let new = moved
        .rows
        .iter()
        .find(|r| r.channel_twitch_user_id == "200")
        .unwrap();
    assert_eq!(
        (
            old.watch_minutes,
            old.points_watch,
            old.points_chat,
            old.points_discovery
        ),
        (0, 0, 0, 0)
    );
    assert_eq!(new.watch_minutes, 1);

    sqlx::query("UPDATE twitch_stream_sessions SET started_at='2026-09-30 20:00Z',ended_at='2026-09-30 21:00Z' WHERE id=1")
        .execute(pool).await.unwrap();
    let days = run_aggregation(pool, now, false).await.unwrap();
    assert!(days.iter().any(|(marked, _)| *marked == day("2026-09-30")));
    assert!(days.iter().any(|(marked, _)| *marked == d));
    let value: i32 = sqlx::query_scalar("SELECT points_watch + points_chat + points_discovery FROM twitch_community_points_viewer_daily WHERE twitch_user_id='111' AND channel_twitch_user_id='200' AND day=$1")
        .bind(d).fetch_one(pool).await.unwrap();
    assert_eq!(value, 0);
}

#[tokio::test]
async fn historische_raidkorrektur_markiert_erfolg_tag_und_beide_ids_nach_aggregation() {
    let db = db::migrated_pool("tb_cp_raid_dirty").await;
    let pool = &db.pool;
    db::seed_partners(pool).await;
    // Beide historischen Tage liegen im selben echten Timescale-Wochenchunk.
    // Die Korrektur muss trotzdem alten und neuen Berliner Tag invalidieren.
    let id: i64 = sqlx::query_scalar("INSERT INTO twitch_raid_history(from_broadcaster_id,from_broadcaster_login,to_broadcaster_id,to_broadcaster_login,executed_at,success) VALUES ('100','alpha','200','beta','2026-09-29 12:00Z',TRUE) RETURNING id")
        .fetch_one(pool).await.unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT tableoid <> 'public.twitch_raid_history'::regclass
             FROM twitch_raid_history WHERE id=$1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
    );
    let now = ts("2026-10-03T12:00:00Z");
    run_aggregation(pool, now, false).await.unwrap();
    let before = list_streamer_points(pool, None, 100).await.unwrap();
    assert_eq!(before.rows[0].raids_to_partners, 1);
    let since = ts(before.next_updated_since.as_deref().unwrap());

    sqlx::query("UPDATE twitch_raid_history SET success=FALSE WHERE id=$1")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    let days = run_aggregation(pool, now, false).await.unwrap();
    assert!(days.iter().any(|(marked, _)| *marked == day("2026-09-29")));
    let after = list_streamer_points(pool, Some(since), 100).await.unwrap();
    assert_eq!(after.rows.len(), 1);
    assert_eq!(after.rows[0].raids_to_partners, 0);

    sqlx::query(
        "UPDATE twitch_raid_history SET success=TRUE,executed_at='2026-09-30 12:00Z' WHERE id=$1",
    )
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
    let days = run_aggregation(pool, now, false).await.unwrap();
    for expected in [day("2026-09-30"), day("2026-09-29")] {
        assert!(days.iter().any(|(marked, _)| *marked == expected));
    }
    sqlx::query("UPDATE twitch_raid_history SET from_broadcaster_id='200',to_broadcaster_id='100' WHERE id=$1")
        .bind(id).execute(pool).await.unwrap();
    run_aggregation(pool, now, false).await.unwrap();
    let rows = list_streamer_points(pool, None, 100).await.unwrap().rows;
    assert_eq!(
        rows.iter()
            .find(|r| r.day == "2026-09-30" && r.streamer_twitch_user_id == "100")
            .unwrap()
            .raids_to_partners,
        0
    );
    assert_eq!(
        rows.iter()
            .find(|r| r.day == "2026-09-30" && r.streamer_twitch_user_id == "200")
            .unwrap()
            .raids_to_partners,
        1
    );

    sqlx::query("UPDATE twitch_raid_history SET to_broadcaster_id='999' WHERE id=$1")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    run_aggregation(pool, now, false).await.unwrap();
    let raids: i32 = sqlx::query_scalar("SELECT raids_to_partners FROM twitch_community_points_streamer_daily WHERE streamer_twitch_user_id='200' AND day='2026-09-30'")
        .fetch_one(pool).await.unwrap();
    assert_eq!(raids, 0);
    sqlx::query("UPDATE twitch_raid_history SET to_broadcaster_id='100' WHERE id=$1")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    run_aggregation(pool, now, false).await.unwrap();
    let before = list_streamer_points(pool, None, 100).await.unwrap();
    let since = ts(before.next_updated_since.as_deref().unwrap());
    sqlx::query("DELETE FROM twitch_raid_history WHERE id=$1")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    run_aggregation(pool, now, false).await.unwrap();
    let after = list_streamer_points(pool, Some(since), 100).await.unwrap();
    assert_eq!(after.rows.len(), 1);
    assert_eq!(after.rows[0].raids_to_partners, 0);
}

#[tokio::test]
async fn aggregation_hält_laden_und_schreiben_auf_einer_pool_connection() {
    let _db = db::migrated_pool_with_max_connections("tb_cp_single_connection", 1).await;
    let pool = _db.pool.clone();

    let stats = aggregate_day(&pool, day("2026-10-01"), ts("2026-10-01T20:00:00Z"))
        .await
        .unwrap();
    assert_eq!(stats, DayWriteStats::default());
}

#[tokio::test]
async fn cursor_ist_stabil_und_eindeutig() {
    let _db = db::migrated_pool("tb_cp_cursor").await;
    let pool = _db.pool.clone();
    db::seed_partners(&pool).await;
    for i in 0..25 {
        ticks(
            &pool,
            1,
            &format!("v{i}"),
            &format!("{}", 1000 + i),
            "2026-10-01T08:00:00Z",
            12,
        )
        .await;
    }
    aggregate_day(&pool, day("2026-10-01"), ts("2026-10-01T20:00:00Z"))
        .await
        .unwrap();
    let mut since = None;
    let mut seen = Vec::new();
    loop {
        let page = list_viewer_points(&pool, since, 7).await.unwrap();
        assert!(page.rows.len() <= 7);
        seen.extend(page.rows.iter().map(|r| r.twitch_user_id.clone()));
        since = page
            .next_updated_since
            .as_deref()
            .map(|s| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc));
        if !page.has_more {
            break;
        }
    }
    assert_eq!(seen.len(), 25);
    let mut dedup = seen.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(dedup.len(), 25, "keine Zeile doppelt oder verloren");
}

#[tokio::test]
async fn streamer_cursor_verliert_bei_gleichzeitigem_tageslauf_keine_zeile() {
    let _db = db::migrated_pool("tb_cp_streamer_cursor").await;
    let pool = _db.pool.clone();
    for i in 0..25_i64 {
        let id = format!("{}", 5000 + i);
        let login = format!("kanal{i}");
        sqlx::query("INSERT INTO twitch_partners(twitch_user_id,twitch_login,status) VALUES ($1,$2,'active')")
            .bind(&id).bind(&login).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_stream_sessions(id,streamer_login,started_at,twitch_user_id) VALUES ($1,$2,'2026-10-01 06:00Z',$3)")
            .bind(i+10).bind(&login).bind(&id).execute(&pool).await.unwrap();
        ticks(&pool, i + 10, "viewer", "1000", "2026-10-01T08:00:00Z", 12).await;
    }
    aggregate_day(&pool, day("2026-10-01"), ts("2026-10-01T20:00:00Z"))
        .await
        .unwrap();
    let stamps: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*),COUNT(DISTINCT updated_at) FROM twitch_community_points_streamer_daily",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stamps, (25, 25));
    let mut since = None;
    let mut seen = std::collections::HashSet::new();
    loop {
        let page = list_streamer_points(&pool, since, 7).await.unwrap();
        for row in page.rows {
            assert!(seen.insert(row.streamer_twitch_user_id));
        }
        since = page
            .next_updated_since
            .as_deref()
            .map(|s| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc));
        if !page.has_more {
            break;
        }
    }
    assert_eq!(seen.len(), 25);
}

#[tokio::test]
async fn presence_id_bleibt_bei_login_neuvergabe_und_bann_gebunden() {
    let _db = db::migrated_pool("tb_cp_identity").await;
    let pool = _db.pool.clone();
    db::seed_partners(&pool).await;
    ticks(&pool, 1, "alter_login", "111", "2026-10-01T08:00:00Z", 10).await;
    sqlx::query("UPDATE twitch_session_chatters SET chatter_id='222' WHERE session_id=1 AND chatter_login='alter_login'")
        .execute(&pool).await.unwrap();
    run_aggregation(&pool, ts("2026-10-01T12:00:00Z"), false)
        .await
        .unwrap();
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_community_points_viewer_daily WHERE points_watch > 0",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(ids, ["111"]);
    sqlx::query("INSERT INTO twitch_chatter_global_ban(chatter_login,chatter_id) VALUES ('alter_login','111')")
        .execute(&pool).await.unwrap();
    ticks(&pool, 1, "alter_login", "333", "2026-10-01T09:00:00Z", 10).await;
    run_aggregation(&pool, ts("2026-10-01T12:00:00Z"), false)
        .await
        .unwrap();
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_community_points_viewer_daily WHERE points_watch > 0",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(ids, ["333"]);
    let old_points: i32 = sqlx::query_scalar(
        "SELECT points_watch FROM twitch_community_points_viewer_daily WHERE twitch_user_id='111'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(old_points, 0);
}
