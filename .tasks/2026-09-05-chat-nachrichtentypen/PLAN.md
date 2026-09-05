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

- M2: Migration `20260905120000_twitch_chat_message_labels.sql` (PK `message_id BIGINT`, KEIN Fremdschlüssel, weil `twitch_chat_messages` eine TimescaleDB-Hypertable ohne PK/Unique auf `id` ist; Index auf `label`). Per psql in BEGIN/ROLLBACK geprüft (CREATE, INSERT, CHECK greift). `lade_unlabelte` und `speichere_labels` als `sqlx::query`/`query_as` ohne Makro (UNNEST-Insert mit ON CONFLICT DO NOTHING).
- M3: `klassifiziere_modell` (Deepseek über tb-llm-Hub, Temp 0, JSON, Timeout 60 s) liefert Labels plus Modellname; unbekannte Typen -> Other. Test mit wiremock-Endpoint belegt: nur Other-Nachrichten landen im Paket, genau ein HTTP-Aufruf, unbekannter Typ -> Other. tb-analytics 9 chat_typen-Tests grün.
- M4: `chat_typen_wiring.rs` (Job `twitch_chat_typen`): beim Start und stündlich, bis 20000 unlabelte laden, Regelstufe für alle (alles außer Other sofort mit quelle regel gespeichert, leere Inhalte als regel-Other), Other in 40er-Paketen ans Modell (quelle modell, Modellname aus der Hub-Antwort), Tageskappe 2000 Modellaufrufe als Konstante mit In-Memory-Zähler samt Tageswechsel-Reset, Paketfehler nur geloggt, ein tracing::info je Lauf. Eingehängt in main.rs neben den anderen Wirings. tb-bot 290 grün (+2 Wiring-Tests).
- M5: chat_analytics.rs Loader per LEFT JOIN auf twitch_chat_message_labels (all_messages als Laufzeit-query_as ohne Makro), Zählung nutzt gespeichertes Label sonst klassifiziere_regel, System aus der messageTypes-Rechnung ausgeschlossen (eigener Nenner type_total), Antwort trägt labelCoverage. viewers.rs Personality analog per LEFT JOIN, System ausgeschlossen, tote classify_message-Kopie entfernt. Frontend: MESSAGE_TYPE_LABELS in dictionary.ts, Karte zeigt deutsche Bezeichnungen plus Hinweis bei labelCoverage < 0.95, MessageTypeStat/Antwort um labelCoverage ergänzt, Test tests/nachrichtentypen.test.ts. Rust 470 + 1114 + 290 grün, Build tb-bot/tb-dashboard EXIT 0. Frontend 231/231 grün, build EXIT 0, eslint auf den geänderten Dateien sauber (EXIT 0). Offen (nicht im Änderungsbereich): eslint meldet in main-Baseline 1 Fehler (`src/hooks/dashboardProfileCache.ts:17` no-useless-assignment) plus 16 Warnungen in unberührten Dateien; gehört in einen eigenen Auftrag.
- Review-Runde 1 (gate_hook --review): 1 BLOCKING plus 8 NIT. Behoben: (1 BLOCKING) Tageskappe zählt jetzt jeden Modellversuch vor dem Aufruf und bricht nach 5 Fehlern in Folge ab; (5) System-Muster für Truhe/Abofalle an die Bot-Ausgabe gebunden statt bloßer Wortsuche; (8) tote classify_message-Kopie aus chat_analytics.rs entfernt, Test nutzt jetzt das Modul; (9) Karten-Hinweis nur bei vorhandenen Nachrichtentypen; (2) Statement in der viewers.rs-Personality auf Other gefaltet, damit die Personality-Karte kein rohes Englisch zeigt; (3) Produktions-Filter als ist_modellkandidat extrahiert und getestet. Bewusst belassen mit Begründung: (4) Teilantworten des Modells schreiben den Rest als Other/modell fest (terminiert, nie schlechter als die Regelstufe; ohne Versionsspalte gewollt eingefroren); (7) lade_unlabelte ohne untere Zeitgrenze ist die im Contract vorgegebene Query; (6) Grants gehören laut REQ-03 nicht in die Migration. Nach den Fixes alles grün: tb-analytics 470, tb-dashboard-api 1114, tb-bot 291, Builds 0, Frontend 231, eslint auf den geänderten Dateien 0.
- Review-Runde 2 (gate_hook --review): ALLOW, FIXED chat_typen_wiring.rs:78, keine Regressionen, REVIEW_EXIT=0.
