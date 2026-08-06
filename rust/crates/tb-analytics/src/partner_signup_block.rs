//! Admin-CRUD für den Signup-Block (`twitch_partner_signup_denylist`).
//!
//! Eigenständiger Zustand: wer hier steht, wird nicht ins Partnerprogramm
//! aufgenommen. Bewusst getrennt von der Raid-Blacklist (Raid-Ziel-Auswahl),
//! vom Opt-out des Streamers und von der technischen Zwangspause.
//!
//! Getrennt vom Nachschlag-Pfad `tb_raid::signup_denylist` (Admin-CRUD ≠
//! Guard im Promotion-Loop), analog zur Trennung
//! `tb_analytics::raid_blacklist` ↔ `tb_raid::RaidBlacklistStore`.
//!
//! # Wirkung von [`add`] (Richtungsregel + Folgewirkungen)
//!
//! 1. Denylist-Eintrag (der eigentliche Zustand).
//! 2. Raid-Blacklist-Eintrag mit Präfix
//!    [`tb_domain::RAID_BLACKLIST_REASON_PREFIX`] — Signup-Block impliziert
//!    Raid-Blacklist, umgekehrt gilt das NICHT.
//! 3. Gespeicherte OAuth-Credentials (`twitch_raid_auth`) werden gelöscht.
//! 4. Eine noch aktive Partner-Zeile wird stillgelegt
//!    (`technical_pause_reason = 'blocked'`), damit der Bot in dem Kanal keine
//!    Rechte mehr ausübt und kein Re-OAuth sie reaktiviert (Guard in
//!    `tb_raid::partner_setup::promote_streamer_to_partner`).
//!
//! [`remove`] nimmt 1., 2. und 4. zurück. Gelöschte Credentials kommen nicht
//! zurück, der Streamer muss neu autorisieren.

use sqlx::PgPool;
use tb_domain::RAID_BLACKLIST_REASON_PREFIX;

/// Ein Denylist-Eintrag (für Check- und List-Route).
#[derive(Debug, sqlx::FromRow)]
pub struct SignupBlockEntry {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub reason: String,
    pub public_message: Option<String>,
    pub added_by: String,
    pub added_at: chrono::DateTime<chrono::Utc>,
}

/// Was [`add`] tatsächlich bewirkt hat. Jedes Feld ist ein Beleg für das Log,
/// damit ein Aufruf ohne Wirkung nicht als Erfolg durchgeht.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AddOutcome {
    /// Der Eintrag war neu (`false` = bestehender Eintrag aktualisiert).
    pub inserted: bool,
    /// Raid-Blacklist-Eintrag geschrieben oder aktualisiert.
    pub raid_blacklisted: bool,
    /// Gespeicherte OAuth-Credentials gelöscht.
    pub credentials_deleted: bool,
    /// Eine aktive Partner-Zeile wurde stillgelegt.
    pub active_partner_paused: bool,
}

/// Was [`remove`] bewirkt hat.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RemoveOutcome {
    pub removed: bool,
    pub raid_entries_removed: u64,
    pub partner_pause_cleared: bool,
}

/// Löst die `twitch_user_id` zu einem Login auf, soweit sie im Bestand bekannt
/// ist. Reihenfolge: Partner-Zeile, Identity-Tabelle, Streamer-Quelle.
/// `None` heißt: der Login ist uns unbekannt, der Aufrufer muss die ID liefern.
pub async fn resolve_user_id(pool: &PgPool, login: &str) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        r#"
        SELECT twitch_user_id FROM twitch_partners
         WHERE lower(twitch_login) = lower($1) AND NULLIF(twitch_user_id, '') IS NOT NULL
         ORDER BY id DESC LIMIT 1
        "#,
    )
    .bind(login)
    .fetch_optional(pool)
    .await?;
    if let Some((Some(id),)) = row {
        return Ok(Some(id));
    }

    let row: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT twitch_user_id FROM twitch_streamer_identities
         WHERE lower(twitch_login) = lower($1) AND NULLIF(twitch_user_id, '') IS NOT NULL
         LIMIT 1
        "#,
    )
    .bind(login)
    .fetch_optional(pool)
    .await?;
    if let Some((id,)) = row {
        return Ok(Some(id));
    }

    let row: Option<(Option<String>,)> = sqlx::query_as(
        r#"
        SELECT twitch_user_id FROM twitch_streamers
         WHERE lower(twitch_login) = lower($1) AND NULLIF(twitch_user_id, '') IS NOT NULL
         LIMIT 1
        "#,
    )
    .bind(login)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(id,)| id))
}

/// Setzt den Signup-Block inklusive aller Folgewirkungen in einer Transaktion.
/// `login` muss normalisiert (lowercase) sein, `twitch_user_id` nicht leer.
pub async fn add(
    pool: &PgPool,
    twitch_user_id: &str,
    login: &str,
    reason: &str,
    public_message: Option<&str>,
    added_by: &str,
) -> Result<AddOutcome, sqlx::Error> {
    let public_message = public_message.map(str::trim).filter(|s| !s.is_empty());
    let mut tx = pool.begin().await?;

    // 1. Der eigentliche Zustand. Ein bestehender Eintrag mit gleicher ID wird
    //    aktualisiert; ein Login-Konflikt mit ANDERER ID wird vorher entfernt,
    //    damit der eindeutige Login-Index nicht bricht (Streamer-Umbenennung).
    sqlx::query(
        "DELETE FROM twitch_partner_signup_denylist
          WHERE lower(twitch_login) = $1 AND twitch_user_id <> $2",
    )
    .bind(login)
    .bind(twitch_user_id)
    .execute(&mut *tx)
    .await?;

    let inserted: bool = sqlx::query_scalar(
        r#"
        INSERT INTO twitch_partner_signup_denylist
            (twitch_user_id, twitch_login, reason, public_message, added_by)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (twitch_user_id) DO UPDATE SET
            twitch_login   = EXCLUDED.twitch_login,
            reason         = EXCLUDED.reason,
            public_message = EXCLUDED.public_message,
            added_by       = EXCLUDED.added_by
        RETURNING (xmax = 0)
        "#,
    )
    .bind(twitch_user_id)
    .bind(login)
    .bind(reason)
    .bind(public_message)
    .bind(added_by)
    .fetch_one(&mut *tx)
    .await?;

    // 2. Richtungsregel: Signup-Block impliziert Raid-Blacklist.
    //    `target_login` ist Primaerschluessel — pro Login gibt es genau einen
    //    Grund. Einen FREMDEN Grund (bot_banned, Vier-Raid-Sperre) duerfen wir
    //    nicht ueberschreiben: sonst raeumt ein spaeteres `remove` ihn mit weg,
    //    obwohl er nie zu uns gehoerte. Der Kanal steht dann eben mit dem
    //    fremden Grund auf der Liste — die Wirkung ist dieselbe.
    let raid_reason = format!("{RAID_BLACKLIST_REASON_PREFIX}{reason}");
    let raid_prefix_like = format!("{RAID_BLACKLIST_REASON_PREFIX}%");
    sqlx::query(
        r#"
        INSERT INTO twitch_raid_blacklist (target_id, target_login, reason)
        VALUES ($1, $2, $3)
        ON CONFLICT (target_login) DO UPDATE SET
            target_id = COALESCE(EXCLUDED.target_id, twitch_raid_blacklist.target_id),
            reason    = EXCLUDED.reason,
            added_at  = CURRENT_TIMESTAMP
        WHERE COALESCE(twitch_raid_blacklist.reason, '') = ''
           OR twitch_raid_blacklist.reason LIKE $4
        "#,
    )
    .bind(twitch_user_id)
    .bind(login)
    .bind(&raid_reason)
    .bind(&raid_prefix_like)
    .execute(&mut *tx)
    .await?;
    // Nicht `rows_affected` melden: bei einem stehen gelassenen Fremdgrund ist
    // sie 0, obwohl der Eintrag existiert. Gemeldet wird der Zustand, nicht der
    // Schreibvorgang — sonst behauptet das Log "keine Wirkung" fuer einen
    // Kanal, der sehr wohl auf der Liste steht.
    let raid_blacklisted: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM twitch_raid_blacklist WHERE lower(target_login) = $1)",
    )
    .bind(login)
    .fetch_one(&mut *tx)
    .await?;

    // 3. Gespeicherte Credentials löschen.
    let creds = sqlx::query("DELETE FROM twitch_raid_auth WHERE twitch_user_id = $1")
        .bind(twitch_user_id)
        .execute(&mut *tx)
        .await?;

    // 4. Noch aktive Partner-Zeile stilllegen. 'blocked' ist der bestehende
    //    Hard-Kill-Zustand, den der Promotion-Guard bereits respektiert; der
    //    Bot übt in dem Kanal danach keine Rechte mehr aus.
    let paused = sqlx::query(
        r#"
        UPDATE twitch_partners
           SET technical_pause_reason = 'blocked',
               raid_bot_enabled       = 0
         WHERE (twitch_user_id = $1 OR lower(twitch_login) = $2)
           AND COALESCE(status, '') = 'active'
           AND COALESCE(technical_pause_reason, '') <> 'blocked'
        "#,
    )
    .bind(twitch_user_id)
    .bind(login)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let outcome = AddOutcome {
        inserted,
        raid_blacklisted,
        credentials_deleted: creds.rows_affected() > 0,
        active_partner_paused: paused.rows_affected() > 0,
    };
    tracing::warn!(
        twitch_user_id = %twitch_user_id,
        twitch_login = %login,
        %reason,
        %added_by,
        inserted = outcome.inserted,
        raid_blacklisted = outcome.raid_blacklisted,
        credentials_deleted = outcome.credentials_deleted,
        active_partner_paused = outcome.active_partner_paused,
        "Signup-Block gesetzt"
    );
    Ok(outcome)
}

/// Hebt den Signup-Block auf. Entfernt aus `twitch_raid_blacklist` NUR
/// Einträge, die dieser Block selbst gesetzt hat (Präfix-Match) — fremde
/// Gründe wie Bot-Ban oder die 4-Raid-Schwelle bleiben stehen.
/// Gelöschte Credentials kommen nicht zurück.
pub async fn remove(
    pool: &PgPool,
    twitch_user_id: Option<&str>,
    login: &str,
) -> Result<RemoveOutcome, sqlx::Error> {
    let user_id = twitch_user_id.map(str::trim).filter(|s| !s.is_empty());
    let mut tx = pool.begin().await?;

    let removed_ids: Vec<(String, String)> = sqlx::query_as(
        r#"
        DELETE FROM twitch_partner_signup_denylist
         WHERE ($1::text IS NOT NULL AND twitch_user_id = $1)
            OR ($2::text <> '' AND lower(twitch_login) = $2)
        RETURNING twitch_user_id, twitch_login
        "#,
    )
    .bind(user_id)
    .bind(login)
    .fetch_all(&mut *tx)
    .await?;

    if removed_ids.is_empty() {
        tx.commit().await?;
        tracing::info!(
            twitch_user_id = %user_id.unwrap_or(""),
            twitch_login = %login,
            "Signup-Block aufheben: kein Eintrag vorhanden, nichts geaendert"
        );
        return Ok(RemoveOutcome::default());
    }

    let like = format!("{RAID_BLACKLIST_REASON_PREFIX}%");
    let mut raid_entries_removed = 0u64;
    let mut partner_pause_cleared = false;
    for (uid, entry_login) in &removed_ids {
        let raid = sqlx::query(
            r#"
            DELETE FROM twitch_raid_blacklist
             WHERE (target_id = $1 OR lower(target_login) = lower($2))
               AND reason LIKE $3
            "#,
        )
        .bind(uid)
        .bind(entry_login)
        .bind(&like)
        .execute(&mut *tx)
        .await?;
        raid_entries_removed += raid.rows_affected();

        let cleared = sqlx::query(
            r#"
            UPDATE twitch_partners
               SET technical_pause_reason = NULL
             WHERE (twitch_user_id = $1 OR lower(twitch_login) = lower($2))
               AND COALESCE(technical_pause_reason, '') = 'blocked'
            "#,
        )
        .bind(uid)
        .bind(entry_login)
        .execute(&mut *tx)
        .await?;
        partner_pause_cleared |= cleared.rows_affected() > 0;
    }

    tx.commit().await?;

    let outcome = RemoveOutcome {
        removed: true,
        raid_entries_removed,
        partner_pause_cleared,
    };
    tracing::warn!(
        twitch_login = %login,
        eintraege = removed_ids.len(),
        raid_entries_removed = outcome.raid_entries_removed,
        partner_pause_cleared = outcome.partner_pause_cleared,
        "Signup-Block aufgehoben"
    );
    Ok(outcome)
}

/// Einzelner Eintrag per ID oder Login.
pub async fn check(
    pool: &PgPool,
    twitch_user_id: Option<&str>,
    login: &str,
) -> Result<Option<SignupBlockEntry>, sqlx::Error> {
    let user_id = twitch_user_id.map(str::trim).filter(|s| !s.is_empty());
    sqlx::query_as::<_, SignupBlockEntry>(
        r#"
        SELECT twitch_user_id, twitch_login, reason, public_message, added_by, added_at
          FROM twitch_partner_signup_denylist
         WHERE ($1::text IS NOT NULL AND twitch_user_id = $1)
            OR ($2::text <> '' AND lower(twitch_login) = $2)
         ORDER BY (twitch_user_id = $1) DESC
         LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(login)
    .fetch_optional(pool)
    .await
}

/// Alle Einträge, jüngste zuerst.
pub async fn list_entries(pool: &PgPool) -> Result<Vec<SignupBlockEntry>, sqlx::Error> {
    sqlx::query_as::<_, SignupBlockEntry>(
        r#"
        SELECT twitch_user_id, twitch_login, reason, public_message, added_by, added_at
          FROM twitch_partner_signup_denylist
         ORDER BY added_at DESC, twitch_login ASC
        "#,
    )
    .fetch_all(pool)
    .await
}

/// Alle geblockten Logins als lowercase-Set. Für Filter, die eine ganze Liste
/// gegen den Block prüfen (Chat-Join-Ziele, Link-Kandidaten).
pub async fn blocked_logins(
    pool: &PgPool,
) -> Result<std::collections::HashSet<String>, sqlx::Error> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT lower(twitch_login) FROM twitch_partner_signup_denylist")
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(l,)| l).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

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

    /// Schema-isolierter Pool mit prod-treuer DDL. `max_connections(1)`, damit
    /// `SET search_path` und die spaeteren Transaktionen dieselbe Verbindung
    /// benutzen.
    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen");

        for ddl in [
            r#"CREATE TABLE twitch_partner_signup_denylist (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login   TEXT NOT NULL,
                reason         TEXT NOT NULL,
                public_message TEXT,
                added_by       TEXT NOT NULL,
                added_at       TIMESTAMPTZ NOT NULL DEFAULT now()
            )"#,
            r#"CREATE UNIQUE INDEX idx_denylist_login
                ON twitch_partner_signup_denylist (lower(twitch_login))"#,
            r#"CREATE TABLE twitch_raid_blacklist (
                target_id    TEXT,
                target_login TEXT NOT NULL PRIMARY KEY,
                reason       TEXT,
                added_at     TEXT DEFAULT CURRENT_TIMESTAMP
            )"#,
            r#"CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login   TEXT
            )"#,
            r#"CREATE TABLE twitch_partners (
                id BIGSERIAL PRIMARY KEY,
                twitch_user_id TEXT,
                twitch_login TEXT,
                status TEXT,
                raid_bot_enabled INTEGER,
                technical_pause_reason TEXT
            )"#,
            r#"CREATE TABLE twitch_streamer_identities (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT
            )"#,
            r#"CREATE TABLE twitch_streamers (
                id BIGSERIAL PRIMARY KEY,
                twitch_login TEXT UNIQUE NOT NULL,
                twitch_user_id TEXT
            )"#,
        ] {
            sqlx::query(ddl).execute(&pool).await.expect("DDL");
        }
        pool
    }

    async fn skalar<T>(pool: &PgPool, sql: &str) -> T
    where
        T: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Send + Unpin,
    {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
    }

    /// Beweisziel: ein `add` bewirkt alle vier Dinge in einem Zug — Eintrag,
    /// Raid-Blacklist, geloeschte Credentials, stillgelegte Partner-Zeile.
    #[tokio::test]
    async fn add_setzt_alle_vier_wirkungen() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "psb_add").await;
        sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login) VALUES ('173926844','temmiee985')")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, raid_bot_enabled)
             VALUES ('173926844','temmiee985','active',1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let outcome = add(
            &pool,
            "173926844",
            "temmiee985",
            "owner_decision:repraesentation",
            None,
            "test",
        )
        .await
        .unwrap();

        assert_eq!(
            outcome,
            AddOutcome {
                inserted: true,
                raid_blacklisted: true,
                credentials_deleted: true,
                active_partner_paused: true,
            }
        );
        let raid_reason: String = skalar(
            &pool,
            "SELECT reason FROM twitch_raid_blacklist WHERE target_login = 'temmiee985'",
        )
        .await;
        assert_eq!(raid_reason, "signup_block:owner_decision:repraesentation");
        let auth: i64 = skalar(&pool, "SELECT COUNT(*) FROM twitch_raid_auth").await;
        assert_eq!(auth, 0, "Credentials muessen weg sein");
        let (pause, raid_flag): (Option<String>, Option<i32>) = sqlx::query_as(
            "SELECT technical_pause_reason, raid_bot_enabled FROM twitch_partners
              WHERE twitch_user_id = '173926844'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(pause.as_deref(), Some("blocked"));
        assert_eq!(raid_flag, Some(0), "Bot darf im Kanal nichts mehr tun");
    }

    /// Beweisziel: `remove` raeumt nur den eigenen Raid-Grund weg. Ein Bot-Ban
    /// oder eine Vier-Raid-Sperre bleibt bestehen — sonst hebt das Aufheben
    /// eines Signup-Blocks stillschweigend eine fremde Sperre auf.
    #[tokio::test]
    async fn remove_laesst_fremde_raid_gruende_stehen() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "psb_remove").await;
        // Derselbe Kanal steht schon aus fremdem Grund auf der Raid-Blacklist.
        // `target_login` ist Primaerschluessel, es kann also nur einen Grund
        // geben — der fremde muss gewinnen.
        sqlx::query(
            "INSERT INTO twitch_raid_blacklist (target_id, target_login, reason)
             VALUES ('1', 'eigener', 'bot_banned')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let add_outcome = add(&pool, "1", "eigener", "owner_decision", None, "test")
            .await
            .unwrap();
        assert!(
            add_outcome.raid_blacklisted,
            "steht auf der Liste, auch wenn der Grund fremd ist"
        );
        let grund_nach_add: String = skalar(
            &pool,
            "SELECT reason FROM twitch_raid_blacklist WHERE target_login = 'eigener'",
        )
        .await;
        assert_eq!(grund_nach_add, "bot_banned", "fremder Grund bleibt stehen");

        let outcome = remove(&pool, Some("1"), "eigener").await.unwrap();
        assert!(outcome.removed);
        assert_eq!(
            outcome.raid_entries_removed, 0,
            "nichts wegraeumen, was uns nicht gehoert"
        );

        let uebrig: Vec<(String, String)> =
            sqlx::query_as("SELECT target_login, reason FROM twitch_raid_blacklist ORDER BY 1")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            uebrig,
            vec![("eigener".to_string(), "bot_banned".to_string())]
        );
        let eintraege: i64 =
            skalar(&pool, "SELECT COUNT(*) FROM twitch_partner_signup_denylist").await;
        assert_eq!(eintraege, 0);
    }

    /// Beweisziel: hat der Kanal keinen fremden Grund, setzt `add` den eigenen
    /// und `remove` raeumt ihn wieder weg — die Richtungsregel wirkt in beide
    /// Richtungen fuer das, was wir selbst gesetzt haben.
    #[tokio::test]
    async fn eigener_raid_grund_wird_gesetzt_und_wieder_entfernt() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "psb_eigener_grund").await;
        add(&pool, "1", "eigener", "owner_decision", None, "test")
            .await
            .unwrap();
        let grund: String = skalar(
            &pool,
            "SELECT reason FROM twitch_raid_blacklist WHERE target_login = 'eigener'",
        )
        .await;
        assert_eq!(grund, "signup_block:owner_decision");

        let outcome = remove(&pool, Some("1"), "eigener").await.unwrap();
        assert_eq!(outcome.raid_entries_removed, 1);
        let rest: i64 = skalar(&pool, "SELECT COUNT(*) FROM twitch_raid_blacklist").await;
        assert_eq!(rest, 0);
    }

    /// Beweisziel: `remove` ohne Treffer meldet das ehrlich und aendert nichts —
    /// kein "ok" auf einen Aufruf ohne Wirkung.
    #[tokio::test]
    async fn remove_ohne_treffer_ist_kein_erfolg() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "psb_remove_leer").await;
        let outcome = remove(&pool, None, "gibtsnicht").await.unwrap();
        assert_eq!(outcome, RemoveOutcome::default());
        assert!(!outcome.removed);
    }

    /// Beweisziel: benennt sich ein geblockter Streamer um, ersetzt `add` den
    /// alten Login-Eintrag statt am eindeutigen Login-Index zu scheitern.
    #[tokio::test]
    async fn add_ueberschreibt_login_konflikt_fremder_id() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "psb_konflikt").await;
        add(&pool, "alt", "gleicherlogin", "grund_a", None, "test")
            .await
            .unwrap();
        add(&pool, "neu", "gleicherlogin", "grund_b", None, "test")
            .await
            .unwrap();

        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT twitch_user_id FROM twitch_partner_signup_denylist ORDER BY 1",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(ids, vec!["neu".to_string()]);
    }

    /// Beweisziel: `check` findet den Eintrag ueber die ID auch dann, wenn der
    /// uebergebene Login nicht mehr passt.
    #[tokio::test]
    async fn check_findet_ueber_id_trotz_anderem_login() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "psb_check").await;
        add(&pool, "173926844", "temmiee985", "grund", None, "test")
            .await
            .unwrap();

        let treffer = check(&pool, Some("173926844"), "ganzandererlogin")
            .await
            .unwrap()
            .expect("Treffer ueber ID erwartet");
        assert_eq!(treffer.twitch_login, "temmiee985");
        assert!(check(&pool, None, "ganzandererlogin")
            .await
            .unwrap()
            .is_none());
    }

    /// Beweisziel: `resolve_user_id` findet die ID aus dem Bestand, damit der
    /// Admin sie nicht von Hand nachschlagen muss.
    #[tokio::test]
    async fn resolve_user_id_findet_id_aus_identitaeten() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "psb_resolve").await;
        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
             VALUES ('839304219','Taiju_Redestein')",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(
            resolve_user_id(&pool, "taiju_redestein").await.unwrap(),
            Some("839304219".to_string())
        );
        assert_eq!(resolve_user_id(&pool, "unbekannt").await.unwrap(), None);
    }
}
