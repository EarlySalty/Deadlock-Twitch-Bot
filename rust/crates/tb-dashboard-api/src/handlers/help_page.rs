//! Serverseitig gerenderte, öffentliche Hilfe-/Befehlsseiten aus der SSOT.
//! Öffentliche Hilfe und kanalbezogene Chat-Befehle ohne Anmeldung.

use axum::http::{header::LOCATION, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::{extract::Query, Extension};
use pulldown_cmark::{html, Options, Parser};
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use tb_knowledge::{ist_oeffentlich, KnowledgeBase, Namespace};

fn knowledge_dir() -> PathBuf {
    tb_config::runtime::active()
        .and_then(|snapshot| {
            snapshot
                .resolve(&snapshot.settings().knowledge.directory)
                .ok()
        })
        .unwrap_or_else(|| PathBuf::from("rust/knowledge"))
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
        .filter(|d| d.namespace == Namespace::Bot && ist_oeffentlich(&d.audience))
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
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Gemeinsame Schwarz-Gold-Flächen für die öffentlichen Bot-Seiten.
const BRAND_CSS: &str = r#"
:root{color-scheme:dark;--ink:#070707;--panel:#100d0a;--bone:#ece0c8;--bone-dim:#c4bbaa;--gold:#c8a86b;--gold-bright:#efd49d;--line:#66512f}
*{box-sizing:border-box}body{max-width:1120px;margin:auto;padding:2rem 1.25rem 5rem;line-height:1.65;font-family:var(--font-body,"Manrope","Segoe UI",sans-serif);color:var(--bone);background-color:var(--ink);background-image:linear-gradient(#ffffff05 1px,transparent 1px),linear-gradient(90deg,#ffffff05 1px,transparent 1px);background-size:48px 48px;min-height:100vh}
h1,h2,h3{font-family:var(--font-display,"Sora","Manrope",sans-serif);line-height:1.2;letter-spacing:-.02em}h1{font-size:clamp(2rem,5vw,3.2rem);margin:1.6rem 0 1rem}h2{font-size:1.45rem;color:var(--gold-bright);margin:2rem 0 1rem}h3{font-size:1.1rem;color:var(--gold);margin:0 0 .7rem}p,li{color:var(--bone-dim)}a{color:var(--gold-bright);text-decoration:none}a:hover{text-decoration:underline}a:focus-visible,input:focus-visible{outline:3px solid var(--gold-bright);outline-offset:4px}
code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;color:var(--gold-bright);background:#0d0806;border:1px solid var(--line);border-radius:6px;padding:.15em .4em}nav{padding:1.25rem;border:1px solid var(--line);border-radius:14px;background:var(--panel)}nav h2{margin:0 0 .75rem}nav ul{margin:0;padding:0;list-style:none;display:grid;gap:.4rem;grid-template-columns:repeat(auto-fill,minmax(190px,1fr))}article{margin-bottom:1.5rem}table{width:100%;border-collapse:collapse}th,td{border:1px solid var(--line);padding:.5rem}
.brand{display:flex;align-items:center;gap:1rem;font-weight:800;font-size:.95rem}.eyebrow{color:var(--gold);font-size:.8rem;letter-spacing:.12em;text-transform:uppercase}.switch{display:flex;flex-wrap:wrap;gap:.5rem;margin:1.75rem 0;padding:.4rem;background:#0d0806;border:1px solid var(--line);border-radius:14px;width:fit-content}.switch a{padding:.65rem 1rem;border-radius:9px;color:var(--bone-dim)}.switch a[aria-current=page]{background:#211a10;color:var(--gold-bright);box-shadow:inset 0 0 0 1px #a88445}.command-grid{list-style:none;padding:0;display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:1rem}.command-card{padding:1.3rem;border:1px solid #a88445;border-radius:15px;background:var(--panel);box-shadow:inset 0 0 0 1px #ffffff09,0 9px 24px #0009}.command-card p{margin:.75rem 0 0;font-size:.95rem}.aliases{font-size:.8rem!important}.notice{border:1px solid var(--line);border-radius:12px;background:#0d0806;padding:1rem}footer{margin-top:3rem;padding-top:1.5rem;border-top:1px solid var(--line)}@media(max-width:650px){.command-grid{grid-template-columns:1fr}body{padding:1.25rem 1rem 3rem}.switch{width:100%}.switch a{flex:1;text-align:center}}
"#;

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"de\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<meta name=\"theme-color\" content=\"#070707\">\
<title>{t}</title>\
<link rel=\"stylesheet\" href=\"/brand/tokens.css\">\
<style>{css}</style></head>\
<body><a class=\"brand\" href=\"/streamer\">Deutsche Deadlock Community</a><h1>{t}</h1>{b}<footer><a href=\"/streamer\">Zum Partner-Netzwerk</a> · <a href=\"/streamer/help\">Bot-Hilfe</a></footer></body></html>",
        t = html_escape(title),
        b = body,
        css = BRAND_CSS
    )
}

pub async fn help_page() -> Response {
    (StatusCode::OK, Html(render_help(knowledge_base()))).into_response()
}

#[derive(Deserialize, Default)]
pub struct CommandsQuery {
    pub streamer_id: Option<String>,
    pub view: Option<String>,
}

#[derive(Default)]
struct ChannelCommands {
    login: String,
    overrides: BTreeMap<String, String>,
    flags: serde_json::Value,
}

fn command_enabled(name: &str, flags: &serde_json::Value) -> bool {
    if let Some(stat) = tb_chat::stat_commands::StatCommand::from_chat(name) {
        return flags["stat"][stat.key()].as_bool().unwrap_or(true);
    }
    let key = match name {
        "!clip" => "clip",
        "!title" => "title",
        "!lurk" => "lurk",
        _ => return true,
    };
    flags[key].as_bool().unwrap_or(true)
}

fn command_summary(summary: &str, overrides: &BTreeMap<String, String>) -> String {
    let mut output = String::new();
    let mut rest = summary;
    while let Some(index) = rest.find('!') {
        output.push_str(&rest[..index]);
        rest = &rest[index..];
        let end = rest[1..]
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
            .map(|index| index + 1)
            .unwrap_or(rest.len());
        let token = &rest[..end];
        if let Some(entry) = tb_chat::catalog::catalog()
            .iter()
            .find(|entry| entry.name == token || entry.aliases.contains(&token))
        {
            output.push_str(&tb_chat::command_names::effective_name(entry, overrides));
        } else {
            output.push_str(token);
        }
        rest = &rest[end..];
    }
    output.push_str(rest);
    output
}

fn render_commands(query: &CommandsQuery, channel: Option<&ChannelCommands>) -> String {
    let standard = query.view.as_deref() == Some("standard") || channel.is_none();
    let id = query.streamer_id.as_deref().unwrap_or("");
    let mut body = String::from("<p class=\"eyebrow\">Twitch-Bot · Chat-Befehle</p>");
    if let Some(channel) = channel {
        body.push_str(&format!("<p>Die Befehle für <strong>{}</strong>. Hier siehst du die Namen, die der Bot in diesem Kanal kennt.</p>", html_escape(&channel.login)));
        body.push_str(&format!(
            "<nav class=\"switch\" aria-label=\"Befehlsansicht\"><a href=\"?streamer_id={id}\"{custom_current}>Streamer-spezifisch</a><a href=\"?streamer_id={id}&amp;view=standard\"{standard_current}>Standardbefehle</a></nav>",
            id = html_escape(id),
            custom_current = if !standard { " aria-current=\"page\"" } else { "" },
            standard_current = if standard { " aria-current=\"page\"" } else { "" },
        ));
    } else {
        body.push_str("<p>Die Standardbefehle unseres Twitch-Bots. Mit dem Link aus dem Chat siehst du die Befehle des jeweiligen Streamers.</p>");
    }
    if standard && channel.is_some() {
        body.push_str("<p class=\"notice\">Das sind die Standardnamen. Im Kanal gelten die Namen unter „Streamer-spezifisch“.</p>");
    }
    let empty = BTreeMap::new();
    let active = channel.filter(|_| !standard);
    let overrides = active.map(|channel| &channel.overrides).unwrap_or(&empty);
    for (group, commands) in tb_chat::catalog::grouped() {
        let commands: Vec<_> = commands
            .into_iter()
            .filter(|command| {
                active.is_none_or(|channel| command_enabled(command.name, &channel.flags))
            })
            .collect();
        if commands.is_empty() {
            continue;
        }
        body.push_str(&format!(
            "<section><h2>{}</h2><ul class=\"command-grid\">",
            html_escape(group.label())
        ));
        for command in commands {
            let name = tb_chat::command_names::effective_name(command, overrides);
            let summary = command_summary(command.summary, overrides);
            body.push_str(&format!(
                "<li class=\"command-card\"><h3><code>{}</code></h3><p>{}</p>",
                html_escape(&name),
                html_escape(&summary)
            ));
            let aliases = tb_chat::command_names::effective_aliases(command, overrides);
            if !aliases.is_empty() {
                body.push_str(&format!(
                    "<p class=\"aliases\">Auch möglich: {}</p>",
                    aliases
                        .iter()
                        .map(|alias| format!("<code>{}</code>", html_escape(alias)))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
            body.push_str("</li>");
        }
        body.push_str("</ul></section>");
    }
    page("Befehle für deinen Chat", &body)
}

pub async fn commands_page(
    Extension(pool): Extension<PgPool>,
    Query(query): Query<CommandsQuery>,
) -> Response {
    if query
        .view
        .as_deref()
        .is_some_and(|view| view != "standard" && view != "streamer")
    {
        return (
            StatusCode::BAD_REQUEST,
            Html(page(
                "Befehlsansicht unbekannt",
                "<p>Bitte wähle eine der beiden Befehlsansichten.</p>",
            )),
        )
            .into_response();
    }
    let channel = if let Some(id) = &query.streamer_id {
        if id.is_empty() || id.len() > 32 || !id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return (
                StatusCode::BAD_REQUEST,
                Html(page(
                    "Kanal unbekannt",
                    "<p>Dieser Kanal-Link ist ungültig.</p>",
                )),
            )
                .into_response();
        }
        let result = sqlx::query_as::<_, (Option<String>, sqlx::types::Json<BTreeMap<String, String>>, serde_json::Value)>(
            "SELECT COALESCE(i.twitch_login, p.twitch_login), COALESCE(p.command_name_overrides, '{}'::jsonb), jsonb_build_object('stat', COALESCE(p.stat_command_settings, '{}'::jsonb), 'clip', COALESCE(p.clip_command_enabled, 1) <> 0, 'title', COALESCE(p.title_command_enabled, 1) <> 0, 'lurk', COALESCE(p.lurk_command_enabled, 1) <> 0) FROM twitch_streamer_identities i LEFT JOIN streamer_plans p ON p.twitch_user_id = i.twitch_user_id WHERE i.twitch_user_id = $1"
        ).bind(id).fetch_optional(&pool).await;
        match result {
            Ok(Some((login, overrides, flags))) => Some(ChannelCommands { login: login.filter(|login| !login.is_empty()).unwrap_or_else(|| "dieser Kanal".into()), overrides: overrides.0, flags }),
            Ok(None) => return (StatusCode::NOT_FOUND, Html(page("Kanal unbekannt", "<p>Für diesen Kanal ist keine Befehlsliste verfügbar.</p><p><a href=\"/streamer/commands\">Standardbefehle ansehen</a></p>"))).into_response(),
            Err(error) => {
                tracing::error!(%error, "Kanalbezogene Befehlsliste konnte nicht geladen werden");
                return (StatusCode::SERVICE_UNAVAILABLE, Html(page("Befehle gerade nicht verfügbar", "<p>Bitte versuch es gleich noch einmal.</p>"))).into_response();
            }
        }
    } else {
        None
    };
    (
        StatusCode::OK,
        Html(render_commands(&query, channel.as_ref())),
    )
        .into_response()
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
    fn kanalbefehle_zeigen_aktive_namen_und_switch() {
        let channel = ChannelCommands {
            login: "nani".into(),
            overrides: BTreeMap::from([
                ("discord".into(), "!community".into()),
                ("rank".into(), "!rang".into()),
                ("commands".into(), "!liste".into()),
            ]),
            flags: serde_json::json!({"stat":{"wins":false},"clip":false}),
        };
        let query = CommandsQuery {
            streamer_id: Some("42".into()),
            view: None,
        };
        let html = render_commands(&query, Some(&channel));
        assert!(html.contains("<code>!community</code>"));
        assert!(!html.contains("<code>!dldc</code>"));
        assert!(!html.contains("<code>!dlde</code>"));
        assert!(html.contains("!rang me"));
        assert!(!html.contains("<code>!wins</code>"));
        assert!(!html.contains("<code>!clip</code>"));
        assert!(html.contains("Streamer-spezifisch"));
        assert!(html.contains("?streamer_id=42&amp;view=standard"));
        let standard = render_commands(
            &CommandsQuery {
                view: Some("standard".into()),
                ..query
            },
            Some(&channel),
        );
        assert!(standard.contains("<code>!dldc</code>"));
        assert!(standard.contains("<code>!wins</code>"));
        assert!(!standard.contains("<code>!community</code>"));
        assert!(command_summary(
            "!rank und !raid_history",
            &BTreeMap::from([
                ("rank".into(), "!raid".into()),
                ("raid".into(), "!start".into())
            ])
        )
        .contains("!raid und !raid_history"));
        let directory = std::path::Path::new("/tmp/tb-dldc-commands-preview");
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join("streamer.html"), html).unwrap();
        std::fs::write(directory.join("standard.html"), standard).unwrap();
    }

    #[tokio::test]
    async fn registrierter_kanal_ohne_planzeile_bekommt_standardbefehle() {
        let database = crate::test_postgres::TestPostgres::start().await;
        sqlx::raw_sql("CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT); CREATE TABLE streamer_plans (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, command_name_overrides JSONB, stat_command_settings JSONB, clip_command_enabled INTEGER, title_command_enabled INTEGER, lurk_command_enabled INTEGER); INSERT INTO twitch_streamer_identities VALUES ('42','nani'),('99','anders'); INSERT INTO streamer_plans VALUES ('99','anders','{\"discord\":\"!fremd\"}','{}',1,1,1)").execute(&database.pool).await.unwrap();
        let response = commands_page(
            Extension(database.pool.clone()),
            Query(CommandsQuery {
                streamer_id: Some("42".into()),
                view: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 131072)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("nani"));
        assert!(html.contains("<code>!dldc</code>"));
        assert!(!html.contains("!fremd"));
        let response = commands_page(
            Extension(database.pool.clone()),
            Query(CommandsQuery {
                streamer_id: Some("99".into()),
                view: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 131072)
            .await
            .unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("<code>!fremd</code>"));
        assert!(!html.contains("<code>!dldc</code>"));
        let response = commands_page(
            Extension(database.pool),
            Query(CommandsQuery {
                streamer_id: Some("404".into()),
                view: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn md_to_html_basics() {
        let h = md_to_html("Ein **fetter** Text.\n\n- a\n- b");
        assert!(h.contains("<strong>fetter</strong>"));
        assert!(h.contains("<li>a</li>"));
    }

    /// Die Seite haengt ohne Auth im oeffentlichen Router. Ein Doc, das sich an
    /// den Concierge richtet, listet interne Faehigkeiten auf (Admin-Funktionen,
    /// Freischalt-Wege, Lastgrenzen) und darf dort nie erscheinen.
    #[test]
    fn render_help_laesst_nicht_oeffentliche_docs_weg() {
        let kb = KnowledgeBase::load_from_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tb-knowledge/tests/fixtures"),
        )
        .unwrap();
        let html = render_help(&kb);
        assert!(!html.contains("NICHT_FREIGEGEBENE_RAID_MECHANIK"));
        assert!(!html.contains("INTERNER_STREAMER_RAID_SCORE"));
        assert!(!html.contains("ohne-freigabe"));
        assert!(!html.contains("streamer-intern"));
        assert!(
            !html.contains("GEHEIMES_INTERNES_WISSEN"),
            "Concierge-Doc auf der oeffentlichen Seite: {html}"
        );
        assert!(
            !html.contains("nur-concierge"),
            "Concierge-Doc steht im Inhaltsverzeichnis: {html}"
        );
        assert!(html.contains("auto-raid"), "Streamer-Docs fehlen dafuer");
    }

    #[test]
    fn nur_explizit_freigegebene_hilfe_ist_oeffentlich() {
        assert!(!ist_oeffentlich(""));
        assert!(!ist_oeffentlich("streamer"));
        assert!(ist_oeffentlich("public"));
        // Groesste Reichweite: der Deadlock-Namespace nutzt es fuer Wissen,
        // das im Chat an jeden geht.
        assert!(ist_oeffentlich("viewer"));
        assert!(!ist_oeffentlich("concierge"));
        assert!(!ist_oeffentlich("intern"));
        // Unbekannte Zielgruppen bleiben drin, nicht draussen.
        assert!(!ist_oeffentlich("was-auch-immer-morgen-dazukommt"));
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

        assert!(html.contains("#070707"), "schwarzer Grund");
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
        let html = page(
            "Bot-Befehle",
            "<h2>Gruppe</h2><ul><li><code>!raid</code></li></ul>",
        );

        assert!(html.contains("<h1>Bot-Befehle</h1>"));
        assert!(html.contains("<h2>Gruppe</h2><ul><li><code>!raid</code></li></ul>"));
        assert!(html.contains("lang=\"de\""));
    }
}
