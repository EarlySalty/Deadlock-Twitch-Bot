//! Serverseitig gerenderte, öffentliche Hilfe-/Befehlsseiten aus der SSOT.
//! Schlichtes, maschinen-/AI-lesbares HTML (keine DB, kein Auth).

use axum::http::{header::LOCATION, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use pulldown_cmark::{html, Options, Parser};
use std::path::PathBuf;
use std::sync::OnceLock;
use tb_knowledge::{KnowledgeBase, Namespace};

fn knowledge_dir() -> PathBuf {
    match std::env::var("KNOWLEDGE_DIR")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from("rust/knowledge"),
    }
}

fn knowledge_base() -> &'static KnowledgeBase {
    static KB: OnceLock<KnowledgeBase> = OnceLock::new();
    KB.get_or_init(|| KnowledgeBase::load_from_dir(&knowledge_dir()).unwrap_or_default())
}

fn md_to_html(md: &str) -> String {
    let parser = Parser::new_ext(md, Options::all());
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

fn render_help(kb: &KnowledgeBase) -> String {
    let mut sections = String::new();
    let mut docs: Vec<_> = kb
        .docs()
        .iter()
        .filter(|d| d.namespace == Namespace::Bot)
        .collect();
    docs.sort_by(|a, b| a.slug.cmp(&b.slug));
    for d in docs {
        sections.push_str(&format!(
            "<section id=\"{slug}\"><h2>{title}</h2>{body}</section>\n",
            slug = d.slug,
            title = html_escape(&d.title),
            body = md_to_html(&d.body)
        ));
    }
    page(
        "Hilfe & Wissen zum Bot",
        &format!(
            "<p>Hier findest du, was der Bot kann und wie du ihn einrichtest.</p>\n{sections}"
        ),
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"de\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{t}</title><style>body{{max-width:760px;margin:2rem auto;padding:0 1rem;\
font-family:system-ui,sans-serif;line-height:1.5}}h1,h2{{line-height:1.2}}\
code{{background:#f0f0f0;padding:.1em .3em;border-radius:3px}}</style></head>\
<body><h1>{t}</h1>{b}</body></html>",
        t = html_escape(title),
        b = body
    )
}

pub async fn help_page() -> Response {
    (StatusCode::OK, Html(render_help(knowledge_base()))).into_response()
}

pub async fn commands_page() -> Response {
    let mut body = String::new();
    for (g, items) in tb_chat::catalog::grouped() {
        body.push_str(&format!("<h2>{}</h2><ul>", html_escape(g.label())));
        for c in items {
            body.push_str(&format!(
                "<li><code>{}</code> — {}</li>",
                html_escape(c.name),
                html_escape(c.summary)
            ));
        }
        body.push_str("</ul>");
    }
    (StatusCode::OK, Html(page("Bot-Befehle", &body))).into_response()
}

pub async fn faq_redirect(uri: Uri) -> Response {
    let loc = match uri.query() {
        Some(q) if !q.is_empty() => format!("/streamer/help?{q}"),
        _ => "/streamer/help".to_string(),
    };
    (StatusCode::MOVED_PERMANENTLY, [(LOCATION, loc)]).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md_to_html_basics() {
        let h = md_to_html("Ein **fetter** Text.\n\n- a\n- b");
        assert!(h.contains("<strong>fetter</strong>"));
        assert!(h.contains("<li>a</li>"));
    }

    #[test]
    fn render_help_setzt_anker() {
        let kb = KnowledgeBase::load_from_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tb-knowledge/tests/fixtures"),
        )
        .unwrap();
        let html = render_help(&kb);
        assert!(html.contains("id=\"auto-raid\""), "Anker pro Slug");
        assert!(html.contains("<h1>Hilfe"));
    }

    #[tokio::test]
    async fn faq_redirect_ist_301() {
        let resp = faq_redirect("/streamer/faq?x=1".parse().unwrap()).await;
        assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
    }
}
