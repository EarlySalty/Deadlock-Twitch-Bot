//! Datenquelle fuer die unscharfe Wachstumskurve auf der Preisseite.
//!
//! Die Preisseite zeigt einem Streamer ohne Premium die eigene Kurve, aber
//! unscharf. Ein CSS-Blur ist keine Zugriffskontrolle: wer die Entwicklertools
//! oeffnet, liest die Zahlen darunter. Deshalb liefert dieser Endpunkt gar
//! keine absoluten Zuschauerzahlen, sondern nur die auf 0..1 normierte Form der
//! Kurve plus die Anzahl der Tage. Das reicht fuer die Grafik und verschenkt
//! nichts, was hinter der Bezahlschranke liegt.
//!
//! Bewusst ohne Premium-Gate: die Seite richtet sich an Leute, die noch nicht
//! zahlen. Der Zugriff bleibt auf den eigenen Account beschraenkt
//! (`resolve_streamer_scope`).

use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;

/// Mehr Punkte als das zeichnet die Kurve nicht auf.
const MAX_PUNKTE: usize = 60;

#[derive(Deserialize, Default)]
pub struct TeaserQuery {
    #[serde(default)]
    pub streamer: Option<String>,
}

pub async fn premium_teaser_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<TeaserQuery>,
) -> Response {
    let login = match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), false)
    {
        Ok(Some(login)) => login,
        Ok(None) => return Json(leerer_teaser()).into_response(),
        Err(resp) => return resp,
    };

    // ponytail: alle beendeten Sitzungen holen und in Rust auf die letzten 60
    // kuerzen. Ein Streamer hat hoechstens ein paar hundert Zeilen; ein
    // Fenster in SQL waere mehr Code als Ersparnis.
    let rows = match sqlx::query!(
        r#"SELECT started_at AS "started_at!",
                  COALESCE(avg_viewers, 0) AS "avg!: f64"
             FROM twitch_stream_sessions
            WHERE LOWER(streamer_login) = $1
              AND ended_at IS NOT NULL
            ORDER BY started_at"#,
        login
    )
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("premium-teaser: Sitzungen laden fehlgeschlagen: {e}");
            return crate::auth::analytics_request_failed_json().into_response();
        }
    };

    let Some(erste) = rows.first() else {
        return Json(leerer_teaser()).into_response();
    };
    let tage = (chrono::Utc::now() - erste.started_at).num_days().max(0);

    let fenster = &rows[rows.len().saturating_sub(MAX_PUNKTE)..];
    let punkte = normiere(&fenster.iter().map(|r| r.avg).collect::<Vec<f64>>());

    Json(json!({
        "tage": tage,
        "sitzungen": rows.len(),
        "punkte": punkte,
    }))
    .into_response()
}

fn leerer_teaser() -> serde_json::Value {
    json!({ "tage": 0, "sitzungen": 0, "punkte": [] })
}

/// Kurvenform ohne Groesse: der hoechste Wert wird 1.0, alle anderen anteilig.
/// Ohne Ausschlag (nur Nullen) gibt es keine Kurve.
fn normiere(werte: &[f64]) -> Vec<f64> {
    let hoechstwert = werte.iter().fold(0.0_f64, |max, v| max.max(*v));
    if hoechstwert <= 0.0 {
        return Vec::new();
    }
    werte
        .iter()
        .map(|v| (v / hoechstwert * 1000.0).round() / 1000.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::normiere;

    /// Die Normierung darf keine absolute Zuschauerzahl durchlassen: der
    /// hoechste Punkt ist immer 1.0, egal ob der Streamer 3 oder 3000 hat.
    #[test]
    fn normierung_verraet_keine_absolutzahlen() {
        for hoechstwert in [3.0_f64, 3000.0] {
            let punkte = normiere(&[hoechstwert / 3.0, hoechstwert]);
            assert_eq!(punkte, vec![0.333, 1.0], "Form gleich, Groesse unsichtbar");
        }
    }

    #[test]
    fn ohne_ausschlag_keine_kurve() {
        assert!(normiere(&[0.0, 0.0]).is_empty());
        assert!(normiere(&[]).is_empty());
    }
}
