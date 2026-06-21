# SP1 / P2 — Konsum-Flächen: Hilfeseite (HTML aus SSOT) + `!commands` + `!help` (Implementierungsplan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Die SSOT-Wissensbasis (P1) als **serverseitig gerendertes HTML** auf der Website konsumierbar machen — eine kanonische Hilfeseite (`/streamer/help`) und eine gruppierte Befehls-Übersicht (`/streamer/commands`), plus die Twitch-Chat-Befehle `!commands` und `!help <thema>`. Die bestehende React-FAQ wird auf die SSOT migriert und retired (nichts doppelt).

**Architecture:** Ein kuratierter Befehls-Katalog in `tb-chat` (`catalog.rs`) ist die einzige Quelle für Chat + Web. Der Rust-Dashboard-Service rendert `/streamer/help` und `/streamer/commands` **serverseitig als schlichtes HTML** (Markdown→HTML via `pulldown-cmark`) — gut maschinen-/AI-lesbar, kein React/JS nötig (User-Direktive: „muss funktionieren, nicht schick"). Die Inhalte der alten React-FAQ (`twitchKnowledgeBase.ts` `FAQ_SECTIONS`) werden als SSOT-Markdown migriert; `/streamer/faq` leitet per 301 auf `/streamer/help`. `!help <thema>` nutzt die P1-Selektion über eine modul-lokale `OnceLock<KnowledgeBase>`.

**Tech Stack:** Rust, Axum, `tb-chat`, `tb-knowledge` (P1), `tb-dashboard-api`. Neu: `pulldown-cmark` (Markdown→HTML, etablierter Standard-Crate).

**Voraussetzung:** **P1 gemergt** (`tb-knowledge` mit `KnowledgeBase`/`Namespace`/`select`/`docs()`; Wissensbasis `rust/knowledge/bot/*.md`).

## Global Constraints

- Rust-Standard, Code nur unter `rust/`/`website/`. Original-Python unangetastet.
- **Neue Rust-Dep erlaubt: `pulldown-cmark`** (Markdown→HTML; sicher, weit verbreitet). Sonst keine neuen Backend-Crates. Frontend: KEIN react-markdown nötig (serverseitig gerendert).
- **User-sichtbare deutsche Texte** (Katalog-`summary`, Chat-Antworten, HTML-Seitentexte, migrierte FAQ-Inhalte) schreibt **Claude**, nicht GPT.
- **Keine DB-Migration.** Katalog = Code-Daten, Doku = Dateien (SSOT).
- Öffentliche Routen CSRF-frei (über `build_website_router`/`build_public_router`). HTML-Seiten sind öffentlich, kein Auth.
- **SSOT ist kanonisch:** Die alte React-FAQ wird ersetzt; `twitchKnowledgeBase.ts` behält aber die **Onboarding-Exports** (`ONBOARDING_VISUAL_STEPS` u.a.) — die braucht **P4**, NICHT löschen.
- Git/Delegation wie P1 (Worktree, Push, CHANGELOG, Discord/In-App; GPT baut, Claude reviewt + schreibt DE-Texte).

---

## Dateistruktur

**Neu:**
- `rust/crates/tb-chat/src/catalog.rs` — `CommandGroup`, `CommandInfo`, `catalog()`, `grouped()`.
- `rust/crates/tb-dashboard-api/src/handlers/help_page.rs` — serverseitiges HTML für `/streamer/help` + `/streamer/commands` + `/streamer/faq`-Redirect.
- `rust/knowledge/bot/*.md` — zusätzliche Docs aus der FAQ-Migration (Claude).

**Geändert:**
- `rust/crates/tb-chat/Cargo.toml` — Dep `tb-knowledge`.
- `rust/crates/tb-chat/src/lib.rs` — `pub mod catalog;`.
- `rust/crates/tb-chat/src/commands.rs` — `!commands`/`!help`-Arme + `cmd_*` + reine Builder + KB-OnceLock.
- `rust/crates/tb-dashboard-api/Cargo.toml` — Deps `tb-chat`, `tb-knowledge`, `pulldown-cmark`.
- `rust/crates/tb-dashboard-api/src/lib.rs` — `build_website_router` um `/streamer/help`, `/streamer/commands`, `/streamer/faq` (vor dem `*path`-Wildcard) erweitern.
- `website/vite.config.ts` — `faq`-Entry entfernen.
- `website/src/faq.tsx`, `website/src/pages/BotFaqPage.tsx`, `website/faq/index.html` — entfernen (retired).
- `website/src/data/twitchKnowledgeBase.ts` — `FAQ_SECTIONS` entfernen, Onboarding-Exports behalten.
- `CHANGELOG.md`.

---

## Task 1: Befehls-Katalog in `tb-chat`

**Files:** Create `rust/crates/tb-chat/src/catalog.rs`; Modify `rust/crates/tb-chat/src/lib.rs`.

**Interfaces:**
- `pub enum CommandGroup { Stats, Match, Mod, Fun }` mit `as_str()` (`"stats"|"match"|"mod"|"fun"`) + `label()` (DE).
- `pub struct CommandInfo { pub name: &'static str, pub group: CommandGroup, pub summary: &'static str }`
- `pub fn catalog() -> &'static [CommandInfo]`; `pub fn grouped() -> Vec<(CommandGroup, Vec<&'static CommandInfo>)>`.

- [ ] **Step 1: Failing test + Typen** — `catalog.rs`:

```rust
//! Kuratierter, user-sichtbarer Befehls-Katalog — einzige Quelle für die
//! `!commands`-Chat-Antwort und die /streamer/commands-Seite. NICHT jeder
//! interne Befehl steht hier.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandGroup { Stats, Match, Mod, Fun }

impl CommandGroup {
    pub fn as_str(&self) -> &'static str {
        match self { CommandGroup::Stats => "stats", CommandGroup::Match => "match", CommandGroup::Mod => "mod", CommandGroup::Fun => "fun" }
    }
    pub fn label(&self) -> &'static str {
        match self { CommandGroup::Stats => "Statistik", CommandGroup::Match => "Match", CommandGroup::Mod => "Moderation", CommandGroup::Fun => "Sonstiges" }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CommandInfo { pub name: &'static str, pub group: CommandGroup, pub summary: &'static str }

pub fn catalog() -> &'static [CommandInfo] { unimplemented!() }
pub fn grouped() -> Vec<(CommandGroup, Vec<&'static CommandInfo>)> { unimplemented!() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn katalog_eindeutig_und_valide() {
        let cat = catalog();
        assert!(!cat.is_empty());
        let mut n: Vec<_> = cat.iter().map(|c| c.name).collect();
        n.sort(); let before = n.len(); n.dedup();
        assert_eq!(before, n.len(), "Namen eindeutig");
        for c in cat { assert!(c.name.starts_with('!')); assert!(!c.summary.trim().is_empty()); }
    }
    #[test]
    fn grouped_summiert_auf_alle() {
        assert_eq!(grouped().iter().map(|(_, v)| v.len()).sum::<usize>(), catalog().len());
    }
    #[test]
    fn help_und_commands_im_katalog() {
        let n: Vec<_> = catalog().iter().map(|c| c.name).collect();
        assert!(n.contains(&"!help") && n.contains(&"!commands"));
    }
}
```

- [ ] **Step 2: Test → FAIL** — `cargo test -p tb-chat catalog` (unimplemented).

- [ ] **Step 3: `catalog()`+`grouped()` implementieren** (Claude-`summary`):

```rust
pub fn catalog() -> &'static [CommandInfo] {
    use CommandGroup::*;
    &[
        CommandInfo { name: "!rank", group: Stats, summary: "Zeigt deinen aktuellen Deadlock-Rang im Chat." },
        CommandInfo { name: "!commands", group: Fun, summary: "Liste aller Bot-Befehle (Link zur Übersicht)." },
        CommandInfo { name: "!help", group: Fun, summary: "Kurzerklärung zu einem Feature: !help <thema>." },
        CommandInfo { name: "!clip", group: Fun, summary: "Erstellt einen Clip vom aktuellen Stream." },
        CommandInfo { name: "!raid", group: Mod, summary: "Startet einen Raid zu einem Deadlock-Streamer (Mods/Broadcaster)." },
        CommandInfo { name: "!invite", group: Fun, summary: "Postet den Einladungslink zur Community." },
    ]
}

pub fn grouped() -> Vec<(CommandGroup, Vec<&'static CommandInfo>)> {
    use CommandGroup::*;
    [Stats, Match, Mod, Fun].iter().map(|g| {
        let items: Vec<&'static CommandInfo> = catalog().iter().filter(|c| c.group == *g).collect();
        (*g, items)
    }).filter(|(_, v)| !v.is_empty()).collect()
}
```

> `!rank` ist Schau-Eintrag (Chat-Befehl erst in P5). Katalog = kuratierte Liste, nicht die Dispatch-Tabelle.

- [ ] **Step 4: `lib.rs`** — `pub mod catalog;`.
- [ ] **Step 5: Test → PASS** — `cargo test -p tb-chat catalog`.
- [ ] **Step 6: Commit** — `git commit -m "feat(tb-chat): kuratierter Befehls-Katalog (SSOT)"`.

---

## Task 2: Chat-Befehle `!commands` + `!help`

**Files:** Modify `rust/crates/tb-chat/Cargo.toml` (Dep `tb-knowledge`), `rust/crates/tb-chat/src/commands.rs`.

**Interfaces (intern):** `fn commands_reply() -> String`; `fn help_reply(kb: &KnowledgeBase, topic: &str) -> String`; `fn knowledge_base() -> &'static KnowledgeBase`; Dispatch-Arme `"!commands"`/`"!help"`.

- [ ] **Step 1: Dep** — `tb-knowledge = { workspace = true }` in `tb-chat/Cargo.toml`.

- [ ] **Step 2: Failing test** (rein, nutzt P1-Fixtures) im `#[cfg(test)] mod tests` von `commands.rs`:

```rust
    fn help_fixture_kb() -> tb_knowledge::KnowledgeBase {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tb-knowledge/tests/fixtures");
        tb_knowledge::KnowledgeBase::load_from_dir(&root).expect("fixtures")
    }
    #[test]
    fn commands_reply_zeigt_link() { assert!(commands_reply().contains("/streamer/commands")); }
    #[test]
    fn help_reply_findet_thema() {
        let kb = help_fixture_kb();
        let r = help_reply(&kb, "raid");
        assert!(r.contains("Auto-Raid") && r.contains("/streamer/help#auto-raid"), "{r}");
    }
    #[test]
    fn help_reply_unbekannt_fallback() {
        let kb = help_fixture_kb();
        let r = help_reply(&kb, "quantenphysik");
        assert!(r.contains("/streamer/help") && !r.contains('#'), "{r}");
    }
```

- [ ] **Step 3: Test → FAIL** — `cargo test -p tb-chat commands_reply`.

- [ ] **Step 4: Builder + OnceLock** (Modulebene in `commands.rs`):

```rust
use std::path::PathBuf;
use std::sync::OnceLock;
use tb_knowledge::{KnowledgeBase, Namespace};

const HELP_BASE_URL: &str = "https://deutsche-deadlock-community.de/streamer/help";
const COMMANDS_URL: &str = "https://deutsche-deadlock-community.de/streamer/commands";

fn knowledge_dir() -> PathBuf {
    match std::env::var("KNOWLEDGE_DIR").ok().filter(|v| !v.trim().is_empty()) {
        Some(p) => PathBuf::from(p), None => PathBuf::from("rust/knowledge"),
    }
}
fn knowledge_base() -> &'static KnowledgeBase {
    static KB: OnceLock<KnowledgeBase> = OnceLock::new();
    KB.get_or_init(|| KnowledgeBase::load_from_dir(&knowledge_dir()).unwrap_or_default())
}
fn commands_reply() -> String { format!("Alle Befehle findest du hier: {COMMANDS_URL}") }
fn help_reply(kb: &KnowledgeBase, topic: &str) -> String {
    let topic = topic.trim();
    if topic.is_empty() { return format!("Sag mir ein Thema, z. B. !help raid — oder schau hier: {HELP_BASE_URL}"); }
    match kb.select(topic, Namespace::Bot, None, 1).first() {
        Some(doc) => format!("{}: {HELP_BASE_URL}#{}", doc.title, doc.slug),
        None => format!("Dazu habe ich nichts gefunden — schau hier: {HELP_BASE_URL}"),
    }
}
```

- [ ] **Step 5: Dispatch-Arme** in `CommandEngine::handle` vor `_ => false`:

```rust
            "!commands" => { self.cmd_commands(event).await; true }
            "!help" => { self.cmd_help(event, args).await; true }
```

und Methoden:

```rust
    async fn cmd_commands(&self, event: &ChatMessageEvent) { self.reply(event, &commands_reply()).await; }
    async fn cmd_help(&self, event: &ChatMessageEvent, args: &str) { self.reply(event, &help_reply(knowledge_base(), args)).await; }
```

- [ ] **Step 6: Test → PASS** — `cargo test -p tb-chat`.
- [ ] **Step 7: Commit** — `git commit -m "feat(tb-chat): !commands + !help (KB-gestützt)"`.

---

## Task 3: Serverseitiges HTML — `/streamer/help` + `/streamer/commands` + FAQ-Redirect

**Files:** Modify `rust/crates/tb-dashboard-api/Cargo.toml`; Create `rust/crates/tb-dashboard-api/src/handlers/help_page.rs`; Modify `rust/crates/tb-dashboard-api/src/lib.rs`.

**Interfaces:**
- `pub async fn help_page() -> axum::response::Response` → `GET /streamer/help` → `text/html` (alle bot-Docs als Abschnitte, Markdown→HTML, `id=<slug>`-Anker).
- `pub async fn commands_page() -> axum::response::Response` → `GET /streamer/commands` → `text/html` (gruppierte Befehle).
- `pub async fn faq_redirect(uri: axum::http::Uri) -> axum::response::Response` → `GET /streamer/faq` → 301 nach `/streamer/help`.

- [ ] **Step 1: Deps** — in `tb-dashboard-api/Cargo.toml`: `tb-chat = { workspace = true }`, `tb-knowledge = { workspace = true }`, `pulldown-cmark = "0.10"` (in `rust/Cargo.toml` `[workspace.dependencies]` aufnehmen: `pulldown-cmark = "0.10"`, hier `{ workspace = true }`). (Version an aktuellem Lockfile ausrichten; 0.10+ ist stabil.)

- [ ] **Step 2: Failing test** — `help_page.rs`:

```rust
//! Serverseitig gerenderte, öffentliche Hilfe-/Befehlsseiten aus der SSOT.
//! Schlichtes, maschinen-/AI-lesbares HTML (keine DB, kein Auth).

use axum::http::{StatusCode, Uri};
use axum::response::{Html, IntoResponse, Redirect, Response};
use std::path::PathBuf;
use std::sync::OnceLock;

use pulldown_cmark::{html, Options, Parser};
use tb_knowledge::{KnowledgeBase, Namespace};

fn knowledge_dir() -> PathBuf {
    match std::env::var("KNOWLEDGE_DIR").ok().filter(|v| !v.trim().is_empty()) {
        Some(p) => PathBuf::from(p), None => PathBuf::from("rust/knowledge"),
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
    let mut docs: Vec<_> = kb.docs().iter().filter(|d| d.namespace == Namespace::Bot).collect();
    docs.sort_by(|a, b| a.slug.cmp(&b.slug));
    for d in docs {
        sections.push_str(&format!(
            "<section id=\"{slug}\"><h2>{title}</h2>{body}</section>\n",
            slug = d.slug, title = html_escape(&d.title), body = md_to_html(&d.body)
        ));
    }
    page("Hilfe & Wissen zum Bot", &format!(
        "<p>Hier findest du, was der Bot kann und wie du ihn einrichtest.</p>\n{sections}"
    ))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"de\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{t}</title><style>body{{max-width:760px;margin:2rem auto;padding:0 1rem;\
font-family:system-ui,sans-serif;line-height:1.5}}h1,h2{{line-height:1.2}}\
code{{background:#f0f0f0;padding:.1em .3em;border-radius:3px}}</style></head>\
<body><h1>{t}</h1>{b}</body></html>",
        t = html_escape(title), b = body
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
            body.push_str(&format!("<li><code>{}</code> — {}</li>", html_escape(c.name), html_escape(c.summary)));
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
    Redirect::permanent(&loc).into_response()
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
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tb-knowledge/tests/fixtures")
        ).unwrap();
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
```

- [ ] **Step 3: Test → FAIL** — `cargo test -p tb-dashboard-api help_page` (Modul/Deps fehlen).

- [ ] **Step 4: Module + Routen registrieren** in `lib.rs`:
  - `pub mod help_page;` im handlers-Baum.
  - In `build_website_router()` **vor** `/streamer/*path` einhängen:

```rust
        .route("/streamer/help", get(handlers::help_page::help_page))
        .route("/streamer/commands", get(handlers::help_page::commands_page))
        .route("/streamer/faq", get(handlers::help_page::faq_redirect))
```

- [ ] **Step 5: Test → PASS** + Router-Overlap-Test bleibt grün — `cargo test -p tb-dashboard-api`.
- [ ] **Step 6: Commit** — `git commit -m "feat(dashboard): /streamer/help + /streamer/commands serverseitig aus SSOT"`.

---

## Task 4: FAQ-Migration in die SSOT + alte React-FAQ retiren

> **Wer:** Claude migriert die deutschen FAQ-Inhalte (Inhalt = `FAQ_SECTIONS` in `website/src/data/twitchKnowledgeBase.ts`, ~42 Fragen) als zusätzliche SSOT-Dokumente. GPT macht das Entfernen der React-FAQ (technisch).

**Files:** Create `rust/knowledge/bot/faq-*.md` (Claude); Modify `website/vite.config.ts`, `website/src/data/twitchKnowledgeBase.ts`; Delete `website/src/faq.tsx`, `website/src/pages/BotFaqPage.tsx`, `website/faq/index.html`.

- [ ] **Step 1: FAQ-Inhalte als SSOT-Docs (Claude)** — Pro FAQ-Sektion (`einstieg`, `analytics`, `raids`, `community`, `affiliate`, …) ein `rust/knowledge/bot/faq-<id>.md` mit Frontmatter (`namespace: bot`, `category: faq`, `audience: streamer`, `tip_eligible: false`) und Body = die Fragen/Antworten der Sektion als Markdown (`### Frage`\n Antwort). Inhalt 1:1 aus `FAQ_SECTIONS` übernehmen (es ist bereits gepflegtes Deutsch). Affiliate-/Pricing-FAQ ggf. `category: affiliate`.

- [ ] **Step 2: Seed-Test erweitern** (`rust/crates/tb-knowledge/tests/seed.rs`) — assert, dass mindestens eine `faq-*`-Doc lädt und eine typische FAQ-Frage ("Was kostet" / "Was ist Deadlock Community") das passende Doc selektiert. `cargo test -p tb-knowledge --test seed`.

- [ ] **Step 3: React-FAQ entfernen (GPT)** — `faq`-Eintrag aus `website/vite.config.ts` `rollupOptions.input` löschen; `website/faq/index.html`, `website/src/faq.tsx`, `website/src/pages/BotFaqPage.tsx` löschen; in `website/src/data/twitchKnowledgeBase.ts` den `FAQ_SECTIONS`-Export + die nur dafür genutzten Typen (`FaqSection`/`FaqItem`) entfernen — **Onboarding-Exports (`ONBOARDING_VISUAL_STEPS`, `ONBOARDING_HIGHLIGHTS`, `START_CHECKLIST`) bleiben** (P4-Abhängigkeit). Prüfen, dass nichts anderes `FAQ_SECTIONS` importiert (`rg FAQ_SECTIONS website/src`).

- [ ] **Step 4: Frontend baut** — `cd website && npm run build` fehlerfrei (keine toten Imports).

- [ ] **Step 5: Commit** — `git commit -m "feat(knowledge): FAQ in SSOT migriert; alte React-FAQ entfernt (/streamer/faq→/help)"`.

---

## Task 5: Verifikation, CHANGELOG, Push, Spiegelung

- [ ] **Step 1: Gesamt** — `cargo build/test/clippy/fmt -p tb-chat -p tb-dashboard-api -p tb-knowledge`; `cd website && npm run build`. Alles grün.
- [ ] **Step 2: CHANGELOG (Claude, oben)** — `## #N — Hilfeseite und Befehls-Übersicht`. Drei Schläge: (1) Streamer mussten raten, was der Bot kann; FAQ lag separat in der Website; (2) jetzt zentrale Hilfeseite + Befehls-Übersicht direkt aus derselben gepflegten Wissensbasis wie der Erklär-Chat, plus `!commands`/`!help <thema>` im Chat; die alte FAQ ist dorthin umgezogen; (3) eine Quelle, kein doppelter Pflegeaufwand. Kein Datei-/Funktionsname.
- [ ] **Step 3: Commit + Push + Merge main + Cleanup** (wie P1).
- [ ] **Step 4: Spiegelung** In-App + Discord (`target:"twitch"`).
- [ ] **Step 5: Live-Smoke** — `/streamer/help` zeigt Bot-Wissen + FAQ; `/streamer/help#auto-raid` springt; `/streamer/commands` listet gruppiert; `/streamer/faq` leitet 301 auf `/help`; im Chat `!commands` → Link, `!help raid` → „Auto-Raid: …#auto-raid".

---

## Self-Review (vom Plan-Autor)

**1. Spec-Coverage (§6 + FAQ-Zentralisierung-Lock):** Hilfeseite ✓ (T3); `!help`/`!commands` ✓ (T2); gruppierte Befehlsseite ✓ (T1/T3); **eine kanonische SSOT, FAQ migriert + retired** ✓ (T4); serverseitiges HTML (User-Direktive) ✓ (T3, `pulldown-cmark`). Onboarding-Exports bleiben für P4 ✓.

**2. Placeholder-Scan:** Voller Code für Katalog, Chat-Builder, HTML-Render, Redirect, Tests. T4 Step 1/Step 3 trennt sauber Claude-Inhalt vs. GPT-Technik; FAQ-Migration ist Inhalt aus vorhandenem `FAQ_SECTIONS` (kein Erfinden).

**3. Typ-Konsistenz:** `catalog()`/`grouped()` (T1) → `commands_page` rendert `g.label()`/`c.name`/`c.summary` (T3) ✓; `help_reply` Deep-Link `…/help#<slug>` (T2) ↔ `render_help` `id="<slug>"` (T3) ✓; `KnowledgeBase::docs()`/`select` aus P1 ✓.

**Scope-Grenze P2:** Keine DB, kein LLM, kein Tipp/Wizard/`!rank`. P2 endet: kanonische SSOT-Hilfe-/Befehlsseiten als HTML, FAQ migriert, zwei Chat-Befehle — live verifiziert.
