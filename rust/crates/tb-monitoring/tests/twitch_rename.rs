use tb_monitoring::scout::ScoutRepository;
use tb_monitoring::rename_streamer_login;

mod support;

macro_rules! pool_or_skip {
    ($schema:expr) => {
        match support::pool_in_schema($schema).await {
            Some(pool) => pool,
            None => return,
        }
    };
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
        ("twitch_engagement_settings", "channel_login"),
        ("twitch_engagement_channel_profile", "channel_login"),
        ("twitch_streamer_invites", "streamer_login"),
        ("twitch_raw_chat_ingest_health", "streamer_login"),
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
    assert_eq!(settings, vec![("new_login".to_string(), true)]);

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
            "neu gewinnt".to_string(),
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
