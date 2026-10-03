//! Community-Vorschläge (Community-Streamer-Brücke, Paket F, Twitch-Seite).
//!
//! Deadlock-Bots reicht Streamer-Vorschläge aus dem Discord über
//! `POST /internal/twitch/v1/scout/community-suggestion` weiter. Hier landen
//! sie als Kandidat in `twitch_scout_candidates` mit Status `vorgeschlagen`
//! und Quelle `community`. Die bestehende Admin-Freigabe bleibt der einzige
//! Weg in die Outreach-Kette; hier wird nie etwas versendet.
//!
//! Antwortstatus:
//! - `created`: neuer Kandidat angelegt, der Vorschlagende ist der erste.
//! - `already_known`: Kanal ist schon Kandidat (Scout oder Community) oder
//!   steht in einer laufenden Outreach-Sperrfrist. Der Vorschlag wird gezählt.
//! - `already_partner`: Kanal steht in `twitch_partners`.
//! - `blocked`: Raid-Blacklist, Partner-Denylist, Pitch-Blacklist, aktive
//!   Recruitment-Suppression oder globaler Bann.
//! - `not_found` vergibt der Endpunkt, wenn Helix den Login nicht kennt.
//!
//! Für Paket C (150 Punkte, wenn ein vorgeschlagener Kanal Partner wird)
//! liefert [`liste_ergebnisse`] gültige individuelle Vorschlagszuordnungen
//! und den Partnerstand, mit Cursor `updated_since`.

use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};

use crate::STATUS_VORGESCHLAGEN;

pub const SOURCE_AUTO: &str = "auto";
pub const SOURCE_COMMUNITY: &str = "community";
/// Längster gespeicherter Vorschlagsgrund (Zeichen).
pub const MAX_REASON_CHARS: usize = 500;
pub const MAX_IDEMPOTENCY_KEY_LEN: usize = 128;
pub const DEFAULT_PAGE_LIMIT: i64 = 1000;
pub const MAX_PAGE_LIMIT: i64 = 5000;
/// Advisory-Lock für Vorschläge und den monotonen Cursor.
const COMMUNITY_LOCK_KEY: i64 = 0x5c07_c0de_f001;

/// Antwortstatus an Deadlock-Bots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VorschlagStatus {
    Created,
    AlreadyKnown,
    AlreadyPartner,
    Blocked,
    NotFound,
}

impl VorschlagStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            VorschlagStatus::Created => "created",
            VorschlagStatus::AlreadyKnown => "already_known",
            VorschlagStatus::AlreadyPartner => "already_partner",
            VorschlagStatus::Blocked => "blocked",
            VorschlagStatus::NotFound => "not_found",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "created" => VorschlagStatus::Created,
            "already_known" => VorschlagStatus::AlreadyKnown,
            "already_partner" => VorschlagStatus::AlreadyPartner,
            "blocked" => VorschlagStatus::Blocked,
            "not_found" => VorschlagStatus::NotFound,
            _ => return None,
        })
    }
}

/// Was die Listen über einen Kanal sagen. Reihenfolge der Prüfung:
/// Partner vor Sperre vor bekanntem Kandidaten/Outreach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Listenstand {
    pub partner: bool,
    pub gesperrt: bool,
    pub kandidat: bool,
    pub outreach_sperrfrist: bool,
}

/// Status aus dem Listenstand (reine Regel).
pub fn status_aus_listen(stand: Listenstand) -> VorschlagStatus {
    if stand.partner {
        VorschlagStatus::AlreadyPartner
    } else if stand.gesperrt {
        VorschlagStatus::Blocked
    } else if stand.kandidat || stand.outreach_sperrfrist {
        VorschlagStatus::AlreadyKnown
    } else {
        VorschlagStatus::Created
    }
}

/// Twitch-Login aus der Eingabe: `@name`, `name` oder ein Kanal-Link
/// (`twitch.tv/name`). Ergebnis klein, nur `[a-z0-9_]`, 1 bis 25 Zeichen.
pub fn normalisiere_vorschlag_login(raw: &str) -> Option<String> {
    let mut value = raw.trim();
    for prefix in ["https://", "http://"] {
        if let Some(rest) = value
            .get(..prefix.len())
            .filter(|p| p.eq_ignore_ascii_case(prefix))
            .map(|_| &value[prefix.len()..])
        {
            value = rest;
        }
    }
    for host in ["www.twitch.tv/", "m.twitch.tv/", "twitch.tv/"] {
        if value
            .get(..host.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(host))
        {
            value = &value[host.len()..];
            value = value.split(['/', '?', '#']).next().unwrap_or_default();
            break;
        }
    }
    let login = value.trim_start_matches('@').to_ascii_lowercase();
    ((1..=25).contains(&login.len())
        && login
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
    .then_some(login)
}

/// Discord-Snowflake: 15 bis 21 Ziffern, keine führende Null.
pub fn gueltige_discord_id(id: &str) -> bool {
    (15..=21).contains(&id.len()) && !id.starts_with('0') && id.bytes().all(|b| b.is_ascii_digit())
}

pub fn gueltiger_idempotency_key(key: &str) -> bool {
    (1..=MAX_IDEMPOTENCY_KEY_LEN).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_graphic())
}

/// Grund: Steuerzeichen raus, getrimmt, höchstens 500 Zeichen; leer → `None`.
pub fn normalisiere_grund(raw: Option<&str>) -> Option<String> {
    let cleaned: String = raw?
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    (!trimmed.is_empty()).then(|| {
        trimmed
            .chars()
            .take(MAX_REASON_CHARS)
            .collect::<String>()
            .trim_end()
            .to_string()
    })
}

/// Partnerzeit aus `twitch_partners.partnered_at` (TEXT, verschiedene
/// Schreibweisen). Unlesbar → `None`.
pub fn parse_partner_zeit(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(ts) = DateTime::parse_from_rfc3339(raw) {
        return Some(ts.with_timezone(&Utc));
    }
    for format in ["%Y-%m-%d %H:%M:%S%.f%#z", "%Y-%m-%d %H:%M:%S%#z"] {
        if let Ok(ts) = DateTime::parse_from_str(raw, format) {
            return Some(ts.with_timezone(&Utc));
        }
    }
    for format in ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S%.f"] {
        if let Ok(ts) = NaiveDateTime::parse_from_str(raw, format) {
            return Some(ts.and_utc());
        }
    }
    None
}

/// Neuer Partnerstand eines Community-Kandidaten (reine Regel).
/// Aktiv: bisheriges `seit` bleibt, sonst Partnerzeit, sonst `jetzt`.
/// Nicht aktiv: `None`.
pub fn partner_seit(
    aktiv: bool,
    bisher: Option<DateTime<Utc>>,
    partnered_at: Option<&str>,
    jetzt: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if !aktiv {
        return None;
    }
    bisher
        .or_else(|| partnered_at.and_then(parse_partner_zeit))
        .or(Some(jetzt))
}

pub fn format_cursor(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

// ─── Vorschlag einreichen ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VorschlagEingabe {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub discord_id: String,
    pub grund: Option<String>,
    pub idempotency_key: String,
    pub submitted_at: Option<DateTime<Utc>>,
    pub privacy_epoch: Option<i64>,
}

#[derive(Debug)]
pub enum VorschlagFehler {
    /// Schlüssel schon für einen anderen Kanal oder eine andere Person benutzt.
    Konflikt,
    /// Ein Login lässt sich nicht eindeutig dem gespeicherten Konto zuordnen.
    IdentitaetUngeklaert,
    PrivacyGesperrt,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for VorschlagFehler {
    fn from(error: sqlx::Error) -> Self {
        VorschlagFehler::Db(error)
    }
}

pub(crate) async fn lock(tx: &mut Transaction<'_, Postgres>) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(COMMUNITY_LOCK_KEY)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Nächster eindeutiger Cursor-Zeitpunkt (unter dem Advisory-Lock).
pub(crate) async fn naechster_stempel(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<DateTime<Utc>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT GREATEST(clock_timestamp(),
                COALESCE((SELECT MAX(community_updated_at) FROM twitch_scout_candidates
                           WHERE source='community'), '-infinity'::timestamptz) + INTERVAL '1 microsecond',
                COALESCE((SELECT MAX(outcome_updated_at) FROM twitch_scout_community_suggestions),
                         '-infinity'::timestamptz) + INTERVAL '1 microsecond')",
    )
    .fetch_one(&mut **tx)
    .await
}

async fn listenstand(
    tx: &mut Transaction<'_, Postgres>,
    user_id: &str,
) -> Result<Listenstand, sqlx::Error> {
    let (partner, gesperrt, kandidat, outreach_sperrfrist): (bool, bool, bool, bool) =
        sqlx::query_as(
            "SELECT
               EXISTS (SELECT 1 FROM twitch_partners p
                        WHERE p.twitch_user_id = $1),
               EXISTS (SELECT 1 FROM twitch_raid_blacklist b
                        WHERE b.target_id = $1)
               OR EXISTS (SELECT 1 FROM twitch_partner_signup_denylist d
                        WHERE d.twitch_user_id = $1)
               OR EXISTS (SELECT 1 FROM twitch_scout_pitch_blacklist pb
                        WHERE pb.twitch_user_id = $1)
               OR EXISTS (SELECT 1 FROM twitch_chatter_global_ban gb
                        WHERE gb.chatter_id = $1)
               OR EXISTS (SELECT 1 FROM twitch_outbound_chat_suppressions sup
                        WHERE sup.target_id = $1
                          AND sup.source = 'recruitment'
                          AND sup.suppressed_until > NOW()),
               EXISTS (SELECT 1 FROM twitch_scout_candidates c
                        WHERE c.twitch_user_id = $1),
               EXISTS (SELECT 1 FROM twitch_partner_outreach o
                        WHERE o.streamer_user_id = $1
                          AND NULLIF(BTRIM(o.cooldown_until), '')::timestamptz > NOW())",
        )
        .bind(user_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(Listenstand {
        partner,
        gesperrt,
        kandidat,
        outreach_sperrfrist,
    })
}

/// Liefert einen gespeicherten Vorschlag erst nach Prüfung seiner Anfragebindung.
/// Eine neue Twitch-Auflösung wird beim Replay nicht benötigt.
pub async fn vorschlag_wiederholen(
    pool: &PgPool,
    key: &str,
    login: &str,
    discord_id: &str,
    grund: Option<&str>,
    submitted_at: Option<DateTime<Utc>>,
    privacy_epoch: Option<i64>,
) -> Result<Option<(VorschlagStatus, String)>, VorschlagFehler> {
    let grund = normalisiere_grund(grund);
    let mut tx = pool.begin().await?;
    lock(&mut tx).await?;
    crate::community_privacy::require_submission(
        &mut tx,
        discord_id,
        key,
        submitted_at,
        privacy_epoch,
    )
    .await?;
    let replay = vorschlag_wiederholen_mit_executor(
        &mut *tx,
        key,
        login,
        discord_id,
        grund.as_deref(),
        submitted_at,
        privacy_epoch,
    )
    .await?;
    tx.commit().await?;
    Ok(replay)
}

#[derive(sqlx::FromRow)]
struct StoredSuggestion {
    twitch_user_id: String,
    twitch_login: String,
    suggested_by_discord_id: String,
    reason: Option<String>,
    result_status: String,
    submitted_at: Option<DateTime<Utc>>,
    privacy_epoch: Option<i64>,
}

async fn vorschlag_wiederholen_mit_executor<'e>(
    executor: impl sqlx::Executor<'e, Database = Postgres>,
    key: &str,
    login: &str,
    discord_id: &str,
    grund: Option<&str>,
    submitted_at: Option<DateTime<Utc>>,
    privacy_epoch: Option<i64>,
) -> Result<Option<(VorschlagStatus, String)>, VorschlagFehler> {
    let gespeichert: Option<StoredSuggestion> = sqlx::query_as(
        "SELECT twitch_user_id, twitch_login, suggested_by_discord_id, reason, result_status,submitted_at,privacy_epoch
           FROM twitch_scout_community_suggestions WHERE idempotency_key = $1",
    )
    .bind(key)
    .fetch_optional(executor)
    .await?;
    let Some(row) = gespeichert else {
        return Ok(None);
    };
    if row.twitch_login != login
        || row.suggested_by_discord_id != discord_id
        || row.reason.as_deref() != grund
        || row.submitted_at != submitted_at
        || row.privacy_epoch != privacy_epoch
    {
        return Err(VorschlagFehler::Konflikt);
    }
    let status = VorschlagStatus::parse(&row.result_status).ok_or_else(|| {
        VorschlagFehler::Db(sqlx::Error::Protocol("Unbekannter Vorschlagsstatus".into()))
    })?;
    Ok(Some((status, row.twitch_user_id)))
}

/// Legt einen Community-Vorschlag ab. Idempotent über `idempotency_key`:
/// eine Wiederholung liefert den damals vergebenen Status.
pub async fn vorschlag_einreichen(
    pool: &PgPool,
    eingabe: &VorschlagEingabe,
) -> Result<VorschlagStatus, VorschlagFehler> {
    let login = eingabe.twitch_login.trim().to_ascii_lowercase();
    let user_id = eingabe.twitch_user_id.trim();
    let grund = normalisiere_grund(eingabe.grund.as_deref());

    let mut tx = pool.begin().await?;
    lock(&mut tx).await?;

    crate::community_privacy::require_submission(
        &mut tx,
        &eingabe.discord_id,
        &eingabe.idempotency_key,
        eingabe.submitted_at,
        eingabe.privacy_epoch,
    )
    .await?;

    if let Some((status, bisher_id)) = vorschlag_wiederholen_mit_executor(
        &mut *tx,
        &eingabe.idempotency_key,
        &login,
        &eingabe.discord_id,
        grund.as_deref(),
        eingabe.submitted_at,
        eingabe.privacy_epoch,
    )
    .await?
    {
        if bisher_id != user_id {
            return Err(VorschlagFehler::Konflikt);
        }
        return Ok(status);
    }

    // Historische Listen ohne Konto-ID brauchen zuerst eine eigene Auflösung.
    // Der Vorschlagspfad verändert weder ihre Identität noch Entscheidungen.
    let ungeklaert: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM (
                 SELECT twitch_login AS login, twitch_user_id AS id FROM twitch_partners
                 UNION ALL SELECT target_login, target_id FROM twitch_raid_blacklist
                 UNION ALL SELECT twitch_login, twitch_user_id FROM twitch_partner_signup_denylist
                 UNION ALL SELECT streamer_login, twitch_user_id FROM twitch_scout_pitch_blacklist
                 UNION ALL SELECT chatter_login, chatter_id FROM twitch_chatter_global_ban
                 UNION ALL SELECT target_login, target_id FROM twitch_outbound_chat_suppressions
                     WHERE source = 'recruitment' AND suppressed_until > NOW()
                 UNION ALL SELECT streamer_login, twitch_user_id FROM twitch_scout_candidates
                 UNION ALL SELECT streamer_login, streamer_user_id FROM twitch_partner_outreach
                     WHERE NULLIF(BTRIM(cooldown_until), '')::timestamptz > NOW()
             ) ids WHERE LOWER(login) = $1 AND NULLIF(BTRIM(id), '') IS NULL
         ) OR EXISTS (
             SELECT 1 FROM twitch_scout_candidates
              WHERE streamer_login = $1 AND twitch_user_id <> $2
         )",
    )
    .bind(&login)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    if ungeklaert {
        return Err(VorschlagFehler::IdentitaetUngeklaert);
    }

    let stand = listenstand(&mut tx, user_id).await?;
    let status = status_aus_listen(stand);

    sqlx::query(
        "INSERT INTO twitch_scout_community_suggestions
             (idempotency_key, twitch_user_id, twitch_login, suggested_by_discord_id, reason,
              result_status,submitted_at,privacy_epoch)
         VALUES ($1, $2, $3, $4, $5, $6,$7,$8)",
    )
    .bind(&eingabe.idempotency_key)
    .bind(user_id)
    .bind(&login)
    .bind(&eingabe.discord_id)
    .bind(grund.as_deref())
    .bind(status.as_str())
    .bind(eingabe.submitted_at)
    .bind(eingabe.privacy_epoch)
    .execute(&mut *tx)
    .await?;

    match status {
        VorschlagStatus::Created => {
            let stempel = naechster_stempel(&mut tx).await?;
            sqlx::query(
                "INSERT INTO twitch_scout_candidates
                     (streamer_login, twitch_user_id, status, source, suggested_by_discord_id,
                      suggestion_reason, suggested_at, suggestion_count, community_updated_at)
                 VALUES ($1, $2, $3, 'community', $4, $5, clock_timestamp(), 1, $6)",
            )
            .bind(&login)
            .bind(user_id)
            .bind(STATUS_VORGESCHLAGEN)
            .bind(&eingabe.discord_id)
            .bind(grund.as_deref())
            .bind(stempel)
            .execute(&mut *tx)
            .await?;
        }
        VorschlagStatus::AlreadyKnown => {
            // Zahl verschiedener Vorschlagender nachziehen; der erste
            // Vorschlagende bleibt stehen.
            let stempel = naechster_stempel(&mut tx).await?;
            sqlx::query(
                "UPDATE twitch_scout_candidates c
                    SET suggestion_count = n.anzahl,
                        community_updated_at = CASE WHEN c.source = 'community'
                                                    THEN $2 ELSE c.community_updated_at END
                   FROM (SELECT COUNT(DISTINCT suggested_by_discord_id)::int AS anzahl
                           FROM twitch_scout_community_suggestions
                          WHERE twitch_user_id = $1
                            AND result_status IN ('created', 'already_known')) n
                  WHERE c.twitch_user_id = $1
                    AND c.suggestion_count <> n.anzahl",
            )
            .bind(user_id)
            .bind(stempel)
            .execute(&mut *tx)
            .await?;
        }
        _ => {}
    }
    tx.commit().await?;
    Ok(status)
}

// ─── Ergebnisse für Paket C ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VorschlagErgebnis {
    pub twitch_user_id: String,
    pub twitch_login: String,
    /// Person dieser gültigen individuellen Vorschlagszuordnung.
    pub suggested_by_discord_id: String,
    pub suggested_at: Option<String>,
    pub submitted_at: Option<String>,
    pub privacy_epoch: Option<i64>,
    pub is_first_eligible: bool,
    pub suggestion_count: i32,
    /// Status in der Scout-Freigabe (`vorgeschlagen`, `approved`, …).
    pub candidate_status: String,
    pub is_partner_active: bool,
    /// Seit wann der Kanal aktiver Partner ist; `null`, solange nicht.
    pub partner_since: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErgebnisSeite {
    pub rows: Vec<VorschlagErgebnis>,
    pub next_updated_since: Option<String>,
    pub has_more: bool,
}

type StandZeile = (
    String,
    Option<DateTime<Utc>>,
    Option<i32>,
    Option<String>,
    Option<DateTime<Utc>>,
);

/// Gleicht den Partnerstand aller Community-Kandidaten mit
/// `twitch_streamers_partner_state` ab und stempelt geänderte Zeilen neu.
/// Liefert die Zahl geänderter Zeilen.
pub async fn aktualisiere_partnerstand(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    lock(&mut tx).await?;
    let zeilen: Vec<StandZeile> = sqlx::query_as(
        "SELECT c.streamer_login, c.partner_active_since, p.is_partner_active, p.created_at,
                clock_timestamp()
           FROM twitch_scout_candidates c
           LEFT JOIN LATERAL (
                SELECT ps.is_partner_active, ps.created_at
                  FROM twitch_streamers_partner_state ps
                 WHERE ps.twitch_user_id = c.twitch_user_id
                 ORDER BY ps.is_partner_active DESC
                 LIMIT 1) p ON TRUE
          WHERE c.source = 'community'
          ORDER BY c.streamer_login",
    )
    .fetch_all(&mut *tx)
    .await?;
    let mut geaendert = 0;
    for (login, bisher, aktiv, partnered_at, jetzt) in zeilen {
        let jetzt = jetzt.unwrap_or_else(Utc::now);
        let neu = partner_seit(aktiv == Some(1), bisher, partnered_at.as_deref(), jetzt);
        if neu == bisher {
            continue;
        }
        let stempel = naechster_stempel(&mut tx).await?;
        sqlx::query(
            "UPDATE twitch_scout_candidates
                SET partner_active_since = $2, community_updated_at = $3
              WHERE streamer_login = $1",
        )
        .bind(&login)
        .bind(neu)
        .bind(stempel)
        .execute(&mut *tx)
        .await?;
        geaendert += 1;
    }
    tx.commit().await?;
    Ok(geaendert)
}

#[derive(sqlx::FromRow)]
struct ErgebnisQuelle {
    id: i64,
    idempotency_key: String,
    twitch_user_id: String,
    streamer_login: String,
    suggested_by_discord_id: String,
    created_at: DateTime<Utc>,
    submitted_at: Option<DateTime<Utc>>,
    privacy_epoch: Option<i64>,
    suggestion_count: i32,
    status: String,
    partner_active_since: Option<DateTime<Utc>>,
    community_updated_at: DateTime<Utc>,
    outcome_updated_at: Option<DateTime<Utc>>,
}

/// Gültige individuelle Zuordnungen mit exklusivem, eindeutigem Cursor.
/// Die Erasure des Erstautors berührt keine fremde Vorschlagszuordnung.
pub async fn liste_ergebnisse(
    pool: &PgPool,
    since: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<ErgebnisSeite, sqlx::Error> {
    aktualisiere_partnerstand(pool).await?;
    let mut tx = pool.begin().await?;
    lock(&mut tx).await?;
    let quellen: Vec<ErgebnisQuelle> = sqlx::query_as(
        "SELECT s.id, s.idempotency_key, s.twitch_user_id, c.streamer_login,
                s.suggested_by_discord_id, s.created_at, s.submitted_at, s.privacy_epoch,
                c.suggestion_count, c.status, c.partner_active_since, c.community_updated_at,
                s.outcome_updated_at
           FROM twitch_scout_community_suggestions s
           JOIN twitch_scout_candidates c ON c.twitch_user_id=s.twitch_user_id
          WHERE c.source='community' AND c.community_updated_at IS NOT NULL
            AND s.result_status IN ('created','already_known')
          ORDER BY c.community_updated_at, COALESCE(s.submitted_at,s.created_at), s.id",
    )
    .fetch_all(&mut *tx)
    .await?;
    let mut paare = std::collections::HashSet::new();
    let mut gueltige = Vec::new();
    let mut erste_kanaele = std::collections::HashSet::new();
    for mut quelle in quellen {
        match crate::community_privacy::require_submission(
            &mut tx,
            &quelle.suggested_by_discord_id,
            &quelle.idempotency_key,
            quelle.submitted_at,
            quelle.privacy_epoch,
        )
        .await
        {
            Ok(()) => {}
            Err(VorschlagFehler::PrivacyGesperrt) => continue,
            Err(VorschlagFehler::Db(error)) => return Err(error),
            Err(error) => return Err(sqlx::Error::Protocol(format!("Outcomeprüfung: {error:?}"))),
        }
        if !paare.insert((
            quelle.twitch_user_id.clone(),
            quelle.suggested_by_discord_id.clone(),
        )) {
            continue;
        }
        if quelle
            .outcome_updated_at
            .is_none_or(|old| old < quelle.community_updated_at)
        {
            let stempel = naechster_stempel(&mut tx).await?;
            sqlx::query(
                "UPDATE twitch_scout_community_suggestions SET outcome_updated_at=$2 WHERE id=$1",
            )
            .bind(quelle.id)
            .bind(stempel)
            .execute(&mut *tx)
            .await?;
            quelle.outcome_updated_at = Some(stempel);
        }
        let is_first_eligible = erste_kanaele.insert(quelle.twitch_user_id.clone());
        gueltige.push((quelle, is_first_eligible));
    }
    gueltige.sort_by_key(|(row, _)| row.outcome_updated_at);
    let mut rows = Vec::new();
    let mut letzter = since;
    let mut has_more = false;
    for (z, is_first_eligible) in gueltige {
        let stempel = z
            .outcome_updated_at
            .expect("Gültige Zuordnung wurde gestempelt");
        if since.is_some_and(|since| stempel <= since) {
            continue;
        }
        if rows.len() >= limit as usize {
            has_more = true;
            break;
        }
        letzter = Some(stempel);
        rows.push(VorschlagErgebnis {
            twitch_user_id: z.twitch_user_id,
            twitch_login: z.streamer_login,
            suggested_by_discord_id: z.suggested_by_discord_id,
            suggested_at: Some(format_cursor(z.submitted_at.unwrap_or(z.created_at))),
            submitted_at: z.submitted_at.map(format_cursor),
            privacy_epoch: z.privacy_epoch,
            is_first_eligible,
            suggestion_count: z.suggestion_count,
            candidate_status: z.status,
            is_partner_active: z.partner_active_since.is_some(),
            partner_since: z.partner_active_since.map(format_cursor),
            updated_at: format_cursor(stempel),
        });
    }
    tx.commit().await?;
    Ok(ErgebnisSeite {
        rows,
        next_updated_since: letzter.map(format_cursor),
        has_more,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn status_aus_listen_reihenfolge() {
        let leer = Listenstand::default();
        assert_eq!(status_aus_listen(leer), VorschlagStatus::Created);
        assert_eq!(
            status_aus_listen(Listenstand {
                kandidat: true,
                ..leer
            }),
            VorschlagStatus::AlreadyKnown
        );
        assert_eq!(
            status_aus_listen(Listenstand {
                outreach_sperrfrist: true,
                ..leer
            }),
            VorschlagStatus::AlreadyKnown
        );
        assert_eq!(
            status_aus_listen(Listenstand {
                gesperrt: true,
                kandidat: true,
                ..leer
            }),
            VorschlagStatus::Blocked
        );
        assert_eq!(
            status_aus_listen(Listenstand {
                partner: true,
                gesperrt: true,
                kandidat: true,
                outreach_sperrfrist: true,
            }),
            VorschlagStatus::AlreadyPartner
        );
        for status in [
            VorschlagStatus::Created,
            VorschlagStatus::AlreadyKnown,
            VorschlagStatus::AlreadyPartner,
            VorschlagStatus::Blocked,
            VorschlagStatus::NotFound,
        ] {
            assert_eq!(VorschlagStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(VorschlagStatus::parse("pending"), None);
    }

    #[test]
    fn login_eingaben() {
        for (raw, login) in [
            ("Name_1", "name_1"),
            ("  @Name ", "name"),
            ("https://www.twitch.tv/Name", "name"),
            ("twitch.tv/name/videos", "name"),
            ("HTTPS://m.twitch.tv/name?x=1", "name"),
        ] {
            assert_eq!(
                normalisiere_vorschlag_login(raw).as_deref(),
                Some(login),
                "{raw}"
            );
        }
        for raw in [
            "",
            "@",
            "na me",
            "name-x",
            "https://youtube.com/name",
            "abcdefghijklmnopqrstuvwxyz",
            "twitch.tv/",
        ] {
            assert_eq!(normalisiere_vorschlag_login(raw), None, "{raw}");
        }
    }

    #[test]
    fn discord_id_schluessel_und_grund() {
        assert!(gueltige_discord_id("388772056717590539"));
        assert!(!gueltige_discord_id("0388772056717590539"));
        assert!(!gueltige_discord_id("1234"));
        assert!(!gueltige_discord_id("38877205671759053a"));
        assert!(gueltiger_idempotency_key("discord-suggestion-1"));
        assert!(!gueltiger_idempotency_key(""));
        assert!(!gueltiger_idempotency_key("mit leerzeichen"));
        assert!(!gueltiger_idempotency_key(&"x".repeat(129)));
        assert_eq!(normalisiere_grund(None), None);
        assert_eq!(normalisiere_grund(Some("  \n ")), None);
        assert_eq!(
            normalisiere_grund(Some(" Spielt\ngut ")).as_deref(),
            Some("Spielt gut")
        );
        assert_eq!(
            normalisiere_grund(Some(&"ä".repeat(600)))
                .unwrap()
                .chars()
                .count(),
            MAX_REASON_CHARS
        );
    }

    #[test]
    fn partnerzeit_und_partnerstand() {
        let erwartet = Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();
        for raw in [
            "2026-10-01T12:00:00Z",
            "2026-10-01T14:00:00+02:00",
            "2026-10-01 12:00:00",
            "2026-10-01 12:00:00+00",
            "2026-10-01 12:00:00.000+00:00",
        ] {
            assert_eq!(parse_partner_zeit(raw), Some(erwartet), "{raw}");
        }
        assert_eq!(parse_partner_zeit("gestern"), None);
        let jetzt = Utc.with_ymd_and_hms(2026, 10, 5, 0, 0, 0).unwrap();
        assert_eq!(partner_seit(false, Some(erwartet), None, jetzt), None);
        assert_eq!(
            partner_seit(true, Some(erwartet), Some("2026-10-03T00:00:00Z"), jetzt),
            Some(erwartet)
        );
        assert_eq!(
            partner_seit(true, None, Some("2026-10-01T12:00:00Z"), jetzt),
            Some(erwartet)
        );
        assert_eq!(partner_seit(true, None, Some("kaputt"), jetzt), Some(jetzt));
    }
}
