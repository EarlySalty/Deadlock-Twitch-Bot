# SP1 / P5 — Stat-Befehle `!rank`/`!wins`/`!winrate` (Implementierungsplan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Twitch-Streamer (und Zuschauer im Chat) sehen den Deadlock-Rang des Streamers via `!rank`, später `!wins`/`!winrate` — über den **bestehenden** Steam-Bot (keine neue Instanz), entlang der Kette `twitch_user_id → discord_user_id → steam_id → Rang`.

**Architecture (Fork X, gelockt):** Kein neuer Steam-Link-Code. Der Streamer hat seinen Steam-Account bereits über den **Discord-Flow** verknüpft (`steam_links`, keyed Discord-ID, mit gecachtem Rang). Neu: (1) ein öffentlicher **HTTP-Endpoint im Steam-Bot** (`steam-web`) `GET /rank?discord_id=` liest `steam_links` und liefert `{linked, steam_id, rank_name, badge_level}`; (2) im Twitch-Bot ein **Resolver** `twitch_user_id → discord_user_id` (aus `twitch_streamer_identities`) + HTTP-Call zum Steam-Bot; (3) die Chat-Befehle in `tb-chat`. `!wins`/`!winrate` brauchen zusätzlich einen neuen GC-Handler im Steam-Bot (`CSOGameAccountClient` → wins/losses).

**Tech Stack:** Rust. **Zwei Repos:** `Deadlock-Steam-Bot` (steam-web Endpoint + GC-Handler) und `Deadlock-Twitch-Bot` (tb-chat Befehle + Resolver). Postgres (twitch) / SQLite (steam).

**Voraussetzung:** **P1 gemergt** (für Konsistenz; P5 nutzt P1 nicht direkt). Der Streamer-Discord-Link-Flow existiert bereits.

## Global Constraints

- Rust-Standard. **Zwei Repos** → zwei eigene Worktrees/Branches, zwei CHANGELOGs, zwei Deploys (Steam-Bot-Service + Twitch-Bot). Original-Python unangetastet.
- **Eigene Streamer-Steam-Instanz: NICHT in P5** (User-Entscheidung, festgehaltenes künftiges To-do). Wir nutzen die bestehende Instanz.
- **User-sichtbare deutsche Chat-Antworten** schreibt **Claude** (z.B. „Rang von {name}: {rank}", Fallback „kein verknüpfter Account / kein Rang").
- Keine neuen externen Crates (reqwest ist vorhanden). Steam-Endpoint read-only, kein Auth nötig (nur lokaler/interner Aufruf; per IP/Token absichern falls öffentlich exponiert — am bestehenden steam-web-Muster orientieren).
- `!wins`/`!winrate` nur, wenn der GC-Handler zuverlässig Daten liefert; sonst ehrlicher Fallback („noch nicht verfügbar"). Kein Fake.
- Git/Delegation wie P1; GPT baut, Claude reviewt + schreibt DE-Texte.

---

## Dateistruktur

**Repo Deadlock-Steam-Bot:**
- Create: `rust/crates/steam-web/src/routes/rank.rs` — `GET /rank` Handler.
- Modify: `rust/crates/steam-web/src/lib.rs` (Route registrieren).
- (Task 4) Modify: `rust/crates/steam-core/src/task/handlers/gc.rs` + Proto-Parsing für `CSOGameAccountClient` (wins/losses).

**Repo Deadlock-Twitch-Bot:**
- Create: `rust/crates/tb-chat/src/stats.rs` — Resolver + Steam-Bot-HTTP-Client + reine Reply-Builder.
- Modify: `rust/crates/tb-chat/src/commands.rs` — `!rank`/`!wins`/`!winrate`-Arme.
- Modify: `rust/crates/tb-chat/Cargo.toml` (reqwest, falls nicht vorhanden).

---

## Task 1 (Steam-Bot): `GET /rank?discord_id=` Endpoint

**Files:** Create `rust/crates/steam-web/src/routes/rank.rs`; Modify `rust/crates/steam-web/src/lib.rs`.

**Interfaces:**
- `GET /rank?discord_id=<i64>` → JSON `{ "linked": bool, "steam_id": string|null, "rank_name": string|null, "badge_level": int|null }`. Liest `steam_persistence::links::get_by_user(discord_id)` (existiert, liefert gecachten Rang).

- [ ] **Step 1: Bestehende Route/State-Struktur lesen** — `rg -n "Router::new|with_state|FlowContext|routes::" rust/crates/steam-web/src/lib.rs rust/crates/steam-web/src/routes/link.rs` — Muster für Handler-Signatur (`State<FlowContext>`, `Query`) + Router-Merge übernehmen.

- [ ] **Step 2: Failing test + Handler** — `rank.rs` (Signatur an steam-web-State anpassen):

```rust
//! GET /rank?discord_id=<id> — liest den gecachten Deadlock-Rang zu einer
//! Discord-ID aus steam_links (read-only). Für den Twitch-Bot (`!rank`).

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
pub struct RankQuery { pub discord_id: String }

pub async fn get_rank(State(ctx): State<FlowContext>, Query(q): Query<RankQuery>) -> Response {
    let Ok(discord_id) = q.discord_id.parse::<i64>() else {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"bad discord_id"}))).into_response();
    };
    match steam_persistence::links::get_by_user(&ctx.db, discord_id).await {
        Ok(Some(link)) => (StatusCode::OK, Json(json!({
            "linked": true,
            "steam_id": link.steam_id.to_string(),
            "rank_name": link.deadlock_rank_name,
            "badge_level": link.deadlock_badge_level,
        }))).into_response(),
        Ok(None) => (StatusCode::OK, Json(json!({"linked": false, "steam_id": null, "rank_name": null, "badge_level": null}))).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"db"}))).into_response(),
    }
}
```

(Exakte `FlowContext`/`State`-Typen + `get_by_user`-Signatur in Step 1 verifizieren und anpassen. Test: ein `#[tokio::test]` mit einer temporären SQLite + eingefügtem `steam_links`-Row, der `linked:true` + rank_name prüft — am bestehenden steam-persistence-Testmuster orientieren.)

- [ ] **Step 3: Route registrieren** in `steam-web/src/lib.rs` (`mod rank;` + `.route("/rank", get(routes::rank::get_rank))` in den Router-Merge).

- [ ] **Step 4: Build/Test** — `cargo build -p steam-web && cargo test -p steam-web rank`.
- [ ] **Step 5: Commit** (Steam-Bot-Repo, eigener Branch) — `git commit -m "feat(steam-web): GET /rank?discord_id Endpoint (Rang-Lookup für Twitch-Bot)"`.

---

## Task 2 (Twitch-Bot): Resolver + Steam-Bot-HTTP-Client + Reply-Builder

**Files:** Create `rust/crates/tb-chat/src/stats.rs`; Modify `rust/crates/tb-chat/src/lib.rs`, `rust/crates/tb-chat/Cargo.toml`.

**Interfaces:**
- `async fn resolve_discord_id(pool, twitch_user_id) -> Option<String>` — SELECT `discord_user_id` aus `twitch_streamer_identities`.
- `async fn fetch_rank(discord_id) -> Option<RankInfo>` — HTTP GET an `STEAM_BOT_RANK_URL` (Env, Default `http://127.0.0.1:8766/rank`) `?discord_id=`.
- `fn rank_reply(streamer_name, rank: Option<RankInfo>) -> String` (rein, Claude-Text).

- [ ] **Step 1: Failing test für `rank_reply` (rein)** — `stats.rs`:

```rust
//! `!rank`-Kette: twitch_user_id → discord_user_id (twitch_streamer_identities)
//! → HTTP an den Steam-Bot (/rank) → Chat-Antwort. Reuse statt Neubau.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct RankInfo {
    pub linked: bool,
    pub rank_name: Option<String>,
    pub badge_level: Option<i64>,
}

pub fn rank_reply(name: &str, info: Option<&RankInfo>) -> String {
    match info {
        Some(i) if i.linked => match &i.rank_name {
            Some(r) => format!("Rang von {name}: {r}"),
            None => format!("{name} hat einen verknüpften Account, aber noch keinen Rang erkannt."),
        },
        _ => format!("{name} hat noch keinen Steam-Account verknüpft — geht im Discord über die Steam-Verknüpfung."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rang_vorhanden() {
        let i = RankInfo { linked: true, rank_name: Some("Archon".into()), badge_level: Some(61) };
        assert_eq!(rank_reply("nani", Some(&i)), "Rang von nani: Archon");
    }
    #[test]
    fn verknuepft_ohne_rang() {
        let i = RankInfo { linked: true, rank_name: None, badge_level: None };
        assert!(rank_reply("nani", Some(&i)).contains("noch keinen Rang"));
    }
    #[test]
    fn nicht_verknuepft() {
        let i = RankInfo { linked: false, rank_name: None, badge_level: None };
        assert!(rank_reply("nani", Some(&i)).contains("noch keinen Steam-Account"));
        assert!(rank_reply("nani", None).contains("noch keinen Steam-Account"));
    }
}
```

- [ ] **Step 2: Test → FAIL → implementieren** — `resolve_discord_id` (sqlx-Query gegen `twitch_streamer_identities`) + `fetch_rank` (reqwest GET, Timeout 5s, parse JSON → `RankInfo`; Fehler → None). Env `STEAM_BOT_RANK_URL`. `lib.rs`: `pub mod stats;`. reqwest in Cargo.toml falls nötig.

- [ ] **Step 3: Test → PASS** — `cargo test -p tb-chat stats`.
- [ ] **Step 4: Commit** — `git commit -m "feat(tb-chat): Rang-Resolver + Steam-Bot-HTTP-Client + Reply"`.

---

## Task 3 (Twitch-Bot): `!rank`-Chat-Befehl

**Files:** Modify `rust/crates/tb-chat/src/commands.rs`.

- [ ] **Step 1: Dispatch-Arm** vor `_ => false`:

```rust
            "!rank" => { self.cmd_rank(event, args).await; true }
```

- [ ] **Step 2: `cmd_rank` implementieren** — Ziel = Streamer (Kanal-Inhaber, `event.broadcaster_user_id`/`broadcaster_user_login`); optional `args` für späteren `!rank <name>` (jetzt ignorieren/nur Streamer). Resolver: `stats::resolve_discord_id(&self.pool, &event.broadcaster_user_id)` → `stats::fetch_rank(discord_id)` → `self.reply(event, &stats::rank_reply(&event.broadcaster_user_name, info.as_ref()))`. Bei keiner Discord-Verknüpfung: passender Fallback (gleicher Reply-Pfad, info=None).

- [ ] **Step 3: Build/Test** — `cargo build -p tb-chat && cargo test -p tb-chat`.
- [ ] **Step 4: Commit** — `git commit -m "feat(tb-chat): !rank-Chat-Befehl (Streamer-Rang)"`.

---

## Task 4 (Steam-Bot): `CSOGameAccountClient` → Wins/Losses (für `!wins`/`!winrate`)

> Höherer Aufwand (neuer GC-Handler). Wenn der bestehende Bot Wins/Losses nicht zuverlässig liefert: `!wins`/`!winrate` mit ehrlichem „noch nicht verfügbar"-Fallback ausliefern und diese Task als Folge-Ticket zurückstellen — NICHT faken.

**Files:** Modify `rust/crates/steam-core/src/task/handlers/gc.rs`; ggf. `steam-web/src/routes/rank.rs` (Response um `wins`/`losses` erweitern).

- [ ] **Step 1: GC-Fähigkeit prüfen** — `rg -n "CSOGameAccountClient|wins|losses|GetAccountStats" rust/crates/deadlock-proto rust/crates/steam-core` — klären, ob `CSOGameAccountClient` (Felder `wins`/`losses`) abrufbar ist oder ein neuer Request (`CMsgClientToGcGetAccountStats`) nötig ist.
- [ ] **Step 2: Handler ergänzen** — analog zum bestehenden `GcProfileCardHandler` einen Handler, der wins/losses liefert; in `steam_links` cachen (neue Spalten via Steam-Bot-Migration) oder on-demand. (Konkrete Umsetzung nach Step-1-Befund; am ProfileCard-Handler-Muster orientieren.)
- [ ] **Step 3: `/rank`-Response + `RankInfo`** um `wins`/`losses` erweitern; `winrate` = wins/(wins+losses).
- [ ] **Step 4: `!wins`/`!winrate`-Befehle** in tb-chat (Reply-Builder + Arme, analog `!rank`).
- [ ] **Step 5: Build/Test + Commits** (beide Repos).

---

## Task 5: Verifikation, CHANGELOG (×2), Push, Deploy, Spiegelung

- [ ] **Step 1: Steam-Bot** — `cargo build/test/clippy/fmt` der berührten Crates; Branch pushen; CHANGELOG (Steam-Bot) Eintrag; nach Review merge + Service rebuild/restart (steam-web/steam-core).
- [ ] **Step 2: Twitch-Bot** — `cargo build/test/clippy/fmt -p tb-chat`; Branch pushen; CHANGELOG (Twitch-Bot); merge + Bot-Neustart.
- [ ] **Step 3: Spiegelung** (Twitch-Bot, user-sichtbar) In-App + Discord (`target:"twitch"`): „!rank zeigt jetzt deinen Deadlock-Rang im Chat".
- [ ] **Step 4: Live-Smoke** — verknüpfter Streamer: `!rank` → „Rang von …: …"; nicht verknüpft → Fallback mit Verweis auf Discord-Steam-Verknüpfung (Flywheel-Funnel!).

---

## Self-Review (vom Plan-Autor)

**1. Spec-Coverage (§3 Stat-Befehle, Fork X):** `!rank` über bestehenden Bot ✓; Kette twitch→discord→steam→rang ✓ (T1+T2); kein neuer Link-Code, kein neuer Bot ✓; `!wins`/`!winrate` als feasibler Ausbau ✓ (T4, ehrlich konditioniert); eigene Instanz deferred ✓.

**2. Placeholder-Scan:** Voller Code für /rank-Endpoint, Resolver/Client, rank_reply, !rank-Befehl. T1 Step-1 + T4 sind bewusst an `rg`-Erkundung gebunden (FlowContext-Typen / GC-Fähigkeit) — keine geratenen Steam-Bot-Internas, ehrlich als Verifikationsschritt geführt.

**3. Typ-Konsistenz:** `/rank`-JSON `{linked,steam_id,rank_name,badge_level}` (T1) ↔ `RankInfo{linked,rank_name,badge_level}` (T2, steam_id für Reply nicht nötig) ↔ `rank_reply` (T2) ↔ `cmd_rank` (T3). `discord_id` als String-Query, i64 geparst (T1) ↔ `resolve_discord_id`→String→`fetch_rank` (T2).

**Scope-Grenze P5:** kein Wizard (P4), keine eigene Instanz, keine Live-Lobby/Overlays (SP2). P5 endet: `!rank` live über den bestehenden Bot; `!wins`/`!winrate` sofern GC-Daten tragen.
