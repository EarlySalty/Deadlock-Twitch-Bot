# Roter Lauf (Tests vor der Implementierung)

Toolchain: stable (rustc/cargo 1.97.1), PATH auf
`~/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin`.

## Baseline (vor dem Diff, gruen)
- `cargo test -p tb-stream-audit --lib`: 116 passed, 0 failed, 0 ignored.
- `cargo test -p tb-bot --bin tb-bot smalltalk_loop_wiring`: 8 passed, 0 failed,
  265 filtered out.

## Neue Tests (rot)
Befehl:
`cargo test --manifest-path rust/Cargo.toml -p tb-bot --bin tb-bot smalltalk_loop_wiring`
Exit 101, Compile-Fehler (Symbole existieren noch nicht):

- T1 `discord_karte_zeigt_server_ueberlast_als_endgrund`
- T2 `gate_haelt_den_loop_an_und_gibt_ihn_wieder_frei`
- T3 `nur_der_uebergang_zu_ueberlast_killt_mit_server_overloaded`

Fehlermeldungen:
- `error[E0425]: cannot find value \`SERVER_OVERLOADED_REASON\` in this scope`
- `error[E0425]: cannot find function \`soll_neue_sitzung_starten\` in this scope`
- `error[E0425]: cannot find function \`kill_grund_bei_uebergang\` in this scope`
- (T1 zusaetzlich: `end_reason_label("server_overloaded")` liefert noch nicht
  "Server überlastet", da Copy-Feld `end_server_overloaded` fehlt.)

Die Tests treffen den Vertrag: Label-Abbildung (REQ4), Start-Sperre bei aktivem
Gate (REQ3) und Kill nur beim Uebergang inaktiv->aktiv (REQ2).
