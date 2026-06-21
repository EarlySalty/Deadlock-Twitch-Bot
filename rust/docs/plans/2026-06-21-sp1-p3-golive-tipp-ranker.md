# SP1 / P3 — Go-Live-Tipp + Ranker (Implementierungsplan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wenn ein Streamer mit Deadlock live geht, postet der Bot **eine** kuratierte Tipp-Nachricht als erste Chat-Nachricht — ausgewählt von einem gewichtet-abklingenden Ranker, der unbenutzte Features hochzieht und vergessene Perlen als Reminder zurückbringt, mit hartem ≥12h-Abstand und Dashboard-Opt-out.

**Architecture:** Der bereits existierende (aktuell noop) Hook `EventSubHooks::on_stream_went_live` wird befüllt. Ein neuer Crate `tb-tips` kapselt die Orchestrierung: Deadlock-Check → Gates (Opt-out, ≥12h) → Ranker wählt aus den `tip_eligible`-Dokumenten der SSOT (P1) den besten Tipp → `ChatApi::send_message`. Der Ranker ist eine **reine Funktion** in `tb-knowledge` (testbar ohne DB/Netz). Pro-Streamer-Zustand (Opt-out, letzter Tipp, gezeigte Tipps, Feature-Nutzung) lebt in neuen Postgres-Tabellen. Die Tipp-Texte sind kuratierte `tip_text`-Frontmatter-Einzeiler (Claude), wiederverwendet aus bestehenden Feature-Texten.

**Tech Stack:** Rust, Axum/sqlx (Postgres), `tb-monitoring` (Hook + `StreamSnapshot`), `tb-chat` (`ChatApi`), `tb-knowledge` (SSOT + Ranker), neue Crate `tb-tips`.

**Voraussetzung:** **P1 gemergt** (`tb-knowledge` mit `KnowledgeDoc`, `KnowledgeBase`, Frontmatter-Parser; Wissensbasis `rust/knowledge/bot/*.md`).

## Global Constraints

- Rust-Standard, Code nur unter `rust/`. Keine neuen externen Crates (nur Workspace-intern + bereits vorhandene: sqlx, chrono, async-trait, tracing, thiserror).
- **Modell:** keiner — der Tipp ist **kuratierter Text**, KEIN LLM beim Go-Live (deterministisch, kein Halluzinationsrisiko, du kontrollierst den Ton).
- **User-sichtbare deutsche Texte** (`tip_text` in den Docs, Dashboard-Opt-out-Label) schreibt **Claude**.
- **Gates (gelockt):** **≥12h** seit letztem Tipp (hart) + **Opt-out** (Dashboard-Flag, global). **KEIN Aktivitäts-Gate, KEIN Delay** — feuert sofort beim `stream.online` (Deadlock).
- **Ranker (gelockt):** gewichtet-abklingend, NICHT binär (nie genutzt ↑, lange-her ↑ = Reminder, niedriges `time_to_value` ↑, kürzlich gezeigt ↓). Gewichte = Defaults dieses Plans (später tunebar).
- **Inhalts-Mix:** Hauptgewicht Feature-Unlock + Flywheel-Nudge (`tip_flags`); unlock-orientiert formulieren („verbinde Steam → dann geht !rank").
- DB: Postgres/sqlx, neue Migration `rust/migrations/` (timestamp-SQL, `CREATE TABLE IF NOT EXISTS`, `TIMESTAMPTZ DEFAULT NOW()`). Tests: reine Logik als `#[test]`; DB als `#[tokio::test]` mit `TB_TEST_DATABASE_URL`/`pool_or_skip!`.
- Git/Delegation wie P1/P2 (Worktree, Push, CHANGELOG, Discord/In-App; GPT baut, Claude reviewt + schreibt DE-Texte).

---

## Dateistruktur

**Neu:**
- `rust/crates/tb-knowledge/src/tips.rs` — `TipState`, `rank_tip` (reine Ranker-Funktion).
- `rust/crates/tb-tips/Cargo.toml` + `src/lib.rs` — Orchestrierung (Gates + Auswahl + Send) + DB-Repo.
- `rust/migrations/20260621070000_golive_tips.sql` — Tipp-State/Historie/Feature-Usage.

**Geändert:**
- `rust/crates/tb-knowledge/src/doc.rs` — Frontmatter-Feld `tip_text`.
- `rust/crates/tb-knowledge/src/base.rs` — `eligible_tips()`-Accessor.
- `rust/crates/tb-knowledge/src/lib.rs` — `mod tips;` + Re-Exports.
- `rust/knowledge/bot/*.md` — `tip_text:` in den tip-eligiblen Docs (Claude).
- `rust/Cargo.toml` — `tb-tips` zu members + workspace.deps.
- Binary-Wiring (das `EventSubHooks` konstruiert — in Task 5 lokalisiert) — `GoLiveTipHook` einhängen.
- `CHANGELOG.md`.

---

## Task 1: `tip_text`-Frontmatter + `eligible_tips()`

**Files:**
- Modify: `rust/crates/tb-knowledge/src/doc.rs`
- Modify: `rust/crates/tb-knowledge/src/base.rs`

**Interfaces:**
- Produces: `KnowledgeDoc.tip_text: String` (optional, default ""); `KnowledgeBase::eligible_tips(&self) -> Vec<&KnowledgeDoc>` (alle mit `tip_eligible == true` und nicht-leerem `tip_text`).

- [ ] **Step 1: Failing test in `doc.rs` ergänzen**

Im `#[cfg(test)] mod tests` von `doc.rs`:

```rust
    #[test]
    fn parst_tip_text() {
        let raw = "---\ntitle: Auto-Raid\nnamespace: bot\ntip_eligible: true\ntip_text: Du gehst offline? Der Bot raidet deine Zuschauer automatisch weiter.\n---\nbody";
        let d = parse_doc(raw, "auto-raid").unwrap();
        assert_eq!(d.tip_text, "Du gehst offline? Der Bot raidet deine Zuschauer automatisch weiter.");
    }
```

- [ ] **Step 2: Test ausführen → FAIL**

Run: `cargo test -p tb-knowledge parst_tip_text`
Expected: FAIL — `tip_text`-Feld existiert nicht (Compile-Error).

- [ ] **Step 3: Feld + Parsing ergänzen**

In `doc.rs` `KnowledgeDoc` um `pub tip_text: String,` erweitern; im Parser eine lokale `let mut tip_text = String::new();`, im match-Block `"tip_text" => tip_text = value.to_string(),`, und im `Ok(KnowledgeDoc { … tip_text, … })` einsetzen. (Bestehende Default-Tests bleiben grün, da Default `""`.)

- [ ] **Step 4: Failing test für `eligible_tips` in `base.rs`**

Im `#[cfg(test)] mod select_tests` (oder neuem `tips_tests`) von `base.rs`:

```rust
    #[test]
    fn eligible_tips_filtert_korrekt() {
        let a = crate::doc::parse_doc("---\ntitle: A\nnamespace: bot\ntip_eligible: true\ntip_text: Tipp A\n---\nx", "a").unwrap();
        let b = crate::doc::parse_doc("---\ntitle: B\nnamespace: bot\ntip_eligible: false\n---\nx", "b").unwrap();
        let c = crate::doc::parse_doc("---\ntitle: C\nnamespace: bot\ntip_eligible: true\n---\nx", "c").unwrap(); // kein tip_text
        let kb = KnowledgeBase { docs: vec![a, b, c] };
        let tips = kb.eligible_tips();
        assert_eq!(tips.len(), 1);
        assert_eq!(tips[0].slug, "a");
    }
```

- [ ] **Step 5: `eligible_tips` implementieren**

In `impl KnowledgeBase`:

```rust
    /// Alle als Tipp ausspielbaren Dokumente (tip_eligible + nicht-leerer tip_text).
    pub fn eligible_tips(&self) -> Vec<&KnowledgeDoc> {
        self.docs
            .iter()
            .filter(|d| d.tip_eligible && !d.tip_text.trim().is_empty())
            .collect()
    }
```

- [ ] **Step 6: Tests grün + Commit**

Run: `cargo test -p tb-knowledge`
Expected: PASS.
```bash
git add rust/crates/tb-knowledge/src/doc.rs rust/crates/tb-knowledge/src/base.rs
git commit -m "feat(tb-knowledge): tip_text-Frontmatter + eligible_tips()"
```

---

## Task 2: Ranker (reine Funktion `rank_tip`)

**Files:**
- Create: `rust/crates/tb-knowledge/src/tips.rs`
- Modify: `rust/crates/tb-knowledge/src/lib.rs`

**Interfaces:**
- Consumes: `KnowledgeDoc`.
- Produces:
  - `pub struct TipState { pub feature_used_days_ago: Option<i64>, pub tip_shown_days_ago: Option<i64> }` — pro Doc-Slug der Streamer-Zustand (`None` = nie).
  - `pub fn rank_tip<'a>(eligible: &[&'a KnowledgeDoc], state: &std::collections::HashMap<String, TipState>) -> Option<&'a KnowledgeDoc>` — höchster Score, Tie-break niedriges `time_to_value` dann Slug; gibt das zu sendende Doc oder `None` (keine Tipps).

- [ ] **Step 1: Failing test**

`rust/crates/tb-knowledge/src/tips.rs`:

```rust
//! Gewichtet-abklingender Tipp-Ranker (rein, deterministisch, kein DB/Netz).
//! Höchster Score gewinnt den nächsten Go-Live-Slot.

use std::collections::HashMap;

use crate::doc::KnowledgeDoc;

/// Pro-Streamer-Zustand zu EINEM Doc (Slug). `None` = noch nie.
#[derive(Debug, Clone, Copy, Default)]
pub struct TipState {
    pub feature_used_days_ago: Option<i64>,
    pub tip_shown_days_ago: Option<i64>,
}

pub fn rank_tip<'a>(
    _eligible: &[&'a KnowledgeDoc],
    _state: &HashMap<String, TipState>,
) -> Option<&'a KnowledgeDoc> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::parse_doc;

    fn doc(slug: &str, ttv: u8) -> KnowledgeDoc {
        parse_doc(
            &format!("---\ntitle: {slug}\nnamespace: bot\ntip_eligible: true\ntip_text: T\ntime_to_value: {ttv}\n---\nx"),
            slug,
        ).unwrap()
    }

    #[test]
    fn nie_genutztes_feature_gewinnt_gegen_genutztes() {
        let a = doc("a", 3); // nie genutzt
        let b = doc("b", 3); // gerade genutzt
        let docs = vec![&a, &b];
        let mut state = HashMap::new();
        state.insert("b".to_string(), TipState { feature_used_days_ago: Some(0), tip_shown_days_ago: None });
        let pick = rank_tip(&docs, &state).unwrap();
        assert_eq!(pick.slug, "a", "unbenutztes Feature wird bevorzugt");
    }

    #[test]
    fn lange_nicht_genutzt_schlaegt_kuerzlich_genutzt() {
        let a = doc("a", 3);
        let b = doc("b", 3);
        let docs = vec![&a, &b];
        let mut state = HashMap::new();
        state.insert("a".to_string(), TipState { feature_used_days_ago: Some(60), tip_shown_days_ago: None });
        state.insert("b".to_string(), TipState { feature_used_days_ago: Some(1), tip_shown_days_ago: None });
        assert_eq!(rank_tip(&docs, &state).unwrap().slug, "a", "vergessenes Feature kommt als Reminder zurück");
    }

    #[test]
    fn kuerzlich_gezeigter_tipp_wird_gedaempft() {
        let a = doc("a", 3);
        let b = doc("b", 3);
        let docs = vec![&a, &b];
        let mut state = HashMap::new();
        // beide nie genutzt, aber a wurde gerade erst als Tipp gezeigt
        state.insert("a".to_string(), TipState { feature_used_days_ago: None, tip_shown_days_ago: Some(0) });
        assert_eq!(rank_tip(&docs, &state).unwrap().slug, "b", "nicht zweimal hintereinander derselbe Tipp");
    }

    #[test]
    fn leere_liste_gibt_none() {
        assert!(rank_tip(&[], &HashMap::new()).is_none());
    }

    #[test]
    fn niedriges_ttv_gewinnt_bei_gleichstand() {
        let a = doc("a", 1);
        let b = doc("b", 5);
        let docs = vec![&a, &b];
        // identischer Zustand (beide nie genutzt/gezeigt) → ttv entscheidet
        assert_eq!(rank_tip(&docs, &HashMap::new()).unwrap().slug, "a");
    }
}
```

- [ ] **Step 2: Test ausführen → FAIL**

Run: `cargo test -p tb-knowledge tips`
Expected: FAIL — `unimplemented!()`.

- [ ] **Step 3: `rank_tip` implementieren**

In `tips.rs` ersetzen:

```rust
pub fn rank_tip<'a>(
    eligible: &[&'a KnowledgeDoc],
    state: &HashMap<String, TipState>,
) -> Option<&'a KnowledgeDoc> {
    eligible
        .iter()
        .copied()
        .map(|d| (score(d, state.get(&d.slug).copied().unwrap_or_default()), d))
        .max_by(|a, b| {
            a.0.cmp(&b.0)
                .then(b.1.time_to_value.cmp(&a.1.time_to_value)) // niedrigeres ttv gewinnt
                .then(b.1.slug.cmp(&a.1.slug)) // Slug aufsteigend (b vs a invertiert für max)
        })
        .map(|(_, d)| d)
}

/// Gewichtet-abklingend. Defaults bewusst grob & später tunebar.
fn score(doc: &KnowledgeDoc, st: TipState) -> i64 {
    // Basis: niedriges time_to_value → höher (ttv 1..5 → 50..10).
    let mut s = (6 - doc.time_to_value.clamp(1, 5) as i64) * 10;
    // Feature-Nutzung: nie genutzt = großer Boost; lange her = Reminder-Boost (gedeckelt).
    match st.feature_used_days_ago {
        None => s += 50,
        Some(days) => s += days.clamp(0, 30),
    }
    // Kürzlich als Tipp gezeigt → dämpfen (nicht zweimal hintereinander).
    if let Some(days) = st.tip_shown_days_ago {
        if days < 14 {
            s -= 40 - days.clamp(0, 14) * 2; // 0 Tage = -40, 14 Tage = ~-12
        }
    }
    s
}
```

- [ ] **Step 4: In `lib.rs` einhängen**

```rust
pub mod tips;
```
und Re-Export: `pub use tips::{rank_tip, TipState};`

- [ ] **Step 5: Tests grün + Commit**

Run: `cargo test -p tb-knowledge`
Expected: PASS (alle + 5 Ranker-Tests).
```bash
git add rust/crates/tb-knowledge/src/tips.rs rust/crates/tb-knowledge/src/lib.rs
git commit -m "feat(tb-knowledge): gewichtet-abklingender Tipp-Ranker"
```

---

## Task 3: DB-Migration + Repo (Tipp-State / Historie / Feature-Usage)

**Files:**
- Create: `rust/migrations/20260621070000_golive_tips.sql`
- Create: `rust/crates/tb-tips/Cargo.toml`, `rust/crates/tb-tips/src/repo.rs` (+ `lib.rs` mod)
- Modify: `rust/Cargo.toml` (members + workspace.deps)

**Interfaces:**
- Produces (in `tb-tips::repo`):
  - `pub async fn tip_settings(pool, twitch_user_id) -> Result<TipSettings, sqlx::Error>` → `{ opt_out: bool, last_tip_sent_at: Option<DateTime<Utc>> }`
  - `pub async fn load_tip_state(pool, twitch_user_id, slugs: &[String]) -> Result<HashMap<String, TipState>, sqlx::Error>` (joint Feature-Usage + letzte Tipp-Anzeige in `tb_knowledge::TipState`)
  - `pub async fn record_tip_shown(pool, twitch_user_id, slug) -> Result<(), sqlx::Error>` (Insert Historie + Upsert `last_tip_sent_at`)
  - `pub async fn record_feature_used(pool, twitch_user_id, feature) -> Result<(), sqlx::Error>` (Upsert Usage)

- [ ] **Step 1: Migration schreiben**

`rust/migrations/20260621070000_golive_tips.sql`:

```sql
-- Go-Live-Tipp-System: pro-Streamer Opt-out + Cap-Timestamp, Tipp-Historie,
-- Feature-Nutzungs-Events. Keyed auf twitch_user_id (stabil).

CREATE TABLE IF NOT EXISTS public.twitch_tip_settings (
    twitch_user_id     TEXT PRIMARY KEY,
    opt_out            BOOLEAN     NOT NULL DEFAULT FALSE,
    last_tip_sent_at   TIMESTAMPTZ,
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS public.twitch_tip_history (
    id              BIGSERIAL PRIMARY KEY,
    twitch_user_id  TEXT        NOT NULL,
    tip_slug        TEXT        NOT NULL,
    shown_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_tip_history_user ON public.twitch_tip_history (twitch_user_id, shown_at DESC);

CREATE TABLE IF NOT EXISTS public.twitch_feature_usage (
    twitch_user_id  TEXT        NOT NULL,
    feature         TEXT        NOT NULL,
    last_used_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    use_count       INTEGER     NOT NULL DEFAULT 1,
    PRIMARY KEY (twitch_user_id, feature)
);
```

- [ ] **Step 2: Crate-Skelett + Cargo**

`rust/Cargo.toml`: `"crates/tb-tips"` zu `members`; `tb-tips = { path = "crates/tb-tips" }` zu `[workspace.dependencies]`.

`rust/crates/tb-tips/Cargo.toml`:

```toml
[package]
name = "tb-tips"
version = "0.1.0"
edition = "2021"

[dependencies]
tb-knowledge = { workspace = true }
chrono       = { workspace = true }
sqlx         = { workspace = true }
tracing      = { workspace = true }
async-trait  = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }
```

(`sqlx`-Features kommen aus dem Workspace-Default wie bei `tb-db`/`tb-dashboard-api` — Postgres. Falls `tb-tips` eine eigene Feature-Liste braucht, an `tb-dashboard-api/Cargo.toml` orientieren.)

- [ ] **Step 3: Failing DB-Test (mit `TB_TEST_DATABASE_URL`)**

`rust/crates/tb-tips/src/repo.rs` — zunächst Signaturen + Test (Schema-Isolation wie in `tb-chat`/`ai_chat`-Tests):

```rust
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tb_knowledge::TipState;

#[derive(Debug, Clone, Default)]
pub struct TipSettings {
    pub opt_out: bool,
    pub last_tip_sent_at: Option<DateTime<Utc>>,
}

pub async fn tip_settings(_pool: &PgPool, _twitch_user_id: &str) -> Result<TipSettings, sqlx::Error> {
    unimplemented!()
}
pub async fn load_tip_state(_pool: &PgPool, _twitch_user_id: &str, _slugs: &[String]) -> Result<HashMap<String, TipState>, sqlx::Error> {
    unimplemented!()
}
pub async fn record_tip_shown(_pool: &PgPool, _twitch_user_id: &str, _slug: &str) -> Result<(), sqlx::Error> {
    unimplemented!()
}
pub async fn record_feature_used(_pool: &PgPool, _twitch_user_id: &str, _feature: &str) -> Result<(), sqlx::Error> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPool::connect(&dsn).await.ok()?;
        // Tabellen anlegen (idempotent — gleiche DDL wie Migration).
        sqlx::query(include_str!("../../../migrations/20260621070000_golive_tips.sql"))
            .execute(&pool).await.ok()?;
        Some(pool)
    }

    #[tokio::test]
    async fn opt_out_default_false_und_record_setzt_timestamp() {
        let Some(pool) = test_pool().await else { eprintln!("skip: kein TB_TEST_DATABASE_URL"); return; };
        let uid = "test_tips_user_1";
        let s = tip_settings(&pool, uid).await.unwrap();
        assert!(!s.opt_out);
        assert!(s.last_tip_sent_at.is_none());
        record_tip_shown(&pool, uid, "auto-raid").await.unwrap();
        let s2 = tip_settings(&pool, uid).await.unwrap();
        assert!(s2.last_tip_sent_at.is_some(), "last_tip_sent_at gesetzt nach record");
    }

    #[tokio::test]
    async fn feature_usage_fliesst_in_tip_state() {
        let Some(pool) = test_pool().await else { return; };
        let uid = "test_tips_user_2";
        record_feature_used(&pool, uid, "auto-raid").await.unwrap();
        let st = load_tip_state(&pool, uid, &["auto-raid".to_string()]).await.unwrap();
        assert_eq!(st.get("auto-raid").and_then(|s| s.feature_used_days_ago), Some(0));
    }
}
```

- [ ] **Step 4: Test ausführen → FAIL**

Run: `cargo test -p tb-tips`
Expected: FAIL — `unimplemented!()` (oder Skip ohne DB; mit DB: panic).

- [ ] **Step 5: Repo implementieren**

In `repo.rs` die vier Funktionen (Slug = Feature-Key; `days_ago` via `EXTRACT(EPOCH ...)`):

```rust
pub async fn tip_settings(pool: &PgPool, twitch_user_id: &str) -> Result<TipSettings, sqlx::Error> {
    let row = sqlx::query_as::<_, (bool, Option<DateTime<Utc>>)>(
        "SELECT opt_out, last_tip_sent_at FROM twitch_tip_settings WHERE twitch_user_id = $1",
    )
    .bind(twitch_user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(opt_out, last)| TipSettings { opt_out, last_tip_sent_at: last }).unwrap_or_default())
}

pub async fn load_tip_state(
    pool: &PgPool,
    twitch_user_id: &str,
    slugs: &[String],
) -> Result<HashMap<String, TipState>, sqlx::Error> {
    let mut out: HashMap<String, TipState> = HashMap::new();
    // Feature-Nutzung (Slug == feature-key).
    let usage = sqlx::query_as::<_, (String, i64)>(
        "SELECT feature, FLOOR(EXTRACT(EPOCH FROM (NOW() - last_used_at)) / 86400)::int8 \
         FROM twitch_feature_usage WHERE twitch_user_id = $1",
    )
    .bind(twitch_user_id)
    .fetch_all(pool)
    .await?;
    for (feature, days) in usage {
        out.entry(feature).or_default().feature_used_days_ago = Some(days);
    }
    // Letzte Tipp-Anzeige je Slug.
    let shown = sqlx::query_as::<_, (String, i64)>(
        "SELECT tip_slug, FLOOR(EXTRACT(EPOCH FROM (NOW() - MAX(shown_at))) / 86400)::int8 \
         FROM twitch_tip_history WHERE twitch_user_id = $1 GROUP BY tip_slug",
    )
    .bind(twitch_user_id)
    .fetch_all(pool)
    .await?;
    for (slug, days) in shown {
        out.entry(slug).or_default().tip_shown_days_ago = Some(days);
    }
    // Sicherstellen, dass abgefragte Slugs als Default existieren (nie = None/None).
    for s in slugs {
        out.entry(s.clone()).or_default();
    }
    Ok(out)
}

pub async fn record_tip_shown(pool: &PgPool, twitch_user_id: &str, slug: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO twitch_tip_history (twitch_user_id, tip_slug) VALUES ($1, $2)")
        .bind(twitch_user_id).bind(slug).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO twitch_tip_settings (twitch_user_id, last_tip_sent_at, updated_at) \
         VALUES ($1, NOW(), NOW()) \
         ON CONFLICT (twitch_user_id) DO UPDATE SET last_tip_sent_at = NOW(), updated_at = NOW()",
    )
    .bind(twitch_user_id).execute(&mut *tx).await?;
    tx.commit().await
}

pub async fn record_feature_used(pool: &PgPool, twitch_user_id: &str, feature: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO twitch_feature_usage (twitch_user_id, feature, last_used_at, use_count) \
         VALUES ($1, $2, NOW(), 1) \
         ON CONFLICT (twitch_user_id, feature) DO UPDATE \
         SET last_used_at = NOW(), use_count = twitch_feature_usage.use_count + 1",
    )
    .bind(twitch_user_id).bind(feature).execute(pool).await?;
    Ok(())
}
```

`rust/crates/tb-tips/src/lib.rs`: `pub mod repo;`

- [ ] **Step 6: Tests grün (gegen frisch migrierte Test-DB) + Commit**

Run: `TB_TEST_DATABASE_URL=postgres://… cargo test -p tb-tips`
Expected: PASS (ohne DSN: Skip-Meldung, kein Fehler).
```bash
git add rust/Cargo.toml rust/migrations/20260621070000_golive_tips.sql rust/crates/tb-tips
git commit -m "feat(tb-tips): Migration + Repo für Tipp-State/Historie/Feature-Usage"
```

---

## Task 4: Orchestrierung — Gates + Auswahl (`tb-tips::engine`)

**Files:**
- Create: `rust/crates/tb-tips/src/engine.rs`
- Modify: `rust/crates/tb-tips/src/lib.rs`

**Interfaces:**
- Consumes: `repo` (Task 3), `tb_knowledge::{KnowledgeBase, rank_tip}` (Tasks 1–2).
- Produces:
  - `pub fn passes_gates(settings: &repo::TipSettings, now: DateTime<Utc>, min_gap_hours: i64) -> bool` (rein: nicht opt_out UND letzter Tipp ≥ min_gap her). `MIN_GAP_HOURS = 12`.
  - `pub async fn pick_tip(pool, kb, twitch_user_id) -> Result<Option<(String /*slug*/, String /*tip_text*/)>, sqlx::Error>` — lädt Zustand, rankt, gibt den zu sendenden Tipp (oder None).

- [ ] **Step 1: Failing test für `passes_gates` (rein)**

`rust/crates/tb-tips/src/engine.rs`:

```rust
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use tb_knowledge::{rank_tip, KnowledgeBase};

use crate::repo::{self, TipSettings};

pub const MIN_GAP_HOURS: i64 = 12;

pub fn passes_gates(settings: &TipSettings, now: DateTime<Utc>, min_gap_hours: i64) -> bool {
    if settings.opt_out {
        return false;
    }
    match settings.last_tip_sent_at {
        Some(last) => now - last >= Duration::hours(min_gap_hours),
        None => true,
    }
}

pub async fn pick_tip(
    _pool: &PgPool,
    _kb: &KnowledgeBase,
    _twitch_user_id: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-21T12:00:00Z").unwrap().with_timezone(&Utc)
    }

    #[test]
    fn opt_out_blockt() {
        let s = TipSettings { opt_out: true, last_tip_sent_at: None };
        assert!(!passes_gates(&s, now(), MIN_GAP_HOURS));
    }

    #[test]
    fn nie_gesendet_passt() {
        let s = TipSettings { opt_out: false, last_tip_sent_at: None };
        assert!(passes_gates(&s, now(), MIN_GAP_HOURS));
    }

    #[test]
    fn innerhalb_12h_blockt() {
        let last = now() - Duration::hours(5);
        let s = TipSettings { opt_out: false, last_tip_sent_at: Some(last) };
        assert!(!passes_gates(&s, now(), MIN_GAP_HOURS));
    }

    #[test]
    fn nach_12h_passt() {
        let last = now() - Duration::hours(13);
        let s = TipSettings { opt_out: false, last_tip_sent_at: Some(last) };
        assert!(passes_gates(&s, now(), MIN_GAP_HOURS));
    }
}
```

- [ ] **Step 2: Test ausführen → FAIL** (Compile/`unimplemented!`)

Run: `cargo test -p tb-tips engine`
Expected: FAIL (bzw. `passes_gates`-Tests grün, `pick_tip` noch unimplemented — die Gate-Tests zwingen die Datei zu kompilieren).

- [ ] **Step 3: `pick_tip` implementieren**

```rust
pub async fn pick_tip(
    pool: &PgPool,
    kb: &KnowledgeBase,
    twitch_user_id: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    let eligible = kb.eligible_tips();
    if eligible.is_empty() {
        return Ok(None);
    }
    let slugs: Vec<String> = eligible.iter().map(|d| d.slug.clone()).collect();
    let state = repo::load_tip_state(pool, twitch_user_id, &slugs).await?;
    Ok(rank_tip(&eligible, &state).map(|d| (d.slug.clone(), d.tip_text.clone())))
}
```

`rust/crates/tb-tips/src/lib.rs`: `pub mod engine;` ergänzen.

- [ ] **Step 4: Tests grün + Commit**

Run: `cargo test -p tb-tips`
Expected: PASS (Gate-Tests; DB-Tests skip/grün).
```bash
git add rust/crates/tb-tips/src/engine.rs rust/crates/tb-tips/src/lib.rs
git commit -m "feat(tb-tips): Gates (Opt-out/≥12h) + Tipp-Auswahl"
```

---

## Task 5: Hook verdrahten (`GoLiveTipHook`) + `tip_text` in Seed-Docs

**Files:**
- Create: `rust/crates/tb-tips/src/hook.rs`
- Modify: `rust/crates/tb-tips/src/lib.rs`
- Modify: das Binary, das `EventSubHooks` konstruiert (in Step 1 lokalisiert)
- Modify: `rust/knowledge/bot/*.md` (`tip_text:` setzen — Claude)
- Modify: `rust/crates/tb-tips/Cargo.toml` (Deps `tb-monitoring`, `tb-chat`)

**Interfaces:**
- Consumes: `engine::{passes_gates, pick_tip, MIN_GAP_HOURS}`, `repo`, `tb_chat::api::ChatApi`, `tb_monitoring::dispatch::EventSubHooks`, `tb_monitoring::stream::StreamSnapshot`, `KnowledgeBase`.
- Produces: `pub struct GoLiveTipHook { pool, chat: Arc<dyn ChatApi>, kb: &'static KnowledgeBase, snapshot_source }` mit `impl EventSubHooks` (`on_stream_went_live`).

- [ ] **Step 1: Konstruktionsstelle des Hooks lokalisieren**

Run: `rg -n "EventSubHooks|on_stream_went_live|dispatch::.*Hooks|impl .*EventSubHooks" rust/`
Ziel: die Stelle finden, an der heute eine (Noop-)`EventSubHooks`-Implementierung gebaut/übergeben wird (Binary `bin/tb-bot` oder ein Service-Setup), inkl. wie der Dispatch an einen `ChatApi`/`HelixChatClient` + `PgPool` herankommt. Diese Stelle wird in Step 4 angepasst.

- [ ] **Step 2: Wie kommt der Hook an den Deadlock-Status?**

Run: `rg -n "is_in_target_category|StreamSnapshot|fn .*snapshot|helix.*stream|get_streams" rust/crates/tb-monitoring/src`
Ziel: klären, ob `on_stream_went_live` schon einen `StreamSnapshot`/`game_name` zur Hand hat oder ob der Hook ihn via Helix nachladen muss. Ergebnis bestimmt die Signatur der Snapshot-Quelle (`snapshot_source`) in Step 3. (Falls das Event keinen `game_name` trägt: einen schmalen Trait `StreamCategorySource { async fn game_name(&self, user_id) -> Option<String> }` definieren und beim Wiring mit dem vorhandenen Helix-Poller erfüllen.)

- [ ] **Step 3: `GoLiveTipHook` implementieren**

`rust/crates/tb-tips/src/hook.rs` (Signatur an Step-2-Ergebnis anpassen; hier die Variante „Hook lädt Kategorie über eine injizierte Quelle"):

```rust
use std::sync::Arc;

use async_trait::async_trait;
use sqlx::PgPool;
use tb_chat::api::ChatApi;
use tb_knowledge::KnowledgeBase;
use tb_monitoring::dispatch::EventSubHooks;

use crate::engine::{self, MIN_GAP_HOURS};
use crate::repo;

/// Quelle für die aktuelle Stream-Kategorie eines Streamers (vom Wiring erfüllt,
/// z. B. mit dem vorhandenen Helix-Poller).
#[async_trait]
pub trait StreamCategorySource: Send + Sync {
    async fn game_name(&self, twitch_user_id: &str) -> Option<String>;
}

pub struct GoLiveTipHook {
    pub pool: PgPool,
    pub chat: Arc<dyn ChatApi>,
    pub kb: &'static KnowledgeBase,
    pub category: Arc<dyn StreamCategorySource>,
}

#[async_trait]
impl EventSubHooks for GoLiveTipHook {
    async fn on_stream_went_live(&self, twitch_user_id: &str, login: &str) {
        if let Err(e) = self.try_send_tip(twitch_user_id, login).await {
            tracing::warn!(%twitch_user_id, err = %e, "go-live-tipp fehlgeschlagen");
        }
    }
}

impl GoLiveTipHook {
    async fn try_send_tip(&self, twitch_user_id: &str, login: &str) -> Result<(), String> {
        // 1) Deadlock-Kategorie? (kein Aktivitäts-Gate, aber nur bei Deadlock.)
        let game = self.category.game_name(twitch_user_id).await.unwrap_or_default();
        if game.trim().to_lowercase() != "deadlock" {
            return Ok(());
        }
        // 2) Gates: opt-out + ≥12h (kein Delay).
        let settings = repo::tip_settings(&self.pool, twitch_user_id).await.map_err(|e| e.to_string())?;
        let now = chrono::Utc::now();
        if !engine::passes_gates(&settings, now, MIN_GAP_HOURS) {
            return Ok(());
        }
        // 3) Ranker wählt den Tipp.
        let Some((slug, tip_text)) = engine::pick_tip(&self.pool, self.kb, twitch_user_id).await.map_err(|e| e.to_string())? else {
            return Ok(());
        };
        // 4) Senden (erste Chat-Nachricht).
        self.chat.send_message(twitch_user_id, &tip_text).await.map_err(|e| e)?;
        // 5) Buchen (Cap + Historie). login nur fürs Log.
        repo::record_tip_shown(&self.pool, twitch_user_id, &slug).await.map_err(|e| e.to_string())?;
        tracing::info!(%login, %slug, "go-live-tipp gesendet");
        Ok(())
    }
}
```

`lib.rs`: `pub mod hook;`. `Cargo.toml` Deps ergänzen: `tb-monitoring`, `tb-chat`, `tb-db` (falls Pool-Typen), `chrono` (vorhanden).

- [ ] **Step 4: Hook ins Binary wiren**

An der in Step 1 gefundenen Stelle die Noop-Hooks durch `GoLiveTipHook` ersetzen/ergänzen: `pool`, den vorhandenen `HelixChatClient` (als `Arc<dyn ChatApi>`), `tb_knowledge::KnowledgeBase` (geladen aus `KNOWLEDGE_DIR` via `OnceLock`, wie in P1/`self_explainer`), und die `StreamCategorySource` (mit dem vorhandenen Helix-Poller erfüllt) injizieren. Falls mehrere Hook-Aspekte existieren (Engagement/Global-Ban): `GoLiveTipHook` additiv einhängen, ohne bestehende Hooks zu entfernen.

- [ ] **Step 5: `tip_text` in die tip-eligiblen Seed-Docs (Claude, aus Bestandstexten)**

In `rust/knowledge/bot/*.md` bei `tip_eligible: true` je ein handlungsorientierter `tip_text:` ergänzen (wiederverwendet aus den vorhandenen Feature-Einzeilern). Beispiele:

- `auto-raid.md`: `tip_text: Wenn du offline gehst, schickt der Bot deine Zuschauer automatisch zu einem anderen Deadlock-Streamer — nichts geht verloren.`
- `discord-golive.md`: `tip_text: Gehst du live, postet der Bot das automatisch im Community-Discord — verbinde dich, damit dich mehr Leute finden.`
- `einrichtung.md`: `tip_text: Schon gewusst? Im Dashboard kannst du in einer Minute alles einrichten — kein Formular, kein extra Konto.`
- (weitere analog; Ton: 1 Nutzen-Satz + optional Aktion. Unlock-Tipps z. B. „Verbinde Steam im Dashboard, dann zeigt !rank deinen Rang.")

- [ ] **Step 6: Build + Tests**

Run: `cargo build -p tb-tips && cargo test -p tb-knowledge -p tb-tips`
Expected: Build grün, Tests grün. Binary-Crate ebenfalls bauen: `cargo build -p tb-bot` (bzw. das in Step 1 gefundene Binary).

- [ ] **Step 7: Commit**

```bash
git add rust/crates/tb-tips/src/hook.rs rust/crates/tb-tips/src/lib.rs rust/crates/tb-tips/Cargo.toml rust/knowledge/bot rust/<binary-wiring-pfad>
git commit -m "feat(tb-tips): GoLiveTipHook verdrahtet + kuratierte tip_text"
```

---

## Task 6: Opt-out im Dashboard (Toggle) — minimal

**Files:**
- Create: `rust/crates/tb-dashboard-api/src/handlers/tip_settings.rs` (GET/POST)
- Modify: `rust/crates/tb-dashboard-api/src/lib.rs` (Routen in `build_authed_router`)
- Modify: dashboard_v2 — ein Toggle (an die in P4 gegroundete Settings-Stelle; falls P4 noch nicht da, hier nur Backend + ein einfacher Toggle in einer bestehenden Settings-Komponente)

**Interfaces:**
- `GET /twitch/api/v2/streamer/tip-settings` → `{ opt_out: bool }` (Streamer aus `DashboardAuthLevel::Partner`).
- `POST /twitch/api/v2/streamer/tip-settings` Body `{ opt_out: bool }` → upsert `twitch_tip_settings.opt_out`.

- [ ] **Step 1: Handler (Muster: `silent_settings.rs`, CSRF-frei, `DashboardAuthLevel`)**

GET + POST analog zum bestehenden `silent_settings.rs`: Identität aus `DashboardAuthLevel::Partner { twitch_user_id, .. }`; POST macht `INSERT … ON CONFLICT (twitch_user_id) DO UPDATE SET opt_out = $2, updated_at = NOW()`. (Vollständiger Code beim Bau am `silent_settings.rs`-Muster orientieren — gleiche Extractor-/Fehler-Struktur.)

- [ ] **Step 2: Routen registrieren** in `build_authed_router` (lib.rs, bei den anderen `streamer/*`-POSTs).

- [ ] **Step 3: Frontend-Toggle** (deutsches Label, Claude): „Go-Live-Tipps im Chat" mit Beschreibung „Der Bot postet beim Live-Gehen ab und zu einen kurzen Tipp als erste Nachricht. Hier abschaltbar." — an eine bestehende Settings-Sektion in dashboard_v2 hängen (oder in P4-Wizard, Schritt „Tipp opt-in").

- [ ] **Step 4: Build/Test + Commit**

Run: `cargo test -p tb-dashboard-api tip_settings && (cd ../dashboard_v2 || cd bot/dashboard_v2; npm run build)`
```bash
git add rust/crates/tb-dashboard-api/src/handlers/tip_settings.rs rust/crates/tb-dashboard-api/src/lib.rs bot/dashboard_v2/src
git commit -m "feat(dashboard): Opt-out-Toggle für Go-Live-Tipps"
```

---

## Task 7: Verifikation, CHANGELOG, Push, Spiegelung

- [ ] **Step 1: Gesamt**

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust
cargo build  -p tb-knowledge -p tb-tips -p tb-dashboard-api
cargo test   -p tb-knowledge -p tb-tips -p tb-dashboard-api
cargo clippy -p tb-knowledge -p tb-tips -p tb-dashboard-api -- -D warnings
cargo fmt    -p tb-knowledge -p tb-tips -p tb-dashboard-api
```
(DB-Tests gegen frisch migrierte Test-Postgres mit `TB_TEST_DATABASE_URL`.)

- [ ] **Step 2: CHANGELOG (Claude, oben)** — `## #N — Tipp beim Live-Gehen`. Drei Schläge: (1) viele Streamer kennen die Bot-Funktionen kaum; (2) jetzt postet der Bot beim Deadlock-Go-Live **eine** kurze, wechselnde Tipp-Nachricht als erste Chat-Zeile, wählt klug (unbenutztes/lange-vergessenes zuerst), hält ≥12h Abstand und ist im Dashboard abschaltbar; (3) so lernen Streamer Stück für Stück, was der Bot kann. Kein Datei-/Funktionsname.

- [ ] **Step 3: Commit + Push + Merge + Cleanup** (wie P1 Task 8).

- [ ] **Step 4: Spiegelung** In-App + Discord (`target:"twitch"`).

- [ ] **Step 5: Live-Smoke** Test-Streamer mit Deadlock live → erste Chat-Nachricht = Tipp; zweites Go-Live <12h → kein Tipp; Opt-out im Dashboard → kein Tipp. Journal: „go-live-tipp gesendet (slug=…)".

---

## Self-Review (vom Plan-Autor)

**1. Spec-Coverage (SP1-Design §4):** Trigger `stream.online`+Deadlock ✓ (Task 5); Gates ≥12h+Opt-out, kein Aktivitäts-Gate/Delay ✓ (Task 4/6); Ranker gewichtet-abklingend ✓ (Task 2); erste Chat-Nachricht = 1 Nutzen-Satz ✓ (`tip_text`, Task 5); Feature-Nutzungs-Tracking ✓ (Task 3, `record_feature_used` — Aufrufe aus den Feature-Pfaden sind Folge-Verdrahtung, hier Infra + ein Aufruf-Beispiel). **Messung** (gezeigt→genutzt→opt-out-Rate) ist über `twitch_tip_history`/`twitch_feature_usage` auswertbar; Dashboards dafür = späterer Schritt.

**2. Placeholder-Scan:** Code vollständig für alle reinen Teile (Frontmatter, Ranker, Gates, Repo, Migration, Hook). Zwei bewusst als „am Muster bauen" markierte Stellen (Task 5 Binary-Wiring, Task 6 Handler) sind durch `rg`-Schritte + ein benanntes Vorbild (`silent_settings.rs`) konkret geführt — das echte Wiring hängt am vorhandenen Hook-Konstruktionsort, der erst lokalisiert werden muss (ehrliche Erdung statt geratenem Pfad).

**3. Typ-Konsistenz:** `TipState{feature_used_days_ago,tip_shown_days_ago}` (T2) ← `load_tip_state` füllt exakt diese Felder (T3) → `rank_tip` liest sie (T2) → `pick_tip` (T4) → `GoLiveTipHook` (T5). `TipSettings{opt_out,last_tip_sent_at}` einheitlich T3/T4. Slug = Feature-Key durchgängig.

**Scope-Grenze P3:** kein Wizard, kein `!rank`, keine Mess-Dashboards. P3 endet: Deadlock-Go-Live → ein gut gewählter Tipp als erste Chat-Nachricht, ≥12h-Cap, Opt-out, live verifiziert.
