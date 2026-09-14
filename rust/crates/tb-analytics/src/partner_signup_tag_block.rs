use std::collections::HashMap;

use sqlx::{PgExecutor, PgPool};

use crate::partner_signup_block;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TagBlockEntry {
    pub tag: String,
    pub display_tag: String,
    pub reason: String,
    pub public_message: Option<String>,
    pub added_by: String,
    pub added_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TagAddOutcome {
    pub inserted: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TagRemoveOutcome {
    pub removed: bool,
}

pub fn normalize_tag(tag: &str) -> Option<String> {
    let tag = tag.trim().to_lowercase();
    (!tag.is_empty()).then_some(tag)
}

pub fn matching_tag(tags: &[String], blocked: &[TagBlockEntry]) -> Option<String> {
    let clean: Vec<String> = tags
        .iter()
        .filter_map(|tag| normalize_tag(tag))
        .collect();
    if clean.is_empty() {
        return None;
    }
    blocked
        .iter()
        .find(|entry| clean.iter().any(|tag| *tag == entry.tag))
        .map(|entry| entry.tag.clone())
}

pub async fn list_entries(pool: &PgPool) -> Result<Vec<TagBlockEntry>, sqlx::Error> {
    sqlx::query_as::<_, TagBlockEntry>(
        r#"
        SELECT tag, display_tag, reason, public_message, added_by, added_at
          FROM twitch_partner_signup_tag_blocks
         ORDER BY added_at DESC, display_tag ASC
        "#,
    )
    .fetch_all(pool)
    .await
}

pub async fn add(
    pool: &PgPool,
    tag: &str,
    reason: &str,
    public_message: Option<&str>,
    added_by: &str,
) -> Result<TagAddOutcome, sqlx::Error> {
    let Some(tag_key) = normalize_tag(tag) else {
        tracing::warn!(%added_by, "Tag-Regel mit leerem Tag abgewiesen");
        return Ok(TagAddOutcome::default());
    };
    let public_message = public_message.map(str::trim).filter(|s| !s.is_empty());

    let inserted: bool = sqlx::query_scalar(
        r#"
        INSERT INTO twitch_partner_signup_tag_blocks
            (tag, display_tag, reason, public_message, added_by)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (tag) DO UPDATE SET
            display_tag    = EXCLUDED.display_tag,
            reason         = EXCLUDED.reason,
            public_message = EXCLUDED.public_message,
            added_by       = EXCLUDED.added_by,
            added_at       = now()
        RETURNING (xmax = 0)
        "#,
    )
    .bind(&tag_key)
    .bind(tag.trim())
    .bind(reason)
    .bind(public_message)
    .bind(added_by)
    .fetch_one(pool)
    .await?;

    tracing::info!(tag = %tag_key, %reason, %added_by, inserted, "Tag-Regel gespeichert");
    Ok(TagAddOutcome { inserted })
}

pub async fn remove(pool: &PgPool, tag: &str) -> Result<TagRemoveOutcome, sqlx::Error> {
    let Some(tag) = normalize_tag(tag) else {
        return Ok(TagRemoveOutcome::default());
    };
    let removed = sqlx::query("DELETE FROM twitch_partner_signup_tag_blocks WHERE tag = $1")
        .bind(&tag)
        .execute(pool)
        .await?
        .rows_affected()
        > 0;
    tracing::info!(%tag, removed, "Tag-Regel aufgehoben");
    Ok(TagRemoveOutcome { removed })
}

pub async fn enforce(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
    tags: &[String],
) -> Result<Option<partner_signup_block::AddOutcome>, sqlx::Error> {
    let twitch_user_id = twitch_user_id.trim();
    let twitch_login = twitch_login.trim();
    if twitch_user_id.is_empty() || twitch_login.is_empty() || tags.is_empty() {
        return Ok(None);
    }

    let blocked = list_entries(pool).await?;
    let Some(tag) = matching_tag(tags, &blocked) else {
        return Ok(None);
    };
    let rule = blocked
        .iter()
        .find(|entry| entry.tag == tag)
        .expect("matching_tag liefert nur Treffer aus blocked");

    if is_active_partner(pool, twitch_user_id, twitch_login).await? {
        tracing::debug!(
            twitch_user_id,
            twitch_login,
            tag = %rule.tag,
            "Tag-Treffer: aktiver Partner bleibt Partner, kein Eintrag"
        );
        return Ok(None);
    }

    if partner_signup_block::check(pool, Some(twitch_user_id), twitch_login)
        .await?
        .is_some()
    {
        tracing::debug!(
            twitch_user_id,
            twitch_login,
            tag = %rule.tag,
            "Tag-Treffer: Kanal steht bereits auf der Denylist, kein Überschreiben"
        );
        return Ok(None);
    }

    let reason = format!("tag_block:{tag}");
    let outcome = partner_signup_block::add(
        pool,
        twitch_user_id,
        twitch_login,
        &reason,
        rule.public_message.as_deref(),
        "tag_block",
    )
    .await?;
    Ok(Some(outcome))
}

async fn is_active_partner<'e, E>(
    executor: E,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Result<bool, sqlx::Error>
where
    E: PgExecutor<'e>,
{
    Ok(sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM twitch_partners
             WHERE (NULLIF(twitch_user_id, '') = $1 OR lower(twitch_login) = $2)
               AND COALESCE(status, '') = 'active'
        )
        "#,
    )
    .bind(twitch_user_id)
    .bind(twitch_login.trim().to_lowercase())
    .fetch_one(executor)
    .await?)
}

fn parse_session_tags(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Vec::new();
    }
    if raw.starts_with('[') {
        match serde_json::from_str::<serde_json::Value>(raw) {
            Ok(serde_json::Value::Array(items)) => items
                .into_iter()
                .map(|value| match value {
                    serde_json::Value::String(text) => text,
                    other => other.to_string(),
                })
                .collect(),
            _ => vec![raw.to_string()],
        }
    } else {
        raw.split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(str::to_string)
            .collect()
    }
}

pub async fn backfill(pool: &PgPool, tag: &str) -> Result<u64, sqlx::Error> {
    let Some(tag_key) = normalize_tag(tag) else {
        return Ok(0);
    };
    let known = list_entries(pool)
        .await?
        .into_iter()
        .any(|entry| entry.tag == tag_key);
    if !known {
        return Ok(0);
    }
    backfill_tags(pool, std::slice::from_ref(&tag_key)).await
}

pub async fn sweep(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let tags: Vec<String> = list_entries(pool)
        .await?
        .into_iter()
        .map(|entry| entry.tag)
        .collect();
    if tags.is_empty() {
        return Ok(0);
    }
    backfill_tags(pool, &tags).await
}

async fn backfill_tags(pool: &PgPool, tags: &[String]) -> Result<u64, sqlx::Error> {
    let rows = session_rows(pool, tags).await?;

    let mut channels: HashMap<String, (Option<String>, String, Vec<String>)> = HashMap::new();
    for (user_id, login, raw) in rows {
        let login = login.trim().to_lowercase();
        if login.is_empty() {
            continue;
        }
        let parsed = parse_session_tags(&raw);
        if parsed.is_empty() {
            continue;
        }
        let user_id = user_id.trim();
        let user_id = (!user_id.is_empty()).then(|| user_id.to_string());
        let anchor = user_id
            .clone()
            .unwrap_or_else(|| format!("login:{login}"));
        let entry = channels
            .entry(anchor)
            .or_insert_with(|| (user_id, login.clone(), Vec::new()));
        entry.2.extend(parsed);
    }

    let mut written = 0u64;
    for (_, (user_id, login, mut channel_tags)) in channels {
        channel_tags.sort();
        channel_tags.dedup();
        let user_id = match user_id {
            Some(id) => id,
            None => match partner_signup_block::resolve_user_id(pool, &login).await? {
                Some(id) => id,
                None => {
                    tracing::debug!(
                        login = %login,
                        "Tag-Backfill: Kanal ohne auflösbare Twitch-ID übersprungen"
                    );
                    continue;
                }
            },
        };
        if enforce(pool, &user_id, &login, &channel_tags)
            .await?
            .is_some()
        {
            written += 1;
        }
    }
    Ok(written)
}

async fn session_rows(
    pool: &PgPool,
    tags: &[String],
) -> Result<Vec<(String, String, String)>, sqlx::Error> {
    if tags.len() == 1 {
        sqlx::query_as(
            r#"
            SELECT COALESCE(twitch_user_id, ''), lower(streamer_login), tags
              FROM twitch_stream_sessions
             WHERE COALESCE(tags, '') <> ''
               AND strpos(lower(tags), $1) > 0
            "#,
        )
        .bind(&tags[0])
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as(
            r#"
            SELECT COALESCE(twitch_user_id, ''), lower(streamer_login), tags
              FROM twitch_stream_sessions
             WHERE COALESCE(tags, '') <> ''
            "#,
        )
        .fetch_all(pool)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    macro_rules! db_dsn_or_skip {
        () => {
            match std::env::var("TB_TEST_DATABASE_URL").ok() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE twitch_partner_signup_tag_blocks (
                tag            TEXT PRIMARY KEY,
                display_tag    TEXT NOT NULL,
                reason         TEXT NOT NULL,
                public_message TEXT,
                added_by       TEXT NOT NULL,
                added_at       TIMESTAMPTZ NOT NULL DEFAULT now()
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE twitch_partner_signup_denylist (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login   TEXT NOT NULL,
                reason         TEXT NOT NULL,
                public_message TEXT,
                added_by       TEXT NOT NULL,
                added_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
                partner_paused_by_block BOOLEAN NOT NULL DEFAULT false
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE UNIQUE INDEX idx_denylist_login
                ON twitch_partner_signup_denylist (lower(twitch_login))",
        )
        .execute(&pool)
        .await
        .unwrap();
        for ddl in [
            "CREATE TABLE twitch_raid_blacklist (
                target_id    TEXT,
                target_login TEXT NOT NULL PRIMARY KEY,
                reason       TEXT,
                added_at     TEXT DEFAULT CURRENT_TIMESTAMP
            )",
            "CREATE TABLE twitch_raid_auth (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT)",
            "CREATE TABLE twitch_partners (
                id BIGSERIAL PRIMARY KEY,
                twitch_user_id TEXT,
                twitch_login TEXT,
                status TEXT,
                raid_bot_enabled INTEGER,
                technical_pause_reason TEXT,
                manual_partner_opt_out INTEGER DEFAULT 0
            )",
            "CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT)",
            "CREATE TABLE twitch_streamers (id BIGSERIAL PRIMARY KEY, twitch_login TEXT UNIQUE NOT NULL, twitch_user_id TEXT)",
            "CREATE TABLE twitch_stream_sessions (
                id BIGSERIAL PRIMARY KEY,
                streamer_login TEXT NOT NULL,
                twitch_user_id TEXT,
                tags TEXT
            )",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        pool
    }

    async fn drop_schema(pool: PgPool, dsn: &str, schema: &str) {
        pool.close().await;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
    }

    async fn denylist_row(pool: &PgPool, user_id: &str) -> (String, Option<String>, String) {
        sqlx::query_as(
            "SELECT reason, public_message, added_by FROM twitch_partner_signup_denylist
              WHERE twitch_user_id = $1",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[test]
    fn normalize_tag_trimmt_und_kleinbuchstaben() {
        assert_eq!(normalize_tag("  German "), Some("german".to_string()));
        assert_eq!(normalize_tag("   "), None);
        assert_eq!(normalize_tag(""), None);
    }

    #[test]
    fn matching_tag_trifft_case_insensitive_und_oder() {
        let blocked = vec![TagBlockEntry {
            tag: "german".to_string(),
            display_tag: "German".to_string(),
            reason: "tag_block".to_string(),
            public_message: None,
            added_by: "discord:1".to_string(),
            added_at: chrono::Utc::now(),
        }];
        assert_eq!(
            matching_tag(&["GERMAN".to_string(), "Competitive".to_string()], &blocked),
            Some("german".to_string())
        );
        assert_eq!(
            matching_tag(&["Competitive".to_string()], &blocked),
            None
        );
        assert_eq!(matching_tag(&[], &blocked), None);
        assert_eq!(matching_tag(&["German".to_string()], &[]), None);
    }

    #[tokio::test]
    async fn add_speichert_normalisiert_mit_display_schreibweise() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_tag_block_add";
        let pool = make_pool(&dsn, schema).await;

        let outcome = add(
            &pool,
            "  German ",
            "tag_block",
            Some("Abgelehnt"),
            "discord:1",
        )
        .await
        .unwrap();
        assert!(outcome.inserted);

        let entries = list_entries(&pool).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tag, "german");
        assert_eq!(entries[0].display_tag, "German");
        assert_eq!(entries[0].public_message.as_deref(), Some("Abgelehnt"));

        let update = add(&pool, "german", "tag_block", None, "discord:2")
            .await
            .unwrap();
        assert!(!update.inserted, "bestehender Tag wird aktualisiert");
        let entries = list_entries(&pool).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].added_by, "discord:2");
        assert_eq!(entries[0].public_message, None);
        drop_schema(pool, &dsn, schema).await;
    }

    #[tokio::test]
    async fn add_mit_leerem_tag_schreibt_nichts() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_tag_block_leer";
        let pool = make_pool(&dsn, schema).await;
        let outcome = add(&pool, "   ", "tag_block", None, "discord:1")
            .await
            .unwrap();
        assert!(!outcome.inserted);
        assert!(list_entries(&pool).await.unwrap().is_empty());
        drop_schema(pool, &dsn, schema).await;
    }

    #[tokio::test]
    async fn remove_nimmt_nur_die_regel_nicht_die_denylist() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_tag_block_remove";
        let pool = make_pool(&dsn, schema).await;
        add(&pool, "German", "tag_block", None, "discord:1")
            .await
            .unwrap();
        partner_signup_block::add(&pool, "42", "beispiel", "tag_block:german", None, "tag_block")
            .await
            .unwrap();

        let outcome = remove(&pool, "GERMAN").await.unwrap();
        assert!(outcome.removed);
        assert!(list_entries(&pool).await.unwrap().is_empty());
        let (reason, _, _) = denylist_row(&pool, "42").await;
        assert_eq!(
            reason, "tag_block:german",
            "Kanal-Ausschluss bleibt nach dem Aufheben der Regel bestehen"
        );

        let again = remove(&pool, "german").await.unwrap();
        assert!(!again.removed);
        drop_schema(pool, &dsn, schema).await;
    }

    #[tokio::test]
    async fn enforce_schreibt_denylist_eintrag_mit_tag_grund() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_tag_block_enforce";
        let pool = make_pool(&dsn, schema).await;
        add(
            &pool,
            "German",
            "tag_block",
            Some("Tag-Regel Text"),
            "discord:1",
        )
        .await
        .unwrap();

        let outcome = enforce(
            &pool,
            "42",
            "beispiel",
            &["Competitive".to_string(), "German".to_string()],
        )
        .await
        .unwrap()
        .expect("Treffer muss einen Ausschluss schreiben");
        assert!(outcome.inserted);
        assert!(outcome.raid_blacklisted);

        let (reason, public_message, added_by) = denylist_row(&pool, "42").await;
        assert_eq!(reason, "tag_block:german");
        assert_eq!(public_message.as_deref(), Some("Tag-Regel Text"));
        assert_eq!(added_by, "tag_block");

        let second = enforce(&pool, "42", "beispiel", &["German".to_string()])
            .await
            .unwrap();
        assert!(
            second.is_none(),
            "bestehender Denylist-Eintrag wird nicht überschrieben"
        );
        drop_schema(pool, &dsn, schema).await;
    }

    #[tokio::test]
    async fn enforce_ohne_treffer_schreibt_nichts() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_tag_block_ohne_treffer";
        let pool = make_pool(&dsn, schema).await;
        add(&pool, "German", "tag_block", None, "discord:1")
            .await
            .unwrap();

        let outcome = enforce(&pool, "42", "beispiel", &["Competitive".to_string()])
            .await
            .unwrap();
        assert_eq!(outcome, None);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_signup_denylist")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);

        let ohne_tags = enforce(&pool, "42", "beispiel", &[]).await.unwrap();
        assert_eq!(ohne_tags, None);
        let ohne_id = enforce(&pool, "  ", "beispiel", &["German".to_string()])
            .await
            .unwrap();
        assert_eq!(ohne_id, None);
        drop_schema(pool, &dsn, schema).await;
    }

    #[tokio::test]
    async fn enforce_laesst_aktive_partner_unberuehrt() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_tag_block_partner";
        let pool = make_pool(&dsn, schema).await;
        add(&pool, "Deutsch", "tag_block", None, "discord:1")
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, raid_bot_enabled)
             VALUES ('99', 'aktiverpartner', 'active', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let outcome = enforce(
            &pool,
            "99",
            "aktiverpartner",
            &["Deutsch".to_string(), "English".to_string()],
        )
        .await
        .unwrap();
        assert_eq!(outcome, None, "aktiver Partner bleibt Partner");

        let outcome = enforce(&pool, "fremde_id", "aktiverpartner", &["Deutsch".to_string()])
            .await
            .unwrap();
        assert_eq!(outcome, None, "Treffer per Login greift genauso");

        let denylist: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_signup_denylist")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(denylist, 0, "kein Denylist-Eintrag für aktive Partner");
        let raid: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_raid_blacklist")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(raid, 0, "kein Raid-Blacklist-Write für aktive Partner");
        let (pause, flag): (Option<String>, Option<i32>) = sqlx::query_as(
            "SELECT technical_pause_reason, raid_bot_enabled FROM twitch_partners
              WHERE twitch_user_id = '99'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(pause, None, "keine Pause für aktive Partner");
        assert_eq!(flag, Some(1), "Bot-Rechte bleiben unangetastet");
        drop_schema(pool, &dsn, schema).await;
    }

    async fn session(pool: &PgPool, user_id: Option<&str>, login: &str, tags: &str) {
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, twitch_user_id, tags)
             VALUES ($1, $2, $3)",
        )
        .bind(login)
        .bind(user_id)
        .bind(tags)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn backfill_schreibt_historische_kanaele_ohne_partner() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_tag_block_backfill";
        let pool = make_pool(&dsn, schema).await;
        add(&pool, "Deutsch", "tag_block", None, "discord:1")
            .await
            .unwrap();
        session(&pool, Some("77"), "kanal_a", "English,Deutsch").await;
        session(&pool, Some("88"), "kanal_b", "German").await;
        session(&pool, None, "kanal_c", " Deutsch ").await;
        session(&pool, Some("66"), "kanal_d", "[\"Fun\",\"Deutsch\"]").await;
        session(&pool, Some("11"), "kanal_e", "Deutschland").await;
        session(&pool, None, "unbekannt", "Deutsch").await;
        session(&pool, Some("99"), "aktiverpartner", "Deutsch").await;
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, raid_bot_enabled)
             VALUES ('99', 'aktiverpartner', 'active', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
             VALUES ('55', 'kanal_c')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let written = backfill(&pool, "Deutsch").await.unwrap();
        assert_eq!(
            written, 3,
            "kanal_a per ID, kanal_c aufgelöst, kanal_d per JSON-Tags"
        );

        let logins: Vec<String> = sqlx::query_scalar(
            "SELECT lower(twitch_login) FROM twitch_partner_signup_denylist ORDER BY 1",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            logins,
            vec![
                "kanal_a".to_string(),
                "kanal_c".to_string(),
                "kanal_d".to_string()
            ],
            "kein Substring-Treffer, kein aktiver Partner, kein Login-only-Eintrag"
        );

        let repeat = backfill(&pool, "deutsch").await.unwrap();
        assert_eq!(repeat, 0, "bestehende Einträge werden nicht überschrieben");
        drop_schema(pool, &dsn, schema).await;
    }

    #[tokio::test]
    async fn backfill_ohne_regel_oder_leerem_tag_schreibt_nichts() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_tag_block_backfill_leer";
        let pool = make_pool(&dsn, schema).await;
        session(&pool, Some("77"), "kanal_a", "Deutsch").await;

        assert_eq!(backfill(&pool, "Deutsch").await.unwrap(), 0);
        assert_eq!(backfill(&pool, "   ").await.unwrap(), 0);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_signup_denylist")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        drop_schema(pool, &dsn, schema).await;
    }

    #[tokio::test]
    async fn sweep_prueft_alle_gesperrten_tags_in_einem_lauf() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_tag_block_sweep";
        let pool = make_pool(&dsn, schema).await;
        add(&pool, "Deutsch", "tag_block", None, "discord:1")
            .await
            .unwrap();
        add(&pool, "English", "tag_block", None, "discord:1")
            .await
            .unwrap();
        session(&pool, Some("77"), "kanal_a", "Deutsch").await;
        session(&pool, Some("88"), "kanal_b", "SoloQ,English").await;
        session(&pool, Some("99"), "aktiverpartner", "English").await;
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, raid_bot_enabled)
             VALUES ('99', 'aktiverpartner', 'active', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let written = sweep(&pool).await.unwrap();
        assert_eq!(written, 2, "zwei Nicht-Partner-Kanäle, Partner übersprungen");

        let leer = sweep(&pool).await.unwrap();
        assert_eq!(leer, 0);
        drop_schema(pool, &dsn, schema).await;
    }

    #[test]
    fn parse_session_tags_kommt_mit_json_und_komma_liste_zurecht() {
        assert_eq!(
            parse_session_tags("English,Deutsch"),
            vec!["English".to_string(), "Deutsch".to_string()]
        );
        assert_eq!(
            parse_session_tags("[\"Fun\",\"Deutsch\"]"),
            vec!["Fun".to_string(), "Deutsch".to_string()]
        );
        assert_eq!(parse_session_tags("  "), Vec::<String>::new());
        assert_eq!(
            parse_session_tags("Deutsch"),
            vec!["Deutsch".to_string()]
        );
    }
}
