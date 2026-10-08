# Lokaler Merge-Gate

## Runde 1

Geprüfter Commit: 1dbcb4f3a16e48c569a24fa8c53983ae75ce3316
Basis: origin/main, 6937e4a61f43a9c08174fa95c96f49da149ca859
Aufruf: `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-moderation-disscord-erkennen --base origin/main --head HEAD`
Exit: 1
Reviewer: gpt-6.1-sol
Urteil: BLOCK

Originalbefund:

> Contact handles can disable the new detection.
> BLOCKING: rust/crates/tb-chat/src/scam_pitch.rs:514. stream_context searches the entire message, including the contact handle. For contact bait without a growth/design/crew match, a handle such as deadlock.player triggers the early return with score 0 and empty features. A contact name now counts as genuine conversation context and suppresses detection. Exclude the matched handle from context checks and add a replay covering this bypass.

Korrekturauftrag: frischer nativer Fixer in derselben Worker-Session. Kontaktlabel und Name vor Gesprächskontext-Prüfung ausnehmen, Scoring und echten Pipeline-Aufrufer mit unabhängigen Kontaktnamen nachweisen. Gate der Folgerunde mit gpt-6.1-sol.

## Runde 2

Korrektur: Die semantischen Prüfungen werten unveränderte Textabschnitte außerhalb sämtlicher erkannter Kontaktspannen aus. Kontaktlabels liefern weiterhin die vorhandenen Plattformsignale; Namen liefern weder Gesprächsbezug noch Lob, Unterstützung oder Pitch-Signale. Die erfragte Kontaktantwort wird ebenfalls außerhalb der Kontaktspannen geprüft, einschließlich des Grenzfalls `Disscord: wie gefragt`. Tatsächliche Wachstums-, Design- und Crew-Angebote außerhalb der Spannen bleiben wirksam.

Nachweisumfang: unabhängige Namen `deadlock.player`, `cool.gefollowt`, `affiliate.overlays` und `instagram.www.player`; mehrere Kontakte; Screenshot-Varianten im tatsächlichen Pipeline-Replay; harmlose erfragte Kontakte und Community-Einladungen; unveränderte Rollen-, Vertrauens-, Kontoalter-, Verlauf- und Einstellungsschutzprüfungen.

### Lokale Prüfungen

- Formatprüfung der beiden eigenen Rust-Dateien: Exit 0.
- Kontaktprüfung: `TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:33084/postgres TB_TEST_REQUIRE_DB=1 SQLX_OFFLINE=true /home/nathanael/.local/bin/cargo-slot test --manifest-path /home/nathanael/.worktrees/tb-moderation-disscord-erkennen/rust/Cargo.toml -p tb-chat --jobs 3 contact_ -- --nocapture --include-ignored`. Exit 0, 7 bestanden, 0 fehlgeschlagen, 0 ignoriert, 1064 ausgefiltert. Die tatsächliche Pipeline führte 18 Fälle und anschließend die Vertrauens- und Einstellungsausnahmen aus. Die Screenshot-Variante mit `deadlock.player` erzeugte die vorhandene öffentliche Warnung; harmlose erfragte Kontakte und Community-Einladungen erzeugten keine Aktion. Kein Replay löste Löschung, Timeout oder Ban aus.
- Ein vorausgehender Aufruf mit `--features testing` scheiterte vor dem Testlauf mit Exit 101, weil tb-chat diese Feature-Definition nicht besitzt. Der erfolgreiche Aufruf oben nutzt die tatsächlich vorhandenen Tests.
- Testprotokoll: `/tmp/claude-1000/-home-nathanael--worktrees-tb-moderation-disscord-erkennen/6447a5b0-e8d1-4d44-8a24-04633736070f/tasks/bc7eiyatp.output`.

Clippy und das Gate-Ergebnis werden nach Abschluss eingetragen. Noch keine Merge-Freigabe.
