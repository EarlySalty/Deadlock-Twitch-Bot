//! Private Kritik und Wünsche. Identität ausschließlich aus der bestehenden Session.
//! Persistente Idempotenz und Ratenlimit liegen innerhalb derselben DB-Transaktion.
use crate::auth::level::DashboardAuthLevel;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgPool, Postgres, Row, Transaction};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateBody {
    pub request_id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub area: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateBody {
    pub request_id: String,
    pub expected_revision: i32,
    pub status: String,
    #[serde(default)]
    pub reply: String,
    pub result_path: Option<String>,
    pub decision_reason: Option<String>,
    pub roadmap_id: Option<i64>,
    pub duplicate_of: Option<i64>,
}
#[derive(Default, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub inbox: bool,
    pub before: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadBody {
    pub observed_at: DateTime<Utc>,
    #[serde(default)]
    pub inbox: bool,
}
fn fail(code: StatusCode, message: &str) -> Response {
    (code, Json(json!({"error": message}))).into_response()
}
fn private_json(value: Value) -> Response {
    let mut response = Json(value).into_response();
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response
}
fn db_failure(error: sqlx::Error) -> Response {
    // Keine DB-Details oder Rückmeldungstexte ins Journal übernehmen.
    tracing::error!(code = ?error.as_database_error().and_then(|e| e.code()), "Rückmeldungen: Datenbankzugriff fehlgeschlagen");
    fail(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Das hat gerade nicht geklappt. Bitte erneut versuchen; dein Text bleibt erhalten.",
    )
}
#[allow(clippy::result_large_err)]
fn owner_id(auth: &DashboardAuthLevel) -> Result<&str, Response> {
    let id = match auth {
        DashboardAuthLevel::Partner { twitch_user_id, .. } => twitch_user_id.as_str(),
        DashboardAuthLevel::Admin { actor: Some(actor) } => actor.twitch_user_id.as_str(),
        _ => "",
    };
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        Err(fail(
            StatusCode::UNAUTHORIZED,
            "Bitte melde dich mit Twitch an, damit deine Rückmeldung deinem Konto zugeordnet wird.",
        ))
    } else {
        Ok(id)
    }
}
#[allow(clippy::result_large_err)]
fn scope(auth: &DashboardAuthLevel, inbox: bool) -> Result<Option<&str>, Response> {
    if inbox {
        if auth.is_privileged() {
            Ok(None)
        } else {
            Err(fail(
                StatusCode::FORBIDDEN,
                "Die Inbox ist nur für Betreiber zugänglich.",
            ))
        }
    } else {
        owner_id(auth).map(Some)
    }
}
fn valid_status(kind: &str, status: &str) -> bool {
    matches!(
        status,
        "received" | "reviewing" | "planned" | "in_progress" | "done" | "declined"
    ) || (kind == "feedback" && status == "answered")
}
/// Nur bestehende Dashboard-Ziele, keine Token-Queries, Auth-/Schreibpfade oder privaten IDs.
fn valid_result_path(path: &str) -> bool {
    if path.chars().any(|c| c.is_control()) || path.contains(['?', '%', '\\']) {
        return false;
    }
    let (base, anchor) = path.split_once('#').unwrap_or((path, ""));
    matches!(
        base,
        "/twitch/dashboard"
            | "/twitch/verwaltung"
            | "/twitch/uplink"
            | "/analyse"
            | "/twitch/abbo"
            | "/social-media"
    ) && anchor
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        && path.len() <= 200
}
impl CreateBody {
    fn normalize(&mut self) -> Result<(), &'static str> {
        self.title = self.title.trim().into();
        self.body = self.body.trim().into();
        self.area = self.area.trim().into();
        if uuid::Uuid::parse_str(&self.request_id).is_err() {
            return Err("Die Sende-ID ist ungültig. Bitte öffne das Formular erneut.");
        }
        if !matches!(self.kind.as_str(), "feedback" | "feature") {
            return Err("Bitte wähle Kritik oder einen Feature-Wunsch.");
        }
        if !(3..=120).contains(&self.title.chars().count()) {
            return Err("Der Titel braucht 3 bis 120 Zeichen.");
        }
        if !(10..=4000).contains(&self.body.chars().count()) {
            return Err("Dein Text braucht 10 bis 4000 Zeichen.");
        }
        if self.area.chars().count() > 80
            || [&self.title, &self.body, &self.area]
                .iter()
                .any(|s| s.contains('\0'))
        {
            return Err("Bitte kürze den Bereich oder entferne ungültige Zeichen.");
        }
        Ok(())
    }
}

// Die gemeinsame Quelle ist entweder der eigene Eintrag oder ein Hauptwunsch.
// Ausschließlich freigegebene Roadmap-Titel und Fortschritt werden übernommen;
// Hauptwunsch-ID, Absender, Text und Antworten verlassen den Adminbereich nicht.
const VIEW: &str = r#"
SELECT f.*, COALESCE(root.status, f.status) AS base_status,
       COALESCE(root.result_path, f.result_path) AS effective_result,
       COALESCE(root.decision_reason, f.decision_reason) AS effective_reason,
       r.id AS public_roadmap_id, r.title AS roadmap_title, r.status AS roadmap_status,
       GREATEST(f.updated_at, root.updated_at, r.updated_at) AS observed_at,
       (f.revision > 0 OR root.revision > 0 OR r.updated_at > f.created_at)
         AND GREATEST(f.updated_at, root.updated_at, r.updated_at) > f.owner_seen_at AS unread
FROM dashboard_feedback f
LEFT JOIN dashboard_feedback root ON root.id = f.duplicate_of
LEFT JOIN twitch_roadmap_items r ON r.id = COALESCE(root.roadmap_id, f.roadmap_id)
"#;
fn entry_json(row: &sqlx::postgres::PgRow, admin: bool) -> Value {
    let roadmap: Option<String> = row.get("roadmap_status");
    let status: String = match roadmap.as_deref() {
        Some("planned" | "in_progress" | "done") => roadmap.clone().unwrap(),
        _ => row.get("base_status"),
    };
    let mut value = json!({
        "id": row.get::<i64,_>("id"), "kind": row.get::<String,_>("kind"),
        "title": row.get::<String,_>("title"), "body": row.get::<String,_>("body"), "area": row.get::<String,_>("area"),
        "status": status, "result_path": row.get::<Option<String>,_>("effective_result"),
        "decision_reason": if status == "declined" { row.get::<Option<String>,_>("effective_reason") } else { None },
        "revision": row.get::<i32,_>("revision"), "bundled": row.get::<Option<i64>,_>("duplicate_of").is_some(),
        "roadmap": row.get::<Option<i64>,_>("public_roadmap_id").map(|id| json!({"id": id, "title": row.get::<Option<String>,_>("roadmap_title"), "status": roadmap})),
        "created_at": row.get::<DateTime<Utc>,_>("created_at"), "observed_at": row.get::<DateTime<Utc>,_>("observed_at"),
        "unread": row.get::<Option<bool>,_>("unread").unwrap_or(false),
    });
    if admin {
        value["owner_id"] = json!(row.get::<String, _>("owner_id"));
        value["author_label"] = json!(row.get::<String, _>("author_label"));
        value["admin_unread"] = json!(!row.get::<bool, _>("admin_read"));
        value["duplicate_of"] = json!(row.get::<Option<i64>, _>("duplicate_of"));
        value["roadmap_id"] = json!(row.get::<Option<i64>, _>("roadmap_id"));
        value["own_status"] = json!(row.get::<String, _>("status"));
        value["own_result_path"] = json!(row.get::<Option<String>, _>("result_path"));
        value["own_decision_reason"] = json!(row.get::<Option<String>, _>("decision_reason"));
    }
    value
}

pub async fn list(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ListQuery>,
) -> Response {
    let owner = match scope(&auth, query.inbox) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let sql = format!("{VIEW} WHERE ($1::TEXT IS NULL OR f.owner_id = $1) AND ($2::BIGINT IS NULL OR f.id < $2) ORDER BY f.id DESC LIMIT 51");
    match sqlx::query(&sql)
        .bind(owner)
        .bind(query.before)
        .fetch_all(&pool)
        .await
    {
        Ok(rows) => {
            let next = if rows.len() > 50 {
                Some(rows[49].get::<i64, _>("id"))
            } else {
                None
            };
            private_json(
                json!({"items": rows.iter().take(50).map(|r| entry_json(r, query.inbox)).collect::<Vec<_>>(), "next": next}),
            )
        }
        Err(e) => db_failure(e),
    }
}
pub async fn counts(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ListQuery>,
) -> Response {
    let owner = match scope(&auth, query.inbox) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let sql = format!("SELECT COUNT(*)::BIGINT AS total, COUNT(*) FILTER (WHERE {})::BIGINT AS unread FROM ({VIEW} WHERE ($1::TEXT IS NULL OR f.owner_id=$1)) q", if query.inbox { "NOT admin_read" } else { "unread" });
    match sqlx::query(&sql).bind(owner).fetch_one(&pool).await {
        Ok(r) => {
            private_json(json!({"total":r.get::<i64,_>("total"),"unread":r.get::<i64,_>("unread")}))
        }
        Err(e) => db_failure(e),
    }
}
pub async fn detail(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(id): Path<i64>,
) -> Response {
    let owner = match scope(&auth, auth.is_privileged()) {
        Ok(v) => v,
        Err(r) => return r,
    };
    // Snapshot verhindert, dass Detail und Verlauf unterschiedliche Antworten bestätigen.
    let mut tx = match pool.begin().await {
        Ok(v) => v,
        Err(e) => return db_failure(e),
    };
    if let Err(e) = sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
    {
        return db_failure(e);
    }
    let sql = format!("{VIEW} WHERE f.id=$1 AND ($2::TEXT IS NULL OR f.owner_id=$2)");
    let row = match sqlx::query(&sql)
        .bind(id)
        .bind(owner)
        .fetch_optional(&mut *tx)
        .await
    {
        Ok(Some(v)) => v,
        Ok(None) => return fail(StatusCode::NOT_FOUND, "Rückmeldung nicht gefunden."),
        Err(e) => return db_failure(e),
    };
    let mut value = entry_json(&row, auth.is_privileged());
    let events = match sqlx::query("SELECT id,status,reply,result_path,decision_reason,created_at FROM dashboard_feedback_events WHERE feedback_id=$1 ORDER BY id")
        .bind(id).fetch_all(&mut *tx).await { Ok(v) => v, Err(e) => return db_failure(e) };
    value["events"] = json!(events.iter().map(|r| json!({"id":r.get::<i64,_>("id"),"status":r.get::<String,_>("status"),"reply":r.get::<String,_>("reply"),"result_path":r.get::<Option<String>,_>("result_path"),"decision_reason":r.get::<Option<String>,_>("decision_reason"),"created_at":r.get::<DateTime<Utc>,_>("created_at")})).collect::<Vec<_>>());
    if let Err(e) = tx.commit().await {
        return db_failure(e);
    }
    private_json(value)
}
pub async fn mark_read(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(id): Path<i64>,
    Json(body): Json<ReadBody>,
) -> Response {
    let owner = match scope(&auth, body.inbox) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let sql = if body.inbox {
        "UPDATE dashboard_feedback SET admin_read=TRUE WHERE id=$1 AND ($2::TEXT IS NULL OR owner_id=$2) AND $3::TIMESTAMPTZ <= clock_timestamp()"
    } else {
        "UPDATE dashboard_feedback SET owner_seen_at=GREATEST(owner_seen_at,$3) WHERE id=$1 AND owner_id=$2 AND $3::TIMESTAMPTZ <= clock_timestamp()"
    };
    match sqlx::query(sql)
        .bind(id)
        .bind(owner)
        .bind(body.observed_at)
        .execute(&pool)
        .await
    {
        Ok(r) if r.rows_affected() == 1 => Json(json!({"ok":true})).into_response(),
        Ok(_) => fail(
            StatusCode::NOT_FOUND,
            "Rückmeldung nicht gefunden oder Lesestand ungültig.",
        ),
        Err(e) => db_failure(e),
    }
}

pub async fn create(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(mut body): Json<CreateBody>,
) -> Response {
    let owner = match owner_id(&auth) {
        Ok(v) => v,
        Err(r) => return r,
    };
    if let Err(message) = body.normalize() {
        return fail(StatusCode::BAD_REQUEST, message);
    }
    let label = match &auth {
        DashboardAuthLevel::Partner {
            display_name,
            twitch_login,
            ..
        } => {
            if display_name.trim().is_empty() {
                twitch_login
            } else {
                display_name
            }
        }
        DashboardAuthLevel::Admin { actor: Some(a) } => &a.twitch_login,
        _ => unreachable!("owner_id hat eine Twitch-Identität geprüft"),
    };
    let mut tx = match pool.begin().await {
        Ok(v) => v,
        Err(e) => return db_failure(e),
    };
    if let Err(e) = sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 918210))")
        .bind(owner)
        .execute(&mut *tx)
        .await
    {
        return db_failure(e);
    }
    let previous = sqlx::query("SELECT id,kind,title,body,area FROM dashboard_feedback WHERE owner_id=$1 AND request_id=$2").bind(owner).bind(&body.request_id).fetch_optional(&mut *tx).await;
    match previous {
        Ok(Some(row)) => {
            if row.get::<String, _>("kind") != body.kind
                || row.get::<String, _>("title") != body.title
                || row.get::<String, _>("body") != body.body
                || row.get::<String, _>("area") != body.area
            {
                return fail(StatusCode::CONFLICT, "Diese Sende-ID gehört schon zu einer anderen Rückmeldung. Öffne ein neues Formular.");
            }
            return Json(json!({"id":row.get::<i64,_>("id"),"saved":true})).into_response();
        }
        Ok(None) => {}
        Err(e) => return db_failure(e),
    }
    let limit: Result<bool,_> = sqlx::query_scalar("SELECT COUNT(*) FILTER (WHERE created_at > clock_timestamp() - INTERVAL '1 hour') >= 5 OR COUNT(*) >= 20 FROM dashboard_feedback WHERE owner_id=$1 AND created_at > clock_timestamp() - INTERVAL '1 day'")
        .bind(owner).fetch_one(&mut *tx).await;
    match limit { Ok(false) => {}, Ok(true) => return fail(StatusCode::TOO_MANY_REQUESTS, "Für heute sind schon einige Rückmeldungen angekommen. Bitte versuche es später noch einmal; dein Text bleibt erhalten."), Err(e) => return db_failure(e) }
    let inserted: Result<i64,_> = sqlx::query_scalar("INSERT INTO dashboard_feedback(owner_id,author_label,request_id,kind,title,body,area) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id")
        .bind(owner).bind(label).bind(&body.request_id).bind(&body.kind).bind(&body.title).bind(&body.body).bind(&body.area).fetch_one(&mut *tx).await;
    let id = match inserted {
        Ok(id) => id,
        Err(e) => return db_failure(e),
    };
    if let Err(e) = tx.commit().await {
        return db_failure(e);
    }
    (StatusCode::CREATED, Json(json!({"id":id,"saved":true}))).into_response()
}

pub async fn update(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(id): Path<i64>,
    Json(mut body): Json<UpdateBody>,
) -> Response {
    if !auth.is_privileged() {
        return fail(
            StatusCode::FORBIDDEN,
            "Nur Betreiber können antworten und den Stand ändern.",
        );
    }
    body.reply = body.reply.trim().into();
    body.decision_reason = body
        .decision_reason
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    body.result_path = body
        .result_path
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if uuid::Uuid::parse_str(&body.request_id).is_err()
        || body.reply.chars().count() > 4000
        || body.reply.contains('\0')
        || body
            .decision_reason
            .as_deref()
            .is_some_and(|s| !(3..=800).contains(&s.chars().count()) || s.contains('\0'))
    {
        return fail(
            StatusCode::BAD_REQUEST,
            "Bitte prüfe Sende-ID und Antwort (höchstens 4000 Zeichen).",
        );
    }
    if body
        .result_path
        .as_deref()
        .is_some_and(|p| !valid_result_path(p))
    {
        return fail(
            StatusCode::BAD_REQUEST,
            "Bitte verlinke eine Dashboard-Seite ohne private IDs oder Zugangsparameter.",
        );
    }
    let mut tx = match pool.begin().await {
        Ok(v) => v,
        Err(e) => return db_failure(e),
    };
    // Gemeinsamer Admin-Lock verhindert gleichzeitig entstehende Duplikatketten/Zyklen.
    if let Err(e) = sqlx::query("SELECT pg_advisory_xact_lock(918210, 1)")
        .execute(&mut *tx)
        .await
    {
        return db_failure(e);
    }
    match apply_update(&mut tx, id, &body).await {
        Ok(Some(r)) => r,
        Ok(None) => match tx.commit().await {
            Ok(()) => Json(json!({"ok":true})).into_response(),
            Err(e) => db_failure(e),
        },
        Err(e) => db_failure(e),
    }
}
async fn apply_update(
    tx: &mut Transaction<'_, Postgres>,
    id: i64,
    body: &UpdateBody,
) -> Result<Option<Response>, sqlx::Error> {
    let payload = serde_json::to_value(body).expect("UpdateBody ist JSON-serialisierbar");
    let prior: Option<Value> = sqlx::query_scalar("SELECT request_payload FROM dashboard_feedback_events WHERE feedback_id=$1 AND request_id=$2").bind(id).bind(&body.request_id).fetch_optional(&mut **tx).await?;
    if let Some(prior) = prior {
        return Ok(Some(if prior == payload {
            Json(json!({"ok":true})).into_response()
        } else {
            fail(
                StatusCode::CONFLICT,
                "Diese Antwort wurde bereits mit anderem Inhalt gespeichert.",
            )
        }));
    }
    let Some(row) =
        sqlx::query("SELECT kind,revision FROM dashboard_feedback WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?
    else {
        return Ok(Some(fail(
            StatusCode::NOT_FOUND,
            "Rückmeldung nicht gefunden.",
        )));
    };
    if row.get::<i32, _>("revision") != body.expected_revision {
        return Ok(Some(fail(StatusCode::CONFLICT,"Der Eintrag wurde inzwischen geändert. Bitte lade den aktuellen Stand; dein Antworttext bleibt erhalten.")));
    }
    let kind: String = row.get("kind");
    let invalid = !valid_status(&kind, &body.status)
        || (body.status == "answered" && body.reply.is_empty())
        || (body.status == "declined"
            && body.decision_reason.is_none()
            && body.duplicate_of.is_none()
            && body.roadmap_id.is_none())
        || (body.status == "done" && body.result_path.is_none() && body.duplicate_of.is_none())
        || (body.roadmap_id.is_some()
            && (kind != "feature" || body.result_path.is_none() || body.duplicate_of.is_some()))
        || (body.duplicate_of.is_some() && kind != "feature");
    if invalid {
        return Ok(Some(fail(StatusCode::BAD_REQUEST,"Bitte prüfe den Status: Ablehnung braucht eine Begründung, Beantwortet eine Antwort und Umgesetzt einen Funktionslink. Roadmap nur für Wünsche mit Funktionsziel; Duplikate ohne eigene Roadmap.")));
    }
    let mut status = body.status.clone();
    if let Some(target) = body.duplicate_of {
        let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM dashboard_feedback WHERE id=$1 AND id<>$2 AND kind='feature' AND duplicate_of IS NULL) AND NOT EXISTS(SELECT 1 FROM dashboard_feedback WHERE duplicate_of=$2)")
            .bind(target).bind(id).fetch_one(&mut **tx).await?;
        if !valid {
            return Ok(Some(fail(StatusCode::BAD_REQUEST,"Bitte wähle einen eigenständigen Hauptwunsch. Selbstzuordnung und Duplikatketten sind nicht erlaubt.")));
        }
        status = sqlx::query_scalar("SELECT COALESCE(r.status, f.status) FROM dashboard_feedback f LEFT JOIN twitch_roadmap_items r ON r.id=f.roadmap_id WHERE f.id=$1")
            .bind(target).fetch_one(&mut **tx).await?;
    }
    if let Some(roadmap_id) = body.roadmap_id {
        let road: Option<String> =
            sqlx::query_scalar("SELECT status FROM twitch_roadmap_items WHERE id=$1 FOR SHARE")
                .bind(roadmap_id)
                .fetch_optional(&mut **tx)
                .await?;
        match road {
            Some(s) if matches!(s.as_str(), "planned" | "in_progress" | "done") => status = s,
            _ => {
                return Ok(Some(fail(
                    StatusCode::BAD_REQUEST,
                    "Der ausgewählte öffentliche Roadmap-Punkt ist nicht verfügbar.",
                )))
            }
        }
    }
    let reason = if status == "declined" && body.duplicate_of.is_none() {
        body.decision_reason.as_deref()
    } else {
        None
    };
    sqlx::query("UPDATE dashboard_feedback SET status=$2,result_path=$3,roadmap_id=$4,duplicate_of=$5,decision_reason=$6,revision=revision+1,updated_at=clock_timestamp(),admin_read=TRUE WHERE id=$1")
        .bind(id).bind(&status).bind(&body.result_path).bind(body.roadmap_id).bind(body.duplicate_of).bind(reason).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO dashboard_feedback_events(feedback_id,request_id,request_payload,status,reply,result_path,decision_reason) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(id).bind(&body.request_id).bind(payload).bind(status).bind(&body.reply).bind(&body.result_path).bind(reason).execute(&mut **tx).await?;
    Ok(None)
}

#[cfg(test)]
mod tests;
