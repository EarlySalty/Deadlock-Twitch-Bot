# Selbstpruefung (adversarial gegen den Contract)

`gate_hook.py --review` liegt in claude-config, nicht in diesem Repo. Deshalb
Diff-Selbstpruefung gegen REQ/INV.

## REQ
- REQ1 erfuellt: `spawn_last_monitor` misst CPU (`cpu_stand`/`cpu_prozent`) und
  RAM (`ram_prozent`) im 20-s-Takt, `max(CPU,RAM)`, fuettert `Lastwaechter`.
- REQ2 erfuellt: `kill_grund_bei_uebergang(vorher, aktiv)` liefert nur auf der
  steigenden Flanke `server_overloaded`; dann `close_active_session`.
- REQ3 erfuellt: `process_once` startet ueber `soll_neue_sitzung_starten` keine
  Sitzung, solange das Gate aktiv ist.
- REQ4 erfuellt: `end_server_overloaded` = "Server überlastet" im Copy-JSON,
  Struct, `configured()`-Vollstaendigkeitsliste (fail-closed via
  `deny_unknown_fields` + Nicht-Leer-Pruefung) und `end_reason_label`.
- REQ5 erfuellt: `(None, None)` -> `zuruecksetzen()` + Gate false (fail-open),
  kein Kill. Grenzwerte aus `STREAM_AUDIT_LOAD_*` (Standard 90/80).

## INV
- INV1 gehalten: genau ein `Lastwaechter` (in `tb-load`), `tb-stream-audit`
  re-exportiert ihn; Messung liegt an genau einer Stelle (`tb-load::messung`),
  von Audit und Bot importiert, keine Kopie.
- INV2 gehalten: bestehende Endgruende unveraendert; `tb-stream-audit` verhaelt
  sich identisch (nur Messung verschoben, `last_ueberwachen` unveraendert). 116
  Bestandstests bleiben gruen (109 Audit + 7 tb-load = 116).
- INV3 gehalten: keine ENV-Datei, keine Secrets; Reuse der bestehenden
  `STREAM_AUDIT_LOAD_*`-Grenzwerte, kein neuer Config-Weg.

## Scope
Nur erlaubte Dateien beruehrt: `smalltalk_loop_wiring.rs`, neue Leaf-Crate
`tb-load`, `tb-stream-audit` (last.rs Re-Export + main.rs Import), betroffene
`Cargo.toml`. `close_active_session` existierte bereits, unveraendert genutzt.
Provider-Fehler-Thema nicht angefasst (Nicht-Ziel).

## Risiken
- Race Monitor/Loop entschaerft: Loop prueft Gate vor jedem Start, kein Neustart
  waehrend Ueberlast. Waehrend des Katchup-Fensters (Gate bewusst offen) kann
  eine Sitzung starten und bei fortdauernder Last erneut gekillt werden - das
  ist das gewuenschte Verhalten bei anhaltender Ueberlast.
