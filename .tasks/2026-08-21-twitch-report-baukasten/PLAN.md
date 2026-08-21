status: erledigt
datum: 2026-08-21

# Plan

## Milestone 1: Vertrag im Report-Test festschreiben

- Änderung: Test erwartet den direkten Satz mit Originalzitat, Datum, UTC-Uhrzeit
  und VOD-Zeitfenster.
- Erwarteter Zwischenzustand: Test ist rot, weil der aktuelle Satz noch den
  vorsichtigen Fallbacktext verwendet.
- Validierung: `env PATH=/home/nathanael/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:/usr/local/bin:/usr/bin:/bin /home/nathanael/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo test --manifest-path /home/nathanael/Documents/Deadlock-Twitch-Bot/rust/Cargo.toml -p tb-stream-audit report::tests::dm_zeigt_originalwortlaut_zeitfenster_und_kopiergrund --lib -- --include-ignored`
- Stop-Regel: Bei unerwartetem Testfehler zuerst den Bestandstest korrigieren,
  nicht den Produktivcode anpassen.

Status: erledigt. Der Test war vor der Änderung rot und danach grün.

## Milestone 2: Deterministischen Baukasten verdrahten

- Änderung: DM-Grund aus Zitat und bestehenden Zeitfunktionen bauen; zweiten
  Meldegrund-LLM-Aufruf und nicht mehr benötigte Statusdaten entfernen.
- Erwarteter Zwischenzustand: Neue DM enthält keinen Modellhinweis und den
  vollständigen Copy-Paste-Satz.
- Validierung: `cargo check -p tb-stream-audit-bin`, gezielte Report-Tests und
  `cargo test -p tb-stream-audit-bin -- --include-ignored`.
- Stop-Regel: Keine weitere Änderung an Erkennung, TOS-Filter oder Sendepfad.

Status: erledigt. Der zweite LLM-Aufruf ist aus dem Auswertungspfad entfernt.

## Milestone 3: Gesamtprüfung

- Änderung: Dokumentation auf den deterministischen Baukasten aktualisieren.
- Validierung: Formatprüfung, Clippy, vollständige Tests des betroffenen
  Bausteins und read-only Diff-Review. Der Live-Dienst läuft noch mit dem
  bisherigen Release-Binärstand; wegen weiterer uncommittierter Änderungen im
  Checkout wird hier kein Release-Build gestartet.
- Stop-Regel: Bei rotem Test oder Review den Fehler beheben, bevor ein Dienst
  neu gestartet wird.

Status: erledigt. Die geänderten Rust-Dateien sind formatiert, Clippy und die
betroffenen Tests sind grün; der globale Formatter zeigt weiterhin die bekannte
Baseline außerhalb dieses Scopes.
