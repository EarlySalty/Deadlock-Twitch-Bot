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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenameCounts {
    pub twitch_streamers: RenameTableCounts,
    pub twitch_live_state: RenameTableCounts,
    pub twitch_partners: RenameTableCounts,
    pub twitch_raid_auth: RenameTableCounts,
    pub twitch_partner_raid_scores: RenameTableCounts,
    pub twitch_streamer_identities: RenameTableCounts,
    pub twitch_engagement_settings: RenameTableCounts,
    pub twitch_engagement_channel_profile: RenameTableCounts,
    pub twitch_stream_sessions: RenameTableCounts,
    pub twitch_streamer_invites: RenameTableCounts,
    pub twitch_raw_chat_ingest_health: RenameTableCounts,
}

impl RenameCounts {
    pub fn total(&self) -> RenameTableCounts {
        let mut total = RenameTableCounts::default();
        for (_, counts) in self.tables() {
            total.add(counts);
        }
        total
    }

    fn tables(&self) -> [(&'static str, &RenameTableCounts); 11] {
        [
            ("twitch_streamers", &self.twitch_streamers),
            ("twitch_live_state", &self.twitch_live_state),
            ("twitch_partners", &self.twitch_partners),
            ("twitch_raid_auth", &self.twitch_raid_auth),
            (
                "twitch_partner_raid_scores",
                &self.twitch_partner_raid_scores,
            ),
            (
                "twitch_streamer_identities",
                &self.twitch_streamer_identities,
            ),
            (
                "twitch_engagement_settings",
                &self.twitch_engagement_settings,
            ),
            (
                "twitch_engagement_channel_profile",
                &self.twitch_engagement_channel_profile,
            ),
            ("twitch_stream_sessions", &self.twitch_stream_sessions),
            ("twitch_streamer_invites", &self.twitch_streamer_invites),
            (
                "twitch_raw_chat_ingest_health",
                &self.twitch_raw_chat_ingest_health,
            ),
        ]
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
    // twitch_streamer_identities zuerst: der Trigger
    // `trg_twitch_streamers_sync_identity` spiegelt jedes Update auf
    // twitch_streamers dorthin. Läge dort noch eine veraltete Fremdzeile mit
    // dem neuen Login, liefe die Umbenennung von twitch_streamers in die
    // Unique-Verletzung des Triggers statt in unsere Konfliktbehandlung.
    let twitch_streamer_identities = rewrite_user_scoped(
        &mut tx,
        "twitch_streamer_identities",
        "twitch_login",
        user_id,
        &new_login,
        true,
    )
    .await?;
    let counts = RenameCounts {
        twitch_streamer_identities,
        twitch_streamers: rewrite_user_scoped(
            &mut tx,
            "twitch_streamers",
            "twitch_login",
            user_id,
            &new_login,
            true,
        )
        .await?,
        twitch_live_state: rewrite_user_scoped(
            &mut tx,
            "twitch_live_state",
            "streamer_login",
            user_id,
            &new_login,
            false,
        )
        .await?,
        twitch_partners: rewrite_partners(&mut tx, user_id, &new_login).await?,
        twitch_raid_auth: rewrite_user_scoped(
            &mut tx,
            "twitch_raid_auth",
            "twitch_login",
            user_id,
            &new_login,
            true,
        )
        .await?,
        twitch_partner_raid_scores: rewrite_user_scoped(
            &mut tx,
            "twitch_partner_raid_scores",
            "twitch_login",
            user_id,
            &new_login,
            false,
        )
        .await?,
        twitch_engagement_settings: rewrite_login_keyed(
            &mut tx,
            "twitch_engagement_settings",
            "channel_login",
            user_id,
            &old_login,
            &new_login,
        )
        .await?,
        twitch_engagement_channel_profile: rewrite_login_keyed(
            &mut tx,
            "twitch_engagement_channel_profile",
            "channel_login",
            user_id,
            &old_login,
            &new_login,
        )
        .await?,
        twitch_stream_sessions: RenameTableCounts {
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
        twitch_streamer_invites: rewrite_login_keyed(
            &mut tx,
            "twitch_streamer_invites",
            "streamer_login",
            user_id,
            &old_login,
            &new_login,
        )
        .await?,
        twitch_raw_chat_ingest_health: rewrite_login_keyed(
            &mut tx,
            "twitch_raw_chat_ingest_health",
            "streamer_login",
            user_id,
            &old_login,
            &new_login,
        )
        .await?,
    };
    record_login_aliases(&mut tx, user_id, &old_login, &new_login).await?;
    tx.commit().await?;

    for (table, table_counts) in counts.tables() {
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
        "Twitch-Login umbenannt"
    );
    Ok(RenameReport {
        twitch_user_id: user_id.to_string(),
        old_login,
        new_login,
        counts,
    })
}

/// Umbenennung einer Tabelle, die eine `twitch_user_id` führt.
///
/// Die eigene Zeile bleibt in jedem Fall erhalten: Twitch hat den neuen Login
/// gerade für diese `user_id` bestätigt, eine fremde Zeile mit demselben Login
/// ist also der veraltete Stand. Blockiert eine solche Fremdzeile den
/// Unique-Index, bekommt sie einen Platzhalter-Login — gelöscht wird nichts,
/// sonst gingen Raid-Tokens, Partner-Konfiguration oder die Discord-Verknüpfung
/// stillschweigend verloren.
///
/// Geschrieben wird über die `user_id`, nicht über den alten Login: Tabellen,
/// die einen abweichenden Stale-Login tragen, konvergieren nur so.
async fn rewrite_user_scoped(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    login_column: &str,
    user_id: &str,
    new_login: &str,
    login_is_unique: bool,
) -> Result<RenameTableCounts, sqlx::Error> {
    let mut counts = RenameTableCounts::default();
    if login_is_unique {
        counts.stale_cleared =
            clear_stale_foreign_login(tx, table, login_column, user_id, new_login, None).await?;
    }
    let update = format!(
        "UPDATE {table} SET {login_column} = $2
         WHERE twitch_user_id = $1 AND LOWER({login_column}) <> LOWER($2)"
    );
    counts.renamed = sqlx::query(&update)
        .bind(user_id)
        .bind(new_login)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(counts)
}

/// Wie [`rewrite_user_scoped`], aber der Unique-Index von `twitch_partners`
/// greift nur für aktive Partner — die Kollisionsprüfung muss das mitführen.
async fn rewrite_partners(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
    new_login: &str,
) -> Result<RenameTableCounts, sqlx::Error> {
    let stale_cleared = clear_stale_foreign_login(
        tx,
        "twitch_partners",
        "twitch_login",
        user_id,
        new_login,
        Some("status = 'active'"),
    )
    .await?;
    let renamed = sqlx::query(
        "UPDATE twitch_partners SET twitch_login = $2
         WHERE twitch_user_id = $1 AND LOWER(twitch_login) <> LOWER($2)",
    )
    .bind(user_id)
    .bind(new_login)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(RenameTableCounts {
        renamed,
        stale_cleared,
        skipped: 0,
    })
}

/// Gibt den Unique-Index frei, den eine veraltete Fremdzeile mit dem neuen
/// Login belegt — durch einen Platzhalter, der die `user_id` der Fremdzeile
/// enthält und deshalb selbst nicht kollidieren kann.
async fn clear_stale_foreign_login(
    tx: &mut Transaction<'_, Postgres>,
    table: &str,
    login_column: &str,
    user_id: &str,
    new_login: &str,
    extra_condition: Option<&str>,
) -> Result<u64, sqlx::Error> {
    let extra = extra_condition
        .map(|condition| format!(" AND target.{condition}"))
        .unwrap_or_default();
    let placeholder = "'stale:' || COALESCE(target.twitch_user_id, 'unbekannt') || ':' || $2";
    let neutralize = format!(
        "UPDATE {table} target
            SET {login_column} = {placeholder}
          WHERE LOWER(target.{login_column}) = LOWER($2)
            AND target.twitch_user_id IS DISTINCT FROM $1{extra}
            AND NOT EXISTS (
                SELECT 1 FROM {table} belegt
                 WHERE belegt.{login_column} = {placeholder}
            )
        RETURNING COALESCE(target.twitch_user_id, 'unbekannt')"
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
