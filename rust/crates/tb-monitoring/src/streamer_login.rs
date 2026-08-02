use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sqlx::{PgPool, Postgres, Transaction};

const KNOWN_LOGIN_SQL: &str = r#"
    SELECT twitch_login
      FROM (
            SELECT twitch_login, 1 AS priority
              FROM twitch_streamers WHERE twitch_user_id = $1
            UNION ALL
            SELECT twitch_login, 2 AS priority
              FROM twitch_partners WHERE twitch_user_id = $1
            UNION ALL
            SELECT twitch_login, 3 AS priority
              FROM twitch_streamer_identities WHERE twitch_user_id = $1
            UNION ALL
            SELECT twitch_login, 4 AS priority
              FROM twitch_raid_auth WHERE twitch_user_id = $1
            UNION ALL
            SELECT twitch_login, 5 AS priority
              FROM twitch_partner_raid_scores WHERE twitch_user_id = $1
            UNION ALL
            SELECT streamer_login AS twitch_login, 6 AS priority
              FROM twitch_live_state WHERE twitch_user_id = $1
      ) known
     WHERE COALESCE(TRIM(twitch_login), '') <> ''
     ORDER BY priority
     LIMIT 1
"#;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenameTableCounts {
    pub renamed: u64,
    /// Veraltete Fremdzeilen, deren Login den Unique-Index blockierte und
    /// deshalb auf einen Platzhalter umgeschrieben wurde. Die Zeile bleibt
    /// inhaltlich erhalten — Tokens und Konfiguration werden nie gelöscht.
    pub stale_cleared: u64,
    pub skipped: u64,
}

impl RenameTableCounts {
    fn add(&mut self, other: &Self) {
        self.renamed += other.renamed;
        self.stale_cleared += other.stale_cleared;
        self.skipped += other.skipped;
    }
}

/// Betriebstabellen mit stabiler ID-Spalte: (Tabelle, Login-Spalte, ID-Spalte,
/// Login trägt einen Unique-Index).
///
/// Reihenfolge ist bedeutsam: `twitch_streamer_identities` steht vorn, weil der
/// Trigger `trg_twitch_streamers_sync_identity` jedes Update auf
/// `twitch_streamers` dorthin spiegelt. Läge dort noch eine veraltete
/// Fremdzeile mit dem neuen Login, liefe die Umbenennung in die
/// Unique-Verletzung des Triggers statt in unsere Konfliktbehandlung.
const ID_TABELLEN: &[(&str, &str, &str, bool)] = &[
    (
        "twitch_streamer_identities",
        "twitch_login",
        "twitch_user_id",
        true,
    ),
    ("twitch_streamers", "twitch_login", "twitch_user_id", true),
    (
        "twitch_live_state",
        "streamer_login",
        "twitch_user_id",
        false,
    ),
    ("twitch_raid_auth", "twitch_login", "twitch_user_id", true),
    (
        "twitch_partner_raid_scores",
        "twitch_login",
        "twitch_user_id",
        false,
    ),
    (
        "twitch_engagement_settings",
        "channel_login",
        "channel_user_id",
        true,
    ),
    (
        "twitch_engagement_channel_profile",
        "channel_login",
        "channel_user_id",
        true,
    ),
    (
        "twitch_scam_guard_settings",
        "channel_login",
        "channel_user_id",
        true,
    ),
    (
        "twitch_channel_match_state",
        "channel_login",
        "channel_user_id",
        true,
    ),
    (
        "twitch_chat_word_groups",
        "streamer_login",
        "twitch_user_id",
        false,
    ),
    (
        "twitch_live_announcement_configs",
        "streamer_login",
        "twitch_user_id",
        true,
    ),
    (
        "twitch_scout_pitch_blacklist",
        "streamer_login",
        "twitch_user_id",
        true,
    ),
    // Unique ist (login, cooldown_type), nicht der Login allein.
    ("twitch_promo_cooldowns", "login", "twitch_user_id", false),
];

/// Aufzeichnungen: Zeilen halten fest, was zu einem Zeitpunkt geschah, und
/// tragen deshalb weiter den damals gültigen Namen — genau wie beendete
/// Sessions. Der Rename trägt hier nur die stabile ID nach, damit die Zeilen
/// trotzdem dem Kanal zuordenbar bleiben.
const HISTORIE_TABELLEN: &[(&str, &str, &str)] = &[
    ("twitch_engagement_log", "channel_login", "channel_user_id"),
    (
        "twitch_engagement_stream_transcripts",
        "channel_login",
        "channel_user_id",
    ),
    (
        "twitch_outreach_shadow_events",
        "channel_login",
        "channel_user_id",
    ),
    (
        "twitch_smalltalk_messages",
        "channel_login",
        "channel_user_id",
    ),
    ("twitch_scout_pitch_ledger", "streamer_login", "twitch_user_id"),
];

/// Tabellen, die (noch) keine ID-Spalte tragen und deshalb allein über den
/// Login gefunden werden können.
const LOGIN_TABELLEN: &[(&str, &str)] = &[
    ("twitch_streamer_invites", "streamer_login"),
    ("twitch_raw_chat_ingest_health", "streamer_login"),
];

/// Zähler pro Tabelle. Bewusst eine Liste statt fester Felder: welche Tabellen
/// ein Rename anfasst, wächst mit dem Umbau weiter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenameCounts {
    tabellen: Vec<(String, RenameTableCounts)>,
}

impl RenameCounts {
    fn push(&mut self, tabelle: &str, counts: RenameTableCounts) {
        self.tabellen.push((tabelle.to_string(), counts));
    }

    /// Zähler einer Tabelle; unberührte Tabellen liefern den Nullstand.
    pub fn for_table(&self, tabelle: &str) -> RenameTableCounts {
        self.tabellen
            .iter()
            .find(|(name, _)| name == tabelle)
            .map(|(_, counts)| counts.clone())
            .unwrap_or_default()
    }

    pub fn total(&self) -> RenameTableCounts {
        let mut total = RenameTableCounts::default();
        for (_, counts) in &self.tabellen {
            total.add(counts);
        }
        total
    }

    fn tables(&self) -> &[(String, RenameTableCounts)] {
        &self.tabellen
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameReport {
    pub twitch_user_id: String,
    pub old_login: String,
    pub new_login: String,
    pub counts: RenameCounts,
}

/// Obergrenze des Login-Caches. Das Roster liegt bei einigen hundert Kanälen;
/// wird es unerwartet größer, ist ein Neuaufbau billiger als unbegrenztes
/// Wachstum.
const LOGIN_CACHE_LIMIT: usize = 10_000;

#[derive(Clone)]
pub struct StreamerLoginStore {
    pool: PgPool,
    /// Zuletzt bestätigter Login je `twitch_user_id`. `reconcile` läuft an
    /// jeder Chatnachricht — ohne diesen Cache wäre das pro Nachricht eine
    /// Abfrage über sechs Tabellen.
    bekannte_logins: Arc<Mutex<HashMap<String, String>>>,
}

impl StreamerLoginStore {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            bekannte_logins: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn cache_treffer(&self, user_id: &str, login: &str) -> bool {
        self.bekannte_logins
            .lock()
            .map(|cache| cache.get(user_id).is_some_and(|bekannt| bekannt == login))
            .unwrap_or(false)
    }

    fn cache_setzen(&self, user_id: &str, login: &str) {
        if let Ok(mut cache) = self.bekannte_logins.lock() {
            if cache.len() >= LOGIN_CACHE_LIMIT {
                cache.clear();
            }
            cache.insert(user_id.to_string(), login.to_string());
        }
    }

    pub async fn reconcile(
        &self,
        twitch_user_id: &str,
        new_login: &str,
    ) -> Result<Option<RenameReport>, sqlx::Error> {
        let user_id = twitch_user_id.trim();
        let new_login = new_login.trim().to_lowercase();
        if user_id.is_empty() || new_login.is_empty() {
            tracing::debug!(
                twitch_user_id = %user_id,
                new_login = %new_login,
                "Twitch-Rename-Prüfung übersprungen: user_id oder Login fehlt"
            );
            return Ok(None);
        }
        if self.cache_treffer(user_id, &new_login) {
            return Ok(None);
        }

        let old_login = sqlx::query_scalar::<_, String>(KNOWN_LOGIN_SQL)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(old_login) = old_login else {
            tracing::debug!(
                twitch_user_id = %user_id,
                new_login = %new_login,
                "Twitch-Rename-Prüfung: keine bekannte Identität, kein Rename nötig"
            );
            return Ok(None);
        };
        if old_login.trim().eq_ignore_ascii_case(&new_login) {
            tracing::debug!(
                twitch_user_id = %user_id,
                old_login = %old_login,
                new_login = %new_login,
                "Twitch-Rename-Prüfung: Login unverändert"
            );
            self.cache_setzen(user_id, &new_login);
            return Ok(None);
        }

        let report = rename_streamer_login(&self.pool, user_id, &old_login, &new_login).await?;
        self.cache_setzen(user_id, &new_login);
        Ok(Some(report))
    }
}

pub async fn rename_streamer_login(
    pool: &PgPool,
    twitch_user_id: &str,
    old_login: &str,
    new_login: &str,
) -> Result<RenameReport, sqlx::Error> {
    let user_id = twitch_user_id.trim();
    let requested_old_login = old_login.trim().to_lowercase();
    let new_login = new_login.trim().to_lowercase();
    if user_id.is_empty() || requested_old_login.is_empty() || new_login.is_empty() {
        return Err(sqlx::Error::Protocol(
            "twitch rename requires user_id, old_login and new_login".into(),
        ));
    }

    let mut tx = pool.begin().await?;
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
             hashtextextended('tb-monitoring:twitch-login:' || $1, 0)
         )",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    let old_login = sqlx::query_scalar::<_, String>(KNOWN_LOGIN_SQL)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .map(|login| login.trim().to_lowercase())
        .filter(|login| !login.is_empty())
        .unwrap_or_else(|| requested_old_login.clone());
    if old_login.eq_ignore_ascii_case(&new_login) {
        tx.commit().await?;
        tracing::debug!(
            twitch_user_id = %user_id,
            requested_old_login = %requested_old_login,
            new_login = %new_login,
            "Twitch-Rename nach Transaktions-Lock nicht mehr nötig"
        );
        return Ok(RenameReport {
            twitch_user_id: user_id.to_string(),
            old_login: requested_old_login,
            new_login,
            counts: RenameCounts::default(),
        });
    }
    let mut counts = RenameCounts::default();
    for (tabelle, login_spalte, id_spalte, login_ist_unique) in ID_TABELLEN {
        let table_counts = rewrite_user_scoped(
            &mut tx,
            tabelle,
            login_spalte,
            id_spalte,
            user_id,
            &old_login,
            &new_login,
            *login_ist_unique,
        )
        .await?;
        counts.push(tabelle, table_counts);
    }
    counts.push(
        "twitch_partners",
        rewrite_partners(&mut tx, user_id, &old_login, &new_login).await?,
    );
    // Nur offene Sessions: eine beendete Session gehört zu dem Namen, unter dem
    // sie lief, und ist Betriebshistorie.
    counts.push(
        "twitch_stream_sessions",
        RenameTableCounts {
            renamed: sqlx::query(
                "UPDATE twitch_stream_sessions SET streamer_login = $2
                 WHERE LOWER(streamer_login) = LOWER($1) AND ended_at IS NULL",
            )
            .bind(&old_login)
            .bind(&new_login)
            .execute(&mut *tx)
            .await?
            .rows_affected(),
            ..RenameTableCounts::default()
        },
    );
    for (tabelle, login_spalte, id_spalte) in HISTORIE_TABELLEN {
        let nachgetragen = sqlx::query(&format!(
            "UPDATE {tabelle} SET {id_spalte} = $1
              WHERE {id_spalte} IS NULL AND LOWER({login_spalte}) = LOWER($2)"
        ))
        .bind(user_id)
        .bind(&old_login)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        counts.push(
            tabelle,
            RenameTableCounts {
                renamed: nachgetragen,
                ..RenameTableCounts::default()
            },
        );
    }
    for (tabelle, login_spalte) in LOGIN_TABELLEN {
        let table_counts =
            rewrite_login_keyed(&mut tx, tabelle, login_spalte, user_id, &old_login, &new_login)
                .await?;
        counts.push(tabelle, table_counts);
    }
    record_login_aliases(&mut tx, user_id, &old_login, &new_login).await?;
    tx.commit().await?;

    let mut unberuehrt = 0usize;
    for (table, table_counts) in counts.tables() {
        if *table_counts == RenameTableCounts::default() {
            unberuehrt += 1;
            continue;
        }
        tracing::info!(
            twitch_user_id = %user_id,
            old_login = %old_login,
            new_login = %new_login,
            table = %table,
            renamed = table_counts.renamed,
            stale_cleared = table_counts.stale_cleared,
            skipped = table_counts.skipped,
            "Twitch-Login-Tabelle aktualisiert"
        );
    }
    let total = counts.total();
    tracing::info!(
        twitch_user_id = %user_id,
        old_login = %old_login,
        new_login = %new_login,
        renamed = total.renamed,
        stale_cleared = total.stale_cleared,
        skipped = total.skipped,
        tabellen = counts.tables().len(),
        unberuehrte_tabellen = unberuehrt,
        "Twitch-Login umbenannt"
    );
    Ok(RenameReport {
        twitch_user_id: user_id.to_string(),
        old_login,
        new_login,
        counts,
    })
}

/// Umbenennung einer Tabelle, die eine stabile ID-Spalte führt.
///
/// Die eigene Zeile bleibt in jedem Fall erhalten: Twitch hat den neuen Login
/// gerade für diese `user_id` bestätigt, eine fremde Zeile mit demselben Login
/// ist also der veraltete Stand. Blockiert eine solche Fremdzeile den
/// Unique-Index, bekommt sie einen Platzhalter-Login — gelöscht wird nichts,
/// sonst gingen Raid-Tokens, Partner-Konfiguration oder die Discord-Verknüpfung
/// stillschweigend verloren.
///
/// Gesucht wird über die ID; solange die Schreibpfade sie noch nicht überall
/// setzen, greift zusätzlich der Login und die ID wird dabei nachgetragen.
#[allow(clippy::too_many_arguments)]
async fn rewrite_user_scoped(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    login_column: &str,
    id_column: &str,
    user_id: &str,
    old_login: &str,
    new_login: &str,
    login_is_unique: bool,
) -> Result<RenameTableCounts, sqlx::Error> {
    let mut counts = RenameTableCounts::default();
    if login_is_unique {
        counts.stale_cleared =
            clear_stale_foreign_login(tx, table, login_column, id_column, user_id, new_login, None)
                .await?;
    }
    let update = format!(
        "UPDATE {table} SET {login_column} = $2, {id_column} = $1
         WHERE ({id_column} = $1
                OR ({id_column} IS NULL AND LOWER({login_column}) = LOWER($3)))
           AND (LOWER({login_column}) <> LOWER($2) OR {id_column} IS DISTINCT FROM $1)"
    );
    let (renamed, skipped) =
        update_mit_konflikt_ruecksprung(tx, table, &update, user_id, new_login, Some(old_login))
            .await?;
    counts.renamed = renamed;
    counts.skipped = skipped;
    Ok(counts)
}

/// Führt ein Rename-Update hinter einem Savepoint aus.
///
/// Trägt die Tabelle schon eine Zeile unter dem neuen Login, die diesem Kanal
/// selbst gehört — etwa weil ein Schreibpfad nach der Umbenennung bereits den
/// neuen Namen benutzt hat —, läuft das Update in eine Unique-Verletzung.
/// Zusammenführen kann der Rename nicht entscheiden, abbrechen darf er nicht:
/// die Transaktion würde jede EventSub-Zustellung dieses Kanals mitreißen und
/// bei jedem Retry erneut scheitern. Also Rücksprung auf den Savepoint, Zeile
/// stehen lassen, Konflikt melden.
async fn update_mit_konflikt_ruecksprung(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    update: &str,
    user_id: &str,
    new_login: &str,
    old_login: Option<&str>,
) -> Result<(u64, u64), sqlx::Error> {
    sqlx::query("SAVEPOINT tb_rename_tabelle")
        .execute(&mut **tx)
        .await?;
    let mut query = sqlx::query(update).bind(user_id).bind(new_login);
    if let Some(old_login) = old_login {
        query = query.bind(old_login);
    }
    match query.execute(&mut **tx).await {
        Ok(result) => {
            sqlx::query("RELEASE SAVEPOINT tb_rename_tabelle")
                .execute(&mut **tx)
                .await?;
            Ok((result.rows_affected(), 0))
        }
        Err(error) if ist_unique_verletzung(&error) => {
            sqlx::query("ROLLBACK TO SAVEPOINT tb_rename_tabelle")
                .execute(&mut **tx)
                .await?;
            sqlx::query("RELEASE SAVEPOINT tb_rename_tabelle")
                .execute(&mut **tx)
                .await?;
            tracing::warn!(
                table = %table,
                twitch_user_id = %user_id,
                new_login = %new_login,
                "Twitch-Rename übersprungen: unter dem neuen Login liegt bereits \
                 eine Zeile dieses Kanals, Zusammenführen entscheidet der Rename nicht"
            );
            Ok((0, 1))
        }
        Err(error) => Err(error),
    }
}

fn ist_unique_verletzung(error: &sqlx::Error) -> bool {
    matches!(
        error
            .as_database_error()
            .and_then(|db| db.code())
            .as_deref(),
        Some("23505") | Some("23P01")
    )
}

/// Wie [`rewrite_user_scoped`], aber der Unique-Index von `twitch_partners`
/// greift nur für aktive Partner — die Kollisionsprüfung muss das mitführen.
async fn rewrite_partners(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    old_login: &str,
    new_login: &str,
) -> Result<RenameTableCounts, sqlx::Error> {
    let stale_cleared = clear_stale_foreign_login(
        tx,
        "twitch_partners",
        "twitch_login",
        "twitch_user_id",
        user_id,
        new_login,
        Some("status = 'active'"),
    )
    .await?;
    let (renamed, skipped) = update_mit_konflikt_ruecksprung(
        tx,
        "twitch_partners",
        "UPDATE twitch_partners SET twitch_login = $2, twitch_user_id = $1
         WHERE (twitch_user_id = $1
                OR (twitch_user_id IS NULL AND LOWER(twitch_login) = LOWER($3)))
           AND (LOWER(twitch_login) <> LOWER($2) OR twitch_user_id IS DISTINCT FROM $1)",
        user_id,
        new_login,
        Some(old_login),
    )
    .await?;
    Ok(RenameTableCounts {
        renamed,
        stale_cleared,
        skipped,
    })
}

/// Gibt den Unique-Index frei, den eine veraltete Fremdzeile mit dem neuen
/// Login belegt — durch einen Platzhalter, der die ID der Fremdzeile enthält
/// und deshalb selbst nicht kollidieren kann.
#[allow(clippy::too_many_arguments)]
async fn clear_stale_foreign_login(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    login_column: &str,
    id_column: &str,
    user_id: &str,
    new_login: &str,
    extra_condition: Option<&str>,
) -> Result<u64, sqlx::Error> {
    let extra = extra_condition
        .map(|condition| format!(" AND target.{condition}"))
        .unwrap_or_default();
    let placeholder =
        format!("'stale:' || COALESCE(target.{id_column}, 'unbekannt') || ':' || $2");
    let neutralize = format!(
        "UPDATE {table} target
            SET {login_column} = {placeholder}
          WHERE LOWER(target.{login_column}) = LOWER($2)
            AND target.{id_column} IS DISTINCT FROM $1{extra}
            AND NOT EXISTS (
                SELECT 1 FROM {table} belegt
                 WHERE belegt.{login_column} = {placeholder}
            )
        RETURNING COALESCE(target.{id_column}, 'unbekannt')"
    );
    let fremde_ids: Vec<String> = sqlx::query_scalar(&neutralize)
        .bind(user_id)
        .bind(new_login)
        .fetch_all(&mut **tx)
        .await?;
    for fremde_id in &fremde_ids {
        tracing::warn!(
            table = %table,
            twitch_user_id = %user_id,
            fremde_user_id = %fremde_id,
            login = %new_login,
            "Twitch-Login war noch auf einer veralteten Fremdzeile eingetragen; \
             deren Login wurde auf einen Platzhalter gesetzt, Daten blieben erhalten"
        );
    }
    Ok(fremde_ids.len() as u64)
}

async fn rewrite_login_keyed(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    login_column: &str,
    user_id: &str,
    old_login: &str,
    new_login: &str,
) -> Result<RenameTableCounts, sqlx::Error> {
    let update = format!(
        "UPDATE {table} target SET {login_column} = $2
         WHERE LOWER(target.{login_column}) = LOWER($1)
           AND NOT EXISTS (
               SELECT 1 FROM {table} current
                WHERE LOWER(current.{login_column}) = LOWER($2)
           )"
    );
    let updated = sqlx::query(&update)
        .bind(old_login)
        .bind(new_login)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    let conflict = format!(
        "SELECT COUNT(*) FROM {table} target
         WHERE LOWER(target.{login_column}) = LOWER($1)
           AND EXISTS (
               SELECT 1 FROM {table} current
                WHERE LOWER(current.{login_column}) = LOWER($2)
           )"
    );
    let skipped = sqlx::query_scalar::<_, i64>(&conflict)
        .bind(old_login)
        .bind(new_login)
        .fetch_one(&mut **tx)
        .await?
        .unsigned_abs();
    if skipped > 0 {
        tracing::warn!(
            table = %table,
            twitch_user_id = %user_id,
            old_login = %old_login,
            new_login = %new_login,
            skipped,
            "Twitch-Login-Konflikt: login-gebundene Zeile bleibt unverändert"
        );
    }
    Ok(RenameTableCounts {
        renamed: updated,
        stale_cleared: 0,
        skipped,
    })
}

async fn record_login_aliases(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    old_login: &str,
    new_login: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE twitch_login_aliases SET is_current = FALSE WHERE twitch_user_id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "INSERT INTO twitch_login_aliases
             (twitch_user_id, login, first_seen_at, last_seen_at, is_current)
         VALUES ($1, $2, NOW(), NOW(), FALSE)
         ON CONFLICT (twitch_user_id, login) DO UPDATE
             SET last_seen_at = EXCLUDED.last_seen_at,
                 is_current = FALSE",
    )
    .bind(user_id)
    .bind(old_login)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO twitch_login_aliases
             (twitch_user_id, login, first_seen_at, last_seen_at, is_current)
         VALUES ($1, $2, NOW(), NOW(), TRUE)
         ON CONFLICT (twitch_user_id, login) DO UPDATE
             SET last_seen_at = EXCLUDED.last_seen_at,
                 is_current = TRUE",
    )
    .bind(user_id)
    .bind(new_login)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
