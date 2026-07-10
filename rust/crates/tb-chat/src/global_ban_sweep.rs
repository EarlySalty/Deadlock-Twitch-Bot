//! Sweep-Executor für den globalen Ban-Mechanismus.
//!
//! Port von `bot/chat/global_ban_sweep.py` (316 Z.).
//!
//! # Aufgabe
//!
//! Bannt alle Einträge der globalen Bannliste (`twitch_chatter_global_ban`)
//! über alle **offline** Partner-Kanäle mit aktivem OAuth. Idempotent via
//! Applied-Ledger (`twitch_chatter_global_ban_applied`) + Twitch-`already
//! banned`-Antwort. Keine Chat-Nachricht, kein Discord-Alert.
//!
//! # Zwei Ausführungspfade
//!
//! 1. **Due-Sweeps** (`run_due_sweeps`, Trigger: 120s-Poll): Kanäle die genau
//!    3600 s nach Stream-Ende fällig sind (`run_after <= NOW()`).
//! 2. **Voll-Sweep** (`run_full_sweep`, Trigger: täglich ab 6 Uhr):
//!    alle offline Partner einmalig pro Tag.
//!
//! # Parallelschutz
//!
//! Ein `tokio::sync::Mutex` auf `GlobalBanSweeper` verhindert, dass gleichzeitige
//! Invokations beider Pfade in denselben Kanal schreiben. Der Applied-Ledger mit
//! `ON CONFLICT DO NOTHING` ist die letzte Sicherheitsschicht.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::{Local, Timelike};
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::api::{BanOutcome, ChatApi};

// ── Konstanten (aus dem Vertrag) ───────────────────────────────────────────────

/// Grund für alle Sweep-Bans. `global_ban_sweep.py:28`.
const GLOBAL_BAN_REASON: &str = "Netzwerkweiter Ban: Verstoß gegen Community-Richtlinien";

/// Maximale Länge des Reason-Feldes. `global_ban_sweep.py:100`.
const REASON_MAX_LEN: usize = 500;

/// Intervall des Due-Check-Loops. `raid/bot.py:257`.
const DUE_CHECK_INTERVAL: Duration = Duration::from_secs(120);

// ── Integrations-Trait (Orchestrator verdrahtet) ───────────────────────────────

/// Liefert die aktiven Partner für den Sweep-Filter.
///
/// Entspricht `partner_utils.get_all_partners(include_archived=False)` +
/// `pg.load_valid_raid_auth_ids()` aus `global_ban_sweep.py:159–183`.
/// Der Orchestrator implementiert diesen Trait mit den echten DB-Queries,
/// Tests nutzen Mock-Implementierungen.
#[async_trait::async_trait]
pub trait PartnerRoster: Send + Sync {
    /// Alle aktiven (nicht-archivierten) Partner, Format: `(login, broadcaster_id)`.
    async fn all_active_partners(&self) -> Vec<(String, String)>;

    /// IDs aller Partner mit aktivem OAuth (`needs_reauth = FALSE`).
    /// `pg.py:4201–4210`: `SELECT twitch_user_id FROM twitch_raid_auth WHERE needs_reauth = FALSE`
    async fn valid_auth_ids(&self) -> HashSet<String>;

    /// IDs aller aktuell live Partner.
    /// `twitch_live_state.is_live = 1` (INTEGER!, `global_ban_sweep.py:145`).
    async fn live_broadcaster_ids(&self) -> HashSet<String>;

    /// Prüft ob `login` ein operativer Partner-Kanal ist (Selbstschutz-Guard).
    /// `partner_utils.is_operational_partner_channel()`, `global_ban_sweep.py:196–197`.
    async fn is_operational_partner_channel(&self, login: &str) -> bool;
}

// ── Sweep-Executor ─────────────────────────────────────────────────────────────

/// Sweep-Executor für globale Bans über alle offline Partner.
///
/// Instantiierung via `GlobalBanSweeper::new(pool, api)`, Start via `spawn`.
pub struct GlobalBanSweeper {
    pool: PgPool,
    api: Arc<dyn ChatApi>,
    /// Mutex verhindert parallele Sweep-Instanzen (z. B. Due-Check trifft auf
    /// gleichzeitigen Voll-Sweep). `global_ban_sweep.py`-Anmerkung: Python hatte
    /// keinen Lock; Rust fügt ihn als explizite Sicherheitsschicht hinzu.
    lock: Mutex<()>,
}

impl GlobalBanSweeper {
    /// Erstellt einen neuen Sweeper.
    pub fn new(pool: PgPool, api: Arc<dyn ChatApi>) -> Self {
        Self {
            pool,
            api,
            lock: Mutex::new(()),
        }
    }

    /// Startet den 120s-Poll-Loop + täglichen 6-Uhr-Checker.
    ///
    /// Loop-Logik aus `raid/bot.py:257–339`:
    /// - alle 120 s: `run_due_sweeps`
    /// - einmal pro Tag wenn `hour >= 6`: `run_full_sweep`
    pub fn spawn(self: Arc<Self>, roster: Arc<dyn PartnerRoster>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(DUE_CHECK_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut last_full_sweep_day: Option<chrono::NaiveDate> = None;

            loop {
                interval.tick().await;

                // Due-Sweeps immer (raid/bot.py:318)
                self.run_due_sweeps(roster.as_ref()).await;

                // 6-Uhr-Vollsweep: einmal pro Tag (raid/bot.py:330–339)
                let now_local = Local::now();
                let today = now_local.date_naive();
                if now_local.hour() >= 6 && last_full_sweep_day != Some(today) {
                    self.run_full_sweep(roster.as_ref()).await;
                    last_full_sweep_day = Some(today);
                }
            }
        });
    }

    // ── Öffentliche Sweep-Methoden (auch direkt aufrufbar) ─────────────────────

    /// Täglicher Sweep über alle offline Partner. `global_ban_sweep.py:253`.
    pub async fn run_full_sweep(&self, roster: &dyn PartnerRoster) {
        let _guard = self.lock.lock().await;
        let targets = offline_partner_targets(roster).await;
        if targets.is_empty() {
            return;
        }

        let mut applied_pairs = load_applied_pairs(&self.pool).await;
        let mut total = 0usize;

        for (login, bid) in &targets {
            let count = self
                .apply_bans_to_channel(login, bid, roster, &mut applied_pairs)
                .await;
            total += count;
        }

        if total > 0 {
            tracing::info!(
                total,
                kanäle = targets.len(),
                "GlobalBanSweep (Voll): {} Ban(s) über {} Kanal/-Kanäle gesetzt",
                total,
                targets.len()
            );
        }
    }

    /// Fällige Einzel-Sweeps (1h nach Stream-Ende). `global_ban_sweep.py:278`.
    pub async fn run_due_sweeps(&self, roster: &dyn PartnerRoster) {
        let _guard = self.lock.lock().await;
        let due = load_due_sweeps(&self.pool).await;
        if due.is_empty() {
            return;
        }

        let live_ids = roster.live_broadcaster_ids().await;
        let mut applied_pairs = load_applied_pairs(&self.pool).await;

        for (login, bid) in &due {
            // Leere Login/BID → Eintrag bereinigen, skip
            if login.is_empty() || bid.is_empty() {
                delete_sweep_due(&self.pool, login).await;
                continue;
            }
            // Kanal wieder live → Fälligkeit bewahren, nicht löschen
            if live_ids.contains(bid.as_str()) {
                continue;
            }

            self.apply_bans_to_channel(login, bid, roster, &mut applied_pairs)
                .await;
            delete_sweep_due(&self.pool, login).await;
        }
    }

    // ── Kanal-Level-Ban ────────────────────────────────────────────────────────

    /// Bannt alle Listen-Einträge in einem einzelnen Kanal.
    /// `global_ban_sweep.py:187`.
    ///
    /// Gibt Anzahl frisch gesetzter Bans zurück.
    async fn apply_bans_to_channel(
        &self,
        broadcaster_login: &str,
        broadcaster_id: &str,
        roster: &dyn PartnerRoster,
        applied_pairs: &mut HashSet<(String, String)>,
    ) -> usize {
        // Guard: leere IDs
        if broadcaster_login.is_empty() || broadcaster_id.is_empty() {
            return 0;
        }
        // Guard: operativer Partner-Kanal
        if !roster
            .is_operational_partner_channel(broadcaster_login)
            .await
        {
            return 0;
        }
        // Guard: live-Check (`global_ban_sweep.py:196`)
        if roster
            .live_broadcaster_ids()
            .await
            .contains(broadcaster_id)
        {
            return 0;
        }

        let entries = list_bans(&self.pool).await;
        let mut count = 0usize;

        for entry in &entries {
            let login_lower = entry.chatter_login.to_lowercase();
            let pair = (login_lower.clone(), broadcaster_id.to_string());

            // Schon gebannt (Applied-Ledger-Dedup)
            if applied_pairs.contains(&pair) {
                continue;
            }
            // Partner schützen: Ziel ist selbst Partner
            if roster.is_operational_partner_channel(&login_lower).await {
                continue;
            }

            // chatter_id auflösen wenn nicht gesetzt
            let target_id = match entry.chatter_id.as_deref().filter(|s| !s.is_empty()) {
                Some(id) => Some(id.to_string()),
                None => {
                    // Helix-Lookup. Erfolgreich aufgelöste IDs werden in die
                    // global-ban-Tabelle zurückgeschrieben (Rust-Erweiterung
                    // gegenüber Python, das nur im Speicher auflöst — Grillme
                    // `ban-sweep-lurker-01`: numerisches Matching ist robuster).
                    match self.api.resolve_user_id(&login_lower).await {
                        Ok(Some(id)) => {
                            write_back_chatter_id(&self.pool, &login_lower, &id).await;
                            Some(id)
                        }
                        _ => None,
                    }
                }
            };

            let Some(target_id) = target_id else {
                // Kein ID auflösbar → überspringen
                continue;
            };

            // Selbstschutz: Ziel == Broadcaster (moderator_id kommt intern aus ban_user)
            if target_id == broadcaster_id {
                continue;
            }

            // Safe-List: NACH der ID-Auflösung, denn Banlisten-Einträge dürfen
            // ohne gespeicherte ID existieren und per Login auflösen. Der Check
            // sitzt unmittelbar vor der Aktion, damit ihn kein Pfad umgeht.
            if crate::safe_list::is_safe(Some(&target_id), &login_lower) {
                tracing::warn!(
                    chatter = %login_lower,
                    "GlobalBanSweep: Safe-List-Konto steht auf der Banliste, kein Ban"
                );
                continue;
            }

            let reason = build_reason(entry.reason.as_deref());

            let outcome = self
                .api
                .ban_user(broadcaster_id, &target_id, &reason)
                .await;

            match outcome {
                Ok(BanOutcome::Banned) | Ok(BanOutcome::AlreadyBanned) => {
                    record_applied(&self.pool, &login_lower, broadcaster_id).await;
                    applied_pairs.insert(pair);
                    count += 1;
                }
                Ok(BanOutcome::Forbidden) => {
                    // 403: kein Ledger-Eintrag → nächster Sweep versucht erneut
                    tracing::warn!(
                        broadcaster = broadcaster_login,
                        chatter = %login_lower,
                        "GlobalBanSweep: Bot kein Moderator (403) — Ban übersprungen"
                    );
                }
                Ok(BanOutcome::Failed { status, ref body }) => {
                    tracing::warn!(
                        broadcaster = broadcaster_login,
                        chatter = %login_lower,
                        status,
                        body = %body.chars().take(120).collect::<String>(),
                        "GlobalBanSweep: Ban fehlgeschlagen"
                    );
                }
                Ok(BanOutcome::Unbanned) => {
                    // Sollte bei ban_user nicht vorkommen, ignorieren
                }
                Err(e) => {
                    tracing::debug!(
                        error = %e,
                        "GlobalBanSweep: Ban-Call Exception"
                    );
                }
            }
        }

        count
    }
}

// ── Hilfsfunktionen ────────────────────────────────────────────────────────────

/// Offline-Partner-Targets: alle aktiven Partner die offline sind und
/// gültiges OAuth haben. `global_ban_sweep.py:159`.
async fn offline_partner_targets(roster: &dyn PartnerRoster) -> Vec<(String, String)> {
    let all = roster.all_active_partners().await;
    let live_ids = roster.live_broadcaster_ids().await;
    let valid_auth = roster.valid_auth_ids().await;

    all.into_iter()
        .filter(|(_, bid)| !bid.is_empty())
        .filter(|(_, bid)| !live_ids.contains(bid.as_str()))
        .filter(|(_, bid)| valid_auth.contains(bid.as_str()))
        .collect()
}

/// Reason mit Truncation auf 500 Zeichen. `global_ban_sweep.py:100`.
fn build_reason(custom_reason: Option<&str>) -> String {
    let base = custom_reason
        .filter(|s| !s.is_empty())
        .unwrap_or(GLOBAL_BAN_REASON);
    base.chars().take(REASON_MAX_LEN).collect()
}

// ── DB-Queries ─────────────────────────────────────────────────────────────────

/// Alle Einträge in `twitch_chatter_global_ban`, neueste zuerst.
/// `pg.py:4217` — `list_chatter_global_bans`.
async fn list_bans(pool: &PgPool) -> Vec<BanEntry> {
    sqlx::query_as!(
        BanEntry,
        r#"
        SELECT chatter_login AS "chatter_login!", chatter_id, reason
          FROM twitch_chatter_global_ban
         ORDER BY added_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_else(|e| {
        tracing::debug!("GlobalBanSweep: list_bans fehlgeschlagen: {e}");
        vec![]
    })
}

/// Lädt alle (chatter_login, broadcaster_id)-Paare aus dem Applied-Ledger.
/// `pg.py:4270` — `load_applied_global_ban_pairs`.
async fn load_applied_pairs(pool: &PgPool) -> HashSet<(String, String)> {
    let rows = sqlx::query!(
        "SELECT chatter_login AS \"chatter_login!\", \
                broadcaster_id AS \"broadcaster_id!\" \
         FROM twitch_chatter_global_ban_applied",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_else(|e| {
        tracing::debug!("GlobalBanSweep: load_applied_pairs fehlgeschlagen: {e}");
        vec![]
    });

    rows.into_iter()
        .map(|row| (row.chatter_login.to_lowercase(), row.broadcaster_id))
        .collect()
}

/// Schreibt Ban-Anwendung in den Applied-Ledger. `pg.py:4257`.
async fn record_applied(pool: &PgPool, chatter_login: &str, broadcaster_id: &str) {
    let result = sqlx::query!(
        r#"
        INSERT INTO twitch_chatter_global_ban_applied (chatter_login, broadcaster_id)
        VALUES ($1, $2)
        ON CONFLICT (chatter_login, broadcaster_id) DO NOTHING
        "#,
        chatter_login.to_lowercase(),
        broadcaster_id,
    )
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::debug!("GlobalBanSweep: record_applied fehlgeschlagen: {e}");
    }
}

/// Lädt fällige Sweep-Einträge (`run_after <= NOW()`). `pg.py:4305`.
async fn load_due_sweeps(pool: &PgPool) -> Vec<(String, String)> {
    sqlx::query!(
        r#"
        SELECT broadcaster_login AS "broadcaster_login!",
               broadcaster_id AS "broadcaster_id!"
          FROM twitch_global_ban_sweep_due
         WHERE run_after <= NOW()
        "#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_else(|e| {
        tracing::debug!("GlobalBanSweep: load_due_sweeps fehlgeschlagen: {e}");
        vec![]
    })
    .into_iter()
    .map(|row| (row.broadcaster_login, row.broadcaster_id))
    .collect()
}

/// Löscht einen fälligen Sweep-Eintrag. `pg.py:4323`.
async fn delete_sweep_due(pool: &PgPool, broadcaster_login: &str) {
    let result = sqlx::query!(
        "DELETE FROM twitch_global_ban_sweep_due WHERE broadcaster_login = $1",
        broadcaster_login.to_lowercase(),
    )
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::debug!("GlobalBanSweep: delete_sweep_due fehlgeschlagen: {e}");
    }
}

/// Schreibt eine per Helix aufgelöste `chatter_id` in `twitch_chatter_global_ban`
/// zurück (nur wenn die Spalte noch `NULL` ist).
///
/// Bewusste Rust-Erweiterung: `global_ban_sweep.py` (`_resolve_user_id`, Z. 31–60)
/// löst den Login nur im Speicher auf und verwirft die ID nach dem Bann. Wir
/// persistieren sie, damit künftige Sweeps die ID direkt nutzen (kein erneuter
/// Helix-Call) und das Matching numerisch statt über den veränderlichen Login
/// läuft (Grillme `ban-sweep-lurker-01` — „behalten, robusteres Matching").
async fn write_back_chatter_id(pool: &PgPool, login: &str, chatter_id: &str) {
    let result = sqlx::query!(
        r#"
        UPDATE twitch_chatter_global_ban
           SET chatter_id = $2
         WHERE chatter_login = $1
           AND chatter_id IS NULL
        "#,
        login,
        chatter_id,
    )
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::debug!("GlobalBanSweep: write_back_chatter_id fehlgeschlagen: {e}");
    }
}

// ── Interne Typen ──────────────────────────────────────────────────────────────

/// Zeile aus `twitch_chatter_global_ban` für den Sweep.
#[derive(Debug, sqlx::FromRow)]
struct BanEntry {
    chatter_login: String,
    chatter_id: Option<String>,
    reason: Option<String>,
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use std::str::FromStr;
    use std::sync::Mutex as StdMutex;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

    // ── Pool-Helpers ───────────────────────────────────────────────────────────

    macro_rules! pool_or_skip {
        ($schema:expr) => {{
            let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
                if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                    panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                }
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            };
            pool_in_schema(&dsn, $schema).await
        }};
    }

    async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .unwrap();
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
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        apply_ddl(&pool).await;
        pool
    }

    /// Prod-treue DDL für alle Sweep-relevanten Tabellen.
    async fn apply_ddl(pool: &PgPool) {
        for ddl in [
            // twitch_chatter_global_ban: TIMESTAMPTZ (prod_schema_twitch.txt)
            r#"CREATE TABLE twitch_chatter_global_ban (
                chatter_login  TEXT PRIMARY KEY,
                chatter_id     TEXT,
                reason         TEXT,
                added_by       TEXT,
                added_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
            // twitch_chatter_global_ban_applied: PRIMARY KEY (chatter_login, broadcaster_id)
            // laut prod_schema_twitch.txt — KEINE id-Spalte!
            r#"CREATE TABLE twitch_chatter_global_ban_applied (
                chatter_login  TEXT NOT NULL,
                broadcaster_id TEXT NOT NULL,
                applied_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (chatter_login, broadcaster_id)
            )"#,
            // twitch_global_ban_sweep_due: run_after + scheduled_at TIMESTAMPTZ
            r#"CREATE TABLE twitch_global_ban_sweep_due (
                broadcaster_login TEXT PRIMARY KEY,
                broadcaster_id    TEXT NOT NULL,
                run_after         TIMESTAMPTZ NOT NULL,
                scheduled_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
            // twitch_live_state: is_live = INTEGER (prod_schema_twitch.txt)
            r#"CREATE TABLE twitch_live_state (
                twitch_user_id TEXT PRIMARY KEY,
                streamer_login TEXT NOT NULL,
                is_live        INTEGER NOT NULL DEFAULT 0
            )"#,
            // twitch_raid_auth: needs_reauth = BOOLEAN (prod_schema_twitch.txt)
            r#"CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login   TEXT,
                needs_reauth   BOOLEAN NOT NULL DEFAULT FALSE
            )"#,
            // twitch_partners: Minimalversion für is_operational_partner_channel
            r#"CREATE TABLE twitch_partners (
                twitch_user_id TEXT,
                twitch_login   TEXT,
                status         TEXT
            )"#,
        ] {
            sqlx::query(ddl).execute(pool).await.unwrap();
        }
    }

    // ── Mock-ChatApi ───────────────────────────────────────────────────────────

    struct MockApi {
        /// Injiziertes Ergebnis für `ban_user` (default: Banned)
        ban_result: StdMutex<BanOutcome>,
        /// Alle ban_user-Calls: (broadcaster_id, target_id, reason)
        ban_calls: StdMutex<Vec<(String, String, String)>>,
        /// Resolve-Ergebnisse: login → Option<user_id>
        resolve_map: StdMutex<std::collections::HashMap<String, Option<String>>>,
    }

    impl MockApi {
        fn new_banned() -> Self {
            Self {
                ban_result: StdMutex::new(BanOutcome::Banned),
                ban_calls: StdMutex::new(vec![]),
                resolve_map: StdMutex::new(std::collections::HashMap::new()),
            }
        }
        fn set_ban_result(&self, r: BanOutcome) {
            *self.ban_result.lock().unwrap() = r;
        }
    }

    impl Default for MockApi {
        fn default() -> Self {
            Self::new_banned()
        }
    }

    #[async_trait]
    impl ChatApi for MockApi {
        async fn send_message(&self, _: &str, _: &str) -> Result<crate::types::SendOutcome, String> {
            unimplemented!()
        }
        async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
            unimplemented!()
        }
        async fn ban_user(
            &self,
            broadcaster_id: &str,
            target_user_id: &str,
            reason: &str,
        ) -> Result<BanOutcome, String> {
            self.ban_calls.lock().unwrap().push((
                broadcaster_id.to_string(),
                target_user_id.to_string(),
                reason.to_string(),
            ));
            let r = self.ban_result.lock().unwrap().clone();
            Ok(r)
        }
        async fn timeout_user(&self, _: &str, _: &str, _: u32, _: &str) -> Result<BanOutcome, String> {
            unimplemented!()
        }
        async fn unban_user(&self, _: &str, _: &str) -> Result<bool, String> {
            unimplemented!()
        }
        async fn delete_message(&self, _: &str, _: &str) -> Result<bool, String> {
            unimplemented!()
        }
        async fn user_created_at(&self, _: &str) -> Result<Option<DateTime<Utc>>, String> {
            unimplemented!()
        }
        async fn resolve_user_id(&self, login: &str) -> Result<Option<String>, String> {
            let map = self.resolve_map.lock().unwrap();
            Ok(map.get(login).cloned().flatten())
        }
        async fn bot_user_id(&self) -> String {
            "bot123".to_string()
        }
    }

    // ── Mock-PartnerRoster ─────────────────────────────────────────────────────

    struct MockRoster {
        partners: Vec<(String, String)>,
        live_ids: HashSet<String>,
        valid_auth: HashSet<String>,
        operational: HashSet<String>,
    }

    impl MockRoster {
        fn new(
            partners: Vec<(&str, &str)>,
            live_ids: Vec<&str>,
            valid_auth: Vec<&str>,
            operational: Vec<&str>,
        ) -> Self {
            Self {
                partners: partners
                    .into_iter()
                    .map(|(l, b)| (l.to_string(), b.to_string()))
                    .collect(),
                live_ids: live_ids.into_iter().map(str::to_string).collect(),
                valid_auth: valid_auth.into_iter().map(str::to_string).collect(),
                operational: operational.into_iter().map(str::to_string).collect(),
            }
        }
    }

    #[async_trait]
    impl PartnerRoster for MockRoster {
        async fn all_active_partners(&self) -> Vec<(String, String)> {
            self.partners.clone()
        }
        async fn valid_auth_ids(&self) -> HashSet<String> {
            self.valid_auth.clone()
        }
        async fn live_broadcaster_ids(&self) -> HashSet<String> {
            self.live_ids.clone()
        }
        async fn is_operational_partner_channel(&self, login: &str) -> bool {
            self.operational.contains(login)
        }
    }

    fn sweeper(pool: PgPool, api: Arc<MockApi>) -> GlobalBanSweeper {
        GlobalBanSweeper::new(pool, api)
    }

    // ── Unit-Tests: build_reason ───────────────────────────────────────────────

    #[test]
    fn build_reason_nutzt_default_wenn_leer() {
        assert_eq!(build_reason(None), GLOBAL_BAN_REASON);
        assert_eq!(build_reason(Some("")), GLOBAL_BAN_REASON);
    }

    #[test]
    fn build_reason_nutzt_custom_reason() {
        assert_eq!(build_reason(Some("Spam")), "Spam");
    }

    #[test]
    fn build_reason_trunciert_auf_500_zeichen() {
        let lang: String = "x".repeat(600);
        let r = build_reason(Some(&lang));
        assert_eq!(r.len(), 500);
    }

    // ── DB-Tests ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn record_applied_idempotent() {
        let pool = pool_or_skip!("gbs_applied_idem");

        record_applied(&pool, "user1", "bid1").await;
        record_applied(&pool, "user1", "bid1").await; // ON CONFLICT DO NOTHING

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM twitch_chatter_global_ban_applied")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "doppeltes record_applied darf keinen Fehler/Duplikat erzeugen");
    }

    #[tokio::test]
    async fn load_due_sweeps_gibt_nur_faellige() {
        let pool = pool_or_skip!("gbs_due_sweeps");

        sqlx::query(
            "INSERT INTO twitch_global_ban_sweep_due (broadcaster_login, broadcaster_id, run_after)
             VALUES ('faellig', 'bid_a', NOW() - INTERVAL '1 minute'),
                    ('noch_nicht', 'bid_b', NOW() + INTERVAL '1 hour')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let due = load_due_sweeps(&pool).await;
        assert_eq!(due.len(), 1, "nur 1 fälliger Eintrag erwartet");
        assert_eq!(due[0].0, "faellig");
    }

    #[tokio::test]
    async fn delete_sweep_due_entfernt_eintrag() {
        let pool = pool_or_skip!("gbs_delete_due");

        sqlx::query(
            "INSERT INTO twitch_global_ban_sweep_due (broadcaster_login, broadcaster_id, run_after)
             VALUES ('kanal', 'bid_c', NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();

        delete_sweep_due(&pool, "kanal").await;

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM twitch_global_ban_sweep_due")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn apply_bans_to_channel_bannt_und_schreibt_ledger() {
        let pool = pool_or_skip!("gbs_apply_bans");

        // Ban-Eintrag anlegen
        sqlx::query(
            "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id, reason)
             VALUES ('boser_user', 'uid_boser', 'Spam')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let api = Arc::new(MockApi::default());
        let roster = MockRoster::new(
            vec![("kanal", "bid1")],
            vec![], // offline
            vec!["bid1"],
            vec!["kanal"], // operational
        );

        let sw = sweeper(pool.clone(), api.clone());
        let mut applied = load_applied_pairs(&pool).await;
        let count = sw
            .apply_bans_to_channel("kanal", "bid1", &roster, &mut applied)
            .await;

        assert_eq!(count, 1, "1 Ban erwartet");
        {
            let calls = api.ban_calls.lock().unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].0, "bid1");
            assert_eq!(calls[0].1, "uid_boser");
        } // Guard vor dem await fallen lassen

        let ledger: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM twitch_chatter_global_ban_applied")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(ledger, 1, "Ledger-Eintrag gesetzt");
    }

    #[tokio::test]
    async fn apply_bans_ueberspringt_bereits_angewendete() {
        let pool = pool_or_skip!("gbs_dedup");

        sqlx::query(
            "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id)
             VALUES ('dup_user', 'uid_dup')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Ledger-Eintrag vorbelegen
        record_applied(&pool, "dup_user", "bid2").await;

        let api = Arc::new(MockApi::default());
        let roster = MockRoster::new(
            vec![("kanal2", "bid2")],
            vec![],
            vec!["bid2"],
            vec!["kanal2"],
        );

        let sw = sweeper(pool.clone(), api.clone());
        let mut applied = load_applied_pairs(&pool).await;
        let count = sw
            .apply_bans_to_channel("kanal2", "bid2", &roster, &mut applied)
            .await;

        assert_eq!(count, 0, "bereits im Ledger → kein erneuter Ban");
        assert!(api.ban_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn apply_bans_403_erzeugt_keinen_ledger_eintrag() {
        let pool = pool_or_skip!("gbs_403");

        sqlx::query(
            "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id)
             VALUES ('blocked', 'uid_blocked')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let api = Arc::new(MockApi::default());
        api.set_ban_result(BanOutcome::Forbidden);

        let roster = MockRoster::new(
            vec![("kanal3", "bid3")],
            vec![],
            vec!["bid3"],
            vec!["kanal3"],
        );

        let sw = sweeper(pool.clone(), api.clone());
        let mut applied = load_applied_pairs(&pool).await;
        let count = sw
            .apply_bans_to_channel("kanal3", "bid3", &roster, &mut applied)
            .await;

        assert_eq!(count, 0, "403 → kein Ban-Zähler");
        let ledger: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM twitch_chatter_global_ban_applied")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(ledger, 0, "kein Ledger-Eintrag bei 403");
    }

    #[tokio::test]
    async fn apply_bans_ueberspringt_live_broadcaster() {
        let pool = pool_or_skip!("gbs_live_skip");

        sqlx::query(
            "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id)
             VALUES ('jemand', 'uid_jemand')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let api = Arc::new(MockApi::default());
        let roster = MockRoster::new(
            vec![("live_kanal", "bid_live")],
            vec!["bid_live"], // ist live!
            vec!["bid_live"],
            vec!["live_kanal"],
        );

        let sw = sweeper(pool.clone(), api.clone());
        let mut applied = load_applied_pairs(&pool).await;
        let count = sw
            .apply_bans_to_channel("live_kanal", "bid_live", &roster, &mut applied)
            .await;

        assert_eq!(count, 0, "live Kanal wird übersprungen");
    }

    /// Safe-List: steht ein Safe-Konto (per ID) auf der Banliste, bannt der
    /// Sweep es nicht.
    #[tokio::test]
    async fn apply_bans_verschont_safe_konto_mit_id() {
        let pool = pool_or_skip!("gbs_safe_id");
        let safe = &crate::safe_list::SAFE_ACCOUNTS[0];

        sqlx::query("INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id) VALUES ($1, $2)")
            .bind(safe.login)
            .bind(safe.twitch_user_id)
            .execute(&pool)
            .await
            .unwrap();

        let api = Arc::new(MockApi::default());
        let roster = MockRoster::new(vec![("ziel", "bid_z")], vec![], vec!["bid_z"], vec!["ziel"]);
        let sw = sweeper(pool.clone(), api.clone());
        let mut applied = load_applied_pairs(&pool).await;

        let count = sw
            .apply_bans_to_channel("ziel", "bid_z", &roster, &mut applied)
            .await;

        assert_eq!(count, 0, "Safe-Konto darf nicht gebannt werden");
        assert!(
            api.ban_calls.lock().unwrap().is_empty(),
            "ban_user wurde fuer Safe-Konto gerufen"
        );
    }

    /// Merge-Kritiker 2026-07-10: Banlisten-Eintrag OHNE chatter_id, dessen
    /// Login per Helix auf ein Safe-Konto auflöst. Der Guard muss NACH der
    /// Auflösung greifen, sonst bannt der Sweep das Safe-Konto.
    #[tokio::test]
    async fn apply_bans_verschont_safe_konto_das_erst_per_login_aufloest() {
        let pool = pool_or_skip!("gbs_safe_resolve");
        let safe = &crate::safe_list::SAFE_ACCOUNTS[0];

        sqlx::query(
            "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id)
             VALUES ('alter_alias', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let api = Arc::new(MockApi::default());
        {
            let mut m = api.resolve_map.lock().unwrap();
            // Der Login loest auf die Safe-ID auf.
            m.insert(
                "alter_alias".to_string(),
                Some(safe.twitch_user_id.to_string()),
            );
        }

        let roster = MockRoster::new(vec![("ziel", "bid_z")], vec![], vec!["bid_z"], vec!["ziel"]);
        let sw = sweeper(pool.clone(), api.clone());
        let mut applied = load_applied_pairs(&pool).await;

        let count = sw
            .apply_bans_to_channel("ziel", "bid_z", &roster, &mut applied)
            .await;

        assert_eq!(count, 0, "aufgeloeste Safe-ID darf nicht gebannt werden");
        assert!(
            api.ban_calls.lock().unwrap().is_empty(),
            "ban_user wurde fuer aufgeloestes Safe-Konto gerufen"
        );
    }

    #[tokio::test]
    async fn apply_bans_loest_fehlende_id_via_api_auf() {
        let pool = pool_or_skip!("gbs_resolve");

        // Kein chatter_id gesetzt
        sqlx::query(
            "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id)
             VALUES ('noname', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let api = Arc::new(MockApi::default());
        {
            let mut m = api.resolve_map.lock().unwrap();
            m.insert("noname".to_string(), Some("uid_resolved".to_string()));
        }

        let roster = MockRoster::new(
            vec![("ziel", "bid_z")],
            vec![],
            vec!["bid_z"],
            vec!["ziel"],
        );

        let sw = sweeper(pool.clone(), api.clone());
        let mut applied = load_applied_pairs(&pool).await;
        let count = sw
            .apply_bans_to_channel("ziel", "bid_z", &roster, &mut applied)
            .await;

        assert_eq!(count, 1, "Resolve erfolgreich → Ban gesetzt");
        // ID zurückgeschrieben
        let stored_id: Option<String> = sqlx::query_scalar(
            "SELECT chatter_id FROM twitch_chatter_global_ban WHERE chatter_login = 'noname'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored_id.as_deref(), Some("uid_resolved"), "ID zurückgeschrieben");
    }

    #[tokio::test]
    async fn run_due_sweeps_loescht_eintrag_nach_ausfuehrung() {
        let pool = pool_or_skip!("gbs_run_due");

        sqlx::query(
            "INSERT INTO twitch_global_ban_sweep_due (broadcaster_login, broadcaster_id, run_after)
             VALUES ('due_kanal', 'bid_due', NOW() - INTERVAL '1 second')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let api = Arc::new(MockApi::default());
        let roster = Arc::new(MockRoster::new(
            vec![("due_kanal", "bid_due")],
            vec![],
            vec!["bid_due"],
            vec!["due_kanal"],
        ));

        let sw = GlobalBanSweeper::new(pool.clone(), api.clone());
        sw.run_due_sweeps(roster.as_ref()).await;

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM twitch_global_ban_sweep_due")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0, "Due-Eintrag nach Ausführung gelöscht");
    }

    #[tokio::test]
    async fn run_due_sweeps_bewaehrt_faelligkeit_bei_live_kanal() {
        let pool = pool_or_skip!("gbs_due_live");

        sqlx::query(
            "INSERT INTO twitch_global_ban_sweep_due (broadcaster_login, broadcaster_id, run_after)
             VALUES ('live_k', 'bid_live2', NOW() - INTERVAL '1 second')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let api = Arc::new(MockApi::default());
        let roster = Arc::new(MockRoster::new(
            vec![("live_k", "bid_live2")],
            vec!["bid_live2"], // live!
            vec!["bid_live2"],
            vec!["live_k"],
        ));

        let sw = GlobalBanSweeper::new(pool.clone(), api);
        sw.run_due_sweeps(roster.as_ref()).await;

        // Fälligkeit bleibt erhalten
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM twitch_global_ban_sweep_due")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "Fälligkeit bei live Kanal bleibt erhalten");
    }

    #[tokio::test]
    async fn run_full_sweep_bannt_alle_offline_partner() {
        let pool = pool_or_skip!("gbs_full_sweep");

        sqlx::query(
            "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id)
             VALUES ('target_user', 'uid_target')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let api = Arc::new(MockApi::default());
        let roster = Arc::new(MockRoster::new(
            vec![("kanal_a", "bid_a"), ("kanal_b", "bid_b")],
            vec![],           // beide offline
            vec!["bid_a", "bid_b"],
            vec!["kanal_a", "kanal_b"],
        ));

        let sw = GlobalBanSweeper::new(pool.clone(), api.clone());
        sw.run_full_sweep(roster.as_ref()).await;

        let calls = api.ban_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 2, "Ban in beiden Kanälen erwartet");
        let ledger: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM twitch_chatter_global_ban_applied")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(ledger, 2, "2 Ledger-Einträge für 2 Kanäle");
    }

    #[tokio::test]
    async fn offline_partner_targets_filtert_korrekt() {
        let roster = MockRoster::new(
            vec![
                ("offline_partner", "bid_off"),
                ("live_partner", "bid_live"),
                ("no_auth_partner", "bid_noauth"),
            ],
            vec!["bid_live"],
            vec!["bid_off"], // nur offline_partner hat Auth
            vec![],
        );

        let targets = offline_partner_targets(&roster).await;
        assert_eq!(targets.len(), 1, "nur 1 Kanal soll durch alle Filter kommen");
        assert_eq!(targets[0].0, "offline_partner");
    }

    #[tokio::test]
    async fn partner_chatter_wird_nicht_gebannt() {
        // Ziel-Chatter ist selbst operativer Partner → Skip
        let pool = pool_or_skip!("gbs_partner_protect");

        sqlx::query(
            "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id)
             VALUES ('partner_kanal', 'uid_partner')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let api = Arc::new(MockApi::default());
        let roster = MockRoster::new(
            vec![("broadcaster", "bid_bc")],
            vec![],
            vec!["bid_bc"],
            vec!["broadcaster", "partner_kanal"], // partner_kanal ist operational!
        );

        let sw = sweeper(pool.clone(), api.clone());
        let mut applied = load_applied_pairs(&pool).await;
        let count = sw
            .apply_bans_to_channel("broadcaster", "bid_bc", &roster, &mut applied)
            .await;

        assert_eq!(count, 0, "Partner-Kanal als Ziel wird übersprungen");
        assert!(api.ban_calls.lock().unwrap().is_empty());
    }
}
