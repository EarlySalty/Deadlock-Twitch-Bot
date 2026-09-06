# Contract: Zuschauer-Register und Pitch nur an echte Neulinge

status: aktiv
datum: 2026-09-06
klasse: hoch
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Der Bot spricht Zuschauer in Partnerkanälen nur noch dann mit einem Community-Pitch an, wenn sie nach seinem Wissen neu sind: zum ersten Mal in irgendeinem Partnerkanal, nicht als Mitglied des Discords bekannt, nie Partner-Streamer gewesen. Dafür entsteht ein Zuschauer-Register in der Twitch-DB, das jede Twitch-User-ID mit einer Wahrscheinlichkeit "ist schon in der Community" führt, einmalig aus den vorhandenen Daten rückwirkend befüllt und danach laufend gepflegt wird. Der heute abgeschaltete Timer-Pfad `targeted_user` (Zufallsziel, erfundener Bezug) kommt nicht zurück; der gezielte Pitch wird ereignisgetrieben neu gebaut und antwortet auf das, was die Person gerade geschrieben hat. Bestandsaufnahme: `EVIDENCE.md` in diesem Ordner.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01 Register: Neue Tabelle `twitch_zuschauer_register` in `twitch_analytics`, Schlüssel Twitch-User-ID (Login nur als Anzeige). Je Zeile: beste Discord-Zuordnung (Discord-User-ID, darf leer sein), Wahrscheinlichkeit 0 bis 1 "ist in der Community", die Signale als JSON, erster Partnerkanal und Zeitpunkt des ersten Auftauchens über alle Partnerkanäle, Berechnungszeitpunkt.
- REQ-02 Wahrscheinlichkeitsmodell: Aus mehreren Signalen, nicht nur Exakttreffer. Harte Zuordnung (bestehender Eintrag in `twitch_streamer_identities` mit Discord-ID) zählt 1,0. Namensabgleich Twitch-Login gegen Discord-Username, Global-Name und Server-Nickname der Guild-Mitgliederliste (Quelle: der bestehende Mitglieder-Weg des Bots über den Discord-Broker, `tb-transport-discord`): Exakttreffer nach Normalisierung hoch, Ähnlichkeit nach demselben Muster wie der Streamer-Matcher in `rust/bin/tb-bot/src/streamer_link.rs` abgestuft. Kanal-Vorwissen als Prior je Kanal: Community-Kanal `dach_lock` 0,8, alle anderen Partnerkanäle 0,2. Kombination `p = 1 - (1 - prior) * (1 - namens_score)`. Alle Schwellen und Priors sind Konstanten im Code, keine Config, kein ENV.
- REQ-03 Backfill: Ein einmalig aufrufbarer Lauf (Subcommand oder Bin im Repo) berechnet das Register für alle Twitch-User-IDs aus `twitch_session_chatters` und `twitch_chat_messages`; Zeilen ohne ID werden über `twitch_login_aliases` nachgeschlüsselt, was dort nicht auflösbar ist, bleibt ohne Registereintrag. Der Lauf ist idempotent und schreibt am Ende Zahlen (Einträge, davon mit Discord-Zuordnung, Verteilung der Wahrscheinlichkeit in vier Stufen) ins Log.
- REQ-04 Laufende Pflege: Taucht eine Twitch-User-ID zum ersten Mal in einem Partnerkanal auf, legt der Bot den Registereintrag sofort aus dem gecachten Mitgliederindex an (kein Helix-Aufruf pro Nachricht). Einträge älter als 7 Tage werden beim nächsten Auftauchen neu berechnet. Kein Nachtrag pro Poll, keine Reparaturschleife.
- REQ-05 Pitch-Gate für alle Zuschauer-Pitches (Anlass-Pitch `on_message_pitch` und der neue gezielte Pitch): Gesendet wird nur, wenn der Registereintrag existiert, `p < 0,35`, die Person laut Register zum ersten Mal in einem Partnerkanal ist (erstes Auftauchen liegt in der laufenden Session), sie nicht Broadcaster, Moderator oder Bot-Konto (`WHITELISTED_BOTS`) des Kanals ist, und sie in keiner dieser Tabellen steht: `twitch_partners` (aktiv oder departnered), `twitch_streamers`, `twitch_raid_auth`, `twitch_partner_signup_denylist`, `twitch_scout_pitch_blacklist`, `twitch_partner_outreach` mit Kontakt. Jede Ablehnung steht mit Grund (`register_community`, `register_fehlt`, `kein_neuling`, `partner_oder_streamer`, `broadcaster_mod_bot`) in `twitch_promo_pitch_log`.
- REQ-06 Gezielter Pitch, neu: Nur als Antwort auf die zweite oder spätere echte Nachricht (kein Befehl, mindestens 15 Zeichen) eines Zuschauers, der das Gate aus REQ-05 besteht, innerhalb weniger Sekunden, nie per Timer. Der Text entsteht aus den Nachrichten dieser Person in dieser Session über `tb_llm::complete` (Use-Case `promo_pitch`, Deepseek V4 Flash, Denken aus); sind keine eigenen Nachrichten der Person vorhanden, gibt es keinen Pitch. Der bestehende Stilvertrag und alle harten Filter (Link, Zahlen, Superlative, Smileys, Injection) gelten. Limits: je Twitch-User-ID genau einmal für immer über alle Kanäle, je Kanal zwei pro Stream, fünfzehn pro Tag insgesamt. Anlass-Pitch und gezielter Pitch schließen sich je Nachricht aus; trifft beides, geht nur der Anlass-Pitch.
- REQ-07 Alter Pfad: Die Konstante `TARGETED_USER_PITCH_AKTIV` und der Timer-Zweig für Einzelzuschauer in `maybe_send_targeted_promo` werden entfernt, `pick_user_target` mit Zufallswahl wird gelöscht. Die periodische Kanal-Ansage (`targeted_global`) bleibt unverändert.
- REQ-08 Sichtbarkeit: Gesendete gezielte Pitches bekommen die Review-Karte in Discord wie Anlass-Pitches, mit Wahrscheinlichkeit und Signalen des Registers als Zusatzzeile. Die Streamer-FAQ `rust/knowledge/bot/faq-werbung.md` beschreibt wahrheitsgemäß, dass der Bot nur neue Zuschauer anspricht.

## Invarianten (darf sich nicht ändern)

- INV-01: Werbefrei-Plan (`promo_disabled`), Kanal-Allowlist, Outbound-Suppression, Startverzögerung, Doppelsend-Lock, Judge-Drossel und Semaphore des Anlass-Pitch gelten unverändert für beide Pitch-Pfade.
- INV-02: Sprachmodell ausschließlich Deepseek V4 Flash über `tb_llm::complete`, Use-Case `promo_pitch`, Denken aus bei kleinem Budget.
- INV-03: Partner-Pitch an streamende Zuschauer (`run_partner_pitch`), Outreach-Kette (`tb-scout`) und `targeted_global` verhalten sich wie in main; ihre Tests bleiben unverändert grün.
- INV-04: Identität nur über Twitch-User-ID und Discord-User-ID; kein Filter über Login oder Anzeigename im Gate. Namen dienen nur der Berechnung des Scores.
- INV-05: Migrationen nur additiv; keine bestehende Tabelle wird umgebaut. `twitch_streamer_identities` bleibt streamer-only und wird nur gelesen.
- INV-06: Keine ENV-Config, kein neues Secret, keine Helix-Aufrufe im Nachrichtenpfad.
- INV-07: Bestehende Tests nicht löschen oder abschwächen; der Regressionstest des Abschalt-Fixes (`fix/targeted-user-pitch-aus`) wird beim Entfernen des Pfads durch einen Test ersetzt, der beweist, dass kein Pitch mehr per Timer an Einzelpersonen geht.
- INV-08: Kein Personendaten-Export in Logs: Log-Zeilen des Backfills tragen nur Zahlen.
- INV-09: Least-Privilege in der DB bleibt; die Migration legt die Tabelle als `postgres` an und vergibt SELECT/INSERT/UPDATE/DELETE an `twitchbot` und `twitchdash`.

## Nicht-Ziele

- Keine Speicherung der Twitch-Connection aus dem Discord-OAuth (Repo Deadlock-Bots, `dl-dashboard/src/oauth.rs:219`); das wird eine eigene Aufgabe, ihre Treffer sollen später als hartes Signal 1,0 ins Register fließen.
- Kein Opt-in-Link-Flow für Zuschauer, kein Dashboard-Schalter, keine Register-Ansicht im Dashboard.
- Keine Änderung an Scout, Outreach, Partner-Pitch, Smalltalk oder Scam-Guard.
- Keine Änderung des Anlass-Pitch-Judges oder seiner Occasions; nur das Gate aus REQ-05 kommt davor.

## Erlaubter Änderungsbereich

- rust/crates/tb-chat/src/promos.rs
- rust/crates/tb-chat/src/promo_pitch.rs
- rust/crates/tb-chat/src/zuschauer_register.rs
- rust/crates/tb-chat/src/pipeline.rs
- rust/crates/tb-chat/src/lib.rs
- rust/crates/tb-chat/tests/
- rust/crates/tb-chat/Cargo.toml
- rust/crates/tb-transport-discord/src/relay.rs
- rust/crates/tb-transport-discord/src/lib.rs
- rust/bin/tb-bot/src/chat_wiring.rs
- rust/bin/tb-bot/src/main.rs
- rust/bin/tb-bot/src/streamer_link.rs
- rust/bin/tb-bot/src/zuschauer_register_backfill.rs
- rust/bin/tb-bot/Cargo.toml
- rust/Cargo.lock
- rust/.sqlx/
- rust/crates/tb-db/tests/fresh_schema_snapshot.txt
- rust/migrations/
- rust/knowledge/bot/faq-werbung.md
- docs/
- .tasks/2026-09-06-zuschauer-register/

## Verbotene Änderungen

- rust/crates/tb-scout/
- rust/crates/tb-engagement/
- rust/crates/tb-dashboard-api/
- rust/crates/tb-chat/src/commands.rs
- rust/crates/tb-chat/src/conversation_scam.rs
- rust/crates/tb-monitoring/
- bestehende Migrationen
- Lint- und CI-Konfiguration

## Offene Produktfragen

- Schwelle 0,35 und Priors 0,8 / 0,2 sind Startwerte des Betreibers; Anpassung nach den Backfill-Zahlen als Amendment.

## Amendments
