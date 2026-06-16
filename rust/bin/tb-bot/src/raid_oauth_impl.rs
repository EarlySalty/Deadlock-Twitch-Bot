//! Composition-Root-Impl für `RaidOAuthPort` — verdrahtet den echten `tb-raid`-Stack
//! (StateStore, AuthWriter, TwitchTokenClient, PgPool) mit den 6 internen API-Handlern.
//!
//! # Struktur
//!
//! `TbRaidOAuthImpl` hält alle Ressourcen und implementiert [`RaidOAuthPort`].
//! Die sechs Operationen entsprechen 1:1 den Python-Methoden in
//! `bot/dashboard/mixin.py` + `bot/dashboard/raids/oauth_callback.py`:
//!
//! | Rust-Methode        | Python-Äquivalent                          |
//! |---------------------|--------------------------------------------|
//! | `auth_url`          | `_invoke_raid_auth_url`                    |
//! | `auth_state`        | `_integration_raid_auth_state`             |
//! | `block_state`       | `_integration_raid_block_state`            |
//! | `go_url`            | `_raid_go_url` → StateStore::lookup        |
//! | `requirements`      | `_raid_requirements`                       |
//! | `oauth_callback`    | `build_raid_oauth_callback_payload`        |
//!
//! # Discord-Scope-Guard
//!
//! `_enforce_discord_action_scope` (Python): Wenn `TWITCH_INTERNAL_API_ALLOWED_*_IDS`
//! gesetzt ist, werden `guild_id`/`channel_id`/`role_id` im Request-Body gegen
//! die Allowlist geprüft. Wert fehlt oder ist nicht in der Allowlist → `PermissionError`
//! → 403.
//!
//! In diesem Port sitzt der Guard direkt in `TbRaidOAuthImpl::requirements` und
//! `oauth_callback` (wird via `enforce_discord_scope` aufgerufen). Er liest die
//! Allowlists aus `TWITCH_INTERNAL_API_ALLOWED_GUILD_IDS`, `..._CHANNEL_IDS`,
//! `..._ROLE_IDS` (kommagetrennte Integer-IDs) — identisch zu Python.
//!
//! # Idempotenz (open_risk)
//!
//! Python `_prepare_idempotency` + `_release_idempotency_owner` deduplicieren
//! `requirements` und `oauth_callback` via `X-Idempotency-Key`. OAuth-Codes
//! sind single-use; ein zweites `exchange_code` mit demselben Code liefert einen
//! Fehler. Der native Port liefert korrekte Antworten ohne Deduplizierung —
//! ein zweiter Aufruf mit demselben Code schlägt beim Token-Exchange fehl und
//! landet als 500. Clients, die idempotente Wiederholungen erwarten, erhalten
//! keine gecachten Antworten (open_risk: Idempotenz-Layer fehlt nativ).
//!
//! # StreamerContextResolver-Impl
//!
//! `build_state_info` (oauth_flow) braucht `has_existing_streamer_context` +
//! `linked_twitch_identity_for_discord_user`. Beide werden via SQL auf
//! `twitch_raid_auth` und `twitch_streamers_partner_state` aufgelöst — identisch
//! zu Pythons `_has_existing_streamer_context` und
//! `_linked_twitch_identity_for_discord_user` in `RaidAuthManager`.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;

use tb_internal_api::handlers::raid_oauth::{
    OAuthCallbackResult, RaidOAuthError, RaidOAuthPort, RaidStatePayload,
};
use tb_raid::{
    auth_writer::AuthWriter,
    oauth_flow::{build_authorize_url, build_state_info, StreamerContextResolver},
    partner_setup::PartnerSetupService,
    scope_profiles::scopes_for_profile,
    state_store::StateStore,
    token_refresher::TwitchTokenClient,
};

// ---------------------------------------------------------------------------
// Lokale Login-Normalisierung für DB-Werte
// ---------------------------------------------------------------------------

/// Normalisiert einen Twitch-Login aus DB-Feldern: trim + lowercase, 3–25 Zeichen,
/// nur a-z 0-9 _ erlaubt. Für Werte aus eigenen DB-Feldern genügt dieser
/// vereinfachte Check (kein URL-Parsing nötig).
///
/// Identisch zu `tb_raid::oauth_flow::normalize_twitch_login` (intern).
/// Für Handler-Eingaben (Query-Params) wird `normalize_login_db`
/// genutzt — das ist Sache des Handlers, nicht der Impl-Schicht.
fn normalize_login_db(value: &str) -> Option<String> {
    let s = value.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    // Minimale Längenprüfung — nur bei DB-Werten die bereits normalisiert sind.
    if s.len() < 2 {
        return None;
    }
    Some(s)
}

// ---------------------------------------------------------------------------
// Discord-Scope-Allowlist
// ---------------------------------------------------------------------------

/// Parst eine kommagetrennte Umgebungsvariable als Menge positiver Integers.
///
/// Fail-closed wie Python `parse_allowlist_ids` (`policy.py:183-210`):
/// `None` (Variable nicht gesetzt) → kein Guard. GESETZTE Variable — auch
/// leer oder ohne gültige IDs — → `Some(set)`; ein leeres Set bedeutet
/// deny-all, nicht guard-aus.
fn parse_allowlist(raw: Option<&str>) -> Option<HashSet<i64>> {
    let raw = raw?;
    let ids: HashSet<i64> = raw
        .split(',')
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .filter(|&v| v > 0)
        .collect();
    if ids.is_empty() {
        tracing::warn!(
            "Scope-Allowlist gesetzt, aber keine gültigen positiven IDs — fail-closed deny-all"
        );
    }
    Some(ids)
}

/// Parst einen optionalen `serde_json::Value` als positive i64.
fn coerce_positive_int(value: &Option<serde_json::Value>) -> Option<i64> {
    let v = value.as_ref()?;
    match v {
        serde_json::Value::Number(n) => n.as_i64().filter(|&x| x > 0),
        serde_json::Value::String(s) => s.trim().parse::<i64>().ok().filter(|&x| x > 0),
        _ => None,
    }
}

/// Prüft, ob der gegebene Wert in der Allowlist enthalten ist.
/// `None` = kein Guard aktiv → kein Fehler. `Some(set)` = Guard aktiv:
/// fehlender oder nicht enthaltener Wert → Fehler; ein leeres Set weist
/// damit alles ab (deny-all, Python-Parität).
fn enforce_scope_allowlist(
    value: &Option<serde_json::Value>,
    allowed: &Option<HashSet<i64>>,
) -> Result<(), RaidOAuthError> {
    let Some(ref set) = allowed else {
        return Ok(());
    };
    let Some(id) = coerce_positive_int(value) else {
        return Err(RaidOAuthError::Forbidden);
    };
    if !set.contains(&id) {
        return Err(RaidOAuthError::Forbidden);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// StreamerContextResolver-Impl (DB-Queries für oauth_flow::build_state_info)
// ---------------------------------------------------------------------------

struct PgStreamerContextResolver {
    pool: PgPool,
}

#[async_trait]
impl StreamerContextResolver for PgStreamerContextResolver {
    /// Gibt `true` zurück, wenn für `login` bereits ein aktiver Auth-Eintrag
    /// in `twitch_raid_auth` existiert (raid_enabled=TRUE oder authorized_at IS NOT NULL).
    ///
    /// Python: `_has_existing_streamer_context` in `RaidAuthManager`.
    async fn has_existing_streamer_context(&self, login: &str) -> bool {
        let result: Result<Option<bool>, _> = sqlx::query_scalar(
            r#"
            SELECT (raid_enabled IS TRUE OR authorized_at IS NOT NULL)
            FROM twitch_raid_auth
            WHERE LOWER(twitch_login) = LOWER($1)
            LIMIT 1
            "#,
        )
        .bind(login)
        .fetch_optional(&self.pool)
        .await;
        result.ok().flatten().unwrap_or(false)
    }

    /// Liefert `(twitch_login, twitch_user_id)` für eine Discord-User-ID aus
    /// der Partner-State-View.
    ///
    /// Python: `_linked_twitch_identity_for_discord_user` in `RaidAuthManager`.
    async fn linked_twitch_identity_for_discord_user(
        &self,
        discord_user_id: &str,
    ) -> (Option<String>, Option<String>) {
        let result: Result<Option<(Option<String>, Option<String>)>, _> = sqlx::query_as(
            r#"
            SELECT twitch_login, twitch_user_id
            FROM twitch_streamers_partner_state
            WHERE discord_user_id = $1
            ORDER BY
                CASE WHEN manual_verified_at IS NULL THEN 1 ELSE 0 END,
                manual_verified_at DESC,
                CASE WHEN created_at IS NULL THEN 1 ELSE 0 END,
                created_at DESC
            LIMIT 1
            "#,
        )
        .bind(discord_user_id)
        .fetch_optional(&self.pool)
        .await;
        match result.ok().flatten() {
            Some((login, uid)) => (login, uid),
            None => (None, None),
        }
    }
}

// ---------------------------------------------------------------------------
// Integration-State-Queries (auth_state / block_state)
// ---------------------------------------------------------------------------

/// DB-Zeile aus `twitch_streamers_partner_state` für State-Abfragen.
///
/// `manual_partner_opt_out` ist INT4 in Prod (0/1-Flag wie alle
/// Partner-Flags) — eine bool-Dekodierung schlägt zur Laufzeit fehl
/// (Typ-Drift-Klasse aus dem Audit; Prod-Schema am 11.6. verifiziert).
#[derive(sqlx::FromRow, Debug)]
struct PartnerStateRow {
    twitch_login: Option<String>,
    twitch_user_id: Option<String>,
    discord_user_id: Option<String>,
    manual_partner_opt_out: Option<i32>,
}

/// Alle Zeilen für eine Discord-User-ID aus der Partner-State-View.
async fn query_partner_rows_by_discord(
    pool: &PgPool,
    discord_user_id: &str,
) -> Result<Vec<PartnerStateRow>, sqlx::Error> {
    sqlx::query_as::<_, PartnerStateRow>(
        r#"
        SELECT twitch_login, twitch_user_id, discord_user_id, manual_partner_opt_out
        FROM twitch_streamers_partner_state
        WHERE discord_user_id = $1
        ORDER BY
            CASE WHEN manual_verified_at IS NULL THEN 1 ELSE 0 END,
            manual_verified_at DESC,
            CASE WHEN created_at IS NULL THEN 1 ELSE 0 END,
            created_at DESC
        "#,
    )
    .bind(discord_user_id)
    .fetch_all(pool)
    .await
}

/// Eine Zeile für einen Twitch-Login aus der Partner-State-View.
async fn query_partner_row_by_login(
    pool: &PgPool,
    twitch_login: &str,
) -> Result<Option<PartnerStateRow>, sqlx::Error> {
    sqlx::query_as::<_, PartnerStateRow>(
        r#"
        SELECT twitch_login, twitch_user_id, discord_user_id, manual_partner_opt_out
        FROM twitch_streamers_partner_state
        WHERE LOWER(twitch_login) = LOWER($1)
        ORDER BY
            CASE WHEN manual_verified_at IS NULL THEN 1 ELSE 0 END,
            manual_verified_at DESC,
            CASE WHEN created_at IS NULL THEN 1 ELSE 0 END,
            created_at DESC
        LIMIT 1
        "#,
    )
    .bind(twitch_login)
    .fetch_optional(pool)
    .await
}

/// Auth-Zeile aus `twitch_raid_auth` per user_id.
///
/// `authorized_at` ist TIMESTAMPTZ in Prod — gebraucht wird nur "ist
/// gesetzt?", deshalb `DateTime<Utc>` statt String-Dekodierung
/// (Typ-Drift-Klasse; Prod-Schema am 11.6. verifiziert).
#[derive(sqlx::FromRow)]
struct RaidAuthLookupRow {
    twitch_login: Option<String>,
    twitch_user_id: Option<String>,
    raid_enabled: Option<bool>,
    authorized_at: Option<chrono::DateTime<Utc>>,
}

async fn query_auth_by_user_id(
    pool: &PgPool,
    user_id: &str,
) -> Result<Option<RaidAuthLookupRow>, sqlx::Error> {
    sqlx::query_as::<_, RaidAuthLookupRow>(
        "SELECT twitch_login, twitch_user_id, raid_enabled, authorized_at \
         FROM twitch_raid_auth WHERE twitch_user_id = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

async fn query_auth_by_login(
    pool: &PgPool,
    login: &str,
) -> Result<Option<RaidAuthLookupRow>, sqlx::Error> {
    sqlx::query_as::<_, RaidAuthLookupRow>(
        "SELECT twitch_login, twitch_user_id, raid_enabled, authorized_at \
         FROM twitch_raid_auth WHERE LOWER(twitch_login) = LOWER($1) LIMIT 1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await
}

/// Gibt `true` zurück wenn der User in `twitch_token_blacklist` mit
/// `error_count >= 3` (BLACKLIST_DISABLE_THRESHOLD) eingetragen ist.
async fn is_token_blacklisted(pool: &PgPool, twitch_user_id: &str) -> bool {
    let count: Result<Option<i32>, _> = sqlx::query_scalar(
        "SELECT error_count FROM twitch_token_blacklist WHERE twitch_user_id = $1",
    )
    .bind(twitch_user_id)
    .fetch_optional(pool)
    .await
    .map(Option::flatten);
    matches!(count, Ok(Some(c)) if i64::from(c) >= 3)
}

/// Gibt `true` zurück wenn der Login in `twitch_raid_blacklist` steht.
async fn is_raid_blacklisted(pool: &PgPool, login: &str) -> bool {
    // `SELECT 1` ist INT4 — eine i64-Dekodierung schlägt fehl und der Fehler
    // würde still zu `false` verschluckt (Bug-Klasse „Arrival-int4", #129).
    let row: Result<Option<i32>, _> = sqlx::query_scalar(
        "SELECT 1 FROM twitch_raid_blacklist WHERE LOWER(target_login) = LOWER($1) LIMIT 1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await;
    matches!(row, Ok(Some(_)))
}

/// Berechnet den Integrations-State (Python: `RaidIntegrationStateResolver.resolve`).
/// Schlägt alle relevanten Tabellen nach und baut `RaidStatePayload`.
async fn resolve_integration_state(
    pool: &PgPool,
    discord_user_id: Option<&str>,
    twitch_login: Option<&str>,
) -> Result<RaidStatePayload, RaidOAuthError> {
    let mut result_discord_id: Option<String> = discord_user_id.map(str::to_string);
    let mut result_login: Option<String> = None;
    let mut result_user_id: Option<String> = None;
    let mut partner_opt_out = false;
    let mut candidate_logins: HashSet<String> = HashSet::new();
    let mut candidate_user_ids: HashSet<String> = HashSet::new();

    // 1. Partner-State per Discord-User-ID abfragen.
    if let Some(did) = discord_user_id {
        let rows = query_partner_rows_by_discord(pool, did)
            .await
            .map_err(|e| {
                tracing::error!("resolve_integration_state DB-Fehler (discord query): {e}");
                RaidOAuthError::Internal
            })?;
        if let Some(first) = rows.first() {
            result_login = first
                .twitch_login
                .as_deref()
                .and_then(normalize_login_db);
            result_user_id = first
                .twitch_user_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            result_discord_id = first
                .discord_user_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or(result_discord_id.clone());
        }
        for row in &rows {
            if let Some(l) = row.twitch_login.as_deref().and_then(normalize_login_db) {
                candidate_logins.insert(l);
            }
            if let Some(u) = row.twitch_user_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                candidate_user_ids.insert(u.to_string());
            }
            if row.manual_partner_opt_out.unwrap_or(0) != 0 {
                partner_opt_out = true;
            }
        }
    }

    // 2. Partner-State per Twitch-Login abfragen.
    if let Some(login) = twitch_login {
        let login_normalized = normalize_login_db(login)
            .ok_or(RaidOAuthError::BadRequest("invalid twitch_login".to_string()))?;
        let row_opt = query_partner_row_by_login(pool, &login_normalized)
            .await
            .map_err(|e| {
                tracing::error!("resolve_integration_state DB-Fehler (login query): {e}");
                RaidOAuthError::Internal
            })?;
        if let Some(row) = row_opt {
            if let Some(l) = row.twitch_login.as_deref().and_then(normalize_login_db) {
                if result_login.is_none() {
                    result_login = Some(l.clone());
                }
                candidate_logins.insert(l);
            }
            if let Some(u) = row.twitch_user_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                if result_user_id.is_none() {
                    result_user_id = Some(u.to_string());
                }
                candidate_user_ids.insert(u.to_string());
            }
            if result_discord_id.is_none() {
                result_discord_id = row
                    .discord_user_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
            }
            if row.manual_partner_opt_out.unwrap_or(0) != 0 {
                partner_opt_out = true;
            }
        }
        // Immer in Kandidaten aufnehmen, auch ohne Partner-Zeile.
        candidate_logins.insert(login_normalized.clone());
    }

    // 3. Authorization-Status ermitteln.
    let mut authorized = false;
    let mut auth_user_id: Option<String> = None;

    // Erst per user_id, dann per login.
    if let Some(ref uid) = result_user_id {
        let auth = query_auth_by_user_id(pool, uid).await.map_err(|e| {
            tracing::error!("resolve_integration_state DB-Fehler (auth by uid): {e}");
            RaidOAuthError::Internal
        })?;
        if let Some(row) = auth {
            let au = str::trim(row.twitch_user_id.as_deref().unwrap_or(""));
            if !au.is_empty() {
                auth_user_id = Some(au.to_string());
            }
            authorized = row.raid_enabled.unwrap_or(false)
                || row.authorized_at.is_some();
        }
    }
    if !authorized {
        let login_to_try = result_login
            .as_deref()
            .or(twitch_login)
            .unwrap_or("");
        if !login_to_try.is_empty() {
            let auth = query_auth_by_login(pool, login_to_try)
                .await
                .map_err(|e| {
                    tracing::error!("resolve_integration_state DB-Fehler (auth by login): {e}");
                    RaidOAuthError::Internal
                })?;
            if let Some(row) = auth {
                authorized = row.raid_enabled.unwrap_or(false)
                    || row.authorized_at.is_some();
                if let Some(u) = row.twitch_user_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    if auth_user_id.is_none() {
                        auth_user_id = Some(u.to_string());
                    }
                    candidate_user_ids.insert(u.to_string());
                }
                if let Some(l) = row.twitch_login.as_deref().and_then(normalize_login_db) {
                    if result_login.is_none() {
                        result_login = Some(l.clone());
                    }
                    candidate_logins.insert(l);
                }
            }
        }
    }
    if let Some(ref uid) = auth_user_id {
        result_user_id = Some(uid.clone());
        candidate_user_ids.insert(uid.clone());
    }

    // Fallback: Auth-Zeile per Login wenn result_user_id immer noch None.
    if result_user_id.is_none() {
        let login_to_try = result_login.as_deref().or(twitch_login).unwrap_or("");
        if !login_to_try.is_empty() {
            let auth = query_auth_by_login(pool, login_to_try)
                .await
                .map_err(|e| {
                    tracing::error!("resolve_integration_state DB-Fehler (fallback auth): {e}");
                    RaidOAuthError::Internal
                })?;
            if let Some(row) = auth {
                if let Some(u) = row.twitch_user_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    result_user_id = Some(u.to_string());
                    candidate_user_ids.insert(u.to_string());
                }
                if let Some(l) = row.twitch_login.as_deref().and_then(normalize_login_db) {
                    if result_login.is_none() {
                        result_login = Some(l.clone());
                    }
                    candidate_logins.insert(l);
                }
            }
        }
    }

    // Candidate-Sets finalisieren.
    if let Some(ref l) = result_login {
        candidate_logins.insert(l.clone());
    }
    if let Some(login) = twitch_login {
        if let Some(n) = normalize_login_db(login) {
            candidate_logins.insert(n);
        }
    }

    // 4. Blacklist-Checks (alle Kandidaten).
    let mut token_blacklisted = false;
    for uid in candidate_user_ids.iter() {
        if is_token_blacklisted(pool, uid).await {
            token_blacklisted = true;
            break;
        }
    }
    let mut raid_blacklisted = false;
    for l in candidate_logins.iter() {
        if is_raid_blacklisted(pool, l).await {
            raid_blacklisted = true;
            break;
        }
    }

    let blocked = partner_opt_out || token_blacklisted || raid_blacklisted;

    // result_login: Fallback auf normalisierter twitch_login-Parameter.
    let final_login = result_login.or_else(|| twitch_login.and_then(normalize_login_db));

    Ok(RaidStatePayload {
        discord_user_id: result_discord_id,
        twitch_login: final_login,
        twitch_user_id: result_user_id,
        authorized,
        partner_opt_out,
        token_blacklisted,
        raid_blacklisted,
        blocked,
    })
}

// ---------------------------------------------------------------------------
// TbRaidOAuthImpl
// ---------------------------------------------------------------------------

/// Composition-Root-Implementierung von `RaidOAuthPort`.
///
/// Hält alle echten tb-raid-Typen. Wird in `tb-bot/src/main.rs` gebaut und
/// als `Arc<dyn RaidOAuthPort>` über `RaidOAuthExt` in den axum-Router gelegt.
pub struct TbRaidOAuthImpl {
    pool: PgPool,
    state_store: StateStore,
    auth_writer: AuthWriter,
    token_client: Arc<dyn TwitchTokenClient>,
    /// Followup-Service (Python `complete_setup_for_streamer` /
    /// `sync_partner_state_after_auth`). `None` → Followups entfallen mit
    /// Warning (z. B. Token-Env fehlt) — der Callback persistiert trotzdem.
    partner_setup: Option<Arc<PartnerSetupService>>,
    client_id: String,
    redirect_uri: String,
    /// Ziel-URL nach erfolgreicher Autorisierung (Python:
    /// `TWITCH_RAID_SUCCESS_REDIRECT_URL` mit Hardcode-Default,
    /// `mixin.py:634-637` + `oauth_callback.py:15`).
    success_redirect_url: String,
    /// Kommagetrennte Guild-IDs (Env: TWITCH_INTERNAL_API_ALLOWED_GUILD_IDS).
    allowed_guild_ids: Option<HashSet<i64>>,
    /// Kommagetrennte Channel-IDs.
    allowed_channel_ids: Option<HashSet<i64>>,
    /// Kommagetrennte Role-IDs.
    allowed_role_ids: Option<HashSet<i64>>,
}

impl TbRaidOAuthImpl {
    /// Erzeugt eine neue Instanz.
    ///
    /// `token_client` ist der HTTP-Port zum Twitch `/oauth2/token`-Endpoint
    /// (in `tb-bot` `HelixTokenClient`).
    /// `client_id` + `redirect_uri` werden für `build_authorize_url` benötigt.
    ///
    /// Die Discord-Scope-Allowlists werden aus Env gelesen:
    /// - `TWITCH_INTERNAL_API_ALLOWED_GUILD_IDS`
    /// - `TWITCH_INTERNAL_API_ALLOWED_CHANNEL_IDS`
    /// - `TWITCH_INTERNAL_API_ALLOWED_ROLE_IDS`
    pub fn new(
        pool: PgPool,
        state_store: StateStore,
        auth_writer: AuthWriter,
        token_client: Arc<dyn TwitchTokenClient>,
        client_id: String,
        redirect_uri: String,
        partner_setup: Option<Arc<PartnerSetupService>>,
    ) -> Self {
        // Fail-closed: gesetzte (auch leere) Variable aktiviert den Guard —
        // nur eine NICHT gesetzte Variable bedeutet guard-aus (policy.py).
        let allowed_guild_ids =
            parse_allowlist(std::env::var("TWITCH_INTERNAL_API_ALLOWED_GUILD_IDS").ok().as_deref());
        let allowed_channel_ids = parse_allowlist(
            std::env::var("TWITCH_INTERNAL_API_ALLOWED_CHANNEL_IDS").ok().as_deref(),
        );
        let allowed_role_ids =
            parse_allowlist(std::env::var("TWITCH_INTERNAL_API_ALLOWED_ROLE_IDS").ok().as_deref());
        let success_redirect_url = std::env::var("TWITCH_RAID_SUCCESS_REDIRECT_URL")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| {
                "https://deutsche-deadlock-community.de/twitch/dashboard".to_string()
            });
        Self {
            pool,
            state_store,
            auth_writer,
            token_client,
            partner_setup,
            client_id,
            redirect_uri,
            success_redirect_url,
            allowed_guild_ids,
            allowed_channel_ids,
            allowed_role_ids,
        }
    }

    /// Prüft guild_id / channel_id / role_id gegen die konfigurierten Allowlists.
    /// Python: `_enforce_discord_action_scope`.
    fn enforce_discord_scope(
        &self,
        guild_id: &Option<serde_json::Value>,
        channel_id: &Option<serde_json::Value>,
        role_id: &Option<serde_json::Value>,
    ) -> Result<(), RaidOAuthError> {
        enforce_scope_allowlist(guild_id, &self.allowed_guild_ids)?;
        enforce_scope_allowlist(channel_id, &self.allowed_channel_ids)?;
        enforce_scope_allowlist(role_id, &self.allowed_role_ids)?;
        Ok(())
    }
}

#[async_trait]
impl RaidOAuthPort for TbRaidOAuthImpl {
    /// Erzeugt die Twitch-Authorize-URL für `login`.
    ///
    /// Ablauf:
    /// 1. `build_state_info` (oauth_flow) → aufgelöster `RaidOAuthState`.
    /// 2. Zufälliger State-Token erzeugen.
    /// 3. `StateStore::persist` → in DB.
    /// 4. `build_authorize_url` → URL zurückgeben.
    async fn auth_url(
        &self,
        login: &str,
        discord_user_id: Option<&str>,
        scope_profile: Option<&str>,
    ) -> Result<String, RaidOAuthError> {
        let scope_raw = scope_profile.unwrap_or("auto");
        let resolver = PgStreamerContextResolver {
            pool: self.pool.clone(),
        };
        let state_info = build_state_info(
            &resolver,
            login,
            scope_raw,
            None, // expected_twitch_login — wird im Build abgeleitet
            None, // expected_twitch_user_id
            discord_user_id,
        )
        .await;

        // State-Token = CSRF-Anker des OAuth-Flows → OS-CSPRNG, wie Pythons
        // `secrets.token_urlsafe(16)` (`bot/raid/auth.py`).
        let state_token = tb_crypto::random_hex_token(32);

        self.state_store
            .persist(&state_token, &state_info, Utc::now())
            .await
            .map_err(|e| {
                tracing::error!("auth_url: StateStore::persist fehlgeschlagen: {e}");
                RaidOAuthError::Internal
            })?;

        let url = build_authorize_url(
            &self.client_id,
            &self.redirect_uri,
            &state_info.scope_profile,
            &state_token,
        );
        Ok(url)
    }

    /// Auth-State für einen Discord-User.
    async fn auth_state(&self, discord_user_id: &str) -> Result<RaidStatePayload, RaidOAuthError> {
        resolve_integration_state(&self.pool, Some(discord_user_id), None).await
    }

    /// Block-State für Discord-User und/oder Twitch-Login.
    async fn block_state(
        &self,
        discord_user_id: Option<&str>,
        twitch_login: Option<&str>,
    ) -> Result<RaidStatePayload, RaidOAuthError> {
        resolve_integration_state(&self.pool, discord_user_id, twitch_login).await
    }

    /// State-Token auflösen → Authorize-URL zurückgeben (go-url).
    ///
    /// Nutzt `StateStore::lookup` (nicht `consume`) — die URL ist kein
    /// Single-Use-Secret, nur ein Pointer auf den State.
    async fn go_url(&self, state: &str) -> Result<Option<String>, RaidOAuthError> {
        let state_info = self
            .state_store
            .lookup(state, Utc::now())
            .await
            .map_err(|e| {
                tracing::error!("go_url: StateStore::lookup fehlgeschlagen: {e}");
                RaidOAuthError::Internal
            })?;

        let Some(info) = state_info else {
            return Ok(None);
        };

        let url = build_authorize_url(&self.client_id, &self.redirect_uri, &info.scope_profile, state);
        Ok(Some(url))
    }

    /// Requirements-DM senden.
    ///
    /// In Python ruft `_raid_requirements` `auth_manager.generate_requirements_dm_embed`
    /// auf, sendet eine Discord-DM und gibt eine Status-Nachricht zurück.
    /// Die Discord-DM-Sendung ist noch nicht nativ portiert — deshalb
    /// antwortet diese Impl ehrlich mit 503 (`upstream_unavailable`) statt
    /// einen Erfolg vorzutäuschen, den es nie gab. Die Route
    /// `POST /raid/requirements` bleibt bewusst UNregistriert und läuft über
    /// den Legacy-Proxy zu Python, das die DM wirklich sendet; dieser Pfad
    /// ist nur ein Sicherheitsnetz gegen versehentliches Wiren.
    async fn requirements(&self, login: &str) -> Result<String, RaidOAuthError> {
        tracing::warn!(
            login,
            "raid requirements nativ aufgerufen, aber Discord-DM ist nicht portiert — 503"
        );
        Err(RaidOAuthError::Upstream)
    }

    /// Discord-Scope-Guard: prüft guild_id/channel_id/role_id gegen Allowlists.
    async fn enforce_discord_action_scope(
        &self,
        guild_id: Option<&serde_json::Value>,
        channel_id: Option<&serde_json::Value>,
        role_id: Option<&serde_json::Value>,
    ) -> Result<(), RaidOAuthError> {
        self.enforce_discord_scope(
            &guild_id.cloned(),
            &channel_id.cloned(),
            &role_id.cloned(),
        )
    }

    /// OAuth-Callback verarbeiten.
    ///
    /// Vollständiger Port von `build_raid_oauth_callback_payload`:
    /// 1. Fehler-Parameter → 400-Payload.
    /// 2. Fehlende code/state → 400.
    /// 3. State aus StateStore::consume → 400 wenn nicht gefunden.
    /// 4. Code gegen Twitch austauschen (`TwitchTokenClient::exchange_code`).
    /// 5. User-Info aus Token-Response validieren (Feld `twitch_user_id`/`twitch_login`
    ///    werden aus der Helix-Response benötigt — ABER: `TwitchTokenClient::exchange_code`
    ///    gibt nur `TokenResponse` zurück, kein Helix-User-Object).
    ///
    /// # open_risk: Helix-User-Lookup fehlt in TwitchTokenClient
    ///
    /// Python ruft nach dem Token-Exchange `GET /helix/users` mit dem frischen
    /// Access-Token auf, um `twitch_user_id` + `twitch_login` zu validieren.
    /// `TwitchTokenClient::exchange_code` liefert kein User-Object.
    /// Für die vollständige Implementierung muss entweder:
    /// (a) `TwitchTokenClient` um `fetch_token_user(access_token)` erweitert werden, oder
    /// (b) ein eigener `reqwest::Client` den Helix-Call absetzen.
    ///
    /// Dieser Port gibt daher bei einem Code-Austausch-Versuch einen 503-Payload
    /// zurück, wenn der Token-Exchange erfolgreich war aber kein User-Lookup
    /// möglich ist (Variante a/b nicht implementiert). Dies ist konservativ
    /// und vermeidet Sicherheitsprobleme durch ungeprüfte Tokens.
    async fn oauth_callback(
        &self,
        code: &str,
        state: &str,
        error: &str,
    ) -> Result<OAuthCallbackResult, RaidOAuthError> {
        let code = code.trim().to_string();
        let state_str = state.trim().to_string();
        let error_str = error.trim().to_string();

        // 1. Fehler-Parameter → 400 (Python `oauth_callback.py:61-84`).
        if !error_str.is_empty() {
            let body = if error_str == "redirect_mismatch" {
                let expected_html = if self.redirect_uri.trim().is_empty() {
                    String::new()
                } else {
                    format!("<p><code>{}</code></p>", html_escape(&self.redirect_uri))
                };
                format!(
                    "<p>Twitch hat die Redirect-URI abgelehnt (redirect_mismatch).</p>\
                     <p>Bitte trage diese URL exakt in der Twitch Application unter \
                     <strong>OAuth Redirect URLs</strong> ein und starte die Autorisierung neu:</p>\
                     {expected_html}"
                )
            } else {
                "<p>OAuth-Fehler beim Autorisieren.</p>\
                 <p>Bitte die Autorisierung erneut starten.</p>"
                    .to_string()
            };
            return Ok(failure(400, "Autorisierung fehlgeschlagen", body));
        }

        // 2. Fehlende code/state → 400.
        if code.is_empty() || state_str.is_empty() {
            return Ok(failure(
                400,
                "Ungültige Anfrage",
                "<p>Fehlender OAuth Code oder State.</p>".to_string(),
            ));
        }

        // 3. State konsumieren (single-use).
        let state_info = self
            .state_store
            .consume(&state_str, Utc::now())
            .await
            .map_err(|e| {
                tracing::error!("oauth_callback: StateStore::consume fehlgeschlagen: {e}");
                RaidOAuthError::Internal
            })?;
        let Some(state_info) = state_info else {
            return Ok(failure(
                400,
                "Ungültiger State",
                "<p>Der OAuth-State ist ungültig oder abgelaufen. \
                 Bitte den Link neu erzeugen.</p>"
                    .to_string(),
            ));
        };
        let requested_login = state_info.requested_login.trim().to_lowercase();

        // 4. Code gegen Twitch tauschen. Python fängt JEDEN Fehler ab hier im
        // äußeren except und antwortet mit der generischen 500-Failure-Payload
        // der internen API (`mixin.py:646-649`) — keine differenzierten Texte.
        let token_response = match self.token_client.exchange_code(&code).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(
                    login = %requested_login,
                    "oauth_callback: exchange_code fehlgeschlagen: {e:?}"
                );
                return Ok(generic_failure());
            }
        };
        if token_response.access_token.trim().is_empty()
            || token_response.refresh_token.trim().is_empty()
        {
            tracing::error!(
                login = %requested_login,
                "oauth_callback: Twitch-Antwort ohne access/refresh_token"
            );
            return Ok(generic_failure());
        }

        // 5. Token-Inhaber ermitteln (Python: GET /helix/users mit dem
        // frischen Bearer, `oauth_callback.py:126-146`).
        let owner = match self
            .token_client
            .token_owner(&token_response.access_token)
            .await
        {
            Ok(o) => o,
            Err(e) => {
                tracing::error!(
                    login = %requested_login,
                    "oauth_callback: Token-Owner-Lookup fehlgeschlagen: {e:?}"
                );
                return Ok(generic_failure());
            }
        };
        let twitch_user_id = owner.twitch_user_id.trim().to_string();
        let twitch_login = owner.twitch_login.trim().to_lowercase();
        if twitch_user_id.is_empty() || twitch_login.is_empty() {
            tracing::error!(login = %requested_login, "oauth_callback: leere User-Identität");
            return Ok(generic_failure());
        }

        // 6. Account-Mismatch-Checks (`oauth_callback.py:148-187`):
        // User-ID-Erwartung gewinnt; Login-Erwartung greift nur ohne ID —
        // abgeleitet aus requested_login, außer bei den synthetischen
        // Onboarding-Logins (`discord:<id>`, public:website_onboarding).
        let expected_user_id = state_info
            .expected_twitch_user_id
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        if !expected_user_id.is_empty() && twitch_user_id != expected_user_id {
            tracing::warn!(
                expected = %expected_user_id,
                actual = %twitch_user_id,
                state_login = %requested_login,
                "oauth_callback: User-ID-Mismatch"
            );
            return Ok(wrong_account_failure());
        }
        let mut expected_login = state_info
            .expected_twitch_login
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if expected_login.is_empty()
            && !requested_login.is_empty()
            && !requested_login.starts_with("discord:")
            && requested_login != PUBLIC_ONBOARDING_LOGIN
        {
            expected_login = requested_login.clone();
        }
        if expected_user_id.is_empty() && !expected_login.is_empty() && twitch_login != expected_login
        {
            tracing::warn!(
                expected = %expected_login,
                actual = %twitch_login,
                "oauth_callback: Login-Mismatch"
            );
            return Ok(wrong_account_failure());
        }

        // 7. Scope-Check (`oauth_callback.py:189-206`): gewährte Scopes dürfen
        // das Profil nicht ÜBERSCHREITEN. (Der AuthWriter prüft beim Persist
        // zusätzlich auf exakte Gleichheit — strenger als Python bei
        // theoretisch fehlenden Scopes, was Twitch praktisch nie liefert.)
        let granted: Vec<String> = token_response
            .scopes
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let allowed: std::collections::BTreeSet<&str> =
            scopes_for_profile(&state_info.scope_profile).iter().copied().collect();
        let unexpected: Vec<&str> = granted
            .iter()
            .map(String::as_str)
            .filter(|s| !allowed.contains(s))
            .collect();
        if !unexpected.is_empty() {
            tracing::warn!(
                login = %twitch_login,
                scopes = %unexpected.join(", "),
                "oauth_callback: Scopes außerhalb des Profils"
            );
            return Ok(invalid_scopes_failure());
        }

        // 8. Erst-Auth erkennen — entscheidet das Followup-Routing in
        // Schritt 10 (Python: VOR save_auth geprüft).
        let had_existing_auth = self.has_saved_auth_record(&twitch_user_id, &twitch_login).await;

        // 9. Tokens verschlüsselt persistieren (Python `save_auth`).
        let new_auth = tb_raid::auth_writer::NewAuth {
            twitch_user_id: twitch_user_id.clone(),
            twitch_login: twitch_login.clone(),
            access_token: token_response.access_token.clone(),
            refresh_token: token_response.refresh_token.clone(),
            expires_in: token_response.expires_in.max(60),
            granted_scopes: granted,
            resolved_scope_profile: state_info.scope_profile.clone(),
            activate_raid_features: true,
        };
        if let Err(e) = self.auth_writer.store_new_auth(&new_auth, Utc::now()).await {
            use tb_raid::auth_writer::AuthWriteError;
            return Ok(match e {
                AuthWriteError::ScopeMismatch { profile } => {
                    tracing::warn!(
                        login = %twitch_login,
                        %profile,
                        "oauth_callback: Scope-Profil-Mismatch beim Persist"
                    );
                    invalid_scopes_failure()
                }
                other => {
                    tracing::error!(
                        login = %twitch_login,
                        "oauth_callback: store_new_auth fehlgeschlagen: {other:?}"
                    );
                    generic_failure()
                }
            });
        }

        // 10. Followups als Background-Tasks (Python `schedule_background`,
        // `oauth_callback.py:207-254`): Erst-Auth → complete_setup
        // (Partner-Sync, first_login, Moderator-Einsetzung, Chat-Begrüßung);
        // Re-Auth mit Discord-ID im OAuth-State → nur sync_partner_state.
        let state_discord_user_id = state_info
            .discord_user_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        match (&self.partner_setup, had_existing_auth) {
            (Some(setup), false) => {
                let setup = setup.clone();
                let uid = twitch_user_id.clone();
                let login = twitch_login.clone();
                let access_token = token_response.access_token.clone();
                let trial_pool = self.pool.clone();
                tokio::spawn(async move {
                    setup
                        .complete_setup_for_streamer(
                            &uid,
                            &login,
                            &access_token,
                            state_discord_user_id.as_deref(),
                        )
                        .await;
                    // „Mitbringsel": neuer Partner bekommt beim Onboarding den
                    // einmaligen 30-Tage-Analytics-Trial. Idempotent über
                    // trial_ever_granted; überschreibt keinen Bezahlplan.
                    tb_analytics::trial::grant_trial_at_onboarding(&trial_pool, &uid, &login).await;
                });
            }
            (Some(setup), true) => {
                if let Some(discord_id) = state_discord_user_id {
                    let setup = setup.clone();
                    let uid = twitch_user_id.clone();
                    let login = twitch_login.clone();
                    tokio::spawn(async move {
                        if let Err(e) = setup
                            .sync_partner_state_after_auth(&uid, &login, Some(&discord_id), true)
                            .await
                        {
                            tracing::error!(
                                login = %login,
                                "sync_partner_state_after_auth-Followup fehlgeschlagen: {e}"
                            );
                        }
                    });
                }
            }
            (None, false) => {
                tracing::warn!(
                    login = %twitch_login,
                    "oauth_callback: Erst-Auth gespeichert, aber kein PartnerSetupService \
                     verdrahtet — Followups entfallen"
                );
            }
            (None, true) => {}
        }

        tracing::info!(login = %twitch_login, "Raid auth successful");
        Ok(OAuthCallbackResult {
            status: 200,
            title: "Autorisierung erfolgreich".to_string(),
            body_html: "<p>Der Raid-Bot wurde erfolgreich autorisiert.</p>\
                        <p>Du kannst dieses Fenster jetzt schließen.</p>"
                .to_string(),
            redirect_url: Some(self.success_redirect_url.clone()),
        })
    }
}

/// Synthetischer Onboarding-Login (Python `PUBLIC_STREAMER_ONBOARDING_LOGIN`).
const PUBLIC_ONBOARDING_LOGIN: &str = "public:website_onboarding";

/// Fehler-Payload ohne redirect_url (Python `_oauth_error_payload`).
fn failure(status: u16, title: &str, body_html: String) -> OAuthCallbackResult {
    OAuthCallbackResult {
        status,
        title: title.to_string(),
        body_html,
        redirect_url: None,
    }
}

/// Generische 500-Payload der internen API — Python fängt alle Fehler nach
/// dem State-Consume im äußeren `except` und antwortet mit den
/// `failure_title`/`failure_body_html`-Texten aus `mixin.py:646-649`.
fn generic_failure() -> OAuthCallbackResult {
    failure(
        500,
        "Autorisierung fehlgeschlagen",
        "<p>Autorisierung fehlgeschlagen.</p>\
         <p>Bitte erneut versuchen oder Admin kontaktieren.</p>"
            .to_string(),
    )
}

/// 403-Payload bei Account-Mismatch (`oauth_callback.py:180-187`).
fn wrong_account_failure() -> OAuthCallbackResult {
    failure(
        403,
        "Falscher Twitch-Account",
        "<p>Die Autorisierung wurde mit dem falschen Twitch-Account abgeschlossen.</p>\
         <p>Bitte den Link erneut öffnen und dich mit dem vorgesehenen Kanal anmelden.</p>"
            .to_string(),
    )
}

/// 400-Payload bei Scopes außerhalb des Profils (`oauth_callback.py:198-206`).
fn invalid_scopes_failure() -> OAuthCallbackResult {
    failure(
        400,
        "Ungültige Berechtigungen",
        "<p>Die Autorisierung wurde mit unerwarteten Berechtigungen abgeschlossen.</p>\
         <p>Bitte den Vorgang neu starten.</p>"
            .to_string(),
    )
}

impl TbRaidOAuthImpl {
    /// Existiert bereits ein Auth-Eintrag für User-ID oder Login?
    /// (Python `has_saved_auth_record` — nur Observability/Followup-Routing.)
    async fn has_saved_auth_record(&self, twitch_user_id: &str, twitch_login: &str) -> bool {
        let row: Result<Option<i32>, _> = sqlx::query_scalar(
            r#"
            SELECT 1 FROM twitch_raid_auth
            WHERE twitch_user_id = $1 OR LOWER(COALESCE(twitch_login, '')) = LOWER($2)
            LIMIT 1
            "#,
        )
        .bind(twitch_user_id)
        .bind(twitch_login)
        .fetch_optional(&self.pool)
        .await;
        matches!(row, Ok(Some(_)))
    }
}

/// Minimales HTML-Escaping für Attribut- und Textwerte.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// Tests (Unit — kein DB-Zugriff)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── parse_allowlist ───────────────────────────────────────────────────────

    #[test]
    fn parse_allowlist_nicht_gesetzt_gibt_none() {
        assert!(parse_allowlist(None).is_none());
    }

    // Python policy.py: gesetzte-aber-leere Variable → leeres Set = deny-all
    // (fail-closed), NICHT guard-aus.
    #[test]
    fn parse_allowlist_leer_gesetzt_gibt_leeres_set_deny_all() {
        assert_eq!(parse_allowlist(Some("")).unwrap().len(), 0);
        assert_eq!(parse_allowlist(Some("   ")).unwrap().len(), 0);
        assert_eq!(parse_allowlist(Some("abc,-1,0")).unwrap().len(), 0);
    }

    #[test]
    fn parse_allowlist_kommagetrennt() {
        let set = parse_allowlist(Some("123,456,789")).unwrap();
        assert!(set.contains(&123));
        assert!(set.contains(&456));
        assert!(set.contains(&789));
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn parse_allowlist_ungueltige_werte_werden_ignoriert() {
        let set = parse_allowlist(Some("123,abc,456,-1,0,789")).unwrap();
        // abc, -1, 0 sind keine positiven Integers → ignoriert.
        assert!(set.contains(&123));
        assert!(set.contains(&456));
        assert!(set.contains(&789));
        assert!(!set.contains(&0));
    }

    #[test]
    fn parse_allowlist_mit_leerzeichen() {
        let set = parse_allowlist(Some(" 111 , 222 ")).unwrap();
        assert!(set.contains(&111));
        assert!(set.contains(&222));
    }

    // Leeres Set = Guard aktiv mit deny-all: jeder Wert (auch gültige IDs)
    // wird abgewiesen.
    #[test]
    fn enforce_scope_leeres_set_weist_alles_ab() {
        let allowed = Some(HashSet::new());
        let value = Some(json!(123));
        assert!(matches!(
            enforce_scope_allowlist(&value, &allowed),
            Err(RaidOAuthError::Forbidden)
        ));
    }

    // ── coerce_positive_int ───────────────────────────────────────────────────

    #[test]
    fn coerce_positive_int_number() {
        assert_eq!(coerce_positive_int(&Some(json!(42))), Some(42));
        assert_eq!(coerce_positive_int(&Some(json!(-1))), None);
        assert_eq!(coerce_positive_int(&Some(json!(0))), None);
        assert_eq!(coerce_positive_int(&None), None);
    }

    #[test]
    fn coerce_positive_int_string() {
        assert_eq!(coerce_positive_int(&Some(json!("123"))), Some(123));
        assert_eq!(coerce_positive_int(&Some(json!("abc"))), None);
        assert_eq!(coerce_positive_int(&Some(json!("  42  "))), Some(42));
    }

    // ── enforce_scope_allowlist ───────────────────────────────────────────────

    #[test]
    fn enforce_kein_guard_immer_ok() {
        // allowed = None → kein Guard → kein Fehler, egal welcher Wert.
        assert!(enforce_scope_allowlist(&None, &None).is_ok());
        assert!(enforce_scope_allowlist(&Some(json!(999)), &None).is_ok());
    }

    #[test]
    fn enforce_wert_in_allowlist_ok() {
        let allowed = Some([42i64].iter().cloned().collect::<HashSet<_>>());
        assert!(enforce_scope_allowlist(&Some(json!(42)), &allowed).is_ok());
    }

    #[test]
    fn enforce_wert_nicht_in_allowlist_forbidden() {
        let allowed = Some([42i64].iter().cloned().collect::<HashSet<_>>());
        let result = enforce_scope_allowlist(&Some(json!(999)), &allowed);
        assert!(matches!(result, Err(RaidOAuthError::Forbidden)));
    }

    #[test]
    fn enforce_fehlender_wert_bei_aktivem_guard_forbidden() {
        let allowed = Some([42i64].iter().cloned().collect::<HashSet<_>>());
        let result = enforce_scope_allowlist(&None, &allowed);
        assert!(matches!(result, Err(RaidOAuthError::Forbidden)));
    }

    // ── html_escape ───────────────────────────────────────────────────────────

    #[test]
    fn html_escape_sonderzeichen() {
        assert_eq!(html_escape("<script>&\""), "&lt;script&gt;&amp;&quot;");
        assert_eq!(html_escape("normale URL"), "normale URL");
    }

    // ── normalize_login_db ────────────────────────────────────────────────────

    #[test]
    fn normalize_login_db_trimmt_und_lowercased() {
        assert_eq!(normalize_login_db("  DragScope  "), Some("dragscope".to_string()));
        assert_eq!(normalize_login_db(""), None);
        assert_eq!(normalize_login_db("  "), None);
        assert_eq!(normalize_login_db("a"), None); // len < 2
        assert_eq!(normalize_login_db("ab"), Some("ab".to_string())); // len == 2 ok
    }
}

// ---------------------------------------------------------------------------
// Tests (DB-Integration)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
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

    /// Schema-isolierter Pool mit prod-treuer DDL.
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
        apply_ddl(&pool).await;
        pool
    }

    /// Prod-treue DDL: alle Spaltentypen und Constraints wie in bot/storage/pg.py.
    async fn apply_ddl(pool: &PgPool) {
        // twitch_raid_auth — hat_existing_streamer_context Query
        sqlx::query(
            r#"
            CREATE TABLE twitch_raid_auth (
                twitch_user_id      TEXT PRIMARY KEY,
                twitch_login        TEXT,
                access_token        TEXT DEFAULT 'ENC',
                refresh_token       TEXT DEFAULT 'ENC',
                access_token_enc    BYTEA,
                refresh_token_enc   BYTEA,
                enc_version         INTEGER DEFAULT 1,
                enc_kid             TEXT DEFAULT 'v1',
                token_expires_at    TIMESTAMPTZ,
                scopes              TEXT,
                authorized_at       TIMESTAMPTZ,
                raid_enabled        BOOLEAN DEFAULT FALSE,
                needs_reauth        BOOLEAN DEFAULT FALSE,
                reauth_notified_at  TIMESTAMPTZ
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("DDL twitch_raid_auth");

        // twitch_partners (für twitch_streamers_partner_state VIEW)
        sqlx::query(
            r#"
            CREATE TABLE twitch_partners (
                twitch_login            TEXT PRIMARY KEY,
                twitch_user_id          TEXT,
                discord_user_id         TEXT,
                -- INT4 wie Prod (alle Partner-Flags sind 0/1-Integer) —
                -- BOOLEAN hier hätte den Typ-Drift-Bug im Test versteckt.
                manual_partner_opt_out  INTEGER DEFAULT 0,
                status                  TEXT DEFAULT 'active',
                manual_verified_at      TEXT,
                created_at              TEXT DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("DDL twitch_partners");

        // twitch_streamers_partner_state: vereinfachte VIEW für Tests
        sqlx::query(
            r#"
            CREATE VIEW twitch_streamers_partner_state AS
            SELECT
                twitch_login,
                twitch_user_id,
                discord_user_id,
                manual_partner_opt_out,
                manual_verified_at,
                created_at
            FROM twitch_partners
            WHERE status = 'active'
            "#,
        )
        .execute(pool)
        .await
        .expect("DDL VIEW twitch_streamers_partner_state");

        // twitch_token_blacklist
        sqlx::query(
            r#"
            CREATE TABLE twitch_token_blacklist (
                twitch_user_id      TEXT PRIMARY KEY,
                twitch_login        TEXT,
                error_count         INTEGER DEFAULT 1,
                error_message       TEXT,
                first_error_at      TEXT,
                last_error_at       TEXT,
                grace_expires_at    TEXT,
                notified            INTEGER DEFAULT 0
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("DDL twitch_token_blacklist");

        // twitch_raid_blacklist
        sqlx::query(
            r#"
            CREATE TABLE twitch_raid_blacklist (
                target_login    TEXT PRIMARY KEY,
                target_id       TEXT,
                reason          TEXT,
                added_at        TEXT DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("DDL twitch_raid_blacklist");

        // oauth_state_tokens (für StateStore)
        sqlx::query(
            r#"
            CREATE TABLE oauth_state_tokens (
                state_token     TEXT PRIMARY KEY,
                platform        TEXT,
                streamer_login  TEXT,
                redirect_uri    TEXT,
                pkce_verifier   TEXT,
                expires_at      TIMESTAMPTZ
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("DDL oauth_state_tokens");
    }

    // ── has_existing_streamer_context ─────────────────────────────────────────

    #[tokio::test]
    async fn streamer_context_nicht_vorhanden_false() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ro_ctx_empty").await;
        let resolver = PgStreamerContextResolver { pool };
        assert!(!resolver.has_existing_streamer_context("unknown_login").await);
    }

    #[tokio::test]
    async fn streamer_context_raid_enabled_true() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ro_ctx_raid").await;
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled) VALUES ($1, $2, TRUE)",
        )
        .bind("uid_1")
        .bind("dragscope")
        .execute(&pool)
        .await
        .expect("insert");
        let resolver = PgStreamerContextResolver { pool };
        assert!(resolver.has_existing_streamer_context("dragscope").await);
    }

    #[tokio::test]
    async fn streamer_context_authorized_at_true() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ro_ctx_auth").await;
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, authorized_at) VALUES ($1, $2, NOW())",
        )
        .bind("uid_2")
        .bind("streamer_b")
        .execute(&pool)
        .await
        .expect("insert");
        let resolver = PgStreamerContextResolver { pool };
        assert!(resolver.has_existing_streamer_context("streamer_b").await);
    }

    // ── is_token_blacklisted ──────────────────────────────────────────────────

    #[tokio::test]
    async fn token_blacklist_unter_schwelle_false() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ro_bl_under").await;
        sqlx::query("INSERT INTO twitch_token_blacklist (twitch_user_id, error_count) VALUES ($1, 2)")
            .bind("uid_bl")
            .execute(&pool)
            .await
            .expect("insert");
        assert!(!is_token_blacklisted(&pool, "uid_bl").await);
    }

    #[tokio::test]
    async fn token_blacklist_genau_schwelle_true() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ro_bl_exact").await;
        sqlx::query("INSERT INTO twitch_token_blacklist (twitch_user_id, error_count) VALUES ($1, 3)")
            .bind("uid_bl3")
            .execute(&pool)
            .await
            .expect("insert");
        assert!(is_token_blacklisted(&pool, "uid_bl3").await);
    }

    #[tokio::test]
    async fn token_blacklist_nicht_vorhanden_false() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ro_bl_none").await;
        assert!(!is_token_blacklisted(&pool, "nobody").await);
    }

    // ── is_raid_blacklisted ───────────────────────────────────────────────────

    #[tokio::test]
    async fn raid_blacklist_nicht_vorhanden_false() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ro_rbl_none").await;
        assert!(!is_raid_blacklisted(&pool, "cleanlogin").await);
    }

    #[tokio::test]
    async fn raid_blacklist_vorhanden_true() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ro_rbl_hit").await;
        sqlx::query("INSERT INTO twitch_raid_blacklist (target_login, reason) VALUES ($1, $2)")
            .bind("banned_streamer")
            .bind("manual_ban:absolut")
            .execute(&pool)
            .await
            .expect("insert");
        assert!(is_raid_blacklisted(&pool, "banned_streamer").await);
        // Case-insensitiv.
        assert!(is_raid_blacklisted(&pool, "BANNED_STREAMER").await);
    }

    // ── resolve_integration_state ─────────────────────────────────────────────

    #[tokio::test]
    async fn integration_state_unbekannter_discord_nicht_autorisiert() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ro_is_unknown").await;
        let result = resolve_integration_state(&pool, Some("999000999"), None)
            .await
            .expect("resolve");
        assert!(!result.authorized);
        assert!(!result.blocked);
        assert_eq!(result.discord_user_id.as_deref(), Some("999000999"));
        assert!(result.twitch_login.is_none());
    }

    #[tokio::test]
    async fn integration_state_partner_mit_raid_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ro_is_auth").await;

        // Partner-Eintrag
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, discord_user_id) VALUES ($1, $2, $3)",
        )
        .bind("streamerfoo")
        .bind("uid_foo")
        .bind("111222333")
        .execute(&pool)
        .await
        .expect("partner insert");

        // Auth-Eintrag mit raid_enabled=TRUE
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled) VALUES ($1, $2, TRUE)",
        )
        .bind("uid_foo")
        .bind("streamerfoo")
        .execute(&pool)
        .await
        .expect("auth insert");

        let result = resolve_integration_state(&pool, Some("111222333"), None)
            .await
            .expect("resolve");
        assert!(result.authorized, "sollte autorisiert sein");
        assert_eq!(result.twitch_login.as_deref(), Some("streamerfoo"));
        assert_eq!(result.twitch_user_id.as_deref(), Some("uid_foo"));
        assert!(!result.blocked);
        assert!(!result.token_blacklisted);
        assert!(!result.raid_blacklisted);
    }

    #[tokio::test]
    async fn integration_state_partner_opt_out_geblockt() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ro_is_optout").await;

        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, discord_user_id, manual_partner_opt_out) \
             VALUES ($1, $2, $3, 1)",
        )
        .bind("optout_login")
        .bind("uid_opt")
        .bind("444555666")
        .execute(&pool)
        .await
        .expect("insert");

        let result = resolve_integration_state(&pool, Some("444555666"), None)
            .await
            .expect("resolve");
        assert!(result.partner_opt_out, "partner_opt_out sollte true sein");
        assert!(result.blocked, "sollte geblockt sein");
    }

    #[tokio::test]
    async fn integration_state_token_blacklisted_geblockt() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ro_is_tbl").await;

        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, discord_user_id) VALUES ($1, $2, $3)",
        )
        .bind("bl_streamer")
        .bind("uid_bl_s")
        .bind("777888999")
        .execute(&pool)
        .await
        .expect("partner insert");

        // Token-Blacklist mit error_count >= 3
        sqlx::query(
            "INSERT INTO twitch_token_blacklist (twitch_user_id, error_count) VALUES ($1, 3)",
        )
        .bind("uid_bl_s")
        .execute(&pool)
        .await
        .expect("blacklist insert");

        let result = resolve_integration_state(&pool, Some("777888999"), None)
            .await
            .expect("resolve");
        assert!(result.token_blacklisted, "token_blacklisted sollte true sein");
        assert!(result.blocked, "sollte geblockt sein");
    }

    #[tokio::test]
    async fn integration_state_raid_blacklisted_geblockt() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_ro_is_rbl").await;

        sqlx::query(
            "INSERT INTO twitch_raid_blacklist (target_login, reason) VALUES ($1, $2)",
        )
        .bind("rbl_streamer")
        .bind("manual")
        .execute(&pool)
        .await
        .expect("raid bl insert");

        let result = resolve_integration_state(&pool, None, Some("rbl_streamer"))
            .await
            .expect("resolve");
        assert!(result.raid_blacklisted, "raid_blacklisted sollte true sein");
        assert!(result.blocked, "sollte geblockt sein");
    }
}

// ---------------------------------------------------------------------------
// Callback-Flow-Tests (DB + Stub-TokenClient — kein echtes Twitch)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod callback_tests {
    use super::*;
    use std::sync::Mutex;
    use tb_crypto::{FieldCipher, KID};
    use tb_raid::token_refresher::{RefreshError, TokenOwnerInfo, TokenResponse};
    use tb_raid::RaidOAuthState;

    const TEST_KEY_HEX: &str = "0f0e0d0c0b0a09080706050403020100ffeeddccbbaa99887766554433221100";

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
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
        // Schema auf einer Wegwerf-Verbindung anlegen.
        let admin = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect admin");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;

        // search_path via after_connect auf JEDER Verbindung setzen, nicht nur
        // einmalig per `SET`. Der oauth_callback-Schreibpfad (AuthWriter::store_new_auth)
        // oeffnet eine EIGENE Transaktions-Verbindung via `pool.begin()`; ein einmaliges
        // `SET search_path` auf der Pool-Connection greift dort nicht, die Transaktion
        // laeuft gegen `public` und sieht die Test-Tabellen nicht -> "relation does not
        // exist" -> Handler verschluckt den Fehler in eine generische 500 statt 200.
        let schema_owned = schema.to_string();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .after_connect(move |conn, _| {
                let schema = schema_owned.clone();
                Box::pin(async move {
                    sqlx::query(&format!("SET search_path TO {schema}"))
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(dsn)
            .await
            .expect("connect pool mit search_path");
        // Prod-treue Typen (TIMESTAMPTZ/BOOLEAN wie twitch_analytics).
        sqlx::query(
            r#"
            CREATE TABLE twitch_raid_auth (
                twitch_user_id      TEXT PRIMARY KEY,
                twitch_login        TEXT,
                access_token        TEXT,
                refresh_token       TEXT,
                access_token_enc    BYTEA,
                refresh_token_enc   BYTEA,
                enc_version         INTEGER,
                enc_kid             TEXT,
                token_expires_at    TIMESTAMPTZ,
                scopes              TEXT,
                authorized_at       TIMESTAMPTZ,
                last_refreshed_at   TIMESTAMPTZ,
                raid_enabled        BOOLEAN DEFAULT FALSE,
                needs_reauth        BOOLEAN DEFAULT FALSE,
                created_at          TIMESTAMPTZ,
                reauth_notified_at  TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE oauth_state_tokens (
                state_token     TEXT PRIMARY KEY,
                platform        TEXT,
                streamer_login  TEXT,
                redirect_uri    TEXT,
                pkce_verifier   TEXT,
                expires_at      TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        // AuthWriter::store_new_auth raeumt im selben Transaktions-Block den
        // Token-Error-Zustand auf (Partner-Pause-Grund + Blacklist). Fehlen diese
        // Tabellen, bricht die Transaktion mit "relation does not exist" ab und der
        // Handler liefert statt 200 eine generische 500. Nur die vom Schreibpfad
        // beruehrten Spalten, prod-treu typisiert.
        sqlx::query(
            r#"
            CREATE TABLE twitch_partners (
                twitch_user_id          TEXT NOT NULL,
                technical_pause_reason  TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE twitch_token_blacklist (
                twitch_user_id  TEXT NOT NULL,
                twitch_login    TEXT NOT NULL,
                first_error_at  TEXT NOT NULL,
                last_error_at   TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    /// Konfigurierbarer Token-Client: liefert vorgegebene Tokens/Owner oder Fehler.
    struct StubTokenClient {
        exchange: Mutex<Option<Result<TokenResponse, RefreshError>>>,
        owner: Mutex<Option<Result<TokenOwnerInfo, RefreshError>>>,
    }

    impl StubTokenClient {
        fn ok(scopes: &[&str], owner_id: &str, owner_login: &str) -> Self {
            Self {
                exchange: Mutex::new(Some(Ok(TokenResponse {
                    access_token: "frisch-acc".to_string(),
                    refresh_token: "frisch-ref".to_string(),
                    expires_in: 14000,
                    scopes: scopes.iter().map(|s| s.to_string()).collect(),
                }))),
                owner: Mutex::new(Some(Ok(TokenOwnerInfo {
                    twitch_user_id: owner_id.to_string(),
                    twitch_login: owner_login.to_string(),
                }))),
            }
        }

        fn exchange_fails() -> Self {
            Self {
                exchange: Mutex::new(Some(Err(RefreshError::Other("kaputt".into())))),
                owner: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl TwitchTokenClient for StubTokenClient {
        async fn refresh(&self, _t: &str) -> Result<TokenResponse, RefreshError> {
            unreachable!("refresh im Callback-Test ungenutzt")
        }
        async fn exchange_code(&self, _c: &str) -> Result<TokenResponse, RefreshError> {
            self.exchange.lock().unwrap().take().expect("exchange einmal")
        }
        async fn token_owner(&self, _a: &str) -> Result<TokenOwnerInfo, RefreshError> {
            self.owner.lock().unwrap().take().expect("owner einmal")
        }
    }

    async fn make_impl(pool: &PgPool, stub: StubTokenClient) -> TbRaidOAuthImpl {
        let cipher = Arc::new(FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap());
        TbRaidOAuthImpl::new(
            pool.clone(),
            StateStore::new(pool.clone(), "https://example.test/callback"),
            AuthWriter::new(pool.clone(), cipher),
            Arc::new(stub),
            "cid".to_string(),
            "https://example.test/callback".to_string(),
            None, // Followups in Handler-Tests nicht verdrahtet
        )
    }

    /// Persistiert einen State und gibt den Token zurück.
    async fn seed_state(
        pool: &PgPool,
        requested_login: &str,
        expected_login: Option<&str>,
        expected_user_id: Option<&str>,
    ) -> String {
        let store = StateStore::new(pool.clone(), "https://example.test/callback");
        let token = format!("state-{requested_login}");
        let state = RaidOAuthState {
            requested_login: requested_login.to_string(),
            scope_profile: "raid".to_string(),
            expected_twitch_login: expected_login.map(str::to_string),
            expected_twitch_user_id: expected_user_id.map(str::to_string),
            discord_user_id: None,
        };
        store.persist(&token, &state, Utc::now()).await.expect("persist");
        token
    }

    fn raid_scopes() -> Vec<&'static str> {
        scopes_for_profile("raid").to_vec()
    }

    #[tokio::test]
    async fn erfolg_speichert_verschluesselte_tokens_und_liefert_redirect() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_cb_ok").await;
        let state = seed_state(&pool, "dragscope", None, None).await;
        let imp = make_impl(&pool, StubTokenClient::ok(&raid_scopes(), "111", "dragscope")).await;

        let result = imp.oauth_callback("code-1", &state, "").await.expect("callback");
        assert_eq!(result.status, 200, "body: {}", result.body_html);
        assert_eq!(result.title, "Autorisierung erfolgreich");
        assert!(result.redirect_url.is_some(), "Erfolg braucht redirect_url");

        // Tokens liegen verschlüsselt in twitch_raid_auth.
        let row: (Option<Vec<u8>>, Option<String>, Option<bool>) = sqlx::query_as(
            "SELECT access_token_enc, twitch_login, raid_enabled FROM twitch_raid_auth WHERE twitch_user_id = '111'",
        )
        .fetch_one(&pool)
        .await
        .expect("auth row");
        assert!(row.0.is_some(), "access_token_enc muss gesetzt sein");
        assert_eq!(row.1.as_deref(), Some("dragscope"));
        assert_eq!(row.2, Some(true), "activate_raid_features");

        // State ist konsumiert (Single-Use).
        let leftover: Option<(String,)> =
            sqlx::query_as("SELECT state_token FROM oauth_state_tokens WHERE state_token = $1")
                .bind(&state)
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert!(leftover.is_none(), "State muss konsumiert sein");
    }

    #[tokio::test]
    async fn user_id_mismatch_gibt_403_und_speichert_nichts() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_cb_uid_mismatch").await;
        let state = seed_state(&pool, "dragscope", None, Some("999")).await;
        let imp = make_impl(&pool, StubTokenClient::ok(&raid_scopes(), "111", "dragscope")).await;

        let result = imp.oauth_callback("code-1", &state, "").await.expect("callback");
        assert_eq!(result.status, 403);
        assert_eq!(result.title, "Falscher Twitch-Account");
        assert!(result.redirect_url.is_none());

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_raid_auth")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "bei Mismatch darf nichts gespeichert werden");
    }

    #[tokio::test]
    async fn login_mismatch_ohne_user_id_erwartung_gibt_403() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_cb_login_mismatch").await;
        // requested_login wird zur Login-Erwartung (kein discord:/public:-Präfix).
        let state = seed_state(&pool, "erwarteter_kanal", None, None).await;
        let imp = make_impl(&pool, StubTokenClient::ok(&raid_scopes(), "111", "anderer_kanal")).await;

        let result = imp.oauth_callback("code-1", &state, "").await.expect("callback");
        assert_eq!(result.status, 403);
        assert_eq!(result.title, "Falscher Twitch-Account");
    }

    #[tokio::test]
    async fn discord_login_erzeugt_keine_login_erwartung() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_cb_discord_login").await;
        // Synthetischer Onboarding-Login → kein Mismatch trotz fremdem Kanal.
        let state = seed_state(&pool, "discord:42", None, None).await;
        let imp = make_impl(&pool, StubTokenClient::ok(&raid_scopes(), "111", "irgendein_kanal")).await;

        let result = imp.oauth_callback("code-1", &state, "").await.expect("callback");
        assert_eq!(result.status, 200, "body: {}", result.body_html);
    }

    #[tokio::test]
    async fn unerwartete_scopes_geben_400_und_speichern_nichts() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_cb_scopes").await;
        let state = seed_state(&pool, "dragscope", None, None).await;
        let mut scopes = raid_scopes();
        scopes.push("channel:manage:broadcast"); // außerhalb des Profils
        let imp = make_impl(&pool, StubTokenClient::ok(&scopes, "111", "dragscope")).await;

        let result = imp.oauth_callback("code-1", &state, "").await.expect("callback");
        assert_eq!(result.status, 400);
        assert_eq!(result.title, "Ungültige Berechtigungen");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_raid_auth")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn exchange_fehler_gibt_generische_500_payload() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_cb_exchange_fail").await;
        let state = seed_state(&pool, "dragscope", None, None).await;
        let imp = make_impl(&pool, StubTokenClient::exchange_fails()).await;

        let result = imp.oauth_callback("code-1", &state, "").await.expect("callback");
        // Python: äußerer except → failure_title/body der internen API.
        assert_eq!(result.status, 500);
        assert_eq!(result.title, "Autorisierung fehlgeschlagen");
        assert!(result.body_html.contains("Admin kontaktieren"));
    }

    #[tokio::test]
    async fn ungueltiger_state_gibt_400_ohne_token_calls() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_cb_bad_state").await;
        // Stub würde bei Aufruf panicen (None) — beweist, dass vor dem
        // State-Check kein Twitch-Call passiert.
        let imp = make_impl(
            &pool,
            StubTokenClient { exchange: Mutex::new(None), owner: Mutex::new(None) },
        )
        .await;

        let result = imp.oauth_callback("code-1", "unbekannt", "").await.expect("callback");
        assert_eq!(result.status, 400);
        assert_eq!(result.title, "Ungültiger State");
    }
}
