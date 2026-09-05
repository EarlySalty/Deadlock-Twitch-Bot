# Contract: Discord-Pitches mit Qualität statt Preset-Sprüchen

status: erledigt
datum: 2026-09-04
klasse: hoch
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Jede Discord-Werbung des Twitch-Bots in Partnerkanälen ist eine auf die Situation geschriebene Nachricht, die zuerst auf die Person oder den Moment eingeht und danach die Community anbietet, so dass Zuschauer Lust bekommen. Fertige Preset-Sprüche wie "@login Mitspieler sind auf Discord: {invite}" gibt es nicht mehr.

Auslöser: Screenshot vom 2026-09-04 aus dem Kanal des Betreibers. First-Time-Chatter Symphooniee: "yo wieso ist deadlock so unpopulär wie haben die den anschluss verpasst" und "so ein krasses game, leider sind alle meine freunde nicht so davon überzeugt die meinen es ist zu tryharded". Der Bot hat nicht reagiert; der Timer-Pfad hätte höchstens zufällig einen Preset-Spruch geschickt.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01 Anlass-Pitch: Schreibt ein Zuschauer in einem Partnerkanal eine Nachricht, die einen Anlass trifft (keine Freunde oder Mitspieler, Spiel unpopulär oder tot, zu tryhard, Solo-Queue-Frust, neu im Spiel, sucht Hilfe oder Coaching), antwortet der Bot innerhalb von 30 Sekunden als Reply auf diese Nachricht. Die Antwort hat zwei Teile in dieser Reihenfolge: eine echte Antwort auf das Gesagte, dann höchstens ein Satz zur Community in dritter Person. Kein Link, kein "komm auf Discord". Der Link kommt nur per `!discord` oder wenn die Person nachfragt.
- REQ-02 Erkennung und Text: Anlass-Erkennung und Antworttext kommen aus einem Aufruf von `tb_llm::complete` mit neuem Use-Case. Der Systemprompt übernimmt den Stilvertrag aus `outreach_shadow.rs` (Deutsch, kurz, locker, Kleinschreibung erlaubt, kein Emoji außer :), keine Superlative, keine Mitgliederzahlen, kein Gedankenstrich, kein Link, keine Ausrufezeichen-Werbung). Das Modell antwortet mit JSON (occasion, reply, confidence); ohne Anlass sendet der Bot nichts.
- REQ-03 Harte Filter vor dem Senden: enthält der Text einen Link, eine Mitgliederzahl, ein Superlativ, einen Gedankenstrich, ein verbotenes Emoji, mehr als 400 Zeichen oder die Wendungen "komm auf", "join", "tritt bei", wird nichts gesendet und der Grund protokolliert. Kein Fallback auf einen festen Text.
- REQ-04 Timer-Pitches: Die Preset-Listen `global_presets`, `user_presets`, `all_promo_messages`, `activity_promo_messages` und `promo_messages_hype` werden gelöscht. Periodische Kanal-Promo und Targeted-Pitches bekommen ihren Text aus demselben LLM-Pfad, geschrieben aus dem aktuellen Kontext (letzte Chat-Nachrichten, Spiel, Stream-Titel, bei Targeted die Nachrichten der Zielperson). Nur die periodische Kanal-Promo trägt den Invite-Link am Ende, personenbezogene Pitches nie. Liefert der LLM-Pfad keinen gültigen Text, geht kein Pitch raus.
- REQ-05 Limits Anlass-Pitch: je Zuschauer (Twitch-User-ID) höchstens ein Anlass-Pitch pro 7 Tage über alle Kanäle; je Kanal höchstens 3 Anlass-Pitches pro Stream mit mindestens 10 Minuten Abstand; ein gesendeter Anlass-Pitch belegt den Promo-Cooldown des Kanals wie `send_timeout_pitch`, damit danach keine Timer-Promo obendrauf kommt. Die Stammgast-Ausnahme gilt weiter für Timer-Pitches, nicht für den Anlass-Pitch.
- REQ-06 Bestehende Gates gelten für alle drei Pfade: Partnerkanal, Werbefrei-Plan (`promo_disabled`), Kanal-Allowlist, Outbound-Suppression, Startverzögerung 10 Minuten nach Go-Live, Doppelsend-Lock.
- REQ-07 Sichtbarkeit: jeder erzeugte Pitch (gesendet oder verworfen) landet als Zeile in der neuen Tabelle `twitch_promo_pitch_log` (Kanal, Twitch-User-ID der Zielperson oder NULL, Pfad, Anlass, Auslöser-Text, erzeugter Text, Verwerfungsgrund, gesendet-Zeitpunkt). Gesendete Anlass-Pitches gehen zusätzlich als Karte in den Discord-Review-Kanal, den das Smalltalk-Modul nutzt, mit Auslöser-Zitat und Antwort.
- REQ-08 Streamer-FAQ `rust/knowledge/bot/faq-werbung.md` beschreibt das neue Verhalten wahrheitsgemäß (Antwort auf Anlass, Link nur in der periodischen Einladung).

## Invarianten (darf sich nicht ändern)

- INV-01: Der Werbefrei-Plan schaltet weiterhin jede Werbung ab, auch den Anlass-Pitch.
- INV-02: Sprachmodell ausschließlich Deepseek V4 Flash über `tb_llm::complete`; kein direkter HTTP-Client, kein anderes Modell, Use-Case in der Nur-Fireworks-Liste.
- INV-03: Dashboard-Override-Texte (Streamer-Promo-Text, globaler Promo-Text) bleiben und haben in der periodischen Kanal-Promo Vorrang vor LLM-Text.
- INV-04: Doppelsend-Lock, Cooldown-Persistenz in `twitch_promo_cooldowns` und der Werbefrei-Pitch (`send_timeout_pitch`) bleiben unverändert.
- INV-05: `!discord`, `!dldc`, `!dlde` und `!invite` bleiben unverändert.
- INV-06: Bestehende Tests werden nicht gelöscht oder abgeschwächt; Tests, die nur die gelöschten Presets prüfen, werden durch Tests des neuen Pfads ersetzt.
- INV-07: Keine ENV-Config, kein neues Secret, keine Änderung an Preisplänen oder `streamer_plans`.
- INV-08: Streamer-Outreach (`outreach_shadow.rs`), Smalltalk-Modul und Engagement-Modul bleiben unberührt.
- INV-09: Migration ausschließlich additiv (neue Tabelle), Anwendung als `postgres` mit Grants an `twitchbot` und `twitchdash`.

## Nicht-Ziele

- Kein Pitch an fremde Streamer (das ist Outreach).
- Keine Dashboard-UI-Änderung, keine neuen Schalter je Streamer.
- Kein Umbau von Lurker-Tax oder des Lurker-Pitch-Tasks vom 2026-08-30.
- Keine Änderung der Sonder-Event-Texte.

## Erlaubter Änderungsbereich

- rust/crates/tb-chat/src/promos.rs
- rust/crates/tb-chat/src/promo_pitch.rs
- rust/crates/tb-chat/src/lib.rs
- rust/crates/tb-chat/src/pipeline.rs
- rust/crates/tb-chat/tests/
- rust/bin/tb-bot/src/chat_wiring.rs
- rust/bin/tb-bot/src/main.rs
- rust/crates/tb-llm/src/selection.rs
- rust/crates/tb-llm/src/hub.rs
- rust/migrations/
- rust/knowledge/bot/faq-werbung.md
- docs/
- .tasks/2026-09-04-discord-pitch-qualitaet/

## Verbotene Änderungen

- rust/crates/tb-engagement/
- rust/crates/tb-dashboard-api/
- rust/crates/tb-chat/src/commands.rs
- website/
- Lint- und CI-Konfiguration
- bestehende Migrationen

## Offene Produktfragen

- keine

## Amendments

- 2026-09-04, REQ-01, "als Reply auf diese Nachricht" -> "als Antwort mit @login-Anrede der Zielperson": ChatApi kennt kein reply_parent_message_id, echter Twitch-Reply braeuchte tb-transport-twitch ausserhalb des Scopes; entschieden von Orchestrator (nur technisch, reversibel)
