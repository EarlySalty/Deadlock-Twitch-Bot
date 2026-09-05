# Plan: Chat-Nachrichtentypen

status: aktiv
datum: 2026-09-05
klasse: mittel
research: EVIDENCE.md

## Ziel

Siehe CONTRACT.md.

## Milestones

### M0: Baseline
Validierung: `cargo test -p tb-analytics -p tb-dashboard-api -p tb-bot` (Toolchain 1.97, Ausgabe in Datei), rote Tests notieren.

### M1: Regelstufe mit Tests (rot, dann grün)
Änderungen: `tb-analytics/src/chat_typen.rs` mit Enum und `klassifiziere_regel`; Tests aus der EVIDENCE-Stichprobe; beide Kopien von `classify_message` durch das Modul ersetzen (bestehender Test `classify` bleibt und läuft gegen das Modul).
Validierung: Paket-Tests grün.

### M2: Migration und Label-Speicher
Änderungen: `rust/migrations/…_twitch_chat_message_labels.sql`; `chat_typen::speichere_labels`, `lade_unlabelte(limit)`; `.sqlx`-Daten neu erzeugen, falls `query!`-Makros genutzt werden (sonst `sqlx::query` ohne Makro).
Validierung: `cargo build` grün; Migration per `psql -d twitch_analytics -f` in einer Transaktion mit ROLLBACK getestet (nur lesen bzw. verwerfen), Anwendung macht der Orchestrator.

### M3: Deepseek-Paketaufruf
Änderungen: `chat_typen::klassifiziere_modell(pakete)`: Prompt mit Typenliste und Definitionen, JSON-Antwort `{"labels":[{"i":0,"t":"Question"}, …]}`, unbekannte Typen werden `Other`; Test mit gezähltem Aufruf über den Test-Endpoint des Hubs.
Validierung: Paket-Tests grün.

### M4: Job in tb-bot
Änderungen: `chat_typen_wiring.rs`, Start in `main.rs` neben den anderen Wirings; Limits als Konstanten; Log je Lauf (Anzahl regel, modell, offen).
Validierung: `cargo build -p tb-bot`; Trockenlauf-Test des Schedulers ohne Netz.

### M5: Endpoint und Frontend
Änderungen: LEFT JOIN auf Labels in `chat_analytics.rs` und `viewers.rs`, `labelCoverage`; deutsche Labels und Hinweis in `chatAnalyticsContent.tsx`, Test `tests/nachrichtentypen.test.ts` für die Übersetzungstabelle.
Validierung: `npm run build`, `npm run lint`, `npm test`.

## Verlauf

- M0: Baseline `cargo test -p tb-analytics -p tb-dashboard-api -p tb-bot` grün (461 + 288 + 1114 usw., EXIT=0), keine rote Baseline.
- M1: `chat_typen.rs` mit `Nachrichtentyp`, `api_key`/`from_api_key`, `klassifiziere_regel`. Rot-Lauf mit gestubbter Regel: `test result: FAILED. 1 passed; 6 failed` (u. a. `chat_typen::tests::evidence_reaction ... FAILED`, `left: "Other" right: "Reaction"`). Nach voller Regel grün: 7 passed. Beide `classify_message`-Kopien (`chat_analytics.rs`, `viewers.rs`) delegieren jetzt an das Modul. tb-analytics 468 + tb-dashboard-api 1114 grün, keine Warnungen. Abweichung von der Prompt-Vorgabe: Statement-Schwelle 5 statt 4 Wörter, weil die Evidence-Zeile "Ai viewers streamboo . Com" (4 Wörter) laut REQ-07 `Other` bleiben muss; Question auf Fragezeichen bzw. Frage-Anfangswort verengt, damit "... velocity ... sniper ..." laut Evidence `Game-Related` wird.

