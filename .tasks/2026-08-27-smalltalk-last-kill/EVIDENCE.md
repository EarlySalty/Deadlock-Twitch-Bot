# Evidence: Bestandsaufnahme Smalltalk-Loop + Last-Wächter

## Der Smalltalk-Test (erzeugt die Karte im Bild)
- Karte "Smalltalk-Testauswertung": `rust/bin/tb-bot/src/smalltalk_loop_wiring.rs:62`
- Copy-Labels inkl. Endgründe: `smalltalk_loop_wiring.rs:61-98`
  - vorhandene Endgrund-Labels: `end_session_timeout` (68), `end_stream_ended`
    (69), `end_process_start` (70), `end_process_shutdown` (71),
    `end_kill_switch` (72), `end_provider_error` (73). Es fehlt ein Label für
    Server-Überlast.
- Loop-Runtime + Start: `smalltalk_loop_wiring.rs:266-320` (`start`)
- Kill-Switch greift NUR beim Prozessstart: `start` liest `SmalltalkConfig::from_env`
  einmal (`smalltalk_loop_wiring.rs:272`, `300-303`). Zur Laufzeit gibt es keinen
  responsiven Stopp.
- Haupt-Loop: `spawn_loop` `smalltalk_loop_wiring.rs:339-356` → ruft
  `process_once` alle 5 s (`LOOP_INTERVAL`, Zeile 23).
- `process_once`: `smalltalk_loop_wiring.rs:438-447` → schließt inaktive Sitzung,
  startet sonst die nächste. HIER muss das Gate greifen (kein Start bei Überlast).
- Heavy-Last-Quelle: `spawn_transcript_capture` `smalltalk_loop_wiring.rs:372-436`
  nimmt Ton auf und transkribiert lokal (Whisper) alle 5 s, ganze Stunde, OHNE
  Last-Gate.

## Store: Endgründe und Sitzungsschluss
- `SESSION_DURATION = 60 min`: `rust/crates/tb-engagement/src/smalltalk_loop_store.rs:10`
- `close_ineligible_session` (Endgründe session_timeout/stream_ended):
  `smalltalk_loop_store.rs:408-438`
- `close_active_session(reason, now)` existiert bereits und schließt genau die
  aktive Sitzung: `smalltalk_loop_store.rs:440-449`  ← der Kill-Pfad.
- Provider-Fehler-Zählung: `smalltalk_loop_store.rs:586-604`
  (`record_provider_error`). Erklärt die 74 im Bild, gehört aber NICHT zu diesem
  Task.

## Vorhandener Last-Wächter (wiederverwenden, NICHT neu bauen)
- `Lastwaechter` (reine Zustandsmaschine, getestet): `rust/crates/tb-stream-audit/src/last.rs:54-169`
  - `beobachten(auslastung, jetzt_s) -> bool`: Fenster + Hysterese + Max-Halte-Deckel
  - `aus_umgebung()`: Grenzwerte aus `STREAM_AUDIT_LOAD_*` (Zeilen 19-43)
  - `zuruecksetzen()` (fail-open): Zeile 159
  - 8 Unit-Tests: `last.rs:213-309`
  - Modul ist rein: nur `std::env` + `tracing`, kein sqlx/tokio/reqwest (geprüft).
- Messung liegt derzeit im BIN, nicht im Crate:
  - `cpu_stand`/`CpuStand`: `rust/bin/tb-stream-audit/src/main.rs:3369-3398`
  - `cpu_prozent`: `main.rs:3402-3410`
  - `ram_prozent`: `main.rs:3415-3433`
  - `last_ueberwachen` (Mess-Loop, Takt 20 s, max(CPU,RAM)): `main.rs:3438-3489`
  - Diese Messung muss geteilt werden (Crate/Leaf-Crate), damit der Smalltalk-Loop
    sie ohne Kopie nutzt.

## Dependency-Lage
- `tb-bot` hängt an `tb-engagement`, NICHT an `tb-stream-audit`:
  `rust/bin/tb-bot/Cargo.toml:40`
- `tb-stream-audit`-Crate hängt weder an `tb-engagement` noch `tb-bot` → kein
  Zirkel, wenn `tb-bot` bzw. eine neue Leaf-Crate ins Spiel kommt (geprüft per
  grep über alle Cargo.toml).

## Fazit
Kill-Pfad (`close_active_session`) und Last-Wächter (`Lastwaechter` + Messung)
existieren beide schon. Der Task verdrahtet sie: Last-Monitor-Task im
Smalltalk-Loop → Gate → aktive Sitzung mit `server_overloaded` schließen + kein
Neustart bei Überlast + Karten-Label. Kein Neubau von Last-Logik.
