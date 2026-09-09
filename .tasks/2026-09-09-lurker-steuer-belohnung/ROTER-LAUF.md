# Roter Lauf: Regressionstests Lurker-Steuer-Belohnung

Toolchain rustc/cargo 1.97.1 (`/home/nathanael/.cargo/bin/cargo`).
Befehle je Crate:

- tb-chat: `SQLX_OFFLINE=1 TB_TEST_DATABASE_URL="postgres:///tb_bb_test?host=/var/run/postgresql" cargo test -p tb-chat --lib <filter>`
- tb-dashboard-api: `SQLX_OFFLINE=1 TB_TEST_DATABASE_URL="postgres:///tb_bb_test?host=/var/run/postgresql" cargo test -p tb-dashboard-api --lib <filter>`

Die Tests der Crate tb-chat wurden gestaffelt belegt: zuerst die kompilierenden Assertion-Tests (REQ-2, REQ-4) allein gefahren, danach die Symbol-Tests (REQ-1, REQ-3, REQ-5) ergaenzt. Der finale committete Stand von tb-chat kompiliert bewusst nicht (fehlende Produktivsymbole = Rot). Die REQ-3- und `set_lurker_reward_checker`-Methodenfehler wurden mit einem temporaeren Produktiv-Stub sichtbar gemacht (Stub danach entfernt), weil die Namensaufloesungsfehler von REQ-1/REQ-5 sie sonst maskieren.

## REQ-2 Erinnerungstext (Assertion rot, kompiliert)

- `promos::tests::req2_lurker_tax_text_ein_name_nennt_belohnung`
  - rust/crates/tb-chat/src/promos.rs:3235 (fn)
  - FAILED: `assertion left == right failed` | left: `Lurker Steuer: @xy falls ihr gerade entspannt mitlest, denkt gern an eure Channel-Points.` | right: `Hey @xy, schön dass du da bist! Vergiss nicht, deine Lurker Steuer zu zahlen: Belohnung 'Lurker Steuer' einlösen.`
  - Ist heute: alter Reminder-Wortlaut. Soll nach Umsetzung: REQ-2-Wortlaut mit Belohnungsname.
- `promos::tests::req2_lurker_tax_text_zwei_namen_ein_satz`
  - rust/crates/tb-chat/src/promos.rs:3245 (fn)
  - FAILED: `Belohnungshinweis fehlt: Lurker Steuer: @alice @bob falls ihr gerade entspannt mitlest, denkt gern an eure Channel-Points.`
  - Ist heute: kein `Belohnung 'Lurker Steuer' einlösen.`, alter Text. Soll: beide Erwaehnungen plus Belohnungshinweis in einem Satz.

## REQ-1 Titel-Erkennung (Compile-Fehler rot)

- `promos::tests::req1_lurker_tax_title_matches_erkennt_varianten`
  - rust/crates/tb-chat/src/promos.rs:3270 (fn), E0425 3271-3278
  - E0425: `cannot find function lurker_tax_title_matches in this scope` (8 Fundstellen 3271-3278).
  - Ist heute: Funktion existiert nicht. Soll: `lurker_tax_title_matches` erkennt `Lurker Steuer`, `lurker steuern`, `LURKER STEUER `, `LURKER STEUER 10`, `Lurker  Steuer`; weist `Steuer`, `Lurker`, `` ab.

## REQ-3 Dank je Zuschauer und Session (Compile-Fehler rot)

- `promos::tests::req3_dank_hoechstens_einmal_je_zuschauer_und_session`
  - rust/crates/tb-chat/src/promos.rs:3282 (fn), Aufrufe 3286/3289
  - E0599: `no method named thank_lurker_tax_redeemer found for struct promos::PromoEngine` (Fundstellen 3286, 3289; unter temporaerem Stub sichtbar gemacht, im finalen Stand von den REQ-1/REQ-5-Namensfehlern maskiert).
  - Ist heute: Methode fehlt. Soll: zweimaliger Dank fuer denselben Login/dieselbe Session sendet genau eine Nachricht.

## REQ-4 Keine Erinnerung nach Einloesung (Assertion rot gegen DB)

- `promos::db_tests::req4_eingeloester_zuschauer_nicht_mehr_erinnert`
  - rust/crates/tb-chat/src/promos.rs:6014 (fn)
  - FAILED (0.14s, echter DB-Lauf): `Wer die Lurker Steuer dieser Session eingelöst hat, darf nicht mehr erinnert werden: ["lurkerin", "steuerzahler"]`
  - Ist heute: Einloeser `steuerzahler` bleibt Kandidat (keine Ausgrenzung); Kontrolle `lurkerin` (andere Belohnung) bleibt korrekt drin. Soll: `steuerzahler` faellt raus, `lurkerin` bleibt.

## REQ-5 Reward-Gate (Compile-Fehler rot)

- `promos::db_tests::req5_reminder_blockt_wenn_belohnung_fehlt` / `promos::db_tests::req5_reminder_sendet_wenn_belohnung_aktiv`
  - rust/crates/tb-chat/src/promos.rs:6055 (Trait), Setter 6083 und 6105 (`set_lurker_reward_checker`)
  - E0405: `cannot find trait LurkerRewardChecker in this scope`; E0599: `no method named set_lurker_reward_checker found for struct promos::PromoEngine` (Methodenfehler unter temporaerem Stub sichtbar gemacht).
  - Ist heute: Trait und Setter fehlen. Soll: Checker `false` blockt die Erinnerung (0 Announcements), Checker `true` laesst sie durch (1 Announcement).

## REQ-7 Dashboard-Readiness (Assertion rot gegen DB)

- `handlers::lurker_tax_settings::tests::req7_readiness_via_bot_kapabilitaet_ohne_streamer_scope`
  - rust/crates/tb-dashboard-api/src/handlers/lurker_tax_settings.rs:399 (fn), Panic 445
  - FAILED (0.04s, echter DB-Lauf): `assertion left == right failed: gesetzte Bot-Kapabilität genügt auch ohne Streamer-Scope` | left: `Bool(false)` | right: `true`
  - Ist heute: `has_moderator_read_chatters` prueft nur `twitch_raid_auth`, ignoriert `twitch_bot_capabilities`. Soll: gesetzte Bot-Kapabilitaet genuegt auch ohne Streamer-Scope.

## Gesamtstand

- tb-chat `--lib`: kompiliert nicht (9 Fehler: 1× E0405, 8× E0425); zusaetzlich maskiert E0599 fuer `thank_lurker_tax_redeemer` und `set_lurker_reward_checker`. Alle sechs REQ-Tests rot.
- tb-dashboard-api `--lib`: kompiliert, REQ-7-Test rot per Assertion.
