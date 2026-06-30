//! Persistenter EventSub-Guard-Store (`eventsub_guard_state`) — das
//! Exactly-once-Primitiv des Monitorings, geteilt über Prozesse und
//! Transporte hinweg.
//!
//! Vertrag wie der Python-`EventSubStateStore`: Ein `claim` gewinnt genau
//! dann, wenn für (kind, key) kein aktiver Eintrag existiert. Die Korrektheit
//! liegt vollständig im konditionalen Upsert
//! (`WHERE expires_at <= EXCLUDED.updated_at`) — abgelaufene Rows räumt ein
//! periodischer [`GuardStore::sweep_expired`] ab, nicht (wie in Python) ein
//! DELETE bei jedem Claim.
//!
//! Zeit ist überall ein expliziter Parameter in Epoch-Sekunden
//! (`DOUBLE PRECISION` in der DB) — deterministisch testbar, kein
//! verstecktes `now()`.

use sqlx::PgPool;

/// Guard-Arten. Die Werte entsprechen 1:1 den Python-Konstanten
/// (Spalte `kind`, dort lowercase-normalisiert).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardKind {
    /// Webhook-Message-Dedup über Transporte hinweg (TTL im Minutenbereich).
    MessageId,
    /// WS-Message-Dedup. Von Rust ungenutzt — der WS-Pool wird nicht portiert
    /// (ADR 0004); die Variante dokumentiert den Tabellen-Vertrag vollständig.
    WsMessageId,
    /// Drosselt Offline-Verarbeitung gegen Online/Offline-Flapping.
    OfflineThrottle,
    /// Dedup fachlicher Effekte (z. B. ein Announcement pro Stream-Start).
    BusinessEffect,
}

impl GuardKind {
    pub fn as_str(self) -> &'static str {
        match self {
            GuardKind::MessageId => "message_id",
            GuardKind::WsMessageId => "ws_message_id",
            GuardKind::OfflineThrottle => "offline_throttle",
            GuardKind::BusinessEffect => "business_effect",
        }
    }
}

/// Zugriff auf `eventsub_guard_state`. Die Tabelle existiert in Prod
/// (von Python angelegt); die hermetischen Tests bilden das DDL nach.
#[derive(Clone)]
pub struct GuardStore {
    pool: PgPool,
}

impl GuardStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Liegt für (kind, key) ein aktiver, nicht abgelaufener Eintrag vor?
    /// Leerer Key zählt als „nicht aktiv" (wie Python).
    pub async fn is_active(
        &self,
        kind: GuardKind,
        key: &str,
        now: f64,
    ) -> Result<bool, sqlx::Error> {
        let key = key.trim();
        if key.is_empty() {
            return Ok(false);
        }
        let row: Option<i32> = sqlx::query_scalar!(
            r#"
            SELECT 1 AS "one!"
              FROM eventsub_guard_state
             WHERE kind = $1
               AND guard_key = $2
               AND expires_at > $3
             LIMIT 1
            "#,
            kind.as_str(),
            key,
            now,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    /// Versucht, (kind, key) für `ttl_seconds` zu beanspruchen.
    ///
    /// `true` = Claim gewonnen, der Aufrufer darf den Effekt ausführen.
    /// `false` = ein anderer Halter (Prozess/Transport) ist noch aktiv.
    /// TTL wird auf >= 1 s geklemmt (wie Python). Leerer Key → `false`.
    pub async fn claim(
        &self,
        kind: GuardKind,
        key: &str,
        ttl_seconds: f64,
        now: f64,
    ) -> Result<bool, sqlx::Error> {
        let key = key.trim();
        if key.is_empty() {
            return Ok(false);
        }
        let expires_at = now + ttl_seconds.max(1.0);
        let row: Option<i32> = sqlx::query_scalar!(
            r#"
            INSERT INTO eventsub_guard_state (kind, guard_key, expires_at, updated_at)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (kind, guard_key) DO UPDATE
               SET expires_at = EXCLUDED.expires_at,
                   updated_at = EXCLUDED.updated_at
             WHERE eventsub_guard_state.expires_at <= EXCLUDED.updated_at
            RETURNING 1 AS "one!"
            "#,
            kind.as_str(),
            key,
            expires_at,
            now,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    /// Gibt einen Guard explizit frei — z. B. nach fehlgeschlagener
    /// Verarbeitung, damit eine Wiederholung nicht den TTL-Ablauf abwarten muss.
    pub async fn release(&self, kind: GuardKind, key: &str) -> Result<(), sqlx::Error> {
        let key = key.trim();
        if key.is_empty() {
            return Ok(());
        }
        sqlx::query!(
            "DELETE FROM eventsub_guard_state WHERE kind = $1 AND guard_key = $2",
            kind.as_str(),
            key,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Garbage-Collection: löscht abgelaufene Einträge, liefert die Anzahl.
    /// Periodisch aufrufen (z. B. aus dem Poll-Loop) — nicht im Claim-Hot-Path.
    pub async fn sweep_expired(&self, now: f64) -> Result<u64, sqlx::Error> {
        let result = sqlx::query!(
            "DELETE FROM eventsub_guard_state WHERE expires_at <= $1",
            now,
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}
