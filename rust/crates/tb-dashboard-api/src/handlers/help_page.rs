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
    let mut toc =
        String::from("<nav aria-label=\"Inhaltsverzeichnis\"><h2>Inhaltsverzeichnis</h2><ul>");
    let mut sections = String::new();
    let mut docs: Vec<_> = kb
        .docs()
        .iter()
        .filter(|d| d.namespace == Namespace::Bot)
        // `audience: concierge` markiert Wissen, das nur der Self-Explainer als
        // Grounding sehen darf (z. B. seine eigenen Antwort-Leitplanken). Auf der
        // oeffentlichen Hilfeseite darf das nie erscheinen.
        .filter(|d| d.audience != "concierge")
        .collect();
    docs.sort_by(|a, b| {
        category_rank(&a.category)
            .cmp(&category_rank(&b.category))
            .then(a.category.cmp(&b.category))
            .then(a.slug.cmp(&b.slug))
    });

    for d in &docs {
        toc.push_str(&format!(
            "<li><a href=\"#{slug}\">{title}</a></li>",
            slug = html_escape(&d.slug),
            title = html_escape(&d.title)
        ));
    }
    toc.push_str("</ul></nav>\n");

    let mut current_category: Option<&str> = None;
    for d in docs {
        if current_category != Some(d.category.as_str()) {
            if current_category.is_some() {
                sections.push_str("</section>\n");
            }
            current_category = Some(d.category.as_str());
            sections.push_str(&format!(
                "<section id=\"category-{slug}\"><h2>{title}</h2>\n",
                slug = category_slug(&d.category),
                title = html_escape(category_label(&d.category))
            ));
        }
        sections.push_str(&format!(
            "<article id=\"{slug}\"><h3>{title}</h3>{body}</article>\n",
            slug = html_escape(&d.slug),
            title = html_escape(&d.title),
            body = md_to_html(&d.body)
        ));
    }
    if current_category.is_some() {
        sections.push_str("</section>\n");
    }
    page(
        "Hilfe & Wissen zum Bot",
        &format!(
            "<p>Hier findest du, was der Bot kann und wie du ihn einrichtest.</p>\n{toc}{sections}"
        ),
    )
}

fn category_rank(category: &str) -> u8 {
    match category {
        "feature" => 0,
        "setup" => 1,
        "trust" => 2,
        "faq" => 3,
        "" => 254,
        _ => 253,
    }
}

fn category_label(category: &str) -> &str {
    match category {
        "faq" => "FAQ",
        "feature" => "Feature",
        "setup" => "Setup",
        "trust" => "Vertrauen",
        "support" => "Support",
        "" => "Sonstiges",
        other => other,
    }
}

fn category_slug(category: &str) -> String {
    let slug = category
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    if slug.is_empty() {
        "sonstiges".to_string()
    } else {
        slug
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Gold-auf-Ink wie der Rest von /streamer. Bewusst ein einzelner Style-Block ohne
/// Framework: die Seiten bleiben maschinenlesbar, das Markup aendert sich nicht.
/// `/brand/tokens.css` liefert Schriften und Farb-Tokens (dieselbe Domain); die
/// Hex-Fallbacks tragen die Seite, falls es fehlt.
const BRAND_CSS: &str = "\
:root{--ink:#241c11;--ink-deep:#1c150d;--panel:rgba(59,47,30,.86);--bone:#ece0c8;\
--bone-dim:#b7aa91;--gold:#c8a86b;--gold-bright:#efd49d;--line:rgba(201,168,106,.24)}\
*{box-sizing:border-box}\
body{max-width:820px;margin:0 auto;padding:3rem 1.25rem 6rem;line-height:1.65;\
font-family:var(--font-body,\"Manrope\",\"Segoe UI\",sans-serif);color:var(--bone,#ece0c8);\
background:radial-gradient(100% 55% at 50% 0,rgba(201,168,106,.10),transparent 62%),\
linear-gradient(180deg,#271f12,#241c11 45%,#1c150d);background-attachment:fixed;min-height:100vh}\
h1,h2,h3{font-family:var(--font-display,\"Sora\",\"Manrope\",sans-serif);line-height:1.2;\
letter-spacing:-.01em}\
h1{font-size:clamp(2rem,5vw,2.75rem);font-weight:800;margin:0 0 2rem;color:#ece0c8}\
h2{font-size:1.5rem;font-weight:700;color:var(--gold-bright,#efd49d);margin:3rem 0 1rem;\
padding-bottom:.5rem;border-bottom:1px solid var(--line,rgba(201,168,106,.24))}\
h3{font-size:1.15rem;font-weight:700;color:var(--gold,#c8a86b);margin:2rem 0 .5rem}\
p,li{color:var(--bone-dim,#b7aa91)}\
a{color:var(--gold,#c8a86b);text-decoration:none;border-bottom:1px solid transparent}\
a:hover{color:var(--gold-bright,#efd49d);border-bottom-color:currentColor}\
code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:.92em;\
color:var(--gold-bright,#efd49d);background:rgba(201,168,106,.10);\
border:1px solid var(--line,rgba(201,168,106,.24));border-radius:3px;padding:.1em .4em}\
nav{background:var(--panel,rgba(59,47,30,.86));border:1px solid var(--line,rgba(201,168,106,.24));\
border-radius:6px;padding:1.25rem 1.5rem;margin-bottom:2rem}\
nav h2{margin:0 0 .75rem;font-size:.8rem;text-transform:uppercase;letter-spacing:.1em;\
border:0;padding:0;color:var(--gold,#c8a86b)}\
nav ul{margin:0;padding:0;list-style:none;display:grid;gap:.4rem;\
grid-template-columns:repeat(auto-fill,minmax(190px,1fr))}\
ul{padding-left:1.25rem}\
article{margin-bottom:1.5rem}\
section{margin-bottom:1rem}";

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"de\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<meta name=\"theme-color\" content=\"#241c11\">\
<title>{t}</title>\
<link rel=\"stylesheet\" href=\"/brand/tokens.css\">\
<style>{css}</style></head>\
<body><h1>{t}</h1>{b}</body></html>",
        t = html_escape(title),
        b = body,
        css = BRAND_CSS
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
        assert!(html.contains("<nav aria-label=\"Inhaltsverzeichnis\">"));
        assert!(html.contains("<a href=\"#auto-raid\">Auto-Raid</a>"));
        assert!(html.contains("<h2>Feature</h2>"));
        assert!(html.contains("<h2>Setup</h2>"));
        assert!(html.contains("<h1>Hilfe"));
    }

    /// `audience: concierge` grundiert nur den Self-Explainer, darf aber nie auf der
    /// oeffentlichen `/streamer/help`-Seite landen (Leitplanken sind kein Publikum).
    /// Ohne den Audience-Filter reisst dieser Test: Sabotage bestaetigt es (Fix
    /// entfernt -> Absatz erscheint).
    #[test]
    fn render_help_versteckt_concierge_audience() {
        let kb = KnowledgeBase::load_from_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tb-knowledge/tests/fixtures"),
        )
        .unwrap();
        let html = render_help(&kb);
        assert!(
            !html.contains("Interngeheimnis"),
            "audience: concierge darf nicht im oeffentlichen HTML landen"
        );
        assert!(
            !html.contains("id=\"concierge-intern\""),
            "auch kein Anker fuer das interne Dokument"
        );
    }

    #[tokio::test]
    async fn faq_redirect_ist_301() {
        let resp = faq_redirect("/streamer/faq?x=1".parse().unwrap()).await;
        assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
    }

    /// `/streamer/commands` und `/streamer/help` sind oeffentlich (der `!commands`-Chat-Link
    /// fuehrt Zuschauer direkt hierher) und muessen dieselbe Marke tragen wie der Rest von
    /// /streamer. Frueher: weisse Systemseite mitten im Gold-auf-Ink-Auftritt.
    #[test]
    fn page_traegt_das_gold_branding() {
        let html = page("Bot-Befehle", "<p>Inhalt</p>");

        assert!(html.contains("#241c11"), "warmer Ink-Grund");
        assert!(html.contains("#ece0c8"), "Bone-Text");
        assert!(html.contains("#c8a86b"), "Gold-Akzent");
        assert!(
            html.contains("/brand/tokens.css"),
            "dl-brand liefert Schriften und Tokens"
        );
        assert!(
            !html.contains("#f0f0f0"),
            "der helle Code-Hintergrund blendet auf dunklem Grund"
        );
    }

    /// Die Seiten sind bewusst maschinenlesbar (FAQ-Bot, Crawler). Styling darf die
    /// Struktur nicht anfassen.
    #[test]
    fn branding_laesst_die_struktur_unangetastet() {
        let html = page("Bot-Befehle", "<h2>Gruppe</h2><ul><li><code>!raid</code></li></ul>");

        assert!(html.contains("<h1>Bot-Befehle</h1>"));
        assert!(html.contains("<h2>Gruppe</h2><ul><li><code>!raid</code></li></ul>"));
        assert!(html.contains("lang=\"de\""));
    }
}
