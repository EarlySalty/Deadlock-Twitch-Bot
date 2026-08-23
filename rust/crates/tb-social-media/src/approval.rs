//! Approval-Workflow (Port der DB-/State-Logik aus
//! `bot/social_media/approval/approval_service.py`).
//!
//! Zustandsmaschine je Clip in `social_media_clip_approval`
//! (awaiting_approval → approved/skipped/editing) + das Einreihen freigegebener
//! Plattformen in die Upload-Queue. Der Discord-DM-/UI-Teil (Approval-Buttons,
//! DM-Versand) ist **B10 (von Nani ausgeschlossen)** und nicht portiert.

use serde_json::{json, Value};
use sqlx::PgPool;

use crate::clip_queue::queue_upload;
use crate::enrichment::get_enrichment;
use crate::posting_plan;

pub const STATE_AWAITING: &str = "awaiting_approval";
pub const STATE_APPROVED: &str = "approved";
pub const STATE_SKIPPED: &str = "skipped";
pub const STATE_EDITING: &str = "editing";

pub const DECISION_APPROVE: &str = "approve";
pub const DECISION_SKIP: &str = "skip";
pub const DECISION_EDIT: &str = "edit";

pub const SUPPORTED_PLATFORMS: [&str; 3] = ["youtube", "tiktok", "instagram"];

#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    #[error("clip_db_id {0} not found")]
    ClipNotFound(i32),
    #[error("clip_db_id {0} is out of range for social_media_clip_approval.clip_db_id")]
    ClipIdOutOfRange(i64),
    #[error("at least one platform must be approved")]
    NoPlatform,
    /// Alle gewaehlten Plattformen stehen auf null Posts und sind damit
    /// ausgeschaltet. Die Freigabe wuerde nie stattfinden, deshalb wird sie
    /// gar nicht erst quittiert.
    ///
    /// Traegt bewusst nur einen stabilen Code plus die betroffenen
    /// Plattformen, wie die uebrigen Varianten auch. Der Satz fuer Menschen
    /// steht im Dashboard und laeuft dort durch die Uebersetzung; ein
    /// deutscher Fliesstext von hier stuende im englischen Dashboard auf
    /// Deutsch da.
    #[error("only_paused_platforms: {0}")]
    NurPausiertePlattformen(String),
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
}

/// Approval-Zustand eines Clips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRecord {
    pub clip_db_id: i32,
    pub state: String,
    pub approved_platforms: Vec<String>,
    pub approver_user_id: Option<String>,
    pub decided_at: Option<String>,
    pub dm_message_id: Option<String>,
    pub dm_channel_id: Option<String>,
    pub last_sent_at: Option<String>,
    /// Plattformen, die bei dieser Entscheidung ausgelassen wurden, weil ihre
    /// Kadenz auf null steht. Steht nur in der Antwort auf eine Entscheidung,
    /// nicht in der Datenbank: sonst quittiert die Oberflaeche eine Freigabe,
    /// die auf dieser Plattform nie stattfindet.
    pub nicht_eingeplant: Vec<String>,
}

/// Trimmt, kleinschreibt, dedupliziert, nur unterstützte Plattformen.
fn normalize_platforms<I: IntoIterator<Item = String>>(platforms: I) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for p in platforms {
        let p = p.trim().to_lowercase();
        if SUPPORTED_PLATFORMS.contains(&p.as_str()) && seen.insert(p.clone()) {
            out.push(p);
        }
    }
    out
}

/// Mappt Eingabe-Decision auf den kanonischen Wert.
///
/// Oeffentlich, weil ein Aufrufer wissen muss, ob seine Eingabe auf `approve`
/// hinauslaeuft, bevor er [`handle_decision`] ruft: nur `approve` reiht Uploads
/// ein, und unbekannte Eingaben fallen hier auf `approve` zurueck. Ein Gate, das
/// stattdessen den Rohwert mit `"approve"` vergleicht, waere umgehbar.
pub fn normalize_decision(decision: &str) -> &'static str {
    match decision.trim().to_lowercase().as_str() {
        "approve" | "approved" => DECISION_APPROVE,
        "skip" | "skipped" => DECISION_SKIP,
        "edit" | "editing" => DECISION_EDIT,
        _ => DECISION_APPROVE, // Python-Default
    }
}

fn decode_platforms(raw: Option<&str>) -> Vec<String> {
    let parsed: Vec<String> = raw
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default();
    normalize_platforms(parsed)
}

type Row = (
    i32,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn row_to_record(r: Row) -> ApprovalRecord {
    ApprovalRecord {
        clip_db_id: r.0,
        state: r.1.unwrap_or_else(|| STATE_AWAITING.to_string()),
        approved_platforms: decode_platforms(r.2.as_deref()),
        approver_user_id: r.3,
        decided_at: r.4,
        dm_message_id: r.5,
        dm_channel_id: r.6,
        last_sent_at: r.7,
        nicht_eingeplant: Vec::new(),
    }
}

const SELECT_SQL: &str = "SELECT clip_db_id, state, approved_platforms::text, approver_user_id, \
    decided_at::text, dm_message_id, dm_channel_id, last_sent_at::text \
    FROM social_media_clip_approval WHERE clip_db_id = $1 LIMIT 1";

/// Approval-Zeile eines Clips.
pub async fn get_approval_record(pool: &PgPool, clip_db_id: i32) -> Option<ApprovalRecord> {
    match fetch_approval_record(pool, clip_db_id).await {
        Ok(record) => record,
        Err(error) => {
            tracing::warn!(
                %error,
                clip_db_id,
                "Social-Media-Approval: Approval-Zeile nicht ladbar"
            );
            None
        }
    }
}

async fn fetch_approval_record(
    pool: &PgPool,
    clip_db_id: i32,
) -> Result<Option<ApprovalRecord>, sqlx::Error> {
    let row: Option<Row> = sqlx::query_as(SELECT_SQL)
        .bind(clip_db_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(row_to_record))
}

/// Stellt eine (awaiting-)Approval-Zeile sicher.
pub async fn ensure_approval_row(pool: &PgPool, clip_db_id: i32) -> ApprovalRecord {
    if let Some(r) = get_approval_record(pool, clip_db_id).await {
        return r;
    }
    if let Err(error) = sqlx::query!(
        "INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms) \
         VALUES ($1, $2, '[]'::jsonb) ON CONFLICT (clip_db_id) DO NOTHING",
        clip_db_id,
        STATE_AWAITING
    )
    .execute(pool)
    .await
    {
        tracing::warn!(
            %error,
            clip_db_id,
            "Social-Media-Approval: Awaiting-Zeile konnte nicht sichergestellt werden"
        );
    }
    get_approval_record(pool, clip_db_id)
        .await
        .unwrap_or(ApprovalRecord {
            clip_db_id,
            state: STATE_AWAITING.to_string(),
            approved_platforms: Vec::new(),
            approver_user_id: None,
            decided_at: None,
            dm_message_id: None,
            dm_channel_id: None,
            last_sent_at: None,
            nicht_eingeplant: Vec::new(),
        })
}

/// Setzt den Clip (zurück) auf „wartet auf Freigabe" (Python
/// `mark_clip_awaiting_approval`; ersetzt den Orchestrator-Stub).
pub async fn mark_clip_awaiting_approval(pool: &PgPool, clip_db_id: i32) {
    ensure_approval_row(pool, clip_db_id).await;
    if let Err(error) = sqlx::query!(
        "UPDATE social_media_clip_approval SET state = $1, approver_user_id = NULL, \
         decided_at = NULL, approved_platforms = '[]'::jsonb, dm_message_id = NULL, \
         dm_channel_id = NULL, last_sent_at = NULL WHERE clip_db_id = $2 AND state <> $1",
        STATE_AWAITING,
        clip_db_id
    )
    .execute(pool)
    .await
    {
        tracing::warn!(
            %error,
            clip_db_id,
            "Social-Media-Approval: Awaiting-Status konnte nicht gesetzt werden"
        );
    }
    if let Err(error) = sqlx::query!(
        "UPDATE twitch_clips_social_media SET status = $1 WHERE id = $2 \
         AND COALESCE(status, '') NOT IN ('published_all', 'published_partial', 'discarded')",
        STATE_AWAITING,
        clip_db_id as i64
    )
    .execute(pool)
    .await
    {
        tracing::warn!(
            %error,
            clip_db_id,
            "Social-Media-Approval: Clip-Status konnte nicht auf awaiting gesetzt werden"
        );
    }
}

/// Clips, die ohne Enrichment direkt in den Approval-Workflow gehoeren.
///
/// Die LLM-Anreicherung laeuft nur fuer Kategorien mit `enrichment_enabled`, und
/// erst an ihrem Ende landet ein Clip in `awaiting_approval`. Clips anderer
/// Kategorien wuerden sonst nie auftauchen; die holt diese Abfrage ab.
pub async fn iter_clips_ohne_enrichment(pool: &PgPool, limit: i64) -> Vec<i32> {
    sqlx::query_scalar!(
        "SELECT c.id::int AS \"id!\" FROM twitch_clips_social_media c \
         JOIN social_media_category k ON k.category_key = c.category_key \
         LEFT JOIN social_media_clip_approval a ON a.clip_db_id = c.id \
         WHERE c.discarded_at IS NULL \
           AND NOT k.enrichment_enabled \
           AND a.clip_db_id IS NULL \
           AND COALESCE(c.status, 'pending') = 'pending' \
           AND c.id BETWEEN 0 AND 2147483647 \
         ORDER BY c.created_at DESC LIMIT $1",
        limit.max(1)
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// Freigegebene Clips, bei denen mindestens eine freigegebene Plattform noch
/// keine Queue-Zeile hat.
///
/// Die Pruefung laeuft plattformgenau, weil das Einreihen plattformgenau
/// laeuft: `ensure_queued_uploads` legt je freigegebener Plattform eine eigene
/// Zeile an. Eine Pruefung pro Clip ("gibt es ueberhaupt eine Zeile") wirft
/// einen Clip dauerhaft aus dem Fenster, sobald eine einzige seiner
/// Plattformen einen Termin bekommen hat. Freigegeben fuer youtube und tiktok,
/// tiktok bekommt einen Termin, youtube laeuft in einen vollen Horizont: die
/// tiktok-Zeile allein reichte, damit youtube nie wieder versucht wird.
///
/// Ein vollstaendig eingereihter Clip faellt dagegen heraus und belegt keinen
/// der wenigen Batch-Plaetze.
///
/// # Reihenfolge: Dauergaeste duerfen niemanden blockieren
///
/// Die Reihenfolge ist nicht beliebig, sie entscheidet bei fester
/// Fenstergroesse darueber, wer verhungert. Es gibt Clips, die im Fenster
/// stehen, aber bei jedem Lauf nichts tun koennen:
///
/// * eine Plattform steht auf „schon hochgeladen" (`uploaded_youtube`), es
///   kann also nie eine Queue-Zeile entstehen (siehe `upload_already_exists`),
/// * der Planungshorizont der Plattform ist voll (`SlotPlan::HorizontVoll`),
///   was ausdruecklich ein Dauerzustand sein darf,
/// * die Freigabepruefung oder `queue_upload` scheitert wiederholt.
///
/// Nach reinem `decided_at ASC` stehen solche Clips fuer immer vorn. Zehn davon
/// fuellen das Zehnerfenster, und keine neue Freigabe wird je wieder
/// eingereiht: ohne Log, ohne Fehler, nur per Hand in der Datenbank
/// aufloesbar.
///
/// Deshalb sortiert die Abfrage zuerst nach `letzter_nachreih_versuch`, dem
/// Zeitpunkt des letzten Nachreih-Versuchs (`approval_worker::run_once` setzt
/// ihn nach jedem Versuch, siehe [`vermerke_nachreih_versuch`]). Das ergibt
/// eine Rotation statt einer Rangliste:
///
/// * `NULL` zuerst, also alles, was der Nachreih-Lauf noch nie angefasst hat.
///   Eine frische Freigabe kommt damit spaetestens im uebernaechsten Lauf
///   dran, auch wenn zehn Dauergaeste im Fenster stehen: die tragen nach dem
///   ersten Lauf alle einen juengeren Stempel als sie.
/// * Danach der aelteste Versuch zuerst, jeder Clip kommt also reihum wieder
///   an die Reihe.
/// * `decided_at ASC` bleibt als Tiebreak: bei gleichem Stand entscheidet
///   weiterhin die aeltere Freigabe, eine Warteschlange, die von hinten
///   abgearbeitet wird, ist keine Warteschlange.
///
/// # Was als „schon eingereiht" gilt
///
/// Gezaehlt wird jede Queue-Zeile, auch eine mit `status = 'failed'`.
/// `upload_already_exists` klammert `failed` aus, diese Abfrage tut es
/// bewusst nicht, und der Unterschied ist gewollt: `queue_upload` reiht eine
/// gescheiterte Zeile nicht neu ein, sondern legt eine zusaetzliche an
/// (`clip_queue::queue_upload` greift nur `pending` und `processing` wieder
/// auf). Wuerde der Nachreih-Lauf `failed` ueberspringen, bekaeme eine
/// dauerhaft scheiternde Plattform bei jedem Fehlschlag eine weitere Zeile,
/// endlos. Ein gescheiterter Upload wird deshalb nicht automatisch
/// wiederbelebt; der Weg zurueck ist die ausdrueckliche erneute Freigabe im
/// Dashboard, die ueber `handle_decision` laeuft und dort auf
/// `upload_already_exists` trifft.
pub async fn iter_approved_clips_pending_queue(pool: &PgPool, limit: i64) -> Vec<i32> {
    sqlx::query_scalar!(
        "SELECT a.clip_db_id AS \"clip_db_id!\" \
           FROM social_media_clip_approval a \
          WHERE a.state = $1 \
            AND EXISTS ( \
                  SELECT 1 \
                    FROM jsonb_array_elements_text( \
                           CASE WHEN jsonb_typeof(a.approved_platforms) = 'array' \
                                THEN a.approved_platforms \
                                ELSE '[]'::jsonb END) AS p(platform) \
                   WHERE NOT EXISTS ( \
                         SELECT 1 FROM twitch_clips_upload_queue q \
                          WHERE q.clip_id = a.clip_db_id::bigint \
                            AND q.platform = p.platform)) \
          ORDER BY a.letzter_nachreih_versuch ASC NULLS FIRST, \
                   a.decided_at ASC NULLS FIRST, a.clip_db_id ASC \
          LIMIT $2",
        STATE_APPROVED,
        limit.max(1)
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// Haelt fest, dass der Nachreih-Lauf diesen Clip gerade versucht hat.
///
/// Das ist die Gegenseite zur Sortierung in
/// [`iter_approved_clips_pending_queue`]: ohne diesen Stempel bliebe die Liste
/// eine Rangliste, in der ein Clip, der nie etwas tun kann, dauerhaft Platz
/// eins belegt. Mit dem Stempel wandert jeder gerade versuchte Clip ans Ende
/// und macht Platz fuer die, die noch nicht dran waren.
///
/// Bewusst nach dem Versuch gesetzt und nicht in `ensure_queued_uploads`
/// selbst: der Stempel beschreibt den Lauf des Workers, nicht die Freigabe.
/// Eine frische Freigabe aus dem Dashboard behaelt deshalb `NULL` und steht
/// beim naechsten Lauf ganz vorn.
///
/// Best effort: schlaegt das Schreiben fehl, verliert der Clip nur seinen
/// Platz in der Rotation, es geht nichts kaputt.
pub async fn vermerke_nachreih_versuch(pool: &PgPool, clip_db_id: i32) {
    if let Err(error) = sqlx::query!(
        "UPDATE social_media_clip_approval SET letzter_nachreih_versuch = now() \
         WHERE clip_db_id = $1",
        clip_db_id
    )
    .execute(pool)
    .await
    {
        tracing::warn!(
            %error,
            clip_db_id,
            "Social-Media-Approval: Nachreih-Versuch konnte nicht vermerkt werden"
        );
    }
}

/// `true`, wenn der Clip für diese Plattform freigegeben ist.
pub async fn is_clip_approved_for(
    pool: &PgPool,
    clip_db_id: i64,
    platform: &str,
) -> Result<bool, ApprovalError> {
    let approval_clip_db_id =
        i32::try_from(clip_db_id).map_err(|_| ApprovalError::ClipIdOutOfRange(clip_db_id))?;
    let platform = platform.trim().to_lowercase();
    if !SUPPORTED_PLATFORMS.contains(&platform.as_str()) {
        return Ok(false);
    }
    Ok(
        match fetch_approval_record(pool, approval_clip_db_id).await? {
            Some(r) if r.state == STATE_APPROVED => r.approved_platforms.contains(&platform),
            _ => false,
        },
    )
}

/// Serialisiert den Record fürs Dashboard.
pub fn serialize_approval_record(record: &ApprovalRecord) -> Value {
    json!({
        "clip_db_id": record.clip_db_id,
        "state": record.state,
        "approved_platforms": record.approved_platforms,
        "approver_user_id": record.approver_user_id,
        "decided_at": record.decided_at,
        "dm_message_id": record.dm_message_id,
        "dm_channel_id": record.dm_channel_id,
        "last_sent_at": record.last_sent_at,
        "not_scheduled": record.nicht_eingeplant,
    })
}

/// Kanal, dem dieser Clip gehoert.
async fn clip_streamer(pool: &PgPool, clip_db_id: i32) -> Option<String> {
    sqlx::query_scalar!(
        "SELECT streamer_login FROM twitch_clips_social_media WHERE id = $1",
        clip_db_id as i64
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

/// Plattformen, auf denen der Kanal dieses Clips automatisch postet.
async fn auto_platforms(pool: &PgPool, clip_db_id: i32) -> Vec<String> {
    let Some(streamer) = clip_streamer(pool, clip_db_id).await else {
        return Vec::new();
    };
    posting_plan::auto_post_platforms(pool, &streamer).await
}

/// Plant einen Clip ohne menschliche Sichtung ein, wenn der Freigabe-Modus des
/// Kanals und der Kategorie-Schalter das hergeben.
///
/// Wird am Ende der Enrichment-Pipeline aufgerufen. Im Modus `manual` passiert
/// hier nichts, der Clip bleibt in `awaiting_approval` liegen.
pub async fn auto_approve_if_allowed(pool: &PgPool, clip_db_id: i32) -> Vec<String> {
    if !posting_plan::auto_schedule_allowed(pool, i64::from(clip_db_id)).await {
        return Vec::new();
    }
    let platforms = auto_platforms(pool, clip_db_id).await;
    if platforms.is_empty() {
        return Vec::new();
    }
    match handle_decision(pool, clip_db_id, DECISION_APPROVE, &platforms, Some("auto")).await {
        Ok(record) => {
            tracing::info!(
                clip_db_id,
                platforms = ?record.approved_platforms,
                "Social-Media-Approval: Clip automatisch eingeplant"
            );
            record.approved_platforms
        }
        Err(error) => {
            tracing::warn!(
                %error,
                clip_db_id,
                "Social-Media-Approval: automatische Freigabe fehlgeschlagen"
            );
            Vec::new()
        }
    }
}

/// Verarbeitet eine Approval-Entscheidung (Python `handle_decision`): setzt
/// State + Plattformen + Clip-Status; bei approve werden die Uploads eingereiht.
pub async fn handle_decision(
    pool: &PgPool,
    clip_db_id: i32,
    decision: &str,
    approved_platforms: &[String],
    user_id: Option<&str>,
) -> Result<ApprovalRecord, ApprovalError> {
    if !clip_exists(pool, clip_db_id).await {
        return Err(ApprovalError::ClipNotFound(clip_db_id));
    }
    let decision = normalize_decision(decision);
    let selected = normalize_platforms(approved_platforms.iter().cloned());

    let mut nicht_eingeplant: Vec<String> = Vec::new();
    let (final_platforms, next_state) = match decision {
        DECISION_APPROVE => {
            // Wer ausdruecklich Plattformen waehlt, bekommt genau diese. Nur wenn
            // nichts gewaehlt wurde, greifen die Auto-Posting-Plattformen des
            // Kanals. Frueher wurden globale Flags immer dazugemischt, was die
            // Auswahl des Nutzers still ueberschrieb.
            let gewaehlt = if selected.is_empty() {
                normalize_platforms(auto_platforms(pool, clip_db_id).await)
            } else {
                selected
            };
            if gewaehlt.is_empty() {
                return Err(ApprovalError::NoPlatform);
            }
            // Eine Plattform mit Kadenz null ist ausgeschaltet. Wuerde sie in
            // `approved_platforms` landen, waere die Freigabe erfolgreich
            // quittiert und wuerde trotzdem nie stattfinden: es entsteht keine
            // Queue-Zeile, und der Clip bliebe als "freigegeben" liegen, ohne
            // dass es irgendwo sichtbar waere.
            let (eingeplant, pausiert) = teile_pausierte_ab(pool, clip_db_id, gewaehlt).await;
            if eingeplant.is_empty() {
                return Err(ApprovalError::NurPausiertePlattformen(pausiert.join(", ")));
            }
            nicht_eingeplant = pausiert;
            (eingeplant, STATE_APPROVED)
        }
        DECISION_SKIP => (Vec::new(), STATE_SKIPPED),
        _ => (selected, STATE_EDITING),
    };

    ensure_approval_row(pool, clip_db_id).await;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query!(
        "UPDATE social_media_clip_approval SET state = $1, approved_platforms = $2::text::jsonb, \
         approver_user_id = $3, decided_at = $4::text::timestamptz WHERE clip_db_id = $5",
        next_state,
        serde_json::to_string(&final_platforms).unwrap_or_else(|_| "[]".to_string()),
        user_id.map(str::trim).filter(|s| !s.is_empty()),
        &now,
        clip_db_id
    )
    .execute(pool)
    .await?;
    sqlx::query!(
        "UPDATE twitch_clips_social_media SET status = $1 WHERE id = $2",
        next_state,
        clip_db_id as i64
    )
    .execute(pool)
    .await?;

    if decision == DECISION_APPROVE {
        ensure_queued_uploads(pool, clip_db_id).await;
    }
    let mut record = ensure_approval_row(pool, clip_db_id).await;
    record.nicht_eingeplant = nicht_eingeplant;
    Ok(record)
}

/// Trennt die gewaehlten Plattformen in "kann eingeplant werden" und "steht auf
/// null Posts".
///
/// Ohne Kanal gibt es keine Kadenz, an die man sich halten koennte; dann bleibt
/// alles wie gewaehlt.
async fn teile_pausierte_ab(
    pool: &PgPool,
    clip_db_id: i32,
    gewaehlt: Vec<String>,
) -> (Vec<String>, Vec<String>) {
    let Some(streamer) = clip_streamer(pool, clip_db_id).await else {
        return (gewaehlt, Vec::new());
    };
    let pausierte = posting_plan::pausierte_plattformen(pool, &streamer).await;
    let (pausiert, eingeplant): (Vec<String>, Vec<String>) = gewaehlt
        .into_iter()
        .partition(|platform| pausierte.contains(platform));
    (eingeplant, pausiert)
}

/// Reiht die freigegebenen Plattformen eines Clips in die Upload-Queue
/// (Python `ensure_queued_uploads`). Nutzt die Enrichment-Texte je Plattform.
///
/// Die Schleife laeuft je Plattform, und jeder ihrer Abbruchgruende
/// (Freigabepruefung gescheitert, Plattform schon hochgeladen, voller
/// Planungshorizont, `queue_upload` gescheitert) laesst genau diese eine
/// Plattform ohne Zeile stehen, waehrend die Schwesterplattformen ihre Zeile
/// bekommen. Das ist gewollt: der Clip bleibt in der Nachreih-Liste, weil
/// `iter_approved_clips_pending_queue` plattformgenau prueft, und wird beim
/// naechsten Lauf erneut versucht.
///
/// Damit das nicht zur Blockade wird, wenn eine Plattform dauerhaft nichts tun
/// kann (Upload-Flag steht schon, Planungshorizont bleibt voll), sortiert der
/// Nachreih-Lauf nach dem letzten Versuch und nicht nach der Entscheidung.
/// Siehe [`iter_approved_clips_pending_queue`] und
/// [`vermerke_nachreih_versuch`].
pub async fn ensure_queued_uploads(pool: &PgPool, clip_db_id: i32) -> Vec<(String, i64)> {
    if !clip_exists(pool, clip_db_id).await {
        return Vec::new();
    }
    let record = ensure_approval_row(pool, clip_db_id).await;
    if record.state != STATE_APPROVED {
        return Vec::new();
    }
    let enrichment = get_enrichment(pool, clip_db_id).await;
    let streamer = clip_streamer(pool, clip_db_id).await;
    let now = chrono::Utc::now();
    let mut queued = Vec::new();
    // Plattformen, die der Kanal nach der Freigabe auf null gestellt hat.
    let mut nachtraeglich_pausiert: Vec<String> = Vec::new();
    for platform in &record.approved_platforms {
        match is_clip_approved_for(pool, i64::from(clip_db_id), platform).await {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                tracing::warn!(clip_db_id, platform = %platform, %e, "approval check failed while queuing uploads");
                continue;
            }
        }
        if upload_already_exists(pool, clip_db_id, platform).await {
            continue;
        }
        let (title, description, hashtags) =
            enrichment
                .as_ref()
                .map_or((None, None, Vec::new()), |e| match platform.as_str() {
                    "youtube" => (
                        e.title_youtube.clone(),
                        e.description_youtube.clone(),
                        e.hashtags_youtube.clone(),
                    ),
                    "tiktok" => (
                        e.title_tiktok.clone(),
                        e.description_tiktok.clone(),
                        e.hashtags_tiktok.clone(),
                    ),
                    _ => (
                        e.title_instagram.clone(),
                        e.description_instagram.clone(),
                        e.hashtags_instagram.clone(),
                    ),
                });
        // Freigegeben heisst ab hier nicht mehr „sofort raus": der Clip bekommt
        // den naechsten Termin aus der Kadenz des Kanals.
        //
        // Ein leeres `scheduled_at` heisst in `get_upload_queue` "sofort
        // faellig". Genau deshalb darf hier nicht jedes "kein Termin" zu einem
        // leeren Feld werden, sonst kippt "die Plattform postet nie" in "die
        // Plattform postet sofort", also ins Gegenteil der Einstellung.
        //
        // Entschieden ist: nicht einreihen statt Termin in ferner Zukunft. Ein
        // Eintrag mit einem nie erreichten Termin haengt dauerhaft in der
        // Warteschlange, blockiert `upload_already_exists` und belegt in
        // `belegte_termine` einen Slot. Ohne Zeile bleibt der Clip dagegen in
        // `iter_approved_clips_pending_queue` stehen, weil diese Abfrage
        // plattformgenau prueft: schon eine einzige freigegebene Plattform ohne
        // Queue-Zeile haelt den Clip im Fenster, auch wenn die
        // Schwesterplattformen laengst ihre Zeile haben. Er wird also bei jedem
        // Lauf erneut versucht, sobald die Kadenz wieder etwas hergibt.
        //
        // Ohne Kanal bleibt es beim alten Verhalten: dort gibt es keine Kadenz,
        // an die man sich halten koennte.
        let scheduled_at = match &streamer {
            Some(login) => match posting_plan::plan_next_slot(pool, login, platform, now).await {
                posting_plan::SlotPlan::Termin(slot) => Some(slot.to_rfc3339()),
                posting_plan::SlotPlan::Ausgeschaltet => {
                    // Die Plattform wurde nach der Freigabe abgeschaltet.
                    // `handle_decision` laesst sie gar nicht erst zu, hier
                    // bleibt nur der nachtraegliche Fall. Die Freigabe gilt
                    // dort nicht mehr, sie wird deshalb weiter unten aus
                    // `approved_platforms` entfernt statt endlos wiederholt.
                    nachtraeglich_pausiert.push(platform.clone());
                    continue;
                }
                posting_plan::SlotPlan::HorizontVoll => {
                    tracing::warn!(
                        clip_db_id,
                        platform = %platform,
                        streamer = %login,
                        "Social-Media-Approval: kein freier Termin im Planungshorizont, \
                         Freigabe bleibt offen und wird spaeter erneut versucht"
                    );
                    continue;
                }
            },
            None => None,
        };
        match queue_upload(
            pool,
            clip_db_id,
            platform,
            title.as_deref(),
            description.as_deref(),
            Some(&hashtags),
            scheduled_at.as_deref(),
            0,
        )
        .await
        {
            Ok(queue_id) => queued.push((platform.clone(), queue_id)),
            Err(error) => {
                tracing::warn!(
                    %error,
                    clip_db_id,
                    platform = %platform,
                    "Social-Media-Approval: Upload konnte nicht eingereiht werden"
                );
            }
        }
    }
    if !nachtraeglich_pausiert.is_empty() {
        entferne_pausierte_freigaben(pool, clip_db_id, &record, &nachtraeglich_pausiert).await;
    }
    queued
}

/// Nimmt abgeschaltete Plattformen aus der Freigabe heraus.
///
/// Bleibt danach keine Plattform uebrig, geht der Clip zurueck auf „wartet auf
/// Freigabe". Sonst stuende er dauerhaft als freigegeben ohne Queue-Zeile da
/// und wuerde in `iter_approved_clips_pending_queue` bei jedem Lauf einen der
/// wenigen Batch-Plaetze belegen, ohne dass je etwas passieren kann.
async fn entferne_pausierte_freigaben(
    pool: &PgPool,
    clip_db_id: i32,
    record: &ApprovalRecord,
    pausiert: &[String],
) {
    let verbleibend: Vec<String> = record
        .approved_platforms
        .iter()
        .filter(|platform| !pausiert.contains(platform))
        .cloned()
        .collect();
    tracing::info!(
        clip_db_id,
        pausiert = ?pausiert,
        verbleibend = ?verbleibend,
        "Social-Media-Approval: abgeschaltete Plattform aus der Freigabe entfernt"
    );
    if let Err(error) = sqlx::query(
        "UPDATE social_media_clip_approval SET approved_platforms = $1::text::jsonb \
         WHERE clip_db_id = $2",
    )
    .bind(serde_json::to_string(&verbleibend).unwrap_or_else(|_| "[]".to_string()))
    .bind(clip_db_id)
    .execute(pool)
    .await
    {
        tracing::warn!(
            %error,
            clip_db_id,
            "Social-Media-Approval: Freigabe konnte nicht bereinigt werden"
        );
        return;
    }
    if verbleibend.is_empty() {
        mark_clip_awaiting_approval(pool, clip_db_id).await;
    }
}

/// Was ein Abbruch tatsaechlich erreicht hat.
///
/// `cancelled` sind die Queue-Zeilen, die noch unangetastet waren und geloescht
/// wurden. `already_running` sind die Zeilen, die schon laufen oder durch sind:
/// die bleiben stehen, damit die Oberflaeche sagen kann, dass eine Plattform
/// schon raus ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CancelOutcome {
    pub cancelled: i64,
    pub already_running: i64,
}

/// Stoppt die eingeplanten, noch nicht angefassten Uploads eines Clips und
/// setzt ihn zurueck auf „wartet auf Freigabe".
///
/// Das ist die Gegenrichtung zu [`ensure_queued_uploads`] und macht den Modus
/// `veto_window` erst ehrlich: eingeplant heisst dort, dass man bis zum Termin
/// noch eingreifen kann.
///
/// Angefasst wird nur, was noch `pending` ist. Eine Zeile in `processing` oder
/// `completed` bleibt unberuehrt und wird nur gezaehlt; ein laufender oder
/// fertiger Upload laesst sich nicht mehr zurueckholen.
///
/// Der Clip-Status wird wie in [`mark_clip_awaiting_approval`] nur gesetzt,
/// solange er nicht schon in einem Endzustand steht (`published_all`,
/// `published_partial`, `discarded`); ein bereits veroeffentlichter Clip soll
/// durch einen ins Leere laufenden Abbruch nicht wieder als offen erscheinen.
pub async fn cancel_scheduled_uploads(
    pool: &PgPool,
    clip_db_id: i32,
) -> Result<CancelOutcome, ApprovalError> {
    if !clip_exists(pool, clip_db_id).await {
        return Err(ApprovalError::ClipNotFound(clip_db_id));
    }
    let mut tx = pool.begin().await?;

    let already_running: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_clips_upload_queue \
         WHERE clip_id = $1 AND status IN ('processing', 'completed')",
    )
    .bind(i64::from(clip_db_id))
    .fetch_one(&mut *tx)
    .await?;

    let cancelled = sqlx::query(
        "DELETE FROM twitch_clips_upload_queue WHERE clip_id = $1 AND status = 'pending'",
    )
    .bind(i64::from(clip_db_id))
    .execute(&mut *tx)
    .await?
    .rows_affected();

    // Approval-Zeile zurueck auf „wartet auf Freigabe": die alte Entscheidung
    // ist mit dem Abbruch hinfaellig, die Plattform-Auswahl also auch. Die
    // Discord-DM-Verweise bleiben stehen, die DM existiert weiterhin.
    sqlx::query(
        "INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms) \
         VALUES ($1, $2, '[]'::jsonb) \
         ON CONFLICT (clip_db_id) DO UPDATE SET state = EXCLUDED.state, \
             approved_platforms = EXCLUDED.approved_platforms, approver_user_id = NULL, \
             decided_at = NULL",
    )
    .bind(clip_db_id)
    .bind(STATE_AWAITING)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE twitch_clips_social_media SET status = $1 WHERE id = $2 \
         AND COALESCE(status, '') NOT IN ('published_all', 'published_partial', 'discarded')",
    )
    .bind(STATE_AWAITING)
    .bind(i64::from(clip_db_id))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(CancelOutcome {
        cancelled: cancelled as i64,
        already_running,
    })
}

async fn clip_exists(pool: &PgPool, clip_db_id: i32) -> bool {
    sqlx::query_scalar!(
        "SELECT 1 AS \"exists!\" FROM twitch_clips_social_media WHERE id = $1",
        clip_db_id as i64
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .is_some()
}

/// `true`, wenn die Plattform schon hochgeladen ODER eine nicht-failed
/// Queue-Zeile existiert (Python `_upload_already_exists`).
///
/// `failed` bleibt hier bewusst aussen vor, in
/// [`iter_approved_clips_pending_queue`] dagegen nicht. Der Unterschied ist
/// gewollt und nicht symmetrisch zu lesen: eine gescheiterte Zeile darf nur
/// durch eine ausdrueckliche erneute Freigabe im Dashboard neu eingereiht
/// werden, nie durch den automatischen Nachreih-Lauf. `queue_upload` greift
/// eine `failed`-Zeile naemlich nicht wieder auf, sondern legt eine
/// zusaetzliche an; ein automatischer Nachreih-Lauf wuerde einer dauerhaft
/// scheiternden Plattform deshalb bei jedem Fehlschlag eine weitere Zeile
/// verpassen, endlos.
async fn upload_already_exists(pool: &PgPool, clip_db_id: i32, platform: &str) -> bool {
    let column = match platform {
        "youtube" => "uploaded_youtube",
        "tiktok" => "uploaded_tiktok",
        "instagram" => "uploaded_instagram",
        _ => return true,
    };
    let row: Option<(Option<bool>, bool)> = sqlx::query_as(&format!(
        "SELECT {column}, EXISTS(SELECT 1 FROM twitch_clips_upload_queue \
         WHERE clip_id = $1 AND platform = $2 AND status <> 'failed') \
         FROM twitch_clips_social_media WHERE id = $3 LIMIT 1"
    ))
    .bind(clip_db_id as i64)
    .bind(platform)
    .bind(clip_db_id as i64)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    match row {
        Some((uploaded, has_queue)) => uploaded.unwrap_or(false) || has_queue,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    use crate::test_support::test_dsn;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = test_dsn()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
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
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, status TEXT DEFAULT 'pending', uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE)",
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, decided_at TIMESTAMPTZ, dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ, letzter_nachreih_versuch TIMESTAMPTZ)",
            "CREATE TABLE twitch_clips_upload_queue (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TIMESTAMPTZ, attempts INTEGER DEFAULT 0, last_error TEXT, last_attempt_at TIMESTAMPTZ, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, completed_at TIMESTAMPTZ)",
            "CREATE TABLE social_media_settings (key TEXT PRIMARY KEY, value JSONB, updated_at TIMESTAMPTZ, updated_by TEXT)",
            "CREATE TABLE social_media_clip_enrichment (clip_db_id INTEGER PRIMARY KEY, transcript_raw TEXT, transcript_corrected TEXT, transcript_segments JSONB, transcript_lang TEXT, detected_terms JSONB DEFAULT '[]'::jsonb, title_youtube TEXT, title_tiktok TEXT, title_instagram TEXT, description_youtube TEXT, description_tiktok TEXT, description_instagram TEXT, hashtags_youtube JSONB DEFAULT '[]'::jsonb, hashtags_tiktok JSONB DEFAULT '[]'::jsonb, hashtags_instagram JSONB DEFAULT '[]'::jsonb, llm_provider TEXT, llm_model TEXT, cost_usd_estimate NUMERIC(10,6), status TEXT DEFAULT 'pending', error_message TEXT, started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, edited_by TEXT, updated_at TIMESTAMPTZ DEFAULT NOW())",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    async fn seed_clip(pool: &PgPool) -> i32 {
        sqlx::query_scalar("INSERT INTO twitch_clips_social_media DEFAULT VALUES RETURNING id")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn approve_setzt_state_und_queued_uploads() {
        let Some(pool) = make_pool("t_sm_approval").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        sqlx::query("INSERT INTO social_media_clip_enrichment (clip_db_id, title_youtube, hashtags_youtube) VALUES ($1, 'YT-Titel', '[\"#deadlock\"]'::jsonb)").bind(clip).execute(&pool).await.unwrap();

        let rec = handle_decision(
            &pool,
            clip,
            "approve",
            &["youtube".to_string()],
            Some("admin"),
        )
        .await
        .unwrap();
        assert_eq!(rec.state, "approved");
        assert_eq!(rec.approved_platforms, vec!["youtube".to_string()]);
        assert_eq!(rec.approver_user_id.as_deref(), Some("admin"));
        // Clip-Status approved.
        let cstatus: String =
            sqlx::query_scalar("SELECT status FROM twitch_clips_social_media WHERE id = $1")
                .bind(clip)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cstatus, "approved");
        // Upload eingereiht mit Enrichment-Titel.
        let (platform, title): (String, Option<String>) = sqlx::query_as(
            "SELECT platform, title FROM twitch_clips_upload_queue WHERE clip_id = $1",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(platform, "youtube");
        assert_eq!(title.as_deref(), Some("YT-Titel"));
        assert!(is_clip_approved_for(&pool, i64::from(clip), "youtube")
            .await
            .unwrap());
        assert!(!is_clip_approved_for(&pool, i64::from(clip), "tiktok")
            .await
            .unwrap());

        // ensure_queued_uploads erneut → idempotent (kein zweiter Job).
        ensure_queued_uploads(&pool, clip).await;
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = $1")
                .bind(clip)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n, 1);

        // iter_approved liefert nur Freigaben, die noch keine Queue-Zeile
        // haben. Dieser Clip ist vollstaendig eingereiht und gehoert deshalb
        // nicht mehr ins Batch-Fenster.
        assert!(iter_approved_clips_pending_queue(&pool, 10)
            .await
            .is_empty());

        // Ohne Queue-Zeile taucht dieselbe Freigabe wieder auf.
        sqlx::query("DELETE FROM twitch_clips_upload_queue WHERE clip_id = $1")
            .bind(clip)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            iter_approved_clips_pending_queue(&pool, 10).await,
            vec![clip]
        );
    }

    #[tokio::test]
    async fn skip_und_approve_ohne_plattform() {
        let Some(pool) = make_pool("t_sm_approval_skip").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        let rec = handle_decision(&pool, clip, "skip", &[], None)
            .await
            .unwrap();
        assert_eq!(rec.state, "skipped");
        assert!(rec.approved_platforms.is_empty());
        // approve ohne ausgewählte Plattform + ohne Auto-Approve → Fehler.
        assert!(matches!(
            handle_decision(&pool, clip, "approve", &[], None).await,
            Err(ApprovalError::NoPlatform)
        ));
        // Unbekannter Clip → ClipNotFound.
        assert!(matches!(
            handle_decision(&pool, 999, "approve", &["youtube".to_string()], None).await,
            Err(ApprovalError::ClipNotFound(999))
        ));
    }

    #[tokio::test]
    async fn mark_awaiting_und_record() {
        let Some(pool) = make_pool("t_sm_approval_mark").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        handle_decision(&pool, clip, "approve", &["tiktok".to_string()], None)
            .await
            .unwrap();
        // mark_awaiting setzt zurück.
        mark_clip_awaiting_approval(&pool, clip).await;
        let rec = get_approval_record(&pool, clip).await.unwrap();
        assert_eq!(rec.state, "awaiting_approval");
        assert!(rec.approved_platforms.is_empty());
        let cstatus: String =
            sqlx::query_scalar("SELECT status FROM twitch_clips_social_media WHERE id = $1")
                .bind(clip)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cstatus, "awaiting_approval");
        // serialize.
        let v = serialize_approval_record(&rec);
        assert_eq!(v["state"], "awaiting_approval");
    }

    #[tokio::test]
    async fn abbruch_raeumt_rein_wartende_uploads_ab() {
        let Some(pool) = make_pool("t_sm_approval_cancel").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        handle_decision(
            &pool,
            clip,
            "approve",
            &["youtube".to_string(), "tiktok".to_string()],
            Some("admin"),
        )
        .await
        .unwrap();
        let offen: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = $1 AND status = 'pending'",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(offen, 2, "zwei eingeplante Uploads erwartet");

        let outcome = cancel_scheduled_uploads(&pool, clip).await.unwrap();
        assert_eq!(outcome.cancelled, 2);
        assert_eq!(outcome.already_running, 0);

        let rest: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = $1")
                .bind(clip)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(rest, 0, "Queue muss leer sein");

        let rec = get_approval_record(&pool, clip).await.unwrap();
        assert_eq!(rec.state, STATE_AWAITING);
        assert!(rec.approved_platforms.is_empty());
        assert!(rec.approver_user_id.is_none());
        let cstatus: String =
            sqlx::query_scalar("SELECT status FROM twitch_clips_social_media WHERE id = $1")
                .bind(clip)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cstatus, STATE_AWAITING);
    }

    #[tokio::test]
    async fn abbruch_laesst_laufenden_upload_stehen_und_meldet_ihn() {
        let Some(pool) = make_pool("t_sm_approval_cancel_run").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        handle_decision(
            &pool,
            clip,
            "approve",
            &["youtube".to_string(), "tiktok".to_string()],
            None,
        )
        .await
        .unwrap();
        // Eine Plattform ist schon unterwegs.
        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = 'processing' \
             WHERE clip_id = $1 AND platform = 'youtube'",
        )
        .bind(clip)
        .execute(&pool)
        .await
        .unwrap();

        let outcome = cancel_scheduled_uploads(&pool, clip).await.unwrap();
        assert_eq!(outcome.cancelled, 1, "nur der wartende Upload faellt weg");
        assert_eq!(outcome.already_running, 1);

        let rest: Vec<(String, String)> = sqlx::query_as(
            "SELECT platform, status FROM twitch_clips_upload_queue WHERE clip_id = $1",
        )
        .bind(clip)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rest,
            vec![("youtube".to_string(), "processing".to_string())]
        );
    }

    #[tokio::test]
    async fn abbruch_unbekannter_clip_ist_clip_not_found() {
        let Some(pool) = make_pool("t_sm_approval_cancel_404").await else {
            return;
        };
        assert!(matches!(
            cancel_scheduled_uploads(&pool, 4242).await,
            Err(ApprovalError::ClipNotFound(4242))
        ));
    }

    /// Ruestet einem Test-Schema die Zeitplan-Tabellen nach, damit
    /// `plan_next_slot` etwas zu lesen hat.
    ///
    /// Das DDL ist von Hand aus `20260815120000_social_media_scheduling.sql`
    /// uebernommen und nicht daraus gezogen. Die Migration ist durchgehend auf
    /// `public.` verdrahtet und haengt an `twitch_streamers`,
    /// `social_media_partner_access` und `social_media_settings`; sie laesst
    /// sich nicht in ein isoliertes Test-Schema ausfuehren, ohne sie
    /// umzuschreiben. Der Preis ist bekannt: driftet eine Spalte, faellt es
    /// hier nicht auf. Dagegen steht der Snapshot-Vertrag in
    /// `tb-db/tests/fresh_migrations_schema.rs`, der das echte Schema gegen
    /// eine eingecheckte Datei prueft.
    async fn zeitplan_tabellen(pool: &PgPool) {
        for ddl in [
            "ALTER TABLE twitch_clips_social_media ADD COLUMN streamer_login TEXT",
            "CREATE TABLE social_media_streamer_settings (streamer_login TEXT PRIMARY KEY, \
             approval_mode TEXT NOT NULL DEFAULT 'manual', \
             timezone TEXT NOT NULL DEFAULT 'Europe/Berlin', \
             updated_at TIMESTAMPTZ DEFAULT NOW(), updated_by TEXT)",
            "CREATE TABLE social_media_platform_schedule (streamer_login TEXT NOT NULL, \
             platform TEXT NOT NULL, auto_post BOOLEAN NOT NULL DEFAULT FALSE, \
             posts_per_week INTEGER NOT NULL DEFAULT 4, \
             max_posts_per_day INTEGER NOT NULL DEFAULT 1, \
             post_times JSONB NOT NULL DEFAULT '[\"18:00\"]'::jsonb, \
             updated_at TIMESTAMPTZ DEFAULT NOW(), updated_by TEXT, \
             PRIMARY KEY (streamer_login, platform))",
        ] {
            sqlx::query(ddl).execute(pool).await.unwrap();
        }
    }

    /// Eine Plattform mit Kadenz null ist ausgeschaltet. Eine Freigabe darauf
    /// darf keinen Upload ausloesen; frueher landete sie mit leerem
    /// `scheduled_at` in der Warteschlange und ging damit sofort raus.
    #[tokio::test]
    async fn freigabe_auf_pausierte_plattform_reiht_nichts_ein() {
        let Some(pool) = make_pool("t_sm_approval_kadenz_null").await else {
            return;
        };
        zeitplan_tabellen(&pool).await;

        let clip: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('kanal') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // youtube steht auf null Posts pro Woche, tiktok auf normaler Kadenz.
        sqlx::query(
            "INSERT INTO social_media_platform_schedule \
                 (streamer_login, platform, auto_post, posts_per_week, max_posts_per_day, post_times) \
             VALUES ('kanal', 'youtube', TRUE, 0, 1, '[\"18:00\"]'::jsonb), \
                    ('kanal', 'tiktok', TRUE, 4, 1, '[\"18:00\"]'::jsonb)",
        )
        .execute(&pool)
        .await
        .unwrap();

        handle_decision(
            &pool,
            clip,
            "approve",
            &["youtube".to_string(), "tiktok".to_string()],
            Some("admin"),
        )
        .await
        .unwrap();

        let zeilen: Vec<(String, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
            "SELECT platform, scheduled_at FROM twitch_clips_upload_queue WHERE clip_id = $1",
        )
        .bind(clip)
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(
            zeilen.len(),
            1,
            "nur die aktive Plattform darf eingereiht werden, war: {zeilen:?}"
        );
        assert_eq!(zeilen[0].0, "tiktok");
        assert!(
            zeilen[0].1.is_some(),
            "die aktive Plattform bekommt einen echten Termin statt sofort"
        );
    }

    /// Eine Freigabe ausschliesslich auf eine pausierte Plattform darf nicht
    /// erfolgreich quittiert werden. Sonst meldet das Dashboard "freigegeben",
    /// und passieren wird nie etwas.
    #[tokio::test]
    async fn freigabe_nur_auf_pausierte_plattform_wird_abgelehnt() {
        let Some(pool) = make_pool("t_sm_approval_nur_pausiert").await else {
            return;
        };
        zeitplan_tabellen(&pool).await;
        let clip: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('kanal') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_platform_schedule \
                 (streamer_login, platform, auto_post, posts_per_week, max_posts_per_day, post_times) \
             VALUES ('kanal', 'youtube', TRUE, 0, 1, '[\"18:00\"]'::jsonb)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let fehler = handle_decision(
            &pool,
            clip,
            "approve",
            &["youtube".to_string()],
            Some("admin"),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&fehler, ApprovalError::NurPausiertePlattformen(p) if p == "youtube"),
            "war: {fehler:?}"
        );

        let record = ensure_approval_row(&pool, clip).await;
        assert_eq!(
            record.state, STATE_AWAITING,
            "eine abgelehnte Entscheidung darf den Clip nicht auf freigegeben setzen"
        );
        let offen: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = $1")
                .bind(clip)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(offen, 0);
    }

    /// Wird nur ein Teil der Auswahl pausiert, laeuft die Freigabe durch, die
    /// Quittung nennt die ausgelassene Plattform aber ausdruecklich.
    #[tokio::test]
    async fn quittung_nennt_die_pausierte_plattform() {
        let Some(pool) = make_pool("t_sm_approval_quittung").await else {
            return;
        };
        zeitplan_tabellen(&pool).await;
        let clip: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('kanal') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_platform_schedule \
                 (streamer_login, platform, auto_post, posts_per_week, max_posts_per_day, post_times) \
             VALUES ('kanal', 'youtube', TRUE, 0, 1, '[\"18:00\"]'::jsonb), \
                    ('kanal', 'tiktok', TRUE, 4, 1, '[\"18:00\"]'::jsonb)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let record = handle_decision(
            &pool,
            clip,
            "approve",
            &["youtube".to_string(), "tiktok".to_string()],
            Some("admin"),
        )
        .await
        .unwrap();
        assert_eq!(record.approved_platforms, vec!["tiktok".to_string()]);
        assert_eq!(record.nicht_eingeplant, vec!["youtube".to_string()]);
        assert_eq!(
            serialize_approval_record(&record)["not_scheduled"],
            serde_json::json!(["youtube"])
        );
    }

    /// Wird eine Plattform nach der Freigabe abgeschaltet, faellt sie aus der
    /// Freigabe heraus. Bleibt nichts uebrig, geht der Clip zurueck auf
    /// „wartet auf Freigabe" statt dauerhaft als freigegeben ohne Queue-Zeile
    /// im Batch-Fenster zu haengen.
    #[tokio::test]
    async fn nachtraeglich_abgeschaltete_plattform_faellt_aus_der_freigabe() {
        let Some(pool) = make_pool("t_sm_approval_nachtraeglich_aus").await else {
            return;
        };
        zeitplan_tabellen(&pool).await;
        let clip: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login, status) \
             VALUES ('kanal', 'approved') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        // Die Freigabe steht schon, die Plattform wird erst danach auf null
        // gestellt. Diesen Weg kann `handle_decision` nicht abfangen.
        sqlx::query(
            "INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms, decided_at) \
             VALUES ($1, 'approved', '[\"youtube\"]'::jsonb, now())",
        )
        .bind(clip)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_platform_schedule \
                 (streamer_login, platform, auto_post, posts_per_week, max_posts_per_day, post_times) \
             VALUES ('kanal', 'youtube', TRUE, 0, 1, '[\"18:00\"]'::jsonb)",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(ensure_queued_uploads(&pool, clip).await.is_empty());

        let record = ensure_approval_row(&pool, clip).await;
        assert!(record.approved_platforms.is_empty());
        assert_eq!(
            record.state, STATE_AWAITING,
            "ohne verbleibende Plattform gehoert der Clip zurueck in die Sichtung"
        );
        let offen: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = $1")
                .bind(clip)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(offen, 0);
        // Und damit ist der Clip auch aus dem Batch-Fenster raus.
        assert!(iter_approved_clips_pending_queue(&pool, 10)
            .await
            .is_empty());
    }

    /// Legt eine Freigabe mit beliebigen Plattformen an und gibt die Clip-ID
    /// zurueck. `vor_minuten` steuert das Alter der Entscheidung.
    async fn seed_freigabe(pool: &PgPool, plattformen: &[&str], vor_minuten: i32) -> i32 {
        let clip: i32 =
            sqlx::query_scalar("INSERT INTO twitch_clips_social_media DEFAULT VALUES RETURNING id")
                .fetch_one(pool)
                .await
                .unwrap();
        let liste = serde_json::to_string(plattformen).unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms, decided_at) \
             VALUES ($1, 'approved', $2::text::jsonb, now() - make_interval(mins => $3))",
        )
        .bind(clip)
        .bind(liste)
        .bind(vor_minuten)
        .execute(pool)
        .await
        .unwrap();
        clip
    }

    async fn seed_queue_zeile(pool: &PgPool, clip: i32, platform: &str) {
        seed_queue_zeile_mit_status(pool, clip, platform, "pending").await;
    }

    async fn seed_queue_zeile_mit_status(pool: &PgPool, clip: i32, platform: &str, status: &str) {
        sqlx::query(
            "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) \
             VALUES ($1, $2, $3)",
        )
        .bind(clip)
        .bind(platform)
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
    }

    /// Festgeschrieben, was `failed` im Nachreih-Lauf bedeutet.
    ///
    /// `upload_already_exists` klammert `failed` aus, die Fenster-Abfrage tut
    /// es nicht. Das ist kein Versehen: `queue_upload` belebt eine
    /// gescheiterte Zeile nicht wieder, sondern legt eine zusaetzliche an.
    /// Wuerde der Nachreih-Lauf `failed` als offen zaehlen, bekaeme eine
    /// dauerhaft scheiternde Plattform bei jedem Fehlschlag eine weitere
    /// Zeile. Der Weg zurueck ist die ausdrueckliche erneute Freigabe im
    /// Dashboard, nicht der Automatismus.
    #[tokio::test]
    async fn nachreih_lauf_belebt_gescheiterte_uploads_nicht_wieder() {
        let Some(pool) = make_pool("t_sm_approval_failed").await else {
            return;
        };

        let gescheitert = seed_freigabe(&pool, &["youtube"], 60).await;
        seed_queue_zeile_mit_status(&pool, gescheitert, "youtube", "failed").await;

        assert!(
            iter_approved_clips_pending_queue(&pool, 10)
                .await
                .is_empty(),
            "eine gescheiterte Zeile gilt als eingereiht, der Automatismus legt keine zweite an"
        );

        // Gegenprobe: der zweite Guard sieht dieselbe Zeile anders. Genau
        // dieser Unterschied traegt die ausdrueckliche erneute Freigabe.
        assert!(
            !upload_already_exists(&pool, gescheitert, "youtube").await,
            "eine erneute Freigabe darf den gescheiterten Upload neu einreihen"
        );
    }

    /// Der eigentliche Fall: das Einreihen laeuft je Plattform, die
    /// Nachreih-Abfrage muss deshalb auch je Plattform pruefen.
    ///
    /// Ein Clip, der fuer youtube und tiktok freigegeben ist und nur fuer
    /// tiktok eine Zeile bekommen hat, war mit der clip-weiten Pruefung
    /// dauerhaft aus dem Fenster: die tiktok-Zeile allein machte `NOT EXISTS`
    /// falsch, youtube wurde nie wieder versucht. Ohne Fehler, ohne Log, ohne
    /// Weg zurueck ausser Handarbeit in der Datenbank.
    #[tokio::test]
    async fn nachreih_lauf_findet_den_clip_mit_halb_eingereihten_plattformen() {
        let Some(pool) = make_pool("t_sm_approval_teilweise").await else {
            return;
        };

        let halb = seed_freigabe(&pool, &["youtube", "tiktok"], 60).await;
        seed_queue_zeile(&pool, halb, "tiktok").await;

        // Gegenprobe: derselbe Clip-Zuschnitt, aber vollstaendig eingereiht.
        // Der darf das Zehnerfenster nicht belegen.
        let voll = seed_freigabe(&pool, &["youtube", "tiktok"], 30).await;
        seed_queue_zeile(&pool, voll, "tiktok").await;
        seed_queue_zeile(&pool, voll, "youtube").await;

        let batch = iter_approved_clips_pending_queue(&pool, 10).await;
        assert_eq!(
            batch,
            vec![halb],
            "eine offene Plattform haelt den Clip im Fenster, ein vollstaendig \
             eingereihter Clip faellt heraus"
        );
    }

    /// Die aelteste offene Freigabe kommt zuerst dran.
    ///
    /// Mit `decided_at DESC` verdraengten zehn frische Freigaben die aelteste
    /// bei jedem Lauf aufs Neue, und zwar schon bei normalem Andrang, ohne
    /// dass eine Kadenz auf null stehen muss. Eine Warteschlange, die von
    /// hinten abgearbeitet wird, ist keine Warteschlange.
    #[tokio::test]
    async fn nachreih_lauf_nimmt_die_aelteste_offene_freigabe_zuerst() {
        let Some(pool) = make_pool("t_sm_approval_reihenfolge").await else {
            return;
        };

        let aelteste = seed_freigabe(&pool, &["youtube"], 600).await;
        for minuten in 1..=10 {
            seed_freigabe(&pool, &["youtube"], minuten).await;
        }

        let batch = iter_approved_clips_pending_queue(&pool, 10).await;
        assert_eq!(batch.len(), 10);
        assert_eq!(
            batch.first().copied(),
            Some(aelteste),
            "die aelteste offene Freigabe darf nicht aus dem Fenster fallen"
        );
    }

    /// Ein Clip ohne freien Termin bleibt in der Nachreih-Liste, auch wenn
    /// danach viele vollstaendig eingereihte Freigaben dazukommen. Vorher
    /// lieferte `iter_approved_clips_pending_queue` einfach die juengsten zehn
    /// Freigaben, und der Clip fiel nach zehn weiteren aus dem Fenster.
    #[tokio::test]
    async fn nachreih_lauf_findet_den_clip_ohne_termin_auch_spaeter_noch() {
        let Some(pool) = make_pool("t_sm_approval_nachreihen").await else {
            return;
        };
        let ohne_termin: i32 =
            sqlx::query_scalar("INSERT INTO twitch_clips_social_media DEFAULT VALUES RETURNING id")
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms, decided_at) \
             VALUES ($1, 'approved', '[\"youtube\"]'::jsonb, now() - INTERVAL '1 hour')",
        )
        .bind(ohne_termin)
        .execute(&pool)
        .await
        .unwrap();

        // Zwanzig juengere Freigaben, alle vollstaendig eingereiht.
        for _ in 0..20 {
            let clip: i32 = sqlx::query_scalar(
                "INSERT INTO twitch_clips_social_media DEFAULT VALUES RETURNING id",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms, decided_at) \
                 VALUES ($1, 'approved', '[\"youtube\"]'::jsonb, now())",
            )
            .bind(clip)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) \
                 VALUES ($1, 'youtube', 'pending')",
            )
            .bind(clip)
            .execute(&pool)
            .await
            .unwrap();
        }

        let batch = iter_approved_clips_pending_queue(&pool, 10).await;
        assert_eq!(
            batch,
            vec![ohne_termin],
            "das Batch-Fenster darf nur wirklich offene Freigaben enthalten"
        );
    }

    /// Ist der Planungshorizont voll, darf die Zeile ebenfalls nicht mit leerem
    /// Termin eingereiht werden.
    #[tokio::test]
    async fn voller_horizont_reiht_nichts_mit_leerem_termin_ein() {
        let Some(pool) = make_pool("t_sm_approval_horizont_voll").await else {
            return;
        };
        zeitplan_tabellen(&pool).await;

        let clip: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('kanal') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_platform_schedule \
                 (streamer_login, platform, auto_post, posts_per_week, max_posts_per_day, post_times) \
             VALUES ('kanal', 'youtube', TRUE, 1, 1, '[\"18:00\"]'::jsonb)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Ein zweiter Clip belegt jeden Tag des Horizonts, damit kein Termin
        // mehr frei ist.
        let belegt: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('kanal') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status, scheduled_at) \
             SELECT $1, 'youtube', 'pending', \
                    (date_trunc('day', now() AT TIME ZONE 'Europe/Berlin') \
                     + make_interval(days => g) + INTERVAL '18 hours') AT TIME ZONE 'Europe/Berlin' \
               FROM generate_series(0, 200) AS g",
        )
        .bind(belegt)
        .execute(&pool)
        .await
        .unwrap();

        handle_decision(
            &pool,
            clip,
            "approve",
            &["youtube".to_string()],
            Some("admin"),
        )
        .await
        .unwrap();

        let offen: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = $1")
                .bind(clip)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(offen, 0, "ohne freien Termin wird nichts eingereiht");
    }

    /// Derselbe Fall wie oben, aber mit einer zweiten Plattform daneben: der
    /// ganze Ablauf von der Freigabe bis in die Nachreih-Liste.
    ///
    /// tiktok bekommt einen Termin, youtube laeuft in `SlotPlan::HorizontVoll`
    /// und bleibt ohne Zeile. Genau hier war der Clip vorher endgueltig raus,
    /// weil die tiktok-Zeile die clip-weite Pruefung erfuellte. Im Dashboard
    /// stand er weiter als "freigegeben fuer youtube", und auf youtube
    /// passierte nie etwas.
    #[tokio::test]
    async fn horizont_voll_auf_einer_plattform_haelt_den_clip_in_der_nachreih_liste() {
        let Some(pool) = make_pool("t_sm_approval_horizont_teilweise").await else {
            return;
        };
        zeitplan_tabellen(&pool).await;

        let clip: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('kanal') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_platform_schedule \
                 (streamer_login, platform, auto_post, posts_per_week, max_posts_per_day, post_times) \
             VALUES ('kanal', 'youtube', TRUE, 1, 1, '[\"18:00\"]'::jsonb), \
                    ('kanal', 'tiktok', TRUE, 7, 1, '[\"18:00\"]'::jsonb)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Nur der youtube-Horizont ist dicht, tiktok bleibt frei.
        let belegt: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('kanal') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_clips_upload_queue (clip_id, platform, status, scheduled_at) \
             SELECT $1, 'youtube', 'pending', \
                    (date_trunc('day', now() AT TIME ZONE 'Europe/Berlin') \
                     + make_interval(days => g) + INTERVAL '18 hours') AT TIME ZONE 'Europe/Berlin' \
               FROM generate_series(0, 200) AS g",
        )
        .bind(belegt)
        .execute(&pool)
        .await
        .unwrap();

        handle_decision(
            &pool,
            clip,
            "approve",
            &["youtube".to_string(), "tiktok".to_string()],
            Some("admin"),
        )
        .await
        .unwrap();

        let plattformen: Vec<String> = sqlx::query_scalar(
            "SELECT platform FROM twitch_clips_upload_queue WHERE clip_id = $1 ORDER BY platform",
        )
        .bind(clip)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            plattformen,
            vec!["tiktok".to_string()],
            "tiktok bekommt einen Termin, youtube bleibt ohne Zeile"
        );

        assert!(
            iter_approved_clips_pending_queue(&pool, 10)
                .await
                .contains(&clip),
            "die offene youtube-Freigabe muss den Clip in der Nachreih-Liste halten"
        );
    }
}
