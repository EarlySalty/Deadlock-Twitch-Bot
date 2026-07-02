# Python→Rust Paritäts-Audit — Triage-Ergebnis (2026-06-30)

Quelle: `Python_Rust Parity Audit.html` (152 Findings). Orchestriert von Claude, implementiert via GPT/Codex, Texte final von Claude.

## Zusammenfassung

- **49 klare Bugs gefixt** (Branch `fix/py-rust-parity-obvious-bugs`).
- **14 schon im Rust-Code vorhanden** → nichts zu tun.
- **88 offene „???"** → diese Liste (kein offensichtlicher Bug; Intent/Vertrag/Design-Entscheidung).

## Deine Rulings (eingearbeitet)

- `RAID-RECRUIT-015` → **RUST_CORRECT**: Rust laedt globale Bans absichtlich in Raid-Blacklist; Python zu eng. Aus FIX entfernt.
- `RAID-SETUP-004` → **NOT_A_BUG**: blocked/bot_banned sind HARD_PAUSE_REASONS (= Python); Reaktivierung korrekt blockiert.
- `RAID-SETUP-008` → **INTENDED**: Onboarding-Trial direkt gewaehren ist gewollt. Bleibt KEEP.
- `RAID-SETUP-014` → **BUG_CONFIRMED**: User: Grace-Expiry darf nicht uebersprungen werden wenn Discord-Broker fehlt. In FIX (tb-bot).
- `RAID-SETUP-016` → **LEAVE**: Restore-nach-gueltigem-Auth (token_lifecycle.rs:856) = Python reactivate (token_error*); korrekt.
- `RAID-SETUP-017` → **BUG_FIX_NARROW**: User delegiert: clear_failure_count-Pfad (token_blacklist.rs:138, auth_writer.rs:186/190) exakt = 'token_error'; Restore-Pfad starts_with bleibt.

## Offene Findings nach Thema (deine Entscheidung)

### Billing/Affiliate/Stripe-Verträge (Geschäftsentscheidung) — 8

- **DASH-AFF-007** — Affiliate signup/connect missing.
- **DASH-AFF-003** — Affiliate me/profile missing.
- **RAID-SETUP-008** — Rust grants onboarding trial direkt.
- **DASH-AFF-004** — Affiliate claims missing.
- **DASH-BILL-008** — invoice-preview fehlt/unklar.
- **DASH-BILL-009** — rechnung/stripe-settings missing/unclear.
- **DASH-AFF-006** — Affiliate gutschriften/pdf missing.
- **DASH-AFF-008** — Affiliate user-facing legacy mostly missing.

### Migrations-/Deploy-/Readiness-Politik (Ops-Entscheidung) — 13

- **STOR-SCHEMA-002** — Observability-Retention fehlt in Rust.
- **STOR-SCHEMA-004** — Legacy-Python kann Rust-Zielschema wieder anfassen.
- **STOR-SCHEMA-005** — Destruktive/lockende Migrationen laufen automatisch.
- **MON-POLL-002** — Rust tracked alle Nicht-Partner aus twitch_streamers, Python nur is_monitored_only=1.
- **STOR-SCHEMA-001** — Rust startet trotz fehlgeschlagener Migration weiter.
- **OPS-RUNTIME-002** — Rust-Dashboard ohne Role/Port-Guard und PID-Lock.
- **OPS-RUNTIME-001** — Dashboard-Readiness schwaecher; Rust DB-only/200 degraded, Python upstream/OAuth/Fingerprint/503.
- **DASH-BILL-002** — Readiness Rust response reduced.
- **RAID-RECRUIT-011** — Rust send task Crash/Drop-Risiko.
- **DASH-LIVEANN-006** — Schema existiert aber API nutzt es nicht.
- **DASH-LIVEANN-011** — Schema exists but not used by dashboard API.
- **RAID-OAUTH-012** — raid/requirements Rust legacy/fallback.
- **ANA-PUBLIC-002** — market_data Monitor-Definition geaendert: Python is_monitored_only=1, Rust nicht in twitch_partners.

### Architektur: Task-Supervision / Config-Validierung / Error-Shapes — 11

- **DASH-INTERNAL-008** — internal API error handling drift.
- **CHAT-IRC-015** — Task-Panic-Risiko im IRC Umfeld.
- **CHAT-IRC-009** — Kein Stop-Signal.
- **OPS-RUNTIME-005** — Config-Validierung strenger/anders.
- **MON-ANN-003** — Rust keine Config-Validation.
- **OPS-RUNTIME-004** — Background-Tasks in Rust nicht zentral ueberwacht.
- **DASH-AFF-011** — Admin auth/CSRF error shape drift.
- **STOR-SCHEMA-006** — Config-/Retry-Parity nicht vollstaendig; Rust fatal bei invalid optional env.
- **CHAT-API-014** — Mutex::lock().unwrap() Poison-Panic-Risiko.
- **CHAT-PIPE-006** — Python isoliert Schritte per try/except, Rust ruft Ports direkt.
- **DASH-INTERNAL-019** — diagnose/scam-guard Rust-only, error shapes mixed.

### EventSub/Webhook-Modell (Webhook-only bewusst, WS-Gaps) — 9

- **MON-SUB-005** — 401 nur debug und 6h reconcile.
- **MON-SUB-012** — Capacity Rust schreibt nur used_slots; Dashboard/Alerts weniger aussagekraeftig.
- **MON-SUB-010** — 403 perm_failed bis Neustart.
- **DASH-BILL-012** — Rust webhook propagiert plan-sync errors in same TX; Python continues.
- **MON-SUB-002** — Rust core subscription ensure best-effort ohne Fehlerstatus; Python startup coverage fail-closed.
- **MON-SUB-006** — 429 nur debug und 6h reconcile.
- **CHAT-PIPE-002** — Rust deserialisiert strict ChatMessageEvent; Python normalisiert TwitchIO-3.x dynamisch.
- **MON-INBOX-011** — Dashboard EventSub bridge Outbox in Rust nicht gefunden.
- **MON-INBOX-014** — EventSub bridge Details unklar.

### Auth/Session/Grace-Vertrag (Cutover-Entscheidung) — 7

- **RAID-SETUP-013** — Grace expiry anders.
- **MON-POLL-009** — Reauth reminder stale stream_id.
- **RAID-OAUTH-006** — Rust Dashboard-Scope fordert zusaetzlich channel:manage:broadcast, Python nicht.
- **DASH-AUTH-011** — Partner cookie Python SameSite=Strict, TTL 1800; Rust Lax, 21600.
- **RAID-OAUTH-010** — Grace expiry Rust setzt kein manual_partner_opt_out=1/token_error_expired.
- **RAID-SETUP-016** — Rust restauriert token_error% anders.
- **RAID-RECRUIT-005** — Rust setzt followers_total=None, Python Bot-OAuth/Streamer fallback.

### Public/Announcement/Config-Rendering-Vertrag — 8

- **ANA-PUBLIC-003** — /twitch/market HTML nicht voll portiert.
- **MON-ANN-001** — Rust rendert nur default config, Python laedt pro Streamer Config.
- **CHAT-API-004** — Announcement-Statusdetails gehen verloren.
- **ENG-SOCIAL-005** — Admin-Promo/Announcements verlieren Actor-Attribution, Rust immer admin.
- **DASH-LIVEANN-008** — Live announcement dashboard gaps.
- **DASH-GATE-014** — Public demo API muss verifiziert werden, dass keine echten Daten leaken.
- **DASH-GATE-011** — live-announcement API removed/redirected; Page wohl gewollt, API intent unklar.
- **ENG-SOCIAL-001** — Social-Legal-Aliases fehlen; Rust Templates linken /privacy und /terms, Router nur /social-media/*.

### Chat/Engagement-Wiring (Klassifizierungs-/Lifecycle-Vertrag) — 3

- **CHAT-IRC-014** — Engagement-IRC Ordering-Risiko.
- **CHAT-IRC-010** — Chatters-Poll kann ohne TokenProvider ausfallen.
- **CHAT-IRC-003** — Rust trackt alle Chatters als Category, Python differenziert.

### Sonstiges (Intent unklar) — 29

- **MON-INBOX-010** — Rust Handler erlaubt nur 4 Worktypes.
- **MON-RAID-009** — Blacklist-Raid Whisper fehlt in Rust.
- **MON-RAID-004** — Manual suppression vor arrival insert in Rust; Python erst nach Insert.
- **MON-INBOX-005** — Requeue ohne Runtime-Wakeup.
- **RAID-ARR-014** — Rust laedt post-confirm DB Score statt Pending _partner_score.
- **RAID-SCORE-014** — Outreach boost queued/detected/aktive Partner anders.
- **RAID-EXEC-013** — Rust manual internal route neu/privilegiert mit wenig Checks.
- **RAID-RECRUIT-010** — Rust spawnt send ohne Join/Rebind/Follow.
- **RAID-SETUP-007** — Rust braucht konkrete Guild-ID fuer role sync, Python fallback alle guilds.
- **DASH-LIVEANN-002** — Native config API missing.
- **DASH-LIVEANN-003** — Native test API missing.
- **DASH-LIVEANN-004** — Native preview API missing.
- **ANA-REPORT-007** — Rust no-plan report blocked, Python MiniMax fallback.
- **ANA-REPORT-015** — AI plan gate/model drift; Python granular analytics.ai_mini/full, Rust consolidated analytics.
- **ANA-REPORT-016** — Rust save_analysis DB error returns None silently.
- **MON-POLL-008** — ScoreRefresh Konsum/Wiring unklar.
- **RAID-OAUTH-013** — Rust raid/requirements Handler nicht registriert/ohne Idempotency, falls aktiviert.
- **RAID-EXEC-012** — Partner delivery Side-Effect in Rust Ziel-Datei nur Planner/TODO; Wiring abhaengig.
- **RAID-RECRUIT-009** — Due external bot-ban-check processor in Rust nicht gefunden.
- **DASH-LIVEANN-005** — Page redirect gewollt, API intent unklar.
- **DASH-LIVEANN-010** — Native APIs missing/unclear.
- **DASH-INTERNAL-012** — /raid/requirements nicht registriert.
- **ANA-REPORT-017** — Python logs raw AI parse response prefix, Rust vermeidet.
- **ANA-PUBLIC-001** — recent-bans zaehlt/listet anders als Python; Rust nur event_type='ban', today CURRENT_DATE, channels_protected aktive Partner.
- **LLM-TITLE-001** — Title-MiniMax-Ledger fehlt in Rust.
- **LLM-TITLE-003** — Stale Rust-Kommentar/Test behauptet !title nicht portiert.
- **STOR-SCHEMA-003** — twitch_auto_raid_pause in Rust entfernt, Python/Doku kennen es noch.
- **OPS-RUNTIME-003** — Logging-Paritaet fehlt; Rust stdout/journald, Python rotating/access logs.
- **OPS-RUNTIME-006** — Live-Cutover haengt an externer Ops-Kopplung.

## Residuals (bewusst offen gelassen)

- **DASH-AUTH-007** — Der OAuth-Kontext-Cookie nutzt Path=/ statt des callback-spezifischen Pfads. Relevant nur im Python/Rust-Mischbetrieb, der nach dem Cutover (Python aus) nicht mehr existiert. Cookie-Name ist angeglichen. Bei Bedarf später nachziehen.

## Lektionen aus dem Lauf

- Ein „Python→Rust-Parität"-Audit setzt Python = Wahrheit. Mehrfach war Rust aber bewusst korrekt/strenger (globale Bans als Raid-Sperre, Hard-Pause-Reasons). Solche Findings sind das Gegenteil eines Bugs — vom User per Ruling bestätigt.
- Zwei-Pässe-Triage (Finder + Skeptiker) trennt „mechanischer Bug" sauber von „Architektur-/Intent-Entscheidung". 45 von 95 Fix-Kandidaten waren in Wahrheit Design-Entscheidungen.
- Der adversariale Schluss-Kritiker fand eine echte Regression (timestamptz-Cast verloren) und ein inertes Cross-Crate-Gate (Engagement-bool ignoriert) — beides trotz grüner Per-Crate-Tests. Cross-Crate-Verträge fängt kein einzelner Crate-Test.
- `cargo fmt` über einen ganzen Crate erzeugt Diff-Müll über unbeteiligte Dateien; vor Commit auf die geänderten Dateien begrenzen.
