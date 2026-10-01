# Review Runde 1: Pakete A und B

## Urteil

**FIX NÖTIG.** Abnahme fertig: **N** (funktioniert im Kern, weicht aber bei einer verbindlichen Nutzer-Entscheidung ab).

Der Schreib- und Lesepfad passen zum API-Vertrag: Feldnamen (`plan.*`, `currentReason`, `history`), Grundcodes und `decision`-Werte sind auf beiden Seiten deckungsgleich. `plan.suggestion` ist auf beiden Seiten korrekt weggelassen (Nachtrag 3). Idempotenz (globaler Unique-Key `auto:{uid}:commercial:{blockzeit}`), Fail-closed über `?`, Budgetgrenze (`next_block_at = None`), kein Selbststart im Match und die Sicherheitspunkte (leere User-ID abgewiesen, `monitor` mit 400, History nur eigener Kanal) sind erfüllt. Kein LLM im Pfad.

Kein harter BLOCKER gefunden. Vor Freigabe die MÄNGEL abarbeiten.

Format je Mangel: Nummer, Schwere, Paket, Pfad:Zeile.

---

## Mängel

### 1 · MANGEL · Paket A · `rust/crates/tb-analytics/src/ad_manager.rs:786`
Die Post-Matchende-Wartezeit wird im Normalfall nie erreicht. In `decide()` steht das Queue/Menü-Fenster (`state.in_deadlock` liefert `commercial("in_queue")`, Zeile 786-787) vor der Post-Match-Logik (Zeile 793-802). Direkt nach einem Match ist der Spieler typischerweise wieder in Queue/Menü (`in_match=false`, `in_deadlock=true`), also feuert sofort `in_queue` und die verbindliche Regel "Nach Matchende nicht sofort: 1 Minute warten, bei Chat verschieben" greift nicht.
Muss: Wenn `match_ended_at` im Wartefenster liegt, muss `post_match_wait` / `post_match_chat_active` / `post_match_quiet` die `in_deadlock`-Werbung vorrangig blocken. Post-Match-Prüfung vor den `in_deadlock`-Zweig ziehen, oder `in_queue` in der ersten Minute nach Matchende sperren.

**behoben in 4e1ce51** · Post-Match-Block steht jetzt vor dem `in_deadlock`-Zweig; `in_match` bleibt davor. Neuer Test `matchende_schlaegt_erneute_queue` deckt die erneute Queue in den beiden Fenstern und den Rückfall auf `in_queue` nach 2 Minuten ab.

### 2 · MANGEL · Paket A · `rust/migrations/20260918120000_werbemanager_budget_smart.sql:41`
Neue Tabelle `twitch_ad_manager_decisions` und ihre Sequence werden ohne GRANT und ohne Rechte-Hinweis für `twitchbot`/`twitchdash` angelegt (Prüfpunkt 4). Fehlen die Rechte in Prod, scheitert `record_decision_if_changed` beim INSERT; der Fehler propagiert per `?` und bricht `process_channel` ab, bevor `claim_due` die Werbung ausführt. Ergebnis: der Manager tut in Prod wieder nichts, also genau der Ausgangsbug. Die Original-Migration `20260901100000` setzt zwar auch keine GRANTs (Default-Privileges greifen dort), der Auftrag verlangt hier aber den ausdrücklichen Nachweis.
Muss: Entweder explizite `GRANT SELECT` (twitchdash) und `GRANT SELECT,INSERT,DELETE` plus `GRANT USAGE` auf die Sequence (twitchbot) in die Migration, oder verifizierter Hinweis im Deploy-Weg, dass Default-Privileges die neue Tabelle und Sequence abdecken.

**behoben in 4e1ce51** · DO-Block mit `pg_roles`-Guard: twitchbot bekommt `SELECT,INSERT,DELETE` auf die Tabelle plus `USAGE,SELECT` auf `twitch_ad_manager_decisions_id_seq`, twitchdash `SELECT`. Guard schützt frische Test-DBs ohne diese Rollen. Schema-Snapshot bleibt unverändert (GRANTs stehen nicht im Snapshot, Tabelle mit 8 Spalten schon enthalten).

### 3 · MANGEL · Paket A · `rust/crates/tb-analytics/src/ad_manager.rs:14`, `rust/bin/tb-bot/src/ad_manager_wiring.rs:388`
Zahlreiche neue Code-Kommentare, verboten laut Auftrag ("Keine Code-Kommentare schreiben") und Prüfpunkt 6. Beispiele in `ad_manager.rs`: 14-16, 20-23, 159-163, 337-338, 478-479, 489-491, 707-709, 794-795, 832, 845-846 sowie die `///`-Doku an fast jeder neuen `pub fn`; in `ad_manager_wiring.rs`: 388-389 und 645-646 (`nearest_ad_length`). Vorbestehende Kommentare bleiben unberührt.
Muss: Neue `//`- und `///`-Kommentare entfernen, Namen sprechen lassen.

**behoben in 4e1ce51** · Alle im Diff neu hinzugekommenen `//`- und `///`-Zeilen entfernt (50 in `ad_manager.rs`, 4 in `ad_manager_wiring.rs`), per Diff-Zeilennummer, damit vorbestehende Kommentare wie `// Aktueller Helix-Stand ist Pflicht` unberührt bleiben. Build grün.

### 4 · NIT · Paket A · `rust/crates/tb-dashboard-api/src/handlers/ad_manager.rs:250`
Der Status berechnet `fit` per `assess_plan(...)` neu, statt die vom Worker gespeicherte Spalte `plan_fit` zu lesen. Zwei Wahrheitsquellen: driftet der Zustand zwischen Worker-Tick und HTTP-Abruf, zeigt der Status-Satz ein anderes `fit` als der Verlauf protokolliert hat. Die Spalte `plan_fit` wird im Status gar nicht gelesen.
Besser: `plan_fit` aus dem State lesen und anzeigen; `assess_plan` nur im Worker halten.

**behoben in 4e1ce51** · Der Status liest jetzt die Spalte `plan_fit` (NULL/unbekannt wird zu `good`); `assess_plan`-Aufruf und der `assess_plan`-Import im Handler sind raus. `assess_plan` bleibt nur im Worker.

### 5 · NIT · Paket A · `rust/crates/tb-analytics/src/ad_manager.rs:1434`
`blocks_in_window` zählt jedes Commercial mit `reason<>'fallback_least_bad'`, also auch `quiet_chat`-Blöcke, die außerhalb von Queue/Menü/Match-Minute laufen. Die Bilanz "davon im Fenster" wird dadurch zu hoch.
Besser: Nur echte Fenster-Gründe zählen (`in_queue`, `match_start_window`, `post_match_quiet`, `pulled_forward`).

**behoben in 4e1ce51** · `blocks_in_window` zählt nur noch `reason IN ('in_queue','match_start_window','post_match_quiet')` (Festlegung 5). `quiet_chat`, `fallback_least_bad` und `pulled_forward` fallen laut Festlegung bewusst raus.

### 6 · NIT · Paket A und B · `rust/crates/tb-analytics/src/ad_manager.rs:521` (Eigenblock-Pfad in `decide`)
Bei Smart-Eigenblöcken wird `min_interval_minutes` nicht mehr geprüft; die Blockperiode ergibt sich nur aus Budget und Helix-Sperrzeit. Die Feineinstellung "Mindestabstand" bleibt im Dashboard (`AdManagerSection.tsx`) sichtbar und wirkt für Eigenblöcke nicht, was irreführend ist.
Besser: Entweder die Periode zusätzlich an `min_interval_minutes` koppeln, oder im UI klarstellen, dass der Mindestabstand nur Twitch-Pausen und die manuelle Werbung betrifft.

**behoben in 4e1ce51** · `plan_next_block` nimmt jetzt `min_interval_minutes` und bildet die Blockperiode als `max(gleichverteilt, Helix-Sperrzeit, Mindestabstand)`; der Wiring-Aufrufer reicht `settings.min_interval_minutes` durch. Kein UI-Text nötig (Festlegung 6), das sichtbare Feld wirkt jetzt auf Eigenblöcke. Paket B braucht dafür keine Änderung.

---

## Geprüft und in Ordnung

- Doppelauslösung: zwei Ticks (stabiler Key), Neustart (ON CONFLICT DO NOTHING), Twitch-Werbung und Eigenblock schließen sich per `twitch_is_budget_source()` gegenseitig aus; stale Commercials werden über `claim_due` plus `automatic_matches` gecancelt.
- Budget: `next_block_at = None` bei Erschöpfung, Grund `budget_reached`; Twitch-eigene Werbung wird über `last_ad_at` aufs Budget angerechnet.
- Sperrzeit: aus der Helix-Antwort (`last_commercial_retry_after`), Konstante nur als Fallback.
- Verlauf: `record_decision_if_changed` schreibt nur beim Wechsel; `plan_fit_changed` einmal je Zustandswechsel; Retention über `cleanup_old_decisions` (30 Tage).
- Sicherheit: leere User-ID in `save_handler`/`action_handler`/`history_handler` abgewiesen, CHECK-Constraint plus DELETE der Geisterzeile, `monitor` mit 400, History strikt `WHERE twitch_user_id=$1`.
- Migration: idempotent, ändert keine angewandte Migration, `monitor`-Zeilen werden zu `enabled=false, strategy='smart'`, Snapshot deckt alle neuen Spalten und die Tabelle ab (Test vergleicht als Menge, Reihenfolge egal).
- Paket B: zwei Strategie-Karten (Optik unverändert), großer Schalter mit Goldkante, manuelle Knöpfe in der Status-Karte, Budget-Eingabe mit Vorschau bzw. Anzeige bei `source='twitch'`, Verlauf mit Bilanz, Feineinstellungen eingeklappt, ehrlicher Leerzustand ohne `history`/`plan`, unbekannte Grundcodes neutral, echte Umlaute, keine Em-Dashes, kein internes Vokabular. Doku aktuell und sauber.
