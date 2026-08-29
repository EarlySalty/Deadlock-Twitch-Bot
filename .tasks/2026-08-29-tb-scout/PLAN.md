# Plan: tb-scout

status: aktiv
datum: 2026-08-29
contract: CONTRACT.md (dieser Ordner)
branch: feat/tb-scout

Ziel, Anforderungen und Invarianten stehen im CONTRACT.md. Dieser Plan
verweist nur noch auf ihn. Nach jedem verifizierten Milestone Status hier
eintragen (Datum + Ergebnis + Validierungsbefehl).

## M1 — Datenmodell und Scout-Kern (Crate tb-scout)

- Änderungen: Migration `twitch_scout_candidates`
  (streamer_login, twitch_user_id, sessions_count, avg_viewers, first_seen,
  last_seen, language, deadlock_share, status: vorgeschlagen|approved|
  uebersprungen|pausiert, entscheid_grund, approver, decided_at,
  dispatched_at, unique(streamer_login)); neues Crate
  `rust/crates/tb-scout`: Store (Upsert-Kandidat, Entscheidung setzen,
  approved-ohne-dispatched lesen), Query "klein + first_seen" über
  `twitch_stats_category` (MIN(ts_utc) als first_seen, Sessions per
  LAG/30-min-Gap wie admin_research.rs) mit den harten Filtern aus REQ-03
  als SQL-NOT-EXISTS plus `is_hard_banned`-Probe im Code (fail-closed).
- Erwarteter Zwischenzustand: Crate kompiliert, Detector liefert gegen
  Test-PG die erwarteten Kandidaten, Filter schlagen korrekt an.
- Validierung: `cargo test -p tb-scout` im `rust/`-Arbeitsbereich (Repo-Flags
  laut rust/CLAUDE.md ergänzen), plus `cargo clippy -p tb-scout`.
- Stop-Regel: rote Tests oder fehlende Listen-Gates → Stop-and-fix.

## M2 — Dashboard-API

- Änderungen: Handler GET `/twitch/api/admin/scout/candidates`
  (Liste vorgeschlagen+pausiert mit Kennzahlen) und POST
  `/twitch/api/admin/scout/candidates/:login/decision`
  (approve|uebersprungen|pausiert + Grund), Anmeldung wie
  admin_research/admin_partner_signup_block (admin + CSRF), Routen in
  lib.rs neben den research-Routen.
- Erwarteter Zwischenzustand: Beide Endpunkte gegen Test-PG grün;
  Nicht-Admin erhält 401/403, CSRF-fehlender POST wird abgewiesen.
- Validierung: `cargo test -p tb-dashboard-api scout` (oder Filter auf die
  neuen Tests), `cargo clippy -p tb-dashboard-api`.
- Stop-Regel: Auth/CSRF-Lücke → sofort fixen, kein Weiterbau.

## M3 — Besuch-Erkennung statt Bot-Dispatch (überholt den ursprünglichen M3)

- Stand 2026-08-29, User: die Trust-Leiter (`recruitment_messaging.rs`) ist
  deaktiviert und wird nicht mehr genutzt; darauf wird NICHT gebaut. Der
  ursprüngliche Dispatch über `enqueue_partner_outreach` entfällt ersatzlos.
- Änderungen: Im Tick von `rust/bin/tb-bot` (oder im tb-scout-Store) eine
  read-only Erkennung: taucht der Owner-Login (earlysalty) im Chat oder in
  `twitch_viewer_presence_ticks` eines Kandidaten-Kanals auf, wird der
  Kandidat auf "persönlich" gesetzt bzw. mit `visited_at` markiert. Kein
  Versand, keine Nachricht, kein LLM.
- Erwarteter Zwischenzustand: Test mit seedeten Chat-/Präsenzzeilen zeigt
  die Markierung; ohne Owner-Auftritt keine Markierung; idempotent.
- Validierung: `cargo test -p tb-scout besuch` (bzw. benannte Tests).
- Stop-Regel: würde ein Versand nötig → Stop, Contract-Verstoß.

## M4 — Frontend Research-Seite

- Änderungen: Section "Scout-Freigaben" in Research.tsx (Tabelle: Login,
  Sessions, Ø Zuschauer, first_seen, last_seen, Sprache, Status, Grund;
  je Zeile Freigeben/Überspringen/Pausieren/Nehme-ich-persönlich mit
  Grund-Eingabe), zwei Client-Funktionen in client.ts, Query-Invalidierung.
  Zusatz 2026-08-29: Ansicht "Meine persönliche Besuchsliste" (Status
  persönlich, sortiert nach Potenzial) und, falls für den Kandidaten eine
  persönliche Invite-URL existiert (`twitch_streamer_invites`, Anzeige über
  Bestands-API `tb-internal-api discord_invite`), deren Anzeige zum
  Rausexen im Chat. Nur Anzeige, kein Versand. Stil an die bestehende
  Gold-Optik der Seite angelehnt (Bestandsklassen wiederverwenden).
- Erwarteter Zwischenzustand: Seite zeigt Kandidaten aus der Test-/Prod-Daten
  korrekt; Aktionen persistieren und überleben Reload; Desktop und Mobil
  sauber (Screenshot-Prüfung).
- Validierung: `npm run build` in bot/admin_dashboard, danach
  Browser-Durchlauf (Login, Aktionen, Reload, Mobil-Viewport).
- Stop-Regel: Aktion ohne Persistenz oder Layoutbruch → fixen vor M5.

## M5 — Review, Merge, Deploy, Live-Check

- Änderungen: keine (nur Review-Befunde abarbeiten).
- Ablauf: frischer Read-only-Reviewer (rust-reviewer plus
  silent-failure-hunter) gegen Contract + Diff; `diff-policy.py` von Hand;
  Quality-Agent-Kritik; danach Merge nach main, `cargo build --release`,
  Migration auf Prod-PG, `systemctl --user restart` der betroffenen Units,
  Live-Check: Research-Seite zeigt echte Kandidaten, Freigabe einer
  Testperson durch den Nutzer, Dispatch sichtbar in
  `twitch_partner_outreach`.
- Stop-Regel: Review-Befund mit Contract-Bezug immer fixen; Deploy nur mit
  grünem Build und grünen Tests.

## Status

- 2026-08-29: Plan erstellt, M1-M5 offen.
- 2026-08-29: M1 erledigt — Migration `20260829090000_twitch_scout_candidates.sql`,
  Crate `rust/crates/tb-scout` (Detector-Query klein + first_seen mit harten
  Filtern als NOT-EXISTS, Global-Ban-Probe fail-closed im Code; Store mit
  Upsert-Schutz nur für `vorgeschlagen`, Entscheidung, approved-ohne-dispatch,
  Dispatch-Stempel). Validierung: `TB_TEST_DATABASE_URL=… cargo test -p tb-scout`
  → 9 bestanden (2 Lib- + 7 PG-Tests), `cargo clippy -p tb-scout --all-targets` sauber.
- 2026-08-29, User-Nachtrag (Contract-Freeze, daher hier): Vierter
  Kandidaten-Status **"persoenlich"** in M1 (DB-Enum) und M2 (POST-Decision)
  aufnehmen: Bedeutung "Owner übernimmt den Kanal persönlich (Chat, Hilfe,
  Beziehung)"; der Bot dispatcht nur "approved", "persönlich" nie (M3 bleibt
  unverändert korrekt). M4 zeigt den Status als eigene Ansicht "Meine
  persönliche Besuchsliste" mit Kennzahlen. DB-Befund als Grund: 51 von 73
  bespielten Kanälen waren reine Chat-Beziehungen ohne Raid.
- 2026-08-29, User-Korrektur: Die Trust-Leiter (`recruitment_messaging.rs`)
  ist deaktiviert und bringt nichts mehr; darauf wird nicht aufgebaut.
  M3 (Dispatch in den Outreach-Weg) ist damit überholt und wurde durch die
  Besuch-Erkennung ersetzt (siehe M3 neu). Zusätzlich in M4: Anzeige einer
  existierenden persönlichen Invite-URL je Kandidat (nur Anzeige).
