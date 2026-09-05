# Contract: Partner-Pitch an Zuschauer, die selbst Deadlock streamen

status: aktiv
datum: 2026-09-05
klasse: hoch
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Der mit den Presets weggefallene Partner-Pitch ("Du streamst? Partner werden") kommt zurück, aber gezielt: nur an Zuschauer, von denen der Bot aus seinen eigenen Daten weiß, dass sie selbst Deadlock streamen und noch nicht Partner sind. Der Pitch antwortet auf das, was die Person gerade geschrieben hat, und bietet die Partnerschaft in dritter Person an, im Stil des Betreibers (Stilvertrag `outreach_shadow.rs`).

## Anforderungen (user-sichtbares Verhalten)

- REQ-01 Erkennung: Ein Chatter in einem Partnerkanal gilt als streamender Kandidat, wenn seine Twitch-User-ID in den Bot-Daten als Deadlock-Streamer geführt wird (Quellen laut Research, z. B. `twitch_scout_candidates`, `twitch_streamers`), er nicht Partner ist (`twitch_partners`), nicht auf der Scout-Blacklist steht, keinen Eintrag in `twitch_scout_pitch_ledger` oder `twitch_partner_outreach` mit Kontakt hat und nicht der Broadcaster des Kanals ist. Ein Login-Vergleich ist keine Erkennung; nur die Twitch-User-ID zählt.
- REQ-02 Auslöser: Der Partner-Pitch geht nur als Antwort auf eine Nachricht des Kandidaten (mindestens 25 Zeichen, kein Befehl), nie unaufgefordert und nie per Timer.
- REQ-03 Text: Antwort aus demselben LLM-Pfad wie der Anlass-Pitch (`tb_llm::complete`, Use-Case `promo_pitch`, Deepseek V4 Flash) mit einem eigenen Systemprompt für den Partner-Anlass: erst auf das Gesagte eingehen, dann in dritter Person, konditional ("wenn du öfter Deadlock streamst") auf das Partner-Netzwerk der Deutschen Deadlock Community hinweisen und die Mechanik ehrlich nennen (gegenseitige Raids, Chat-Schutz). Kein Link, keine Superlative, keine Mitgliederzahlen, kein Gedankenstrich, kein "komm auf Discord", keine Emojis außer :). Der Weg zur Anmeldung wird nur genannt, wenn die Person nachfragt oder `!invite` tippt. Alle harten Filter des Anlass-Pitch gelten.
- REQ-04 Limits: je Kandidat (Twitch-User-ID) genau ein Partner-Pitch, für immer, über alle Kanäle; je Kanal höchstens ein Partner-Pitch pro Stream; höchstens fünf Partner-Pitches pro Tag insgesamt. Ein gesendeter Partner-Pitch belegt den Promo-Cooldown des Kanals wie der Anlass-Pitch.
- REQ-05 Gates: Partnerkanal, Werbefrei-Plan (`promo_disabled`), Kanal-Allowlist, Outbound-Suppression, Startverzögerung 10 Minuten, Doppelsend-Lock, Judge-Drossel und Semaphore des Anlass-Pitch gelten unverändert.
- REQ-06 Sichtbarkeit: jeder Partner-Pitch (gesendet oder verworfen) steht in `twitch_promo_pitch_log` mit Pfad `partner`, und gesendete Partner-Pitches gehen als Karte in den Discord-Review-Kanal wie Anlass-Pitches, mit Hinweis auf den Kandidatenstatus. Der gesendete Pitch wird zusätzlich in `twitch_scout_pitch_ledger` (oder der von der Research benannten Kontakt-Tabelle) vermerkt, damit die Outreach-Kette dieselbe Person nicht ein zweites Mal anspricht.
- REQ-07 Der Anlass-Pitch und der Partner-Pitch schließen sich je Nachricht aus: trifft beides, geht nur der Partner-Pitch raus.
- REQ-08 Streamer-FAQ `rust/knowledge/bot/faq-werbung.md` nennt den Partner-Hinweis an streamende Zuschauer wahrheitsgemäß.

## Invarianten (darf sich nicht ändern)

- INV-01: Werbefrei-Plan schaltet auch den Partner-Pitch ab.
- INV-02: Sprachmodell ausschließlich Deepseek V4 Flash über `tb_llm::complete`, Use-Case `promo_pitch`.
- INV-03: Anlass-Pitch, periodische Promo und Targeted-Pitch verhalten sich wie seit Release 2c0a8bbf; ihre Tests bleiben unverändert grün.
- INV-04: Outreach-Kette (`tb-scout`, `twitch_partner_outreach`, `outreach_shadow.rs`) und ihre Dispatch-Logik werden nicht verändert; nur lesend genutzt plus ein Ledger-Eintrag.
- INV-05: Keine neue Tabelle, wenn `twitch_promo_pitch_log` und die bestehende Ledger-Tabelle reichen; Migrationen nur additiv.
- INV-06: Bestehende Tests nicht löschen oder abschwächen.
- INV-07: Keine ENV-Config, kein neues Secret, keine Änderung an `commands.rs`.
- INV-08: Identität nur über Twitch-User-ID.

## Nicht-Ziele

- Kein Pitch an Streamer in deren eigenem Kanal (das bleibt Outreach).
- Keine Änderung an Scout-Erkennung oder Admin-Freigabe.
- Kein Dashboard-Schalter.

## Erlaubter Änderungsbereich

- rust/crates/tb-chat/src/promos.rs
- rust/crates/tb-chat/src/promo_pitch.rs
- rust/crates/tb-chat/src/pipeline.rs
- rust/crates/tb-chat/src/lib.rs
- rust/crates/tb-chat/tests/
- rust/crates/tb-chat/Cargo.toml
- rust/bin/tb-bot/src/chat_wiring.rs
- rust/bin/tb-bot/src/main.rs
- rust/.sqlx/
- rust/crates/tb-db/tests/fresh_schema_snapshot.txt
- rust/migrations/
- rust/knowledge/bot/faq-werbung.md
- docs/
- .tasks/2026-09-05-partner-pitch-chatter/

## Verbotene Änderungen

- rust/crates/tb-scout/
- rust/crates/tb-engagement/
- rust/crates/tb-dashboard-api/
- rust/crates/tb-chat/src/commands.rs
- rust/bin/tb-bot/src/smalltalk_loop_wiring.rs
- bestehende Migrationen
- Lint- und CI-Konfiguration

## Offene Produktfragen

- keine

## Amendments

