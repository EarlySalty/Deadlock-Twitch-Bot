status: aktiv
datum: 2026-08-20

# Plan: Start-DM, Mitschnitt bis Sendungsende, Auswertung danach

## Entscheidung

Aufnahme laeuft parallel und pausiert nicht wegen Auswerte-Stau. Auswertung eines Laufs erst, wenn der Kanal offline ist. Start-DM und eine Ende-DM (nur ToS-Funde). Last-Gate fuer STT, Platten-Gate statt 6h-Sendungsdeckel.

## Milestones

1. Tests in `plan.rs` / `lib.rs` / `report.rs`: LaufSperre, Disk/Last, Start-/Ende-Text, ToS-Filter, Deckel = aufgenommene Zeit.
   Validierung: `cargo test -p tb-stream-audit --offline`
   Stop: rot.

2. Crate-Logik + `main.rs` verdrahten. Doku anpassen.
   Validierung: dieselben Tests plus `tb-stream-audit-bin`.
   Stop: rot oder Start-DM-Text ohne Umlaut/mit Gedankenstrich.

3. Review, Merge, Release-Build `-j 2`, Restart, Live-Beweis (Journal-Anker, Start-DM wenn Ricky noch live).
