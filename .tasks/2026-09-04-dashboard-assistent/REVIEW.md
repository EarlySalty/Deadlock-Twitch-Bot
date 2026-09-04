status: erledigt
datum: 2026-09-04
head: ab9d44cd

# Merge-Review: KI-Hilfe-Assistent im Streamer-Dashboard

## Urteil: FREIGABE

Keine blockierenden Mängel. Auth, Datenschutz, Grounding, Rate-Limits, Logging und
die Wiederverwendung der Lesefunktionen sind korrekt umgesetzt. Alle offenen Punkte
sind Klasse sollte oder kann und können vor oder nach dem Merge fallen.

Tests: tb-dashboard-api 459 plus weitere Suiten grün (Lib 1105 passed, 1 ignored),
tb-analytics 12 passed (DB-Tests skippen ohne TB_TEST_DATABASE_URL). Frontend
dashboardAssistent.test.ts 6/6 grün. `cargo clippy -p tb-dashboard-api -- -D warnings`
sauber.

## Mängel

1. Klasse sollte. bot/dashboard_v2/src/components/assistent/DashboardAssistent.tsx:234.
   Der Umschaltknopf zeigt im offenen Zustand `t('Schließen')`, aber `'Schließen'`
   fehlt im EN-Wörterbuch (nur die aria-labels sind übersetzt). Englische Nutzer
   sehen dort das deutsche Wort. Verletzt REQ-02 minimal.
   Fix: `'Schließen': 'Close'` in den EN-Block von dictionary.ts aufnehmen.

2. Klasse sollte. bot/dashboard_v2/src/i18n/dictionary.ts (neue Zeile mit `//`-Kommentar).
   Neuer Code-Kommentar in einer angefassten Datei; Hausregel und INV-08 verlangen
   keine Code-Kommentare. Fix: Kommentarzeile entfernen.

3. Klasse kann. rust/crates/tb-dashboard-api/src/handlers/dashboard_assistent.rs:42,54,446.
   `karten_sind_frei_von_geheimnissen` prüft per Teilzeichenkette gegen die ganze
   Karte. Ein Raid-Ziel-Login mit einem verbotenen Fragment (etwa Kanalname
   "monkey" enthält "key") verwirft die komplette Raid-Karte still. Fällt sicher
   (Karte fehlt, kein Leak), aber echte Daten gehen ohne Signal verloren. Die
   eigentliche Datenschutz-Garantie kommt ohnehin aus der Feldauswahl (die Karten
   enthalten konstruktiv keine Tokens, Keys, URLs), der Filter ist nur ein Netz.
   Fix: auf konkrete Secret-Muster statt Ganze-Karte-Substring gehen.

4. Klasse kann. bot/dashboard_v2/src/api/assistent.ts:38,52,60.
   429- und generischer Fehlertext sind hart auf Deutsch; die Komponente ersetzt
   sie zwar über `t(...)` erneut, der Server liefert im 429-Body aber bereits eine
   sprachrichtige `message`, die der Client ignoriert. Konsistent, aber doppelt
   gepflegt. Kein Nutzerfehler, da die Komponente übersetzt.

5. Klasse kann. dashboard_assistent.rs:399-420,466-478.
   Nur der Modellaufruf ist zeitgedeckelt (timeout/total_deadline 110 s). Die
   vorgelagerten DB- und Uplink-Aufrufe (tokio::join!, last_stream_summary,
   partner_id/live_status/verbindungen_lesen) sind unbegrenzt. Bei DB-Stau kann die
   Gesamtzeit das Client-Budget von 125 s überschreiten; der Client bricht ab, der
   Server rechnet weiter (verbrauchtes Rate-Token, LLM-Kosten). Risiko gering, da
   die Blöcke fehlertolerant und normal schnell sind.

6. Klasse kann. rust/crates/tb-dashboard-api/src/handlers/ad_manager.rs:55.
   `scopes` wurde auf `pub(crate)` gehoben, wird vom neuen Handler aber nicht
   genutzt (er verwendet `oauth_block` für die Rechte-Karte). Sichtbarkeitsweitung
   ist im Amendment erlaubt, hier aber überflüssig. Fix: auf `fn` zurücknehmen.

7. Klasse kann. dashboard_assistent.rs:387.
   Reihenfolge `!tages_limit().allow() || !minuten_limit().allow()`: bei erlaubtem
   Tages- und blockiertem Minutenlimit ist der Tagesdeckel bereits um 1 erhöht,
   obwohl die Anfrage mit 429 abgelehnt wird. Nur zulasten des aggressiven Nutzers,
   funktional unkritisch.

8. Klasse kann. dashboard_assistent.rs (Tests).
   Die Verdrahtung des Injection-Flags in den Log-Eintrag (REQ-10) ist nicht
   getestet; nur `looks_like_injection` selbst ist (in self_explainer) getestet.
   Kein Test gefunden, der von Anfang an grün wäre und nichts prüft; keine
   bestehenden Tests abgeschwächt.

9. Info/Deploy. rust/migrations/20260904120000_twitch_dashboard_assistent_log.sql.
   Additiv und idempotent (CREATE TABLE/INDEX IF NOT EXISTS), Index
   (twitch_user_id, created_at DESC) passend, Spaltentypen decken die Bind-Reihenfolge
   der insert-Funktion exakt (TEXT/TEXT/TEXT/TEXT/TEXT/BOOLEAN/BOOLEAN/TEXT/TEXT/BIGINT).
   Der Insert nutzt `sqlx::query` (Laufzeit), also kein Offline-Metadatum nötig.
   Achtung Deploy: Bot migriert nicht selbst (TB_DB_MIGRATE=0). Migration als Rolle
   postgres anwenden und INSERT an twitchbot geben, sonst schlägt der Log-Spawn still
   fehl (in tokio::spawn mit 3 s Timeout, Fehler wird verworfen).

10. Info. dashboard_assistent.rs:535.
    Response-Feld `grounded` ist hart auf `true`, der echte Grounding-Wert geht nur
    ins Log. Entspricht Plan M3 Schritt 8, Frontend wertet das Feld nicht aus, kein
    Nutzereffekt.

## REQ/INV-Tabelle

| Punkt | Status | Beleg |
|---|---|---|
| REQ-01 Widget je Route, Panel, Enter/Escape, Laden/Fehler | erfüllt | App.tsx:452 global im LanguageProvider; DashboardAssistent.tsx:73-235 |
| REQ-02 Anzeigename, Duzen, Sprachwahl | teilweise | Gruß mit Name DashboardAssistent.tsx:128-130; en/de vorhanden; `Schließen` untranslated (Mangel 1) |
| REQ-03 POST-Route, 401 ohne Session, Identität aus Session | erfüllt | lib.rs:743; dashboard_assistent.rs:345-355; Test ohne_session_gibt_401 |
| REQ-04 Grounding Korpus plus Live-Datenkarten | erfüllt | dashboard_assistent.rs:399-452, Namespace::Bot audience "streamer" |
| REQ-05 nur eigene Daten, keine Secrets, Fremdkanäle ablehnen | erfüllt | alle Blöcke mit session-login/user_id; Filter:446; Prompt:310 |
| REQ-06 Rate-Limit 20/min zu 429, ehrlicher Fehler | erfüllt | dashboard_assistent.rs:387-393,497-504 (Text-Lang-Randfall Mangel 4) |
| REQ-07 dauerhaftes Log mit Flags, kein Discord-Log | erfüllt | dashboard_assistent_log.rs; Handler:510-529; kein Embed-Aufruf |
| REQ-08 tb-llm Use-Case, kein Hardcode | erfüllt | dashboard_assistent.rs:466-477 (USE_CASE, kein endpoint) |
| REQ-09 Vorschläge je Seite (min 3 plus Standard) | erfüllt | vorschlaege.ts; Test liefert je Seite/Sprache 3 |
| REQ-10 Injection-Marker erkannt, trotzdem beantwortet, geloggt | erfüllt | Handler:507,517 (Wiring ungetestet Mangel 8) |
| REQ-11 ehrlicher Discord-Verweis bei Lücke | erfüllt | Prompt:309 |
| INV-01 self_explainer unverändert | erfüllt | Diff nur fn/struct zu pub(crate), keine Logik |
| INV-02 bestehender Session-Extractor, kein zweiter Weg | erfüllt | DashboardAuthLevel plus uplink::twitch_identitaet |
| INV-03 keine neuen ENV/Secrets | erfüllt | keine env-Zugriffe im Diff, tb-llm/tb-db-Wege |
| INV-04 Modellwahl in tb-llm | erfüllt | kein endpoint()/eigene Kette |
| INV-05 Tests nicht gelöscht/abgeschwächt | erfüllt | nur additive Tests; package.json nur ergänzt |
| INV-06 Datenkarten aus bestehenden Lesefunktionen | erfüllt | internal_home/moderation/scam_guard/uplink; GET-Refactor verhaltensgleich (None-Defaults, Err zu 500 identisch) |
| INV-07 Shell unangetastet, Widget global | erfüllt | App.tsx nur Import plus ein Element |
| INV-08 keine Kommentare/Gedankenstriche, Umlaute | teilweise | Rust neu ohne Kommentare, keine Em-Dashes in Nutzertexten; ein Kommentar in dictionary.ts (Mangel 2) |
| INV-09 tb-knowledge einzige Doku-Quelle | erfüllt | knowledge_base() aus self_explainer, kein zweiter Korpus |

## Scope
Alle geänderten Dateien liegen im erlaubten Bereich (inkl. Amendments: internal_home,
moderation_settings, scam_guard_settings, uplink, ad_manager nur Sichtbarkeit/Extraktion;
rust/migrations statt tb-db/migrations; rust/knowledge nicht angefasst). Keine
Scope-Verstöße.

## Regressionen
moderation- und scam-guard-GET nach dem Umbau verhaltensgleich (Some zu gleicher JSON,
None zu gleichen Defaults, Err zu 500). scam_guard von query! auf query_as umgestellt,
Spaltenreihenfolge und Typen decken sich. Keine Verhaltensänderung an bestehenden
Endpoints erkannt, clippy und alle Suiten grün.

## Fix-Runde 1

- Mangel 1: behoben in a43cb772 (bereits erfüllt: `Schließen: 'Close'` liegt seit 17abf9e8 im EN-Block Zeile 232 und deckt `t('Schließen')` ab; ein zweiter Eintrag wäre ein doppelter Schlüssel und würde eslint no-dupe-keys auslösen, daher keine Änderung).
- Mangel 2: behoben in a43cb772 (neuen Kommentar `// -- Dashboard-Assistent ---` in dictionary.ts entfernt).
- Mangel 3: behoben in a43cb772 (`karten_sind_frei_von_geheimnissen` auf Regex mit Wortgrenzen umgestellt: `\b(token|secret|key|url|session|cookie|passwort|password)\b`, plus `https?://`/`rtmps?://`/`srt://` und `[A-Za-z0-9_-]{32,}`; verworfene Karte erzeugt `tracing::warn!` mit Kartenname ohne Inhalt; bestehender Test angepasst, Tests für "monkey" (durch), `https://` (verworfen) und 40-stellige Kette (verworfen) ergänzt).
- Mangel 5: behoben in a43cb772 (Datenkarten-Block inklusive `tokio::join!`, `last_stream_summary`, `partner_id`/`live_status`/`verbindungen_lesen` in `tokio::time::timeout(Duration::from_secs(10), ...)`; bei Ablauf leere Karte "nicht verfügbar" plus `tracing::warn!`; Gesamtbudget 10 s Karten + 110 s Modell = 120 s unter 125 s).
- Mangel 6: behoben in a43cb772 (`scopes` in ad_manager.rs wieder `fn` privat).
- Mangel 7: behoben in a43cb772 (Reihenfolge auf `!minuten_limit().allow() || !tages_limit().allow()` getauscht; per Kurzschluss erhöht ein per Minutenlimit abgelehnter Aufruf den Tagesdeckel nicht mehr; Test `minutenlimit_blockt_ohne_tageszaehler_zu_erhoehen` ergänzt).
- Mangel 8: behoben in a43cb772 (Eintrag-Aufbau in Helfer `baue_log_eintrag` gezogen; Test `log_eintrag_setzt_injection_flag` prüft ohne DB, dass eine Frage mit Injection-Marker `flagged_injection = true` setzt und eine harmlose Frage `false`).

Validierung: `cargo test -p tb-dashboard-api dashboard_assistent` 14 passed; `cargo test -p tb-dashboard-api -p tb-analytics` 459 + 1110 (1 ignored) + 12 passed, 0 failed; `cargo clippy -p tb-dashboard-api -- -D warnings` sauber; `npm test` 185 passed, `npm run lint` 0 errors (16 vorbestehende Warnungen in nicht angefassten Dateien), `npm run build` grün.
