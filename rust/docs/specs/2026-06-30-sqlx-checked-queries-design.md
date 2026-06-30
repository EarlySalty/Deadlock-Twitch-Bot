# sqlx compile-time-checked Queries (Tier 3) — Design & Phase-0-Spec

Datum: 2026-06-30 · Branch: `chore/twitch-sqlx-checked-queries` · Status: Spec (zur Freigabe)

## 1. Ziel

Alle **konvertierbaren** SQL-Queries im Rust-Workspace von String-`query()` auf die
compile-time-geprüften Makros `query!` / `query_as!` / `query_scalar!` (bzw.
`query_file!` / `query_file_as!` für `include_str!`-SQL) umstellen. Eine fehlende
oder typ-falsche Spalte wird damit zum **Compile-Fehler** statt Runtime-Panic — die
Kern-Direktive aus `feedback_tests_catch_prod_breakage`.

**Ambition (User-Entscheid):** Vollkonvertierung als Ziel — nicht nur die „mechanisch
billigen" Stellen. Der Maßstab ist: alles, was ein String-Literal-SQL ist, wird Makro.

**Harte Grenze:** Queries, deren SQL zur Laufzeit gebaut wird (`format!`-Identifier,
`QueryBuilder`/`push_bind`, runtime-`CREATE TABLE`-Tabellen), **können** technisch nicht
zu Makros werden (das Makro braucht ein Literal zur Compile-Zeit). Diese ~121 Stellen
bleiben `query()` — aber als **bewusste, dokumentierte Ausnahme** (Pflicht-Kommentar pro
Stelle + Lint-Guard gegen neue rohe `query()`), nicht als stiller Default.

## 2. Ist-Zustand (gemessen 2026-06-30)

| Fakt | Wert |
|------|------|
| Dynamische Query-Calls gesamt | 2836 (`query(` 2132 · `query_as(` 401 · `query_scalar(` 303) |
| Compile-checked Makros heute | 1 |
| `.sqlx`-Offline-Cache | existiert nicht |
| sqlx | 0.8, Features `postgres,macros,migrate,chrono,runtime-tokio,tls-rustls` aktiv |
| Crates mit Queries | 11 — tb-analytics 828, tb-dashboard-api 823, tb-social-media 292, tb-chat 210, tb-internal-api 198, tb-raid 178, tb-engagement 157, tb-monitoring 134, tb-llm 7, tb-highlight 5, tb-tips 4 |
| Echt-dynamisch (nicht makrofähig) | ~195 Dateien `format!`-SQL · 126 QueryBuilder · 187 runtime-`CREATE TABLE` |
| Migrationen | `rust/migrations/*.sql` (34) |
| Rust-CI | existiert NICHT (`lint-and-typecheck.yml` = Python/Frontend, nur `workflow_dispatch`) |

## 3. Architektur-Entscheidungen

### 3.1 Offline `.sqlx`-Cache (NICHT live DATABASE_URL beim Build)
- `cargo sqlx prepare --workspace` erzeugt **ein** `.sqlx/`-Verzeichnis im rust-Workspace-Root,
  committed ins Repo.
- `rust/.cargo/config.toml`: `[env] SQLX_OFFLINE = "true"` → jeder Build (Deploy auf dem
  Server, nächtlicher disk-autoclean-Rebuild, CI) ist hermetisch und braucht **keine** DB.
  Regeneriert wird nur explizit per `cargo sqlx prepare`.
- **Begründung:** Deploy baut aus `…/<repo>/rust/target/release/<bin>`; ein Live-DB-Zwang bei
  jedem Build bräche Deploy-Pipeline, disk-autoclean und CI-Hermetik. Der Cache wird gegen die
  **fresh-aus-Migrationen-DB** erzeugt (die in Ticket 1.x als `==prod` bewiesen wurde) → `.sqlx/`
  wird zur dritten Schema-Wahrheit neben Migrationen und `fresh_schema_snapshot.txt`.

### 3.2 Prepare-DB = fresh aus Migrationen (== prod)
- Wegwerf-DB `tb_migtest_drift` auf der Timescale-Instanz (172.17.0.2), aus den 34 Migrationen
  gebaut — Reuse von `scratchpad/harness.py mkfresh`.
- DSN ausschließlich via Infisical-Loader (`export_claude_secret.py … TWITCH_ANALYTICS_DSN`,
  `--no-confirm`), gemutet. **Nie** in Chat/Log/Datei.

### 3.3 Drift-Gate (der eigentliche Wert)
- **Lokal/Build:** `SQLX_OFFLINE=true cargo build` schlägt fehl, wenn der `.sqlx`-Cache stale
  gegenüber den `query!`-Calls ist (Query geändert, Cache nicht regeneriert).
- **CI gegen echtes Schema:** `cargo sqlx prepare --workspace --check` gegen eine
  Postgres+TimescaleDB-Service-DB mit angewandten Migrationen → rot, wenn Cache stale gegenüber
  dem **Schema** (Schema geändert, Cache nicht). Das ist das Gate aus
  `feedback_tests_catch_prod_breakage`.

## 4. Phase 0 — Tracer Bullet (DIESE Session)

Scope: Infra + Pilot-Crate `tb-tips` + CI-Gate. Danach **Stopp + Review** vor den Wellen.

### 4.1 Codex-Deliverables (gpt-5.5 / xhigh) — reiner Code, KEINE DB-Ops
1. `rust/.cargo/config.toml` neu: `[env] SQLX_OFFLINE = "true"`.
2. `tb-tips` (`crates/tb-tips/src/repo.rs`, 4 Query-Stellen) voll konvertieren:
   - String-Literal-SQL → `query!` / `query_scalar!` / `query_as!` (Binds wandern ins Makro,
     Row-Zugriff wird benannte Felder statt `.get()`).
   - `sqlx::query(include_str!("…"))` (Zeile 118) → `query_file!` bzw. `query_file_as!`
     (Pfad relativ zur Crate-Root).
   - `query_as`-Zielstructs bleiben erhalten, nur makro-geprüft.
3. `scripts/sqlx-prepare.sh`: Wrapper, der den Prepare-Workflow dokumentiert/ausführt — liest
   `DATABASE_URL` aus der **vom Operator gesetzten** Env, enthält **kein** Secret, kein Default-DSN.
4. CI-Workflow `.github/workflows/rust-sqlx-check.yml` (neu):
   - Job A „offline-build": `SQLX_OFFLINE=true cargo build --workspace` (ohne DB).
   - Job B „schema-gate": `services: postgres` (Image mit TimescaleDB), Migrationen anwenden,
     `cargo sqlx prepare --workspace --check`.
   - Trigger: siehe §7 (offene Policy) — Default-Vorschlag `pull_request` + `push` auf `rust/**`.
5. Blockierte Stellen (z. B. eine tb-tips-Tabelle nicht in Migrationen) als `Datei:Zeile`
   zurückmelden, **nicht** umbiegen.

> **Compile-Verifikation liegt beim Orchestrator:** `query!`-Makros kompilieren ohne `.sqlx`-Cache
> nicht. Codex liefert die Edits; ich generiere den Cache gegen die DB und baue. Compile-Fehler
> (Typ-Mismatch etc., die das Makro fängt) gehen als Rework zurück an Codex.

### 4.2 Orchestrator-Deliverables (privilegiert, secret-/DB-seitig)
1. sqlx-cli bereitstellen (`cargo install sqlx-cli --no-default-features --features postgres,rustls`
   falls nicht vorhanden).
2. `tb_migtest_drift` fresh aus 34 Migrationen bauen (harness).
3. `DATABASE_URL=<DSN> cargo sqlx prepare --workspace` → `.sqlx/` erzeugen + committen.
4. Verifikation (siehe §5).
5. Branch/Commit/Push/Merge (Git-Integration macht der Orchestrator, nie der Impl-Worker).

## 5. Definition of Done (Phase 0) — Verifikations-Beweise

- [ ] `SQLX_OFFLINE=true cargo build -p tb-tips` **grün** (offline, ohne DB).
- [ ] **Negativtest:** ein absichtlich gebrochener Spaltenname in einer tb-tips-Query macht den
      Build **rot** → danach revert. (Beweist, dass das Gate wirklich greift —
      `feedback_tests_catch_prod_breakage` „Negativtests".)
- [ ] `cargo sqlx prepare --workspace --check` **grün** gegen fresh==prod-DB.
- [ ] `cargo test -p tb-tips` grün; abhängige Crates bauen weiterhin (`cargo build` workspace).
- [ ] clippy: 0 **neu** vom Branch eingeführte Warnungen (Alt-Schuld in tb-highlight/tb-raid/
      tb-social-media ist pre-existing, außerhalb Scope).
- [ ] `.sqlx/` committed; `.cargo/config.toml` committed; CI-Workflow committed.
- [ ] CHANGELOG.md-Eintrag (Infra, nicht user-sichtbar → kein Discord-Post).

## 6. Roadmap — Wellen nach Phase 0 (zur späteren Freigabe)

Reihenfolge klein→groß, jede Welle eigener Codex-Worker, jede gegated durch
fresh-DB-`prepare` + offline-Build + `--check`:

1. tb-highlight (5) · tb-llm (7) — Mini-Crates, festigen den Workflow.
2. tb-monitoring (134) · tb-raid (178) · tb-engagement (157) — mittel.
3. tb-internal-api (198) · tb-chat (210) · tb-social-media (292) — groß.
4. tb-analytics (828) · tb-dashboard-api (823) — Schwergewichte, höchstes Risiko, zuletzt.

Pro Welle: dynamische Reste explizit als `query()` + Begründungs-Kommentar; runtime-DDL-Tabellen,
die ein Makro blockieren, an Ticket-1.2-Nachzug eskalieren (Tabelle migrieren) oder begründet
dynamisch lassen.

## 7. Offene Policy-Entscheidung (Rücksprache)

**CI-Trigger:** Das bestehende CI ist nur `workflow_dispatch` (manuell). Ein Drift-Gate, das nur
manuell läuft, schützt schwach. Vorschlag: der **neue** Rust-Job läuft auf `pull_request` + `push`
für `rust/**` (scoped, bestehende Jobs unverändert). Alternative: ebenfalls nur `workflow_dispatch`
(konsistent, aber schwächer). → Default `pull_request`+`push`, sofern nicht anders gewünscht.

## 8. Risiken & Gegenmaßnahmen

- **TimescaleDB im CI-Service:** Standard-`postgres`-Image hat kein Timescale. Image
  `timescale/timescaledb:*-pg16` (o. ä.) nötig, sonst scheitern Hypertable-Migrationen. Codex muss
  das im Workflow lösen oder als blockiert melden.
- **Runtime-DDL-Tabellen:** Trifft ein `query!` eine nur-zur-Laufzeit erzeugte Tabelle, schlägt
  `prepare` fehl („relation does not exist"). Das ist der Detektor für die letzten Doppelquellen-
  Reste aus Ticket 1.2 — eskalieren, nicht umbiegen.
- **`sqlx::migrate!`-Embed ist STALE** bei reiner `.sql`-Änderung (Lektion Ticket 1.x): vor
  DB-Bauten ggf. `touch` der Migrator-Quelle erzwingen.
- **Ergonomik-Bruch `query(` → `query!`:** Row-Zugriff wechselt von `.get("col")` auf benannte
  Felder/`Option<T>`-Nullability. Das ist echtes Refactoring; Nullability-Annahmen (`col!` force-
  non-null) müssen stimmen, sonst Laufzeit-`unwrap`-Risiko an anderer Stelle.

## 9. Bezug

`feedback_tests_catch_prod_breakage`, `project_twitch_migration_live_schema_reconcile`
(Drift-Gate + fresh==prod-Maschinerie), CLAUDE.md „Deploy-Verifikation: Artefakt + Live-Zustand".
