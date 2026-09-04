# Contract: Streams-Tab zeigt ehrliche Werte (Bindung, Geistersessions, Titel)

## Ziel
Der Streams-Tab im Analyse-Dashboard (`/analyse?tab=streams`) zeigt je Stream eine aussagekräftige Bindungs-Kennzahl statt der fast immer 100 % zeigenden 10m-Retention, blendet Geistersessions (0 Zuschauer, unter 5 Minuten) aus Liste und Kennzahlen aus, und neue Sessions bekommen ihren Titel auch dann, wenn EventSub sie ohne Titel eröffnet hat.

## Umfang (erlaubter Bereich)
- `rust/crates/tb-analytics/src/overview.rs`
- `rust/crates/tb-dashboard-api/src/handlers/overview.rs`
- `rust/crates/tb-monitoring/src/sessions/store.rs`
- `rust/crates/tb-monitoring/src/sessions/tracker.rs`
- `rust/crates/tb-monitoring/tests/`
- `bot/dashboard_v2/src/pages/Sessions.tsx`
- `bot/dashboard_v2/src/types/analytics.ts`

## REQ
- REQ1: `overview_sessions` liefert je Session ein neues Feld `holdPct` = `avg_viewers / peak_viewers * 100`, geklemmt auf 0..100; bei `peak_viewers = 0` ist der Wert 0. Die Felder `retention5m/10m/20m` bleiben unverändert erhalten.
- REQ2: Geistersessions sind Sessions mit `peak_viewers = 0` UND effektiver Dauer unter 300 Sekunden. Sie fallen aus `overview_sessions` UND aus `overview_metrics` (session_count, total_airtime_hours, avg_avg_viewers, max_peak_viewers) heraus. Die Bedingung steht genau einmal als SQL-Fragment (Konstante in `overview.rs`) und wird an beiden Stellen eingesetzt.
- REQ3: In `Sessions.tsx` zeigt die rechte Kennzahl der Session-Karte `holdPct` mit Label "Bindung" und Unterzeile "Ø Zuschauer / Peak" statt "10m Retention". Farbschwellen: ≥ 60 grün, ≥ 40 gelb, sonst rot. Die aufgeklappten Detail-Boxen (5/10/20 Min, Dropoff) bleiben unverändert. `StreamSession` in `types/analytics.ts` bekommt `holdPct: number`.
- REQ4: Der Store bekommt eine Methode, die für eine bestehende Session `stream_title` und `game_name` nur dann setzt, wenn sie in der DB leer sind (`NULL` oder `''`), unabhängig von `samples` und `start_viewers`. `ensure_session` im Tracker ruft sie bei jedem Aufruf für eine bereits offene Session mit nicht-leerem Titel bzw. Spiel des Snapshots auf, zusätzlich zu `adopt_incomplete`.

## INV
- INV1: Die Spalten `retention_5m/10m/20m` und ihre Berechnung in `tb-monitoring/src/sessions/metrics.rs` bleiben unangetastet; alle anderen Konsumenten (Overview-Tab, Coaching, Vergleich, Timeline) sehen dieselben Werte wie heute.
- INV2: Keine Migration, keine neue Spalte, keine Änderung an `adopt_incomplete`.
- INV3: `overview_sessions` und `overview_metrics` zählen dieselbe Menge Sessions (gleiches Filterfragment), damit "Total Streams" und die Liste zusammenpassen.
- INV4: Ein vorhandener, nicht-leerer Titel in der DB wird nie überschrieben.
- INV5: Keine Code-Kommentare, echte Umlaute in nutzersichtbaren Texten, keine Em-Dashes.

## Nicht-Ziele
- Kein Zusammenlegen von Flap-Sessions (kurze Sessions mit Zuschauern bleiben sichtbar).
- Kein Backfill fehlender Titel für historische Sessions.
- Keine Änderung an EventSub-Handlern oder am Poller-Ablauf.
- Keine Anpassung anderer Tabs oder der Python-Legacy.

## Regressionstests (Pflicht, vor dem Fix rot)
- T1 (`tb-monitoring`): Session mit `start_viewers > 0`, `samples > 0` und leerem Titel; nach `ensure_session` mit einem Snapshot mit Titel steht der Titel in der DB. Vor REQ4 rot.
- T2 (`tb-analytics`, DB-Test wie `overview.rs` Tests mit `TB_TEST_DATABASE_URL`): eine Geistersession (peak 0, 60 s) plus eine echte Session (peak 10, avg 6, 30 min) für denselben Streamer; `overview_sessions` liefert genau eine Session mit `holdPct = 60`, `overview_metrics.session_count = 1`. Vor REQ1/REQ2 rot.
Roten Lauf mit Testname und Fehlermeldung in `EVIDENCE.md` unter `## Roter Lauf` festhalten.

## Amendments
- A1 (2026-09-04, nach Review-Hinweis): `bot/dashboard_v2/src/components/tables/SessionTable.tsx` (Overview-Tab) zeigt in der Spalte "Bindung" ebenfalls `holdPct` statt `retention10m`, damit beide Tabs dieselbe Kennzahl zeigen. Klasse niedrig, kein eigener Test.
- A2 (2026-09-04, nach Merge-Gate-Befund): Die Geister-Definition (`GEISTER_FILTER`) gilt zusätzlich für `letzte_beendete_session` in `rust/crates/tb-analytics/src/stufe.rs` (Fenster-Anker `last_stream`) und für `overview_session_count` in `overview.rs` (Existenz-Check vor `Empty`), damit Anker, Existenz-Check, Liste und Summary dieselbe Session-Menge sehen. Erlaubter Bereich um `stufe.rs` und `bot/dashboard_v2/src/utils/formatters.ts` (gemeinsame Farbfunktion `getHoldColor` mit Schwellen 60/40 für beide Tabs) erweitert.
