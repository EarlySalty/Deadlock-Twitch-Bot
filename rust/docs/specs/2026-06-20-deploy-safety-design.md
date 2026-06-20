# Design: Rust-Cutover deploy-sicher (Twitch-Bot, Typ-Drift + ScamGuard-Queue)

**Datum:** 2026-06-20
**Ziel (übergeordnet):** Den Rust-Twitch-Bot-Cutover deploy-sicher machen. Es gibt Schema-Typ-Drift-Bugs, die Live-Postgres-Endpoints crashen, während die Tests grün lügen (Fixtures bilden das Prod-Schema nicht ab und sind teils env-gated übersprungen). Endzustand: saubere DB-Zuweisungen, echte Flags, Tests die das **echte Rust-/Baseline-Schema** reproduzieren. **Kein Pfusch, keine Band-Aids, nichts erfinden — was fehlt, wird sauber gebaut.**

---

## Verifizierte Befunde (Stand origin/main = ae96047)

| # | Datei:Zeile | Ist | Soll | Referenz/Vorlage |
|---|-------------|-----|------|------------------|
| **P1** | `rust/crates/tb-analytics/src/partner_access.rs:82` | `role_removed: Option<bool>` decodiert `integer DEFAULT 0`-Spalte → SQLx-Decode-Panic | `Option<i32>`, Ableitung `.unwrap_or(0) != 0` | `tb-dashboard-api/.../internal_home.rs:659` macht dieselbe Spalte schon korrekt |
| **P3a** | `tb-dashboard-api/.../leaderboard.rs:180` | `MAX(CASE WHEN s.is_partner <> 0 ...)` gegen BOOLEAN → `operator does not exist: boolean <> integer` | echte Bool-Semantik (`COALESCE(is_partner, FALSE)`) | `category_leaderboard.rs:20` = `BOOL_OR(COALESCE(c.is_partner, FALSE))`, Kommentar Z.74 |
| **P3a** | `tb-internal-api/.../stats_native.rs:337` | `MAX(CASE WHEN is_partner <> 0 ...)` gegen BOOLEAN | dito | dito |
| **P3b** | Test-Fixtures: `tb-monitoring/tests/support/mod.rs:134`, `leaderboard.rs:348/349`, `stats_native` make_pool, `streamer_lifecycle.rs:1104/1108` | `is_partner INTEGER` → lügt gegen Prod-BOOLEAN, versteckt P3a | Fixtures auf `is_partner BOOLEAN` + INSERTs `0/1`→`false/true`; brechende Tests aufs neue Schema umbauen | Prod-Baseline `20260601000000_baseline_schema.sql:1445/1455` = `is_partner boolean DEFAULT false` |
| **P5** | `tb-dashboard-api/.../scam_guard_queue.rs:85` (List) + `:174` (Revoke) | Lädt/revoked nur `action_taken='suggested'`; Auto-Bans (`banned`/`timed_out`) erscheinen nie + nicht widerrufbar (Test `ignore_overturns_suggested_and_rejects_banned_or_foreign:379` zementiert das) | Queue zeigt auch `banned`/`timed_out` (nur `overturned` raus); Revoke akzeptiert diese **mit echtem Twitch-Unban** | Unban-Pfad existiert: `tb-transport-twitch/src/chat.rs:56` (`_unban_user`) |

**P4 (`dashboard_preview`): erledigt** — 0 Treffer in Rust und Frontend. Keine Aktion.

**Prod-Schema-Fakt:** `twitch_streamers_partner_state` ist ein **VIEW** (`baseline:1529`), `is_partner`/`is_partner_active` darin sind Integer-Konstanten/CASE → alle `is_partner_active = 1`-Vergleiche sind **korrekt**, kein Bug. Drift nur auf den echten Tabellen `twitch_stats_tracked`/`twitch_stats_category` (BOOLEAN).

---

## Branch-Landschaft (WICHTIG — nicht drüberbügeln)

- `origin/main` @ ae96047 = kanonisch. Enthält bereits: Bot-Token-Write-Back (SecretSink), gemergtes ScamGuard-Backend (Slices 1–3 + Revoke), `category_leaderboard`-Bool-Fix (#239), `runtime_type_contract.sql`.
- **`codex/rust-native-fixes` (1 Commit, STALE — NICHT MERGEN):** vor dem Bot-Token-Merge abgezweigt, nie rebased. Merge würde `secret_sink.rs` (−428), `token.rs` (−551), ADR 0005 (−94), CHANGELOG (−16) **löschen** und `category_leaderboard` auf das kaputte `<> 0` zurückrollen. Fixt die SQL-Seite NICHT. Nur als Ideen-Referenz. Schicksal: User entscheidet separat (vermutlich verwerfen).
- **`worktree-scam-autoban-persist` (1 Commit):** anderes Feature („Chat aller Spiele persistieren"). Nicht in Scope.
- **0-ahead = bereits gemergt, Cleanup-Kandidaten:** `wiring-pass-tb-bot`, `worktree-conversation-scam-guard`, `feat/bot-token-infisical-writeback`.

---

## Bauplan (Phasen-DAG)

```
P2 (Git-Reconcile, Claude)  ─► sauberer main-Base (= origin/main + NoopChatGreeter-Commit)
      │
      ├─ P1  role_removed-Decode            (Codex, frisch)   ─┐
      ├─ P3  is_partner clean (SQL+Fixtures)(Codex, frisch)   ─┼─► Review + Verify (Claude, DB-Suite)
      └─ P5  ScamGuard show+revoke+unban    (Codex Backend)   ─┘        │
              └─ P5-Frontend + dt. Strings  (Claude)                    ▼
                                                          CHANGELOG → push → merge → Discord
```

P1/P3/P5-Backend unabhängig → parallele Codex-Worker. Jede Phase eigener Worktree+Branch.

### P2 — Git-Reconcile (Claude, zuerst)
1. NoopChatGreeter-Änderung (`main.rs` Warn-Text + `oauth_followups.rs` No-Op-`ChatGreeterPort`) auf Branch `fix/oauth-noop-chat-greeter` committen.
2. Branch auf `origin/main` rebasen, Build-Check, `main` ff auf `origin/main`, Branch mergen, CHANGELOG, push.
3. `WORKFLOW.md` + `website/testing/` (fremder Session-Scratch) untracked lassen.

### P1 — partner_access role_removed (Codex)
`Option<bool>` → `Option<i32>` (Z.82), `.unwrap_or(false)` → `.unwrap_or(0) != 0` (Z.220). Test mit `role_removed integer DEFAULT 0`-Fixture (wie `tb-raid/tests/token_blacklist.rs:52`, schon prod-treu).

### P3 — is_partner clean (Codex, EINE saubere Stufe, kein Cast)
1. **SQL:** `leaderboard.rs:180` + `stats_native.rs:337`: `is_partner <> 0` → echte Bool-Semantik analog `category_leaderboard.rs:20`. Vollständiger `is_partner`-Sweep (nicht `is_partner_active`!) auf weitere `<> 0`/`= 1`-gegen-BOOLEAN-Stellen.
2. **Fixtures:** alle `is_partner INTEGER`-DDL der betroffenen Tabellen → `BOOLEAN`; INSERTs `0/1` → `false/true`. Ziel: Test-DDL == Prod-Spaltentyp. Falls machbar Baseline-Migration im Harness fahren (max. Treue); sonst exakt-typtreues DDL + Hinweis.
3. Brechende Tests **aufs neue Schema umbauen**, nicht Schema ans alte DDL anpassen.

### P5 — ScamGuard-Queue (Codex Backend + Claude Frontend/Strings)
1. **List-Query (Z.85):** zusätzlich `banned`/`timed_out` laden (nur `overturned` raus), Status-Feld mitliefern.
2. **Revoke (Z.174):** akzeptiert `banned`/`timed_out`; löst **echten Twitch-Unban** aus via vorhandenem Pfad (`chat.rs` `_unban_user`). Falls Verdrahtung dashboard-api→Bot-Moderation fehlt: **sauber bauen** (interner Endpoint mit Auth), nicht abkürzen.
3. Test `ignore_overturns_suggested_and_rejects_banned_or_foreign` entsprechend umbauen.
4. **Frontend (`bot/dashboard_v2`, React) + alle dt. Strings = Claude.** Codex setzt nur `"Platzhalter"` + meldet Datei:Zeile. Status-Badges (auto-gebannt/getimeoutet/vorgeschlagen) + „Bann zurücknehmen".

---

## Delegation & Verifikation

- **Codex:** `model=gpt-5.5`, `effort=xhigh`. Briefing pro Ticket: exakter Scope + Dateien + Referenz-Muster + DoD. **User-sichtbare Texte = Platzhalter** (Claude schreibt final).
- **Claude reviewt jede `changed_files`** vor Commit. Build (`cargo build`) + **DB-Suite mit gesetztem `TB_TEST_DATABASE_URL`** gegen frisch migriertes Postgres (sonst läuft P3-Test nicht). Kein „grün" ohne real gelaufene DB-Tests.
- Jede Phase eigener Worktree; nach Verifikation merge nach `main` + Worktree/Branch-Cleanup.
- CHANGELOG je Schlag (#N, drei Schläge) → push → Discord (`localhost:8899`, nur user-sichtbares = P5) → ggf. Twitch In-App.

## Offene Risiken
1. P5-Unban-Verdrahtung dashboard-api→Bot ist Cross-Prozess; ggf. eigener interner Endpoint (sauber gebaut).
2. P3b kann andere Fixture-Tests brechen (Annahme INTEGER) → werden mitgezogen.
3. `codex/rust-native-fixes`-Schicksal: User-Entscheidung (Empfehlung verwerfen).
