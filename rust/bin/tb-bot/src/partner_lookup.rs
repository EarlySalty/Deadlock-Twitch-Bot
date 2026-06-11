//! Gemeinsame Partner-/Streamer-Lookups für Arrival-Klassifikation und
//! Manual-Raid-Key-Auflösung (genutzt vom Arrival-Sink UND dem
//! channel.raid-Koordinator — eine SQL-Quelle statt Kopien).

use sqlx::PgPool;
use tb_raid::arrival_confirmation::{KnownStreamerLookup, PartnerLookup};

/// Vorab geladene Lookup-Antworten für die synchrone Klassifikations-Engine
/// (Partner-Status, Quell-Auflösung) — Sync/Async-Brücke.
pub struct PrefetchedLookups {
    pub target_is_partner: bool,
    pub known_source: Option<bool>,
}

impl PartnerLookup for PrefetchedLookups {
    fn lookup_partner(&self, _id: Option<&str>, _login: Option<&str>) -> bool {
        self.target_is_partner
    }
}

impl KnownStreamerLookup for PrefetchedLookups {
    fn lookup_known_streamer(&self, _id: Option<&str>, _login: Option<&str>) -> Option<bool> {
        self.known_source
    }
}

/// Ist das Ziel ein aktiver Partner? (`twitch_partners`, Python
/// `resolve_partner_target_status`).
pub async fn is_target_partner(pool: &PgPool, to_id: &str, to_login: &str) -> bool {
    let row: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM twitch_partners
          WHERE ((NULLIF($1,'') IS NOT NULL AND twitch_user_id = $1)
              OR (NULLIF($2,'') IS NOT NULL AND LOWER(twitch_login) = LOWER($2)))
            AND status = 'active' LIMIT 1",
    )
    .bind(to_id)
    .bind(to_login)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);
    row.is_some()
}

/// Quell-Streamer-Auflösung (`twitch_streamer_identities`): `Some(true)` =
/// bekannt mit ID, `Some(false)` = nur Login bekannt, `None` = unbekannt
/// (Python `resolve_known_streamer_identity`).
pub async fn known_source(pool: &PgPool, from_id: Option<&str>, from_login: &str) -> Option<bool> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT twitch_user_id FROM twitch_streamer_identities
          WHERE ((NULLIF($1,'') IS NOT NULL AND twitch_user_id = $1)
              OR (NULLIF($2,'') IS NOT NULL AND LOWER(twitch_login) = LOWER($2)))
          LIMIT 1",
    )
    .bind(from_id.unwrap_or(""))
    .bind(from_login)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);
    // Python-Parität: `_identity_value(known_source, "twitch_user_id", "user_id")` —
    // prüft den tatsächlich gespeicherten DB-Wert, nicht den Aufrufer-Parameter from_id.
    // Some(true) = twitch_user_id in DB nicht leer → "known_streamer_id"
    // Some(false) = Zeile gefunden, aber twitch_user_id leer → "known_streamer_login"
    row.map(|r| !r.0.trim().is_empty())
}

/// Broadcaster-ID eines aktiven Partners per Login (Python
/// `resolve_streamer_id_by_login` — Fallback für den Manual-Raid-Key).
pub async fn resolve_active_partner_id_by_login(pool: &PgPool, login: &str) -> Option<String> {
    let login = login.trim().to_lowercase();
    if login.is_empty() {
        return None;
    }
    sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_partners
          WHERE LOWER(twitch_login) = $1 AND status = 'active'
          ORDER BY id DESC LIMIT 1",
    )
    .bind(&login)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .filter(|id: &String| !id.trim().is_empty())
}
