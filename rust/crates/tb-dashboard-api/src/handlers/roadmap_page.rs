//! Öffentliche Roadmap-Anzeige-Seite (B1-ROADMAP-PAGE).
//!
//! `GET /twitch/roadmap` — rendert den vom Admin gepflegten Roadmap-Body
//! (`data/admin_dashboard/roadmap_body.json`, Quelle wie der Admin-Editor in
//! [`crate::handlers::admin_roadmap`]) in einer schlanken, **read-only**,
//! auth-freien HTML-Seite. Kein Admin-Kanban, keine Drag&Drop-Skripte — nur die
//! Anzeige des aktuellen Stands für die Öffentlichkeit.
//!
//! Der gespeicherte Body ist vom Admin gepflegtes HTML (vertrauenswürdige
//! Quelle) und wird unverändert in die Seitenschale eingebettet, exakt wie
//! Pythons `server._html(build_roadmap_body(), "roadmap")` ihn ausliefert — nur
//! ohne die Admin-Navigation/-Skripte.

use axum::{
    http::header,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use crate::handlers::admin_roadmap;

/// Minimaler, dunkler Seiten-Rahmen für die öffentliche Roadmap (eigenständig,
/// ohne Admin-Tabs/-Skripte). `{body}` wird durch den gespeicherten Roadmap-Body
/// ersetzt.
const PAGE_SHELL: &str = r#"<!doctype html>
<html lang="de">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Roadmap – Deutsche Deadlock Community</title>
<style>
  :root { color-scheme: dark; --bg:#07151d; --card:#102635; --bd:rgba(194,221,240,0.14);
    --text:#e9f1f7; --muted:#9bb3c5; --accent:#ff7a18; --teal:#10b7ad; }
  * { box-sizing: border-box; }
  body { font-family: "Manrope","Segoe UI",sans-serif; margin:0; color:var(--text);
    padding:2.6rem 1.4rem 3.4rem; max-width:1100px; margin:0 auto;
    background:
      radial-gradient(1200px 540px at 92% -10%, rgba(255,122,24,0.18), transparent 65%),
      radial-gradient(940px 500px at 9% -18%, rgba(16,183,173,0.20), transparent 60%),
      linear-gradient(160deg,#07151d 0%,#081a24 55%,#0a202c 100%); }
  h1,h2,h3 { font-family:"Sora","Segoe UI",sans-serif; letter-spacing:-0.02em; }
  a { color:var(--accent); }
  .hero { margin-bottom:1.6rem; }
  .hero .eyebrow { text-transform:uppercase; letter-spacing:.12em; font-size:.72rem;
    color:var(--teal); margin:0 0 .3rem; font-weight:600; }
  .hero h1 { font-size:2.1rem; margin:0; }
  .hero .lead { color:var(--muted); margin:.5rem 0 0; }
  .card { background:linear-gradient(160deg,rgba(16,38,53,.92),rgba(10,30,42,.92));
    border:1px solid var(--bd); border-radius:1rem; padding:1.2rem; margin:.9rem 0; }
  /* Admin-Editier-Steuerelemente in einem vom Admin gepflegten Body inert schalten. */
  button, [draggable], form { pointer-events:none; }
</style>
</head>
<body>
{body}
</body>
</html>"#;

/// `GET /twitch/roadmap` — öffentliche Read-Only-Roadmap.
pub async fn roadmap_page_handler() -> Response {
    let body = admin_roadmap::load_roadmap_body().await;
    let html = PAGE_SHELL.replacen("{body}", &body, 1);
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

/// Baut den öffentlichen Roadmap-Router (kein Auth, kein Pool).
pub fn build_roadmap_page_router() -> Router {
    Router::new().route("/twitch/roadmap", get(roadmap_page_handler))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn roadmap_seite_rendert_body_in_schale() {
        let app = build_roadmap_page_router();
        let resp = app
            .oneshot(Request::builder().uri("/twitch/roadmap").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers().get(header::CONTENT_TYPE).unwrap().to_str().unwrap();
        assert!(ct.contains("text/html"));
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        let html = String::from_utf8_lossy(&bytes);
        // Schale vorhanden + Default-Body (enthält "Roadmap") eingebettet.
        assert!(html.contains("<!doctype html>"));
        assert!(html.contains("Roadmap"));
        // Kein Platzhalter mehr übrig.
        assert!(!html.contains("{body}"));
    }
}
