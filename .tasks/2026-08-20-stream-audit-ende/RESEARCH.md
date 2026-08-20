status: aktiv
datum: 2026-08-20

# Research: Coaching-Audit startet still und wertet live aus

## Beobachtung

- Dienst `deadlock-twitch-stream-coaching-watch` aktiv, PID 3921822, Binary 04:56.
- Ricky (`helmbombenricky`) wird seit 23:19 aufgenommen (Journal: `Aufnahme gestartet`). Bloecke 1..n, funde=0.
- Keine Start-DM. `dm_senden` schickt nur bei Funden (`main.rs:2558`).
- Parallelaufnahme je Kanal existiert. Auswertung ist seriell und laeuft waehrend der Sendung.
- Rueckstand `MAX_WARTESCHLANGE=180` pausiert die Aufnahme (`main.rs:1130`, `1299`).
- Deckel 6h zaehlt Sendungszeit ab Helix `started_at` (`plan.rs:288`). skifahrertv heute 03:29: `Aufnahmedeckel` nach 11354s Aufnahme, weil die Sendung schon lief.
- STT teilt sich mit Reaktionen (RTF ~0.25). Last 7.6 bei 16 Kernen, Platte 829G frei.

## Hypothese

User sieht "laeuft nicht", weil keine Start-DM kommt. Gewollt: mitschneiden solange gesendet wird (mehrere parallel), auswerten erst am Ende, Server nicht unter Last/STT zusammenbrechen.

## Bestand

BESTAND[BS-1]: ja | Fundort: rust/bin/tb-stream-audit/src/main.rs:1263, rust/crates/tb-stream-audit/src/plan.rs:25 | Anknuepfung: bestehende Parallelaufnahme, serielle Queue, DM-Pfad, Disk-Ablage
