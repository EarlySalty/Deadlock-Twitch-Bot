# Contract: Session-Titel an der Ursache setzen statt pro Poll nachtragen

## Ziel
Eine Twitch-Session bekommt Titel und Spiel beim Anlegen (EventSub-Pfad holt die Kanal-Infos vorher) oder einmalig beim ersten Poll (`adopt_incomplete`), und das funktioniert auch, wenn der Titel als leerer String ankam. Das Pro-Poll-Nachtragen `backfill_missing_meta` aus Commit 2bd53d33 wird wieder entfernt.

## Umfang (erlaubter Bereich)
- `rust/crates/tb-monitoring/src/handlers.rs`
- `rust/crates/tb-monitoring/src/sessions/store.rs`
- `rust/crates/tb-monitoring/src/sessions/tracker.rs`
- `rust/crates/tb-monitoring/tests/`
- Lesende Stellen von `stream_title` und `game_name`, nur falls sie `NULL` nicht vertragen (Nachweis in EVIDENCE, jede Stelle einzeln)

## REQ
- REQ1: `start_session` speichert leeren Titel und leeres Spiel als `NULL`, nie als `''`. `adopt_incomplete` füllt Titel und Spiel per `COALESCE(NULLIF(stream_title, ''), $x)` bzw. `COALESCE(NULLIF(game_name, ''), $y)`, damit auch Altbestand mit `''` gefüllt wird. Die Fenster-Bedingung `samples = 0 AND start_viewers = 0` bleibt für das Setzen von `start_viewers`/`peak_viewers`; Titel und Spiel dürfen zusätzlich gefüllt werden, solange sie leer sind und die Session noch offen ist (ein UPDATE, kein zweites).
- REQ2: Im EventSub-Handler `handle_stream_online` läuft der Kanal-Lookup (`stream_online_channel_info`) VOR `stream_online_session`. Sein Ergebnis (Titel, Spiel) geht in den `StreamSnapshot`, mit dem `ensure_session` aufgerufen wird, und wie bisher in `apply_channel_info` für den Live-State. Schlägt der Lookup fehl oder liefert er nichts, wird die Session wie heute ohne Titel eröffnet (Best-Effort bleibt). Die Bedingung `announcement_action != Reconcile` für den Live-State-Schreibvorgang bleibt; der Lookup selbst darf für den Snapshot immer laufen. `had_deadlock` des Snapshots ergibt sich aus dem gefundenen Spiel wie im Poll-Pfad (`is_in_target_category`).
- REQ3: `backfill_missing_meta` (store.rs) und sein Aufruf in `ensure_session` (tracker.rs) werden entfernt. Der Test `offene_session_bekommt_titel_nach_verpasstem_adopt_fenster` wird durch die Tests unten ersetzt.
- REQ4: Alle lesenden Stellen von `stream_title`/`game_name` im Rust-Code werden auf `NULL`-Verträglichkeit geprüft (Option-Typ oder `COALESCE`); Stellen, die `''` voraussetzen, werden angepasst. Liste in EVIDENCE.md.

## INV
- INV1: Pro Poll-Tick höchstens das eine bereits vorhandene `adopt_incomplete`-UPDATE je offener Session, kein zusätzliches UPDATE.
- INV2: Ein vorhandener nicht-leerer Titel wird nie überschrieben.
- INV3: Idempotenz des EventSub-Handlers bleibt (beide `run_business_effect_once`-Schlüssel bestehen weiter, nur die Reihenfolge ändert sich).
- INV4: Keine Migration, keine neue Spalte, keine Änderung an Retention/Bindung/Geisterfilter.
- INV5: Keine Code-Kommentare, echte Umlaute in nutzersichtbaren Texten, keine Em-Dashes.

## Nicht-Ziele
- Kein weiterer Backfill historischer Daten (ist erledigt, siehe `.tasks/2026-09-04-streams-tab-bindung/`).
- Keine Änderung am Poller-Snapshot oder an der Helix-Anbindung.
- Kein Umbau des Live-State.

## Regressionstests (Pflicht, vor dem Fix rot)
- T1 (`tests/write_core.rs`): `start_session` mit leerem Titel, danach `adopt_incomplete` mit Titel "Ranked Grind Titel": Titel steht in der DB. Rot vor REQ1, weil `''` gespeichert wird und `COALESCE` es als gesetzt sieht.
- T2 (`tests/eventsub_dispatch.rs` oder passende Support-Infrastruktur): `stream.online` mit einer `ChannelInfoSource`, die Titel "Go-Live Titel" und Spiel "Deadlock" liefert; die eröffnete Session hat sofort Titel, Spiel und `had_deadlock_in_session = true`, ohne dass ein Poll lief. Rot vor REQ2.
- T3: Nach Entfernen von `backfill_missing_meta` darf kein Test mehr darauf verweisen (`cargo test -p tb-monitoring` grün).
Roten Lauf mit Testname und Fehlermeldung in `EVIDENCE.md` unter `## Roter Lauf` festhalten.
