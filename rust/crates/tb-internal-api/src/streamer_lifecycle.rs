//! Nativer Streamer-Partner-Lifecycle für die interne API (Block 10).
//!
//! # Warum dieses Modul im `tb-internal-api`-Crate liegt
//!
//! Der Partner-Lifecycle (Departner / Promote / Stats-Backfill / require-link)
//! ist eine API-spezifische Geschäftsregel hinter den `/streamers/*`-Mutationen.
//! Block 10 portiert ausschließlich diese Schicht; die generische CRUD-Schicht
//! (`tb_analytics::streamers_crud`) bleibt unberührt. Bei einer späteren
//! Konsolidierung kann der Code 1:1 nach `tb-analytics` umziehen — die
//! Funktionen sind reine `&PgPool`/`&mut PgConnection`-Operationen ohne
//! HTTP-/Port-Abhängigkeiten.
//!
//! # Parität zum Python-Orakel
//!
//! - `departner_active_partner` → `bot/storage/partner_registry.py:1130`
//! - `promote_streamer_to_partner` (Verify-Teilpfad) → `…:782`
//! - `verification_payload` → `…:2188`
//! - `backfill_tracked_stats_from_category` → `bot/storage/pg.py:1404`
//! - `upsert_non_partner_streamer` (require-link-Backfill) → `…:563`
//!
//! Alle Zeitstempel-Spalten in `twitch_partners` sind in Prod `TEXT` (Python
//! schreibt `datetime.isoformat()`); dieses Modul schreibt deshalb ISO-Strings
//! via `now_iso()`/[`super::security::datetime_to_iso`], nicht `NOW()`.
//! `discord_user_id` ist überall `TEXT` — Snowflake-IDs bleiben Strings
//! (Serializer-Parität, s. `security.rs` json_default).

use chrono::Utc;
use sqlx::PgPool;

use crate::security::datetime_to_iso;

const STATUS_ACTIVE: &str = "active";
const STATUS_DEPARTNERED: &str = "departnered";
/// Legacy-Status für administrativ archivierte Partner (Python
/// `_LEGACY_PARTNER_STATUS_ARCHIVED`). Wird nur noch im History-/Reactivate-Pfad
/// gelesen; aktive Pfade schreiben `departnered`.
const STATUS_ARCHIVED: &str = "archived";

/// `departnered` oder `archived` — die beiden inaktiven Partner-Status, aus
/// denen `reactivate_partner` zurückholen kann (Python
/// `_is_inactive_partner_status`).
fn is_inactive_partner_status(status: Option<&str>) -> bool {
    matches!(
        status.map(|s| s.trim().to_lowercase()).as_deref(),
        Some(STATUS_DEPARTNERED) | Some(STATUS_ARCHIVED)
    )
}

/// ISO-8601-Zeitstempel exakt wie Pythons `_now_iso()` (UTC, `+00:00`).
fn now_iso() -> String {
    datetime_to_iso(Utc::now())
}

// ── Partner-Payload (Python `verification_payload`) ────────────────────────────

/// Partner-Felder je Verify-Modus — nach Entfernung der alten
/// manuellen Verifikationsspalten bleibt nur der Opt-out-Status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationPayload {
    pub manual_partner_opt_out: i32,
}

impl VerificationPayload {
    /// `permanent`/`temp` → Partner aktiv halten, `clear`/`failed` → Opt-out setzen,
    /// sonst `None`.
    pub fn for_mode(mode: &str) -> Option<Self> {
        match mode.trim().to_lowercase().as_str() {
            "permanent" => Some(Self {
                manual_partner_opt_out: 0,
            }),
            "temp" => Some(Self {
                manual_partner_opt_out: 0,
            }),
            "clear" | "failed" => Some(Self {
                manual_partner_opt_out: 1,
            }),
            _ => None,
        }
    }
}

// ── Aktiver Partner laden ──────────────────────────────────────────────────────

/// Minimal-Projektion eines aktiven Partner-Datensatzes für den Lifecycle.
///
/// Python lädt via `load_active_partner` die volle Zeile; der Lifecycle nutzt
/// davon nur Identität + Discord-Felder. `status='active'` ist
/// das Aktiv-Kriterium (Python `PARTNER_STATUS_ACTIVE`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ActivePartnerRow {
    pub id: i64,
    pub twitch_login: String,
    pub twitch_user_id: Option<String>,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
    pub is_on_discord: Option<i32>,
}

/// Lädt den aktiven Partner zu einem Login (`status='active'`), oder `None`.
///
/// Die Discord-Felder leben in Prod NICHT auf `twitch_partners`, sondern in
/// `twitch_streamer_identities` (Python liest sie über die View
/// `twitch_partners_all_state`). Deshalb LEFT JOIN auf die Identity-Tabelle —
/// ein direkter `SELECT discord_user_id FROM twitch_partners` würde in Prod
/// fehlschlagen (Spalte existiert dort nicht).
pub async fn load_active_partner(
    pool: &PgPool,
    login: &str,
) -> Result<Option<ActivePartnerRow>, sqlx::Error> {
    sqlx::query_as!(
        ActivePartnerRow,
        r#"
        SELECT p.id AS "id!",
               p.twitch_login AS "twitch_login!",
               p.twitch_user_id AS "twitch_user_id?",
               i.discord_user_id, i.discord_display_name, i.is_on_discord
          FROM twitch_partners p
          LEFT JOIN twitch_streamer_identities i
            ON i.twitch_user_id = p.twitch_user_id
         WHERE LOWER(p.twitch_login) = LOWER($1)
           AND COALESCE(p.status, '') = 'active'
         ORDER BY p.id DESC
         LIMIT 1
        "#,
        login
    )
    .fetch_optional(pool)
    .await
}

// ── Identity-Upsert (Python `upsert_streamer_identity`) ────────────────────────

/// Upsert in `twitch_streamer_identities` — nur wenn eine `twitch_user_id`
/// vorliegt (Python no-opt ohne user_id). Discord-Felder werden mitgeführt.
async fn upsert_streamer_identity(
    pool: &PgPool,
    twitch_user_id: Option<&str>,
    twitch_login: &str,
    discord_user_id: Option<&str>,
    discord_display_name: Option<&str>,
    is_on_discord: i32,
) -> Result<(), sqlx::Error> {
    let Some(uid) = twitch_user_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let normalized_login = twitch_login.to_lowercase();
    let now = now_iso();
    sqlx::query!(
        r#"
        INSERT INTO twitch_streamer_identities (
            twitch_user_id, twitch_login, discord_user_id,
            discord_display_name, is_on_discord, created_at, updated_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $6)
        ON CONFLICT (twitch_user_id) DO UPDATE SET
            twitch_login         = EXCLUDED.twitch_login,
            discord_user_id      = COALESCE(EXCLUDED.discord_user_id, twitch_streamer_identities.discord_user_id),
            discord_display_name = COALESCE(EXCLUDED.discord_display_name, twitch_streamer_identities.discord_display_name),
            is_on_discord        = EXCLUDED.is_on_discord,
            updated_at           = EXCLUDED.updated_at
        "#,
        uid,
        normalized_login,
        discord_user_id,
        discord_display_name,
        is_on_discord,
        now
    )
    .execute(pool)
    .await?;
    Ok(())
}

// ── Departner (Python `departner_active_partner`) ──────────────────────────────

/// Ergebnis eines Departner-Vorgangs — trägt die Discord-Daten, damit der
/// Aufrufer (Handler) die Streamer-Rolle entfernen kann.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepartnerOutcome {
    pub twitch_login: String,
    pub twitch_user_id: Option<String>,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
}

/// Departnert einen aktiven Partner — Parität zu `departner_active_partner`
/// (`partner_registry.py:1130`) im Dashboard-/Admin-Standardpfad
/// (`disable_raid_auth=True`, `restore_non_partner=False`).
///
/// Schritte (Reihenfolge wie Python):
/// 1. Aktiven Partner laden — `None` wenn keiner aktiv ist.
/// 2. Identity-Upsert (Discord-Daten erhalten).
/// 3. `twitch_partners`: `status='departnered'`, `departnered_at=now`,
///    `admin_archived_at=NULL`.
/// 4. Raid-Auth deaktivieren (`raid_enabled=FALSE`).
/// 5. Engagement-Settings deaktivieren (best-effort; Tabelle existiert in Prod).
///
/// `restore_non_partner` ist im Dashboard-/Admin-Pfad immer `False`, daher hier
/// bewusst NICHT portiert (kein toter Code; bei Bedarf separat ergänzen).
/// `_normalize_related_tables` ist ein Python-Cleanup ohne user-sichtbaren
/// Effekt im Lifecycle-Pfad — als Handoff dokumentiert, nicht in Block 10.
pub async fn departner_active_partner(
    pool: &PgPool,
    login: &str,
    clear_verification: bool,
) -> Result<Option<DepartnerOutcome>, sqlx::Error> {
    let Some(row) = load_active_partner(pool, login).await? else {
        return Ok(None);
    };

    let normalized_login = row.twitch_login.to_lowercase();
    let normalized_user_id = row
        .twitch_user_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let discord_user_id = row
        .discord_user_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let discord_display_name = row
        .discord_display_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let is_on_discord = row.is_on_discord.unwrap_or(0);
    let departnered_at = now_iso();

    upsert_streamer_identity(
        pool,
        normalized_user_id.as_deref(),
        &normalized_login,
        discord_user_id.as_deref(),
        discord_display_name.as_deref(),
        is_on_discord,
    )
    .await?;

    // `clear_verification` ist der Opt-out-Fall (`verify mode=clear|failed`):
    // Der Streamer will nichts mehr vom Bot. Ohne das Flag hätte ein späteres
    // Promote ihn wortlos zurückgeholt — `is_partner_active` wertet es aus.
    let opt_out = i32::from(clear_verification);
    sqlx::query!(
        r#"
        UPDATE twitch_partners
        SET status = $1,
            departnered_at = $2,
            admin_archived_at = NULL,
            twitch_login = $3,
            twitch_user_id = $4,
            manual_partner_opt_out = GREATEST(COALESCE(manual_partner_opt_out, 0), $6)
        WHERE id = $5
        "#,
        STATUS_DEPARTNERED,
        &departnered_at,
        &normalized_login,
        normalized_user_id.as_deref(),
        row.id,
        opt_out
    )
    .execute(pool)
    .await?;

    // Raid-Auth deaktivieren (Python disable_raid_auth=True default).
    sqlx::query!(
        r#"
        UPDATE twitch_raid_auth
        SET raid_enabled = FALSE,
            twitch_login = $1
        WHERE twitch_user_id = $2
           OR LOWER(twitch_login) = LOWER($1)
        "#,
        &normalized_login,
        normalized_user_id.as_deref()
    )
    .execute(pool)
    .await?;

    // Engagement-Layer abschalten — best-effort wie Python (Tabelle kann fehlen).
    let _ = sqlx::query!(
        "UPDATE twitch_engagement_settings SET enabled = FALSE WHERE LOWER(channel_login) = LOWER($1)",
        &normalized_login
    )
    .execute(pool)
    .await;

    Ok(Some(DepartnerOutcome {
        twitch_login: normalized_login,
        twitch_user_id: normalized_user_id,
        discord_user_id,
        discord_display_name,
    }))
}

// ── Verify-Quelldaten (Python `_dashboard_verify_storage_step`-Lookup) ─────────

use tb_transport_twitch::HelixClient;

/// Auflösungsergebnis für `twitch_user_id` + Discord-Daten beim Verify
/// (`streamer_admin_mixin.py:297-324`): erst `twitch_streamers`, sonst aktiver
/// Partner, sonst Helix-Lookup für die `twitch_user_id`.
#[derive(Debug, Clone)]
pub struct VerifySource {
    pub twitch_user_id: String,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
}

impl VerifySource {
    /// `twitch_user_id` ist nach Trim nicht leer (Python:
    /// `if not twitch_user_id: "{login} ist nicht gespeichert"`).
    pub fn twitch_user_id_present(&self) -> bool {
        !self.twitch_user_id.trim().is_empty()
    }
}

#[derive(sqlx::FromRow)]
struct VerifySourceRow {
    twitch_user_id: Option<String>,
    discord_user_id: Option<String>,
    discord_display_name: Option<String>,
}

/// Lädt die Verify-Quelldaten: `twitch_streamers`-Zeile, sonst aktiver Partner,
/// sonst Helix-Lookup für die `twitch_user_id` (Discord-Daten dann leer).
pub async fn load_verify_source(
    pool: &PgPool,
    login: &str,
    helix: Option<&HelixClient>,
) -> Result<Option<VerifySource>, sqlx::Error> {
    let streamer: Option<VerifySourceRow> = sqlx::query_as!(
        VerifySourceRow,
        r#"
        SELECT s.twitch_user_id,
               i.discord_user_id,
               i.discord_display_name
          FROM twitch_streamers s
          LEFT JOIN twitch_streamer_identities i
            ON i.twitch_login <> ''
           AND LOWER(i.twitch_login) = LOWER(s.twitch_login)
         WHERE LOWER(s.twitch_login) = LOWER($1)
        "#,
        login
    )
    .fetch_optional(pool)
    .await?;

    if let Some(row) = streamer {
        let uid = row.twitch_user_id.unwrap_or_default();
        if !uid.trim().is_empty() {
            return Ok(Some(VerifySource {
                twitch_user_id: uid.trim().to_string(),
                discord_user_id: clean_opt(row.discord_user_id),
                discord_display_name: clean_opt(row.discord_display_name),
            }));
        }
    }

    // Fallback: aktiver Partner.
    if let Some(p) = load_active_partner(pool, login).await? {
        if let Some(uid) = p
            .twitch_user_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Ok(Some(VerifySource {
                twitch_user_id: uid.to_string(),
                discord_user_id: clean_opt(p.discord_user_id),
                discord_display_name: clean_opt(p.discord_display_name),
            }));
        }
    }

    // Letzter Fallback: Helix-Lookup (Discord-Daten bleiben leer).
    if let Some(h) = helix {
        if let Ok(users) = h.get_users(&[login]).await {
            if let Some(uid) = users
                .values()
                .next()
                .map(|u| u.id.clone())
                .filter(|s| !s.trim().is_empty())
            {
                return Ok(Some(VerifySource {
                    twitch_user_id: uid,
                    discord_user_id: None,
                    discord_display_name: None,
                }));
            }
        }
    }

    Ok(None)
}

fn clean_opt(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

// ── Promote (Python `promote_streamer_to_partner`, Verify-Teilpfad) ────────────

/// Promotet einen Streamer zum aktiven Partner und setzt die Verifikation —
/// der frühere Verify-Teilpfad von `promote_streamer_to_partner` (`…:782`).
///
/// Block-10-Scope ist GENAU der Verify-Aufruf aus
/// `_dashboard_verify_storage_step` (`streamer_admin_mixin.py:330`): Identität +
/// Discord-Daten + Partner-Payload. Existiert bereits ein (auch
/// inaktiver) Partner-Datensatz, wird er reaktiviert; sonst wird neu eingefügt.
/// Die zahlreichen optionalen Spalten (`silent_*`, `live_ping_*`,
/// `last_link_*`) bleiben auf ihren bestehenden Werten bzw. Defaults — der
/// Verify-Pfad setzt sie in Python nicht (alle `_UNSET`).
#[allow(clippy::too_many_arguments)]
pub async fn promote_streamer_to_partner(
    pool: &PgPool,
    login: &str,
    twitch_user_id: &str,
    discord_user_id: Option<&str>,
    discord_display_name: Option<&str>,
    is_on_discord: i32,
    verification: &VerificationPayload,
) -> Result<(), sqlx::Error> {
    let normalized_login = login.to_lowercase();
    let normalized_user_id = twitch_user_id.trim();
    if normalized_login.is_empty() || normalized_user_id.is_empty() {
        // Python: ValueError("twitch_login_and_user_id_required").
        // Der Handler stellt sicher, dass user_id vorhanden ist; defensiv no-op.
        return Ok(());
    }

    upsert_streamer_identity(
        pool,
        Some(normalized_user_id),
        &normalized_login,
        discord_user_id,
        discord_display_name,
        is_on_discord,
    )
    .await?;

    let partnered_at = now_iso();

    // Bestehenden Partner-Datensatz (egal welcher Status) reaktivieren …
    let updated = sqlx::query!(
        r#"
        UPDATE twitch_partners
        SET twitch_login = $1,
            twitch_user_id = $2,
            manual_partner_opt_out = $3,
            partnered_at = COALESCE(NULLIF(partnered_at, ''), $4),
            admin_archived_at = NULL,
            departnered_at = NULL,
            technical_pause_reason = NULL,
            status = $5
        WHERE id = (
            SELECT id FROM twitch_partners
             WHERE LOWER(twitch_login) = LOWER($1) OR twitch_user_id = $2
             ORDER BY (COALESCE(status,'') = 'active') DESC, id DESC
             LIMIT 1
        )
        "#,
        &normalized_login,
        normalized_user_id,
        verification.manual_partner_opt_out,
        &partnered_at,
        STATUS_ACTIVE
    )
    .execute(pool)
    .await?
    .rows_affected();

    if updated > 0 {
        return Ok(());
    }

    // … oder neu einfügen, wenn noch kein Partner-Datensatz existiert.
    // id ist in Prod bigint NOT NULL ohne DEFAULT → MAX(id)+1 (Python-Inserts
    // setzen id ebenfalls explizit über die Sequenz; im Test reicht MAX+1).
    sqlx::query!(
        r#"
        INSERT INTO twitch_partners (
            id, twitch_user_id, twitch_login,
            manual_partner_opt_out, partnered_at, status
        ) VALUES (
            COALESCE((SELECT MAX(id) FROM twitch_partners), 0) + 1,
            $1, $2, $3, $4, $5
        )
        "#,
        normalized_user_id,
        &normalized_login,
        verification.manual_partner_opt_out,
        &partnered_at,
        STATUS_ACTIVE
    )
    .execute(pool)
    .await?;

    Ok(())
}

// ── Reactivate aus History (Python `reactivate_partner`) ───────────────────────

/// Ergebnis einer Reaktivierung — die (normalisierte) Identität des wieder
/// aktiven Partners. Parität zum Python-Rückgabewert (`twitch_login` +
/// `twitch_user_id`); der Aufrufer braucht nur die Identität.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactivateOutcome {
    pub twitch_login: String,
    pub twitch_user_id: Option<String>,
}

/// Jüngste Partner-Historienzeile (egal welcher Status) für die Reaktivierung.
/// `latest=True` in Python ordnet nach `COALESCE(departnered_at, partnered_at)`
/// DESC, dann `id` DESC.
#[derive(sqlx::FromRow)]
struct HistoryPartnerRow {
    id: i64,
    twitch_login: String,
    twitch_user_id: Option<String>,
    status: Option<String>,
}

/// Holt einen departnerten/archivierten Partner aus der History zurück —
/// Parität zu `reactivate_partner` (`partner_registry.py:1283`).
///
/// Ablauf wie Python:
/// 1. Ist bereits ein Partner `status='active'` → No-op, dessen Identität
///    zurückgeben (kein erneutes Promoten).
/// 2. Sonst jüngste Historienzeile laden. Fehlt sie oder ist ihr Status nicht
///    inaktiv (`departnered`/`archived`) → `None` (nichts zu reaktivieren).
/// 3. Die Zeile auf `status='active'` flippen: `departnered_at`,
///    `admin_archived_at`, `technical_pause_reason` nullen, `partnered_at=now`,
///    `manual_partner_opt_out=0`.
///    Alle übrigen Partner-Spalten (`silent_*`, `live_ping_*`,
///    `require_discord_link`) bleiben auf der Zeile erhalten.
///    `raid_bot_enabled` wird bei Reaktivierung explizit wieder eingeschaltet.
/// 4. Nur wenn der alte Status `archived` (nicht `departnered`) war: Raid-Auth
///    wiederherstellen (`raid_enabled=TRUE`), aber nur wenn `needs_reauth`
///    nicht gesetzt ist (sonst bleibt die Auth deaktiviert).
pub async fn reactivate_partner(
    pool: &PgPool,
    login: &str,
) -> Result<Option<ReactivateOutcome>, sqlx::Error> {
    // 1. Bereits aktiv → No-op.
    if let Some(active) = load_active_partner(pool, login).await? {
        return Ok(Some(ReactivateOutcome {
            twitch_login: active.twitch_login.to_lowercase(),
            twitch_user_id: clean_opt(active.twitch_user_id),
        }));
    }

    // 2. Jüngste Historienzeile (egal welcher Status).
    let history: Option<HistoryPartnerRow> = sqlx::query_as!(
        HistoryPartnerRow,
        r#"
        SELECT id AS "id!",
               twitch_login AS "twitch_login!",
               twitch_user_id AS "twitch_user_id?",
               status
          FROM twitch_partners
         WHERE LOWER(twitch_login) = LOWER($1)
         ORDER BY COALESCE(departnered_at, partnered_at, '') DESC, id DESC
         LIMIT 1
        "#,
        login
    )
    .fetch_optional(pool)
    .await?;

    let Some(history) = history else {
        return Ok(None);
    };
    if !is_inactive_partner_status(history.status.as_deref()) {
        return Ok(None);
    }

    let normalized_login = history.twitch_login.to_lowercase();
    let normalized_user_id = clean_opt(history.twitch_user_id);
    let was_departnered = history
        .status
        .as_deref()
        .map(|s| s.trim().to_lowercase() == STATUS_DEPARTNERED)
        .unwrap_or(false);
    let partnered_at = now_iso();

    // 3. Zeile reaktivieren — Konfig-Spalten bleiben unangetastet (No-Touch auf
    //    derselben Zeile entspricht Pythons "Werte aus der Zeile zurückschreiben").
    sqlx::query!(
        r#"
        UPDATE twitch_partners
        SET status = $1,
            departnered_at = NULL,
            admin_archived_at = NULL,
            technical_pause_reason = NULL,
            manual_partner_opt_out = 0,
            raid_bot_enabled = 1,
            partnered_at = $2
        WHERE id = $3
        "#,
        STATUS_ACTIVE,
        &partnered_at,
        history.id
    )
    .execute(pool)
    .await?;

    // 4. Raid-Auth nur beim archived→active-Übergang wiederherstellen.
    if !was_departnered {
        restore_raid_auth_for_reactivated_partner(
            pool,
            &normalized_login,
            normalized_user_id.as_deref(),
        )
        .await?;
    }

    Ok(Some(ReactivateOutcome {
        twitch_login: normalized_login,
        twitch_user_id: normalized_user_id,
    }))
}

/// Reaktiviert die Raid-Auth (`raid_enabled=TRUE`) eines zurückgeholten
/// Partners — Parität zu `_restore_raid_auth_for_reactivated_partner`
/// (`partner_registry.py:63`). Reaktiviert NUR, wenn eine Auth-Zeile existiert
/// und deren `needs_reauth` nicht gesetzt ist (ein abgelaufener Token darf nicht
/// stillschweigend wieder scharf geschaltet werden).
async fn restore_raid_auth_for_reactivated_partner(
    pool: &PgPool,
    login: &str,
    user_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    let uid = user_id.unwrap_or("");
    if login.is_empty() && uid.is_empty() {
        return Ok(());
    }

    let needs_reauth: Option<Option<bool>> = sqlx::query_scalar!(
        r#"
        SELECT needs_reauth
          FROM twitch_raid_auth
         WHERE ($1 <> '' AND twitch_user_id = $1)
            OR ($2 <> '' AND LOWER(twitch_login) = LOWER($2))
         LIMIT 1
        "#,
        uid,
        login
    )
    .fetch_optional(pool)
    .await?;

    // Keine Auth-Zeile → nichts zu tun (Python `return False`).
    let Some(needs_reauth) = needs_reauth else {
        return Ok(());
    };
    if needs_reauth.unwrap_or(false) {
        return Ok(());
    }

    sqlx::query!(
        r#"
        UPDATE twitch_raid_auth
        SET raid_enabled = TRUE,
            twitch_login = COALESCE(NULLIF($2, ''), twitch_login)
        WHERE ($1 <> '' AND twitch_user_id = $1)
           OR ($2 <> '' AND LOWER(twitch_login) = LOWER($2))
        "#,
        uid,
        login
    )
    .execute(pool)
    .await?;
    Ok(())
}

// ── Stats-Backfill (Python `backfill_tracked_stats_from_category`) ─────────────

/// Kopiert historische Kategorie-Stats idempotent nach `twitch_stats_tracked` —
/// Parität zu `backfill_tracked_stats_from_category` (`pg.py:1404`). Gibt die
/// Anzahl kopierter Zeilen zurück (für die "(N historische Datenpunkte
/// übernommen)"-Meldung). Best-effort: fehlende Tabellen → 0, kein Hard-Fail.
pub async fn backfill_tracked_stats_from_category(
    pool: &PgPool,
    login: &str,
) -> Result<i64, sqlx::Error> {
    let normalized = login.trim().to_lowercase();
    if normalized.is_empty() {
        return Ok(0);
    }
    let res = sqlx::query!(
        r#"
        INSERT INTO twitch_stats_tracked
            (ts_utc, streamer, viewer_count, is_partner, game_name, stream_title, tags)
        SELECT c.ts_utc, c.streamer, c.viewer_count, c.is_partner,
               c.game_name, c.stream_title, c.tags
          FROM twitch_stats_category c
         WHERE LOWER(c.streamer) = $1
           AND NOT EXISTS (
               SELECT 1 FROM twitch_stats_tracked t
                WHERE LOWER(t.streamer) = LOWER(c.streamer)
                  AND t.ts_utc = c.ts_utc
           )
        "#,
        &normalized
    )
    .execute(pool)
    .await;

    match res {
        Ok(r) => Ok(r.rows_affected() as i64),
        // Tabellen fehlen evtl. (Stats-Subsystem nicht migriert) → 0 wie Python.
        Err(error) => {
            tracing::warn!(
                %error,
                login = %normalized,
                "Streamer-Lifecycle: Stats-Archivierung fehlgeschlagen"
            );
            Ok(0)
        }
    }
}

// ── require-link-Backfill (Python `upsert_non_partner_streamer`-Auszug) ────────

/// Setzt beim Hinzufügen eines Nicht-Partners `require_discord_link` und
/// `next_link_check_at` — der require-link-Teil von `upsert_non_partner_streamer`
/// (`partner_registry.py:563`), aufgerufen aus `_cmd_add`
/// (`admin.py:233`, `next_link_check_at = now + 30 Tage`).
///
/// Der Add-Pfad legt die Streamer-/Partnerzeilen bereits via
/// `tb_analytics::streamers_crud::add_streamer` an; dieser Helfer trägt nur die
/// beiden Link-Lifecycle-Spalten auf der aktiven Partnerzeile nach.
pub async fn backfill_require_link(
    pool: &PgPool,
    login: &str,
    require_link: bool,
) -> Result<(), sqlx::Error> {
    let normalized = login.to_lowercase();
    let next_check = datetime_to_iso(Utc::now() + chrono::Duration::days(30));
    let require_int: i32 = if require_link { 1 } else { 0 };
    sqlx::query!(
        r#"
        UPDATE twitch_partners
        SET require_discord_link = $2,
            next_link_check_at = $3
        WHERE LOWER(twitch_login) = LOWER($1)
          AND COALESCE(status, 'active') = 'active'
          AND admin_archived_at IS NULL
          AND departnered_at IS NULL
        "#,
        &normalized,
        require_int,
        &next_check
    )
    .execute(pool)
    .await?;
    Ok(())
}

// ── Archive mit Kontext-Meldung (Python `_dashboard_archive_sync`) ─────────────

/// Ergebnis von [`archive_with_message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveOutcome {
    /// Aktion ausgeführt (oder No-op mit Statusmeldung), `String` = Python-Meldung.
    Done(String),
    /// Weder aktiv noch in der History gespeichert (Python: "ist nicht
    /// gespeichert" → 4xx).
    NotStored,
    /// History-Zeile vorhanden, aber nicht archiviert reaktivierbar (departnert
    /// statt nur archiviert / kein aktiver Partner). Python wirft hier einen
    /// `ValueError` mit dieser Meldung → 4xx-Konflikt, `String` = Python-Meldung.
    Conflict(String),
}

/// Aktueller Archiv-/Block-Zustand eines aktiven Partners für die Meldung.
#[derive(sqlx::FromRow)]
struct ArchiveStateRow {
    admin_archived_at: Option<String>,
    technical_pause_reason: Option<String>,
}

/// Führt die Archiv-/Block-Mutation aus und liefert die kontextspezifische
/// Python-Meldung — Parität zu `_dashboard_archive_sync`
/// (`streamer_admin_mixin.py:58`).
///
/// Ohne aktiven Partner greift der History-Pfad (`reactivate_partner`): eine
/// `archived`-Historienzeile wird beim Unarchive/Toggle reaktiviert, eine
/// `departnered`-Zeile als [`ArchiveOutcome::Conflict`] gemeldet (Python
/// `ValueError`), gar keine Zeile als [`ArchiveOutcome::NotStored`].
pub async fn archive_with_message(
    pool: &PgPool,
    login: &str,
    raw_mode: &str,
) -> Result<ArchiveOutcome, sqlx::Error> {
    use tb_analytics::streamers_crud::{archive_streamer, ArchiveMode};

    // desired wie Python `_dashboard_archive` (mode_clean → desired).
    let desired = match raw_mode.trim().to_lowercase().as_str() {
        "archive" | "on" | "set" => "archive",
        "unarchive" | "off" | "unset" | "restore" => "unarchive",
        "block" | "blocked" | "ban" => "block",
        "unblock" | "allow" => "unblock",
        "toggle_block" | "block_toggle" => "toggle_block",
        _ => "toggle",
    };

    let state: Option<ArchiveStateRow> = sqlx::query_as!(
        ArchiveStateRow,
        r#"
        SELECT admin_archived_at, technical_pause_reason
          FROM twitch_partners
         WHERE LOWER(twitch_login) = LOWER($1)
           AND COALESCE(status, '') = 'active'
         ORDER BY id DESC
         LIMIT 1
        "#,
        login
    )
    .fetch_optional(pool)
    .await?;

    let Some(state) = state else {
        // Kein aktiver Partner — History-Pfad (Python `if not active_row and
        // history_row`). Nur für die Archive-/Unarchive-/Toggle-Modi relevant;
        // der Block-Pfad gegen eine reine History-Zeile bleibt (wie bisher)
        // unbehandelt und fällt unten auf NotStored.
        if !matches!(desired, "block" | "unblock" | "toggle_block") {
            return archive_history_path(pool, login, desired).await;
        }
        return Ok(ArchiveOutcome::NotStored);
    };

    let currently_archived = state
        .admin_archived_at
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let currently_blocked = state
        .technical_pause_reason
        .as_deref()
        .map(|s| s.trim().to_lowercase() == "blocked")
        .unwrap_or(false);

    // Block-/Unblock-/ToggleBlock-Pfad.
    if matches!(desired, "block" | "unblock" | "toggle_block") {
        let should_block = match desired {
            "block" => true,
            "unblock" => false,
            _ => !currently_blocked,
        };
        let mode = if should_block {
            ArchiveMode::Block
        } else {
            ArchiveMode::Unblock
        };
        // Bei Unblock greift archive_streamer nur, wenn aktuell blockiert —
        // sonst rows_affected=0. Die Meldung ist trotzdem deterministisch.
        let _ = archive_streamer(pool, login, mode).await?;
        let msg = if should_block {
            format!("{login} dauerhaft blockiert")
        } else {
            format!("{login} entsperrt")
        };
        return Ok(ArchiveOutcome::Done(msg));
    }

    // Archive-/Unarchive-/Toggle-Pfad (aktiver Partner).
    match desired {
        "archive" => {
            if currently_archived {
                let since = state
                    .admin_archived_at
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                return Ok(ArchiveOutcome::Done(match since {
                    Some(s) => format!("{login} ist bereits archiviert (seit {s})"),
                    None => format!("{login} ist bereits archiviert"),
                }));
            }
            archive_streamer(pool, login, ArchiveMode::Archive).await?;
            Ok(ArchiveOutcome::Done(format!("{login} archiviert")))
        }
        "unarchive" => {
            if !currently_archived {
                return Ok(ArchiveOutcome::Done(format!(
                    "{login} ist nicht archiviert"
                )));
            }
            archive_streamer(pool, login, ArchiveMode::Unarchive).await?;
            Ok(ArchiveOutcome::Done(format!("{login} ent-archiviert")))
        }
        _ => {
            // toggle: archiviert → reaktiviert, sonst → archiviert.
            if currently_archived {
                archive_streamer(pool, login, ArchiveMode::Unarchive).await?;
                Ok(ArchiveOutcome::Done(format!("{login} reaktiviert")))
            } else {
                archive_streamer(pool, login, ArchiveMode::Archive).await?;
                Ok(ArchiveOutcome::Done(format!("{login} archiviert")))
            }
        }
    }
}

/// History-Pfad von [`archive_with_message`] (Python `if not active_row and
/// history_row`): ohne aktiven Partner über die jüngste Historienzeile
/// entscheiden.
///
/// - Status `archived`:
///   - `desired == "archive"` → "ist bereits archiviert (seit …)" (No-op).
///   - sonst `reactivate_partner` → "ent-archiviert" (unarchive) bzw.
///     "reaktiviert" (toggle).
/// - Status `departnered` → [`ArchiveOutcome::Conflict`] ("ist departnered und
///   nicht nur archiviert").
/// - keine Historienzeile → [`ArchiveOutcome::NotStored`].
async fn archive_history_path(
    pool: &PgPool,
    login: &str,
    desired: &str,
) -> Result<ArchiveOutcome, sqlx::Error> {
    #[derive(sqlx::FromRow)]
    struct Row {
        status: Option<String>,
        admin_archived_at: Option<String>,
        departnered_at: Option<String>,
    }

    let row: Option<Row> = sqlx::query_as!(
        Row,
        r#"
        SELECT status, admin_archived_at, departnered_at
          FROM twitch_partners
         WHERE LOWER(twitch_login) = LOWER($1)
         ORDER BY COALESCE(departnered_at, partnered_at, '') DESC, id DESC
         LIMIT 1
        "#,
        login
    )
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(ArchiveOutcome::NotStored);
    };

    let status = row.status.as_deref().map(str::trim).unwrap_or("");
    if status.eq_ignore_ascii_case(STATUS_ARCHIVED) {
        if desired == "archive" {
            let since = row
                .admin_archived_at
                .as_deref()
                .or(row.departnered_at.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            return Ok(ArchiveOutcome::Done(match since {
                Some(s) => format!("{login} ist bereits archiviert (seit {s})"),
                None => format!("{login} ist bereits archiviert"),
            }));
        }
        // reaktivieren; bei Fehlschlag Python-ValueError-Parität als Conflict.
        if reactivate_partner(pool, login).await?.is_none() {
            return Ok(ArchiveOutcome::Conflict(format!(
                "{login} konnte nicht reaktiviert werden"
            )));
        }
        return Ok(ArchiveOutcome::Done(if desired == "unarchive" {
            format!("{login} ent-archiviert")
        } else {
            format!("{login} reaktiviert")
        }));
    }

    // Nicht-leerer, nicht-aktiver Status (z. B. `departnered`) → Konflikt.
    if !status.is_empty() && !status.eq_ignore_ascii_case(STATUS_ACTIVE) {
        return Ok(ArchiveOutcome::Conflict(format!(
            "{login} ist departnered und nicht nur archiviert"
        )));
    }
    Ok(ArchiveOutcome::Conflict(format!(
        "{login} ist kein aktiver Partner"
    )))
}

// ── Live-State löschen (Python `_cmd_remove`-DELETE) ───────────────────────────

/// Löscht die `twitch_live_state`-Zeile eines Logins (idempotent) — der
/// explizite `DELETE FROM twitch_live_state` aus `_cmd_remove` (`admin.py:267`).
pub async fn clear_live_state(pool: &PgPool, login: &str) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM twitch_live_state WHERE LOWER(streamer_login) = LOWER($1)",
        login.to_lowercase()
    )
    .execute(pool)
    .await?;
    Ok(())
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

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("DB-Verbindung");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("drop schema");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("create schema");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");

        // prod-treue DDL (Timestamp-Spalten TEXT wie in Prod — Typ-Drift-Schutz).
        for ddl in [
            r#"CREATE TABLE twitch_partners (
                id BIGINT PRIMARY KEY,
                twitch_user_id TEXT,
                twitch_login TEXT NOT NULL,
                require_discord_link INTEGER DEFAULT 0,
                next_link_check_at TEXT,
                manual_partner_opt_out INTEGER DEFAULT 0,
                raid_bot_enabled INTEGER DEFAULT 0,
                silent_ban INTEGER DEFAULT 0,
                silent_raid INTEGER DEFAULT 0,
                live_ping_role_id BIGINT,
                live_ping_enabled INTEGER DEFAULT 1,
                partnered_at TEXT DEFAULT CURRENT_TIMESTAMP,
                admin_archived_at TEXT,
                departnered_at TEXT,
                technical_pause_reason TEXT,
                status TEXT DEFAULT 'active'
            )"#,
            r#"CREATE TABLE twitch_streamers (
                twitch_login TEXT PRIMARY KEY,
                twitch_user_id TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            )"#,
            r#"CREATE TABLE twitch_streamer_identities (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT NOT NULL,
                discord_user_id TEXT,
                discord_display_name TEXT,
                is_on_discord INTEGER DEFAULT 0,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP
            )"#,
            r#"CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT,
                raid_enabled BOOLEAN,
                needs_reauth BOOLEAN
            )"#,
            r#"CREATE TABLE twitch_stats_category (
                ts_utc TIMESTAMPTZ, streamer TEXT, viewer_count INTEGER,
                is_partner BOOLEAN DEFAULT FALSE, game_name TEXT, stream_title TEXT, tags TEXT
            )"#,
            r#"CREATE TABLE twitch_stats_tracked (
                ts_utc TIMESTAMPTZ, streamer TEXT, viewer_count INTEGER,
                is_partner BOOLEAN DEFAULT FALSE, game_name TEXT, stream_title TEXT, tags TEXT
            )"#,
        ] {
            sqlx::query(ddl).execute(&pool).await.expect("DDL");
        }
        pool
    }

    /// Legt einen aktiven Partner an. Discord-Daten leben (prod-treu) in
    /// `twitch_streamer_identities`, nicht auf `twitch_partners` — der Helper
    /// schreibt sie dorthin, damit `load_active_partner` sie via JOIN liefert.
    async fn insert_active_partner(pool: &PgPool, id: i64, login: &str, uid: &str) {
        sqlx::query(
            "INSERT INTO twitch_partners (id, twitch_login, twitch_user_id, status)
             VALUES ($1, $2, $3, 'active')",
        )
        .bind(id)
        .bind(login)
        .bind(uid)
        .execute(pool)
        .await
        .expect("insert active partner");
        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id, discord_display_name, is_on_discord)
             VALUES ($1, $2, '999', 'Drag', 1)",
        )
        .bind(uid)
        .bind(login)
        .execute(pool)
        .await
        .expect("insert identity");
    }

    #[tokio::test]
    async fn departner_setzt_status_und_disabled_raid_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_departner").await;
        insert_active_partner(&pool, 1, "drag", "42").await;
        sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled) VALUES ('42', 'drag', TRUE)")
            .execute(&pool).await.unwrap();

        let outcome = departner_active_partner(&pool, "drag", false)
            .await
            .unwrap()
            .expect("muss departnern");
        assert_eq!(outcome.twitch_login, "drag");
        assert_eq!(outcome.discord_user_id.as_deref(), Some("999"));

        let (status, departnered_at): (Option<String>, Option<String>) =
            sqlx::query_as("SELECT status, departnered_at FROM twitch_partners WHERE id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status.as_deref(), Some("departnered"));
        assert!(departnered_at.is_some());

        let raid_enabled: Option<bool> = sqlx::query_scalar(
            "SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id = '42'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(raid_enabled, Some(false), "raid-auth disabled");

        // Identity-Upsert hat stattgefunden.
        let ident: Option<String> = sqlx::query_scalar(
            "SELECT discord_user_id FROM twitch_streamer_identities WHERE twitch_user_id = '42'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap()
        .flatten();
        assert_eq!(ident.as_deref(), Some("999"));
    }

    #[tokio::test]
    async fn departner_clear_verification_setzt_opt_out() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_departner_clear").await;
        insert_active_partner(&pool, 1, "drag", "42").await;

        departner_active_partner(&pool, "drag", true)
            .await
            .unwrap()
            .expect("departnert");

        let (status, opt_out): (Option<String>, Option<i32>) = sqlx::query_as(
            "SELECT status, manual_partner_opt_out FROM twitch_partners WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status.as_deref(), Some("departnered"));
        // Opt-out ist die einzige Sperre gegen ein automatisches Zurückholen —
        // ohne sie wäre `verify mode=clear` nach dem nächsten Promote wirkungslos.
        assert_eq!(opt_out, Some(1));
    }

    #[tokio::test]
    async fn departner_ohne_clear_laesst_opt_out_unberuehrt() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_departner_plain").await;
        insert_active_partner(&pool, 1, "drag", "42").await;

        departner_active_partner(&pool, "drag", false)
            .await
            .unwrap()
            .expect("departnert");

        let (status, opt_out): (Option<String>, Option<i32>) = sqlx::query_as(
            "SELECT status, manual_partner_opt_out FROM twitch_partners WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status.as_deref(), Some("departnered"));
        // Reines Departnern (DELETE-Route) ist kein Opt-out: der Streamer darf
        // später ohne Admin-Eingriff wieder Partner werden.
        assert_eq!(opt_out, Some(0));
    }

    #[tokio::test]
    async fn departner_ohne_aktiven_partner_gibt_none() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_departner_none").await;
        let res = departner_active_partner(&pool, "niemand", false)
            .await
            .unwrap();
        assert!(res.is_none());
    }

    #[tokio::test]
    async fn promote_fuegt_neuen_partner_ein() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_promote_insert").await;
        let payload = VerificationPayload::for_mode("permanent").unwrap();
        promote_streamer_to_partner(
            &pool,
            "newpartner",
            "777",
            Some("555"),
            Some("Name"),
            1,
            &payload,
        )
        .await
        .unwrap();

        let (status, opt_out, uid): (Option<String>, Option<i32>, Option<String>) = sqlx::query_as(
            "SELECT status, manual_partner_opt_out, twitch_user_id FROM twitch_partners WHERE LOWER(twitch_login) = 'newpartner'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status.as_deref(), Some("active"));
        assert_eq!(opt_out, Some(0));
        assert_eq!(uid.as_deref(), Some("777"));

        // Identity wurde angelegt.
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_streamer_identities WHERE twitch_user_id = '777'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn promote_reaktiviert_departnerten_partner() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_promote_reactivate").await;
        sqlx::query(
            "INSERT INTO twitch_partners (id, twitch_login, twitch_user_id, status, departnered_at)
             VALUES (1, 'comeback', '888', 'departnered', '2026-01-01T00:00:00+00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let payload = VerificationPayload::for_mode("temp").unwrap();
        promote_streamer_to_partner(&pool, "comeback", "888", None, None, 0, &payload)
            .await
            .unwrap();

        let (status, departnered_at, opt_out): (Option<String>, Option<String>, Option<i32>) =
            sqlx::query_as("SELECT status, departnered_at, manual_partner_opt_out FROM twitch_partners WHERE id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status.as_deref(), Some("active"), "reaktiviert");
        assert!(departnered_at.is_none(), "departnered_at genullt");
        assert_eq!(opt_out, Some(0), "temp haelt Partner aktiv");

        // Kein zweiter Datensatz angelegt.
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partners")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "reaktiviert statt dupliziert");
    }

    #[tokio::test]
    async fn backfill_kopiert_kategorie_stats_idempotent() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_backfill").await;
        sqlx::query(
            "INSERT INTO twitch_stats_category (ts_utc, streamer, viewer_count) VALUES
                (NOW() - INTERVAL '2 hours', 'Cat', 100),
                (NOW() - INTERVAL '1 hour', 'cat', 200)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let copied = backfill_tracked_stats_from_category(&pool, "cat")
            .await
            .unwrap();
        assert_eq!(copied, 2);

        // Zweiter Lauf kopiert nichts mehr (idempotent).
        let copied2 = backfill_tracked_stats_from_category(&pool, "cat")
            .await
            .unwrap();
        assert_eq!(copied2, 0);
    }

    #[tokio::test]
    async fn backfill_require_link_setzt_spalten() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_require_link").await;
        sqlx::query(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('linkme', '12')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (id, twitch_login, twitch_user_id, status) VALUES (1, 'linkme', '12', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();

        backfill_require_link(&pool, "linkme", true).await.unwrap();

        let (req, next): (Option<i32>, Option<String>) = sqlx::query_as(
            "SELECT require_discord_link, next_link_check_at FROM twitch_partners WHERE twitch_login = 'linkme'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(req, Some(1));
        assert!(next.is_some(), "next_link_check_at gesetzt (now+30d)");
    }

    #[tokio::test]
    async fn verification_payload_modi() {
        let perm = VerificationPayload::for_mode("permanent").unwrap();
        assert_eq!(perm.manual_partner_opt_out, 0);

        let temp = VerificationPayload::for_mode("temp").unwrap();
        assert_eq!(temp.manual_partner_opt_out, 0);

        let clear = VerificationPayload::for_mode("clear").unwrap();
        assert_eq!(clear.manual_partner_opt_out, 1);

        assert!(VerificationPayload::for_mode("quatsch").is_none());
    }

    // ── reactivate_partner (Python `reactivate_partner`) ───────────────────────

    /// Legt eine inaktive Partner-Historienzeile mit gegebenem Status an
    /// (`departnered`/`archived`) inkl. erhaltener Konfig-Spalten.
    async fn insert_history_partner(pool: &PgPool, id: i64, login: &str, uid: &str, status: &str) {
        sqlx::query(
            "INSERT INTO twitch_partners
            	(id, twitch_login, twitch_user_id, status, departnered_at,
            	 admin_archived_at, technical_pause_reason, manual_partner_opt_out,
            	 silent_ban, live_ping_enabled)
             VALUES ($1, $2, $3, $4, '2026-01-01T00:00:00+00:00',
                     '2026-01-01T00:00:00+00:00', 'blocked', 1, 1, 0)",
        )
        .bind(id)
        .bind(login)
        .bind(uid)
        .bind(status)
        .execute(pool)
        .await
        .expect("insert history partner");
    }

    #[tokio::test]
    async fn reactivate_holt_archivierten_partner_zurueck_und_restored_raid() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_reactivate_archived").await;
        insert_history_partner(&pool, 1, "back", "42", STATUS_ARCHIVED).await;
        // Auth-Zeile ohne needs_reauth → muss reaktiviert werden.
        sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth) VALUES ('42', 'back', FALSE, FALSE)")
            .execute(&pool).await.unwrap();

        let out = reactivate_partner(&pool, "back")
            .await
            .unwrap()
            .expect("muss reaktivieren");
        assert_eq!(out.twitch_login, "back");
        assert_eq!(out.twitch_user_id.as_deref(), Some("42"));

        #[derive(sqlx::FromRow)]
        struct State {
            status: Option<String>,
            departnered_at: Option<String>,
            admin_archived_at: Option<String>,
            technical_pause_reason: Option<String>,
            manual_partner_opt_out: Option<i32>,
            raid_bot_enabled: Option<i32>,
            silent_ban: Option<i32>,
        }
        let s: State = sqlx::query_as(
            "SELECT status, departnered_at, admin_archived_at, technical_pause_reason,
                    manual_partner_opt_out, raid_bot_enabled, silent_ban
             FROM twitch_partners WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(s.status.as_deref(), Some("active"));
        assert!(s.departnered_at.is_none(), "departnered_at genullt");
        assert!(s.admin_archived_at.is_none(), "admin_archived_at genullt");
        assert!(
            s.technical_pause_reason.is_none(),
            "technical_pause_reason genullt"
        );
        assert_eq!(s.manual_partner_opt_out, Some(0), "opt_out zurückgesetzt");
        assert_eq!(s.raid_bot_enabled, Some(1), "raid_bot_enabled reaktiviert");
        assert_eq!(
            s.silent_ban,
            Some(1),
            "Konfig-Spalte silent_ban bleibt erhalten"
        );

        // archived → active ⇒ Raid-Auth wiederhergestellt.
        let raid: Option<bool> = sqlx::query_scalar(
            "SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id = '42'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(raid, Some(true), "archived→active restored raid auth");
    }

    #[tokio::test]
    async fn reactivate_archived_aber_needs_reauth_laesst_raid_aus() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_reactivate_needsreauth").await;
        insert_history_partner(&pool, 1, "back", "42", STATUS_ARCHIVED).await;
        sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth) VALUES ('42', 'back', FALSE, TRUE)")
            .execute(&pool).await.unwrap();

        reactivate_partner(&pool, "back")
            .await
            .unwrap()
            .expect("reaktiviert");

        let raid: Option<bool> = sqlx::query_scalar(
            "SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id = '42'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(raid, Some(false), "needs_reauth → raid bleibt aus");
    }

    #[tokio::test]
    async fn reactivate_departnerten_partner_ohne_raid_restore() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_reactivate_departnered").await;
        insert_history_partner(&pool, 1, "back", "42", STATUS_DEPARTNERED).await;
        sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth) VALUES ('42', 'back', FALSE, FALSE)")
            .execute(&pool).await.unwrap();

        let out = reactivate_partner(&pool, "back")
            .await
            .unwrap()
            .expect("reaktiviert");
        assert_eq!(out.twitch_login, "back");

        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM twitch_partners WHERE id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status.as_deref(), Some("active"));

        // departnered → active ⇒ Raid-Auth NICHT angefasst.
        let raid: Option<bool> = sqlx::query_scalar(
            "SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id = '42'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            raid,
            Some(false),
            "departnered→active berührt raid auth nicht"
        );
    }

    #[tokio::test]
    async fn reactivate_bei_aktivem_partner_ist_noop() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_reactivate_active_noop").await;
        insert_active_partner(&pool, 1, "drag", "42").await;

        let out = reactivate_partner(&pool, "drag")
            .await
            .unwrap()
            .expect("liefert Identität");
        assert_eq!(out.twitch_login, "drag");
        assert_eq!(out.twitch_user_id.as_deref(), Some("42"));

        // status bleibt active, keine Mutation.
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partners WHERE status = 'active'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn reactivate_ohne_historie_gibt_none() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_reactivate_none").await;
        let res = reactivate_partner(&pool, "ghost").await.unwrap();
        assert!(res.is_none());
    }

    #[tokio::test]
    async fn archive_history_path_reaktiviert_archivierten() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_archive_hist_reactivate").await;
        insert_history_partner(&pool, 1, "back", "42", STATUS_ARCHIVED).await;

        // toggle ohne aktiven Partner → reaktiviert.
        let out = archive_with_message(&pool, "back", "toggle").await.unwrap();
        assert_eq!(out, ArchiveOutcome::Done("back reaktiviert".into()));

        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM twitch_partners WHERE id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status.as_deref(), Some("active"));

        // unarchive-Variante liefert die "ent-archiviert"-Meldung.
        insert_history_partner(&pool, 2, "back2", "43", STATUS_ARCHIVED).await;
        let out2 = archive_with_message(&pool, "back2", "unarchive")
            .await
            .unwrap();
        assert_eq!(out2, ArchiveOutcome::Done("back2 ent-archiviert".into()));
    }

    #[tokio::test]
    async fn archive_history_path_departnered_ist_conflict() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_archive_hist_conflict").await;
        insert_history_partner(&pool, 1, "gone", "42", STATUS_DEPARTNERED).await;

        let out = archive_with_message(&pool, "gone", "unarchive")
            .await
            .unwrap();
        assert_eq!(
            out,
            ArchiveOutcome::Conflict("gone ist departnered und nicht nur archiviert".into())
        );
    }

    #[tokio::test]
    async fn archive_history_path_ohne_zeile_ist_notstored() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_lc_archive_hist_notstored").await;
        let out = archive_with_message(&pool, "nobody", "toggle")
            .await
            .unwrap();
        assert_eq!(out, ArchiveOutcome::NotStored);
    }
}
