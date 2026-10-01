# Review Runde 1: Pakete C (Telemetrie) und D (Chat-Hinweis)

## Urteil: FIX NÖTIG

- Abnahme Paket C fertig: **JA** (funktional erfüllt; drei Mängel, kein Blocker).
- Abnahme Paket D fertig: **NEIN** (ein BLOCKER, ein MANGEL).

Kurzbegründung: C erfüllt den Wunsch (Moment je Werbung gespeichert, Wirkung gegen
werbefreie Momente derselben Sendung herausgerechnet, Empfehlung erst ab 15 je Gruppe,
Vorzeichen korrekt, kein Fremdkanal-Leak). D funktioniert im Normalfall, aber ein
DB-Fehler beim Hinweis kann den Entscheider abbrechen (Briefing-Punkt 2), und
vorgezogene Werbung bekommt entgegen NACHTRAG-3 keinen Hinweis.

SQL gegen `fresh_schema_snapshot.txt` geprüft: alle Abfragen in `telemetry.rs`,
`BACKFILL-C.sql` und `ad_manager.rs` (Paket D) treffen vorhandene Spalten und Typen.
`store_ad_break_event` läuft im EventSub-Dispatch (`dispatch.rs:934`), nicht im
Entscheider-Tick; sein `?` gefährdet den Entscheider nicht. Migrationen 140000/150000
kollidieren nicht, keine Änderung an der auf Prod stehenden 120000. Neue Spalten nur
per ADD COLUMN, keine neue Tabelle oder Sequenz, daher keine GRANTs nötig.

---

## 1. BLOCKER, D, `rust/bin/tb-bot/src/ad_manager_wiring.rs:390,396`
`store.last_hint(...).await?` und `store.record_hint(...).await?` stehen im Hinweis-Block
vor der Ausführung der Entscheidung (`if let (Some(decision), Some(schedule)) = ...`).
Beide `?` propagieren nach `process_channel` und geben `Err(WorkerError)` zurück.
Ein transienter DB-Fehler beim Lesen oder Schreiben des Hinweis-Merkers bricht damit den
ganzen Kanal-Tick ab, bevor der Commercial- oder Snooze-Zweig läuft: die eigentliche
Werbung wird in diesem Tick nicht gestartet. Das verletzt Briefing-Punkt 2 ("Ein Fehler
im Hinweis darf den Entscheider nie abbrechen").
Muss gelten: Hinweis-DB-Fehler loggen und schlucken (wie im `send_message`-Zweig schon
geschehen), nie per `?`. Alternativ den Hinweis-Block hinter die Entscheidungsausführung
ziehen, so dass ein Hinweisfehler die schon gelaufene Aktion nicht mehr verhindert.

## 2. MANGEL, D, `rust/crates/tb-analytics/src/ad_manager.rs:829-847` (ad_hint), `:727` (pulled_forward)
Vorgezogene Werbung bekommt keinen Chat-Hinweis. Beim Pull-Forward ist `decision.action`
gleich `Commercial` und `next_ad_at` gesetzt; der Twitch-Zweig von `ad_hint` gibt bei
`Commercial` `None` zurück (und `secs` läge ohnehin über dem 45s-Fenster, weil
`should_pull_forward` `next_ad > now + action_lead` verlangt). NACHTRAG-3 Punkt 1
verlangt den Hinweis ausdrücklich auch für Werbung, "die der Bot ... vorzieht".
Muss gelten: entweder beim Pull-Forward-Beschluss einen Hinweis senden (mit einer
Textvariante ohne das "in 30 Sekunden"-Versprechen, da der Block sofort startet), oder
die Abweichung beim Auftraggeber ausdrücklich freigeben lassen. Reiner Bot-Eigenblock
und ungeschobene Twitch-Werbung sind korrekt abgedeckt.

## 3. MANGEL, C, `rust/crates/tb-analytics/src/monetization.rs:447,455`
Nutzersichtbare Empfehlungstexte mit ASCII-Ersatz statt echter Umlaute:
"Noch zu wenig Werbungen fuer eine Empfehlung ..." und "Am besten laeuft Werbung ...".
Verstößt gegen die Pflicht zu echten Umlauten in nutzersichtbaren Texten.
Muss gelten: "für" und "läuft".

## 4. MANGEL, C, `rust/crates/tb-analytics/src/monetization.rs:631-632` (load_monetization_payload)
`compute_ad_effects` läuft bei jedem Dashboard-Aufruf zweimal: einmal je Streamer
(LIMIT 200) und einmal netzwerkweit (LIMIT 5000) inklusive Viewer-Timelines aller
betroffenen Sessions, ohne Cache. Die netzwerkweite Berechnung skaliert mit wachsender
Partnerzahl schlecht und wird für jeden Seitenaufruf neu gemacht, obwohl die Empfehlungen
für alle gleich sind.
Muss gelten: Netzwerk-Empfehlungen periodisch vorberechnen oder cachen, nicht pro
Request. Kein neuer Poll- oder Nachtrag-Job, nur ein günstigerer Lesepfad.

## 5. NIT, C, `bot/dashboard_v2/src/pages/Monetization.tsx:146` (StatTile "Netto Viewer-Wirkung")
Die Headline-Kachel zeigt `net_effect.avg_net_drop_pct` auch bei `sample = 1`, ohne die
Stichprobengröße daneben, während die Moment-Aufschlüsselung erst ab 15 wertet. Ein
n=1-Wert wird so prominent als Zahl gezeigt. Empfehlung: Stichprobenzahl an der Kachel
anzeigen oder die Headline erst ab Mindestzahl mit Zahl belegen, sonst "Noch zu wenig
Daten".

## 6. NIT, C und D, neue Code-Kommentare entgegen Auftrag-Rahmen
`monetization.rs` Tests Z520 und Z542, `Monetization.tsx:157` (JSX-Kommentar
`{/* Netto-Wirkung */}`), Kommentarköpfe in `BACKFILL-C.sql` und
`20260918150000_werbemanager_chat_hinweis.sql`. Der Bedienhinweis in `BACKFILL-C.sql`
(wie ausführen) ist operativ vertretbar; die Test- und JSX-Kommentare entfernen.

## 7. MANGEL, C und D, `rust/crates/tb-db/tests/fresh_schema_snapshot.txt` (Zweit-Merge)
Beide Branches ändern den Snapshot in getrennten, alphabetisch nicht überlappenden
Regionen (C: `twitch_ad_break_events`; D: `twitch_ad_manager_settings` und
`twitch_ad_manager_state`), Git merged also sauber. Jeder Branch-Snapshot kennt aber nur
die eigenen neuen Spalten: Ds Snapshot fehlen Cs Telemetrie-Spalten und umgekehrt.
Nach dem zweiten Merge steht auf main ein Snapshot, der nur eine der beiden Migrationen
kennt, während eine frische DB beide anwendet, der tb-db-Snapshot-Test schlägt dann fehl.
Muss gelten: wer als Zweiter merged, erzeugt den Snapshot gegen eine frische DB mit beiden
Migrationen neu (Union beider Spaltensätze) und committet das im Merge.

---

## Fix-Runde 1 (2026-09-18)

1. BLOCKER D: behoben in 93e9450d. `last_hint`/`record_hint`-Fehler werden geloggt
   und geschluckt, nie per `?`. Lesefehler -> kein Hinweis in diesem Tick, der
   Entscheider laeuft weiter. Zwillingssuche: im Hinweis-Pfad gibt es kein weiteres `?`.
2. MANGEL D: behoben in 93e9450d. Vorgezogene Werbung bekommt jetzt einen Hinweis:
   im Vorzieh-Tick nur Hinweis senden und Merker schreiben (`pull:`-Key), die Werbung
   startet im naechsten Tick, wenn das Fenster offen bleibt; sonst faellt sie ohne
   Korrekturnachricht aus. Eigene Sofort-Texte ohne Sekundenversprechen. Test:
   `vorgezogene_werbung_bekommt_sofort_hinweis`, `sofort_hinweistext_nennt_kein_sekundenversprechen`.
3. MANGEL C: behoben in 12e6a275. "für" und "läuft" mit echten Umlauten. Zwillingssuche
   ueber neue nutzersichtbare Texte beider Pakete: keine weiteren ASCII-Ersatzumlaute.
4. MANGEL C: behoben in 12e6a275. Netzwerkweite Empfehlungen 15 Minuten im Prozess
   gecacht (OnceLock+Mutex, Muster wie `pause_loop`-Cache), ein Wert je Zeitfenster,
   neu gerechnet beim ersten Aufruf nach Ablauf. Kein Job, keine Tabelle, kein ENV.
5. NIT C: behoben in 12e6a275. Kopfkachel zeigt die Zahl erst ab 15 Werbungen, sonst
   "Noch zu wenig Daten" mit der bisherigen Anzahl (`<n>/15`).
6. NIT C und D: behoben in 12e6a275 (Test- und JSX-Kommentare) und 93e9450d
   (Kommentarkopf Migration 20260918150000). Bedienhinweis in BACKFILL-C.sql bleibt.
7. MANGEL C und D: behoben in dbbd6bea (D). C in D gemergt (Merge 002dc2ee, keine
   Konflikte), danach `fresh_schema_snapshot.txt` gegen eine frische DB mit beiden
   Migrationen (140000+150000) neu erzeugt. Enthaelt die Spalten beider Migrationen,
   `fresh_migrations_match_committed_schema_snapshot` gruen.
8. Clippy im eigenen Code beider Pakete sauber (C: komplexer Typ per Alias behoben,
   12e6a275). Fremde Warnungen (`partner_signup_tag_block.rs`, `title_ai.rs`,
   `uplink_config.rs`) bleiben liegen.

## Zusatzbefund beim Snapshot-Neubau (nicht in der Maengelliste)

9. Prod-Blocker in Cs Migration 20260918140000: `twitch_ad_break_events` ist eine
   komprimierte TimescaleDB-Hypertable. `ADD COLUMN ... CHECK/REFERENCES` schlaegt dort
   hart fehl ("cannot add column with constraints to a hypertable that has compression
   enabled"), die Hand-Migration und der fresh_migrations-Test waeren gescheitert.
   Behoben in c62e06f8 (C) und dbbd6bea (D): Spalten als reine Spalten, ohne Inline-CHECK
   und ohne FK, wie das Hausmuster 20260909220000. match_state/source werden im
   Schreibpfad ohnehin aus Rust-Enums gesetzt; die decision_id-FK (ON DELETE SET NULL)
   entfaellt, decision_id bleibt als reine BIGINT-Verknuepfung. Der Schema-Snapshot
   aendert sich dadurch nicht (CHECK/FK stehen nicht in der Spaltenliste).

---

## Runde 2

**Urteil: FREIGABE**

Gegenstand: Worktree C `feat/werbemanager-telemetrie` (`c62e06f8`) und Worktree D
`feat/werbemanager-chat-hinweis` (`dbbd6bea`, enthält C). Diff jeweils
`origin/main...HEAD`. Kein Code geändert.

WIRKUNGSPRUEFUNG[WP-1]: 0 Befunde | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 2/2 geprüft

Offene Punkte: keine.

### Befunde 1 bis 9

1. Behoben. In `ad_manager_wiring.rs` stehen `last_hint` (Z391) und `record_hint`
   (Z405) in `match Ok/Err`, nicht hinter `?`. Lesefehler: Warnung, bei `pull:`-Key
   wird die Werbung in diesem Tick unterdrückt, der Rest des Ticks läuft. Schreibfehler:
   Warnung, kein Chat-Send. `send_message` loggt `Sent` / anderer Outcome / `Err`.
   Zwilling: die einzigen Aufrufer von `last_hint`/`record_hint` sind dieser Block;
   das `?` in den Store-Methoden bleibt hinter `Result` und erreicht `process_channel`
   nicht. Andere `?` in `process_channel` (Z361 upsert_state, Z451/Z474 enqueue)
   liegen außerhalb des Hinweis-Pfads.

2. Behoben. `ad_hint` liefert bei `reason == "pulled_forward"` den Key `pull:{next_ad}`
   (`ad_manager.rs` Z856-860). Erster Tick: `immediate && !already_announced` setzt
   `suppress_pull_forward_commercial` (`ad_manager_wiring.rs` Z394-396), Commercial
   wird nicht enqueued (Z453-454). Nächster Tick (Worker-Intervall 25s, Z56): gleicher
   Key, `already_announced`, kein zweiter Hinweis, Commercial läuft. Endlosschleife:
   Send nur bei `!already_announced` und nach erfolgreichem `record_hint`. Doppelstart:
   erstes Tick enqueue't nicht; Folgeticks teilen `idempotency_key`
   `auto:{uid}:commercial:{next_ad}` mit `ON CONFLICT DO NOTHING`
   (`ad_manager.rs` Z1668). Merker: `last_hint_*` in `twitch_ad_manager_state`;
   `upsert_state` (Z1891) fasst diese Spalten nicht an, einziger Schreiber ist
   `record_hint` (Z1479). Sofort-Texte ohne "in 30 Sekunden" (Test
   `sofort_hinweistext_nennt_kein_sekundenversprechen`,
   `vorgezogene_werbung_bekommt_sofort_hinweis`). Schließt das Vorziehfenster vor
   dem zweiten Tick, fällt die Werbung ohne Korrekturnachricht aus: so dokumentiert
   in der Fix-Runde.

3. Behoben. "für" / "läuft" in `monetization.rs` Z596 und Z604. Zwilling über die
   neuen nutzersichtbaren Texte in C und D: keine ASCII-Ersatzumlaute.

4. Behoben. Netzwerk-Empfehlungen 15 Minuten in `OnceLock<Mutex<HashMap<i64, ...>>>`
   (`monetization.rs` Z25-50), Key nur `days`, Wert nur `Vec<String>` aus
   `build_network_recommendations`. Kein neuer Job.

5. Behoben. Kopfkachel "Netto Viewer-Wirkung" erst ab `sample >= 15`, sonst
   "Noch zu wenig Daten" plus `{n}/15` (`Monetization.tsx` Z379-425).

6. Behoben. Im Diff gegen `origin/main` keine neuen Test- oder JSX-Kommentare.
   Migrationsdatei 150000 ohne Kommentarkopf. Bedienhinweis in `BACKFILL-C.sql` bleibt.

7. Behoben in D. Snapshot enthält Telemetrie-Spalten (`twitch_ad_break_events.match_state`
   bis `decision_id`) und Hinweis-Spalten (`chat_notice_before_ad`, `last_hint_*`).
   C allein kennt die Hinweis-Spalten nicht; Merge-Gegenstand ist D (enthält C).

9. Behoben. Migration 140000 in C und D identisch: reine nullable Spalten, kein
   CHECK, kein FK.

### Schwerpunkt (1) Hinweis-`?`

Kein `?` mehr im Hinweis-Block `ad_manager_wiring.rs` Z388-437, der den Tick
abbrechen könnte. Geprüft.

### Schwerpunkt (2) vorgezogene Werbung

Hinweis im ersten Tick, Werbung im nächsten, kein Hinweis-Loop, kein doppelter
Start, Merker überlebt Neustart (Postgres, nicht RAM). Geprüft.

Unauffällig: ist `chat_api` `None` oder `chat_notice_before_ad` aus, bleibt
`suppress` false und die vorgezogene Werbung startet ohne Hinweis (kein Chat
bzw. Schalter aus). Send-Fehler nach erfolgreichem `record_hint` wiederholt den
Hinweis nicht, die Werbung startet im Folgetick trotzdem; gleicher Vertrag wie
bei Twitch- und Eigenblock-Hinweis, loggt `warn`.

### Schwerpunkt (3) Migration 140000 auf Prod

Prod/CI: TimescaleDB 2.17.2. `twitch_ad_break_events` ist Hypertable mit
Compression, Policy 7 Tage, `compress_segmentby=twitch_user_id`
(`20260630130000_reconcile_event_hypertables.sql` Z16-26).

`ADD COLUMN IF NOT EXISTS` von elf nullable Spalten ohne Default, CHECK und FK:
ab Timescale 2.1 katalogseitig, keine Dekompression, keine Zeilenumschreibung,
bestehende Zeilen lesen als NULL. Genau das Muster, an dem CHECK/FK zuvor
scheiterten. Kurzer ACCESS EXCLUSIVE nur für den Katalog, keine Datenänderung.

`CREATE INDEX IF NOT EXISTS twitch_ad_break_events_session_started (session_id, started_at)`
(Z14-15) stand schon in der Ursprungs-Migration, der Constraint-Fix hat ihn nicht
neu eingeführt. Der Index ist auf Prod neu (Bestand ist nur
`idx_twitch_ad_break_events_session` auf `session_id`). Timescale 2.17 baut
nicht-unique Btrees auf komprimierten Chunks ohne Heap-Dekompression. ShareLock
für die Dauer des Builds; die Tabelle ist pro Werbeblock, nicht in der Klasse
der Millionen-Zeilen-Hypertables. Keine Datenänderung. Kein offener Punkt.

### Schwerpunkt (4) Netzwerk-Cache, kein Fremdkanal-Leak

`network_recommendations` rechnet mit `compute_ad_effects(pool, "", cutoff, 5000)`
und legt nur die fertigen Empfehlungssätze ab. `AdEffects` enthält Mittelwerte
je Moment/Quelle, keine Login-, Kanal- oder Session-Identität.
`build_network_recommendations` formuliert nur Moment-Labels
("in der Queue", "mitten im Match", …) und Bot-gegen-Twitch-Plan. Cache-Key ist
`days`, nicht der aufrufende Streamer: alle sehen denselben Netztextsatz. Die
kanalbezogene Auswertung (`compute_ad_effects(pool, streamer, …, 200)` und
`worst_ads`) bleibt ungecacht und gefiltert über `LOWER(s.streamer_login) = $2`.
Kein Leak.

### Fixes ohne neue Fehler

Zwillingssuche unauffällig: kein zweites `?` im Hinweis-Pfad; `upsert_state` und
`record_match_transition` überschreiben `last_hint_*` nicht; Commercial-Enqueue
nur wenn nicht `(pulled_forward && suppress)`; Cache speichert keine Kanalzeilen.

Fremddienst-Pfade: (a) `ChatApi.send_message` im Hinweis-Block, Fehler und
Nicht-`Sent` geloggt, Tick läuft weiter. (b) Helix-Commercial über
`enqueue_automatic`/`claim_due`/`execute`, unverändert nach dem Hinweis-Block.
