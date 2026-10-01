# Contract: Pitch-Stimme und Lernschleife

status: aktiv
datum: 2026-09-09
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu. Wer ein REQ oder INV ändern will, schreibt ein
Amendment mit Begründung; Produkt-, API- oder Datenänderungen entscheidet der User.

## Ziel

Die KI-Pitches des Twitch-Bots (Anlass-Pitch, Partner-Pitch, periodische Einladung) klingen lustig und frech, aber nie herabsetzend, sprechen erkennbar als Bot, erfinden kein Spielwissen und keine eigenen Erlebnisse, schweigen bei Nachrichten, die an jemand anderen gerichtet sind, und lernen aus Daumen hoch und Daumen runter an den Review-Karten in Discord.

Beleg für den Ist-Zustand (Log `twitch_promo_pitch_log`, Stand 2026-09-09, 17 gesendete Pitches auf den Pfaden anlass, partner, gezielt):
- Ich-Form als Mensch: "bin gerade in den ersten ranked games" (partner, marcymcwhy, 09-09), "ich hab bock auf die picks" (partner, earlysalty, 09-07), "war ein paar tage weg, aber jetzt bin ich wieder da" (gezielt, earlysalty, 09-07), "wir spielen gerade die normale version" (anlass, denoshock, 09-05).
- Erfundenes Wissen: "ohne green investment wird das gegen die tanky builds wackelig" (gezielt, ismile_e, 09-08), "die scrim teams werden gerade ordentlich durchgeschüttelt" (gezielt, earlysalty, 09-07), "der prime für affiliate direkt dazu" (gezielt, luckymakkydead, 09-07).
- Antwort auf Nachrichten an den Streamer: "Na du nippel. Schön eingeranked?" (partner, marcymcwhy, 09-09), "oh marcy, oh marcy" (partner, marcymcwhy, 09-07), "Was geht alles fit" (gezielt, earlysalty, 09-07).
- Immer derselbe Schlusssatz "die community hier ist ..." ohne Witz.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: Jeder Pitch-Prompt (Anlass, Partner, periodische Einladung) enthält einen gemeinsamen Stilvertrag mit Bot-Identität: der Sprecher ist der Bot der Deutschen Deadlock Community, sagt das auf Nachfrage oder wenn es den Witz trägt, und benutzt Ich-Form nie für eigenes Spielen, eigene Ränge, eigene Erlebnisse, eigene Abwesenheit oder eigene Meinung zu Builds.
- REQ-02: Ist eine Chat-Nachricht an jemand anderen gerichtet, gibt es keinen Anlass- und keinen Partner-Pitch: Reply auf eine fremde Nachricht (EventSub `reply.parent_user_id` ungleich Bot), @-Erwähnung eines anderen Nutzers, oder der Text enthält Login oder Anzeigename des Broadcasters. Die Ablehnung steht im Log mit `reject_reason = 'adressat_fremd'`. `ChatMessageEvent` trägt dafür `reply` (Parent-User-ID und -Login) und bei Mention-Fragmenten die User-ID.
- REQ-03: Der Stilvertrag verbietet Aussagen über Spielmechanik, Items, Builds, Ränge, Patches, Turniere, Scrims, Community-Interna und Ereignisse, die nicht wörtlich im Auslösetext oder Chatverlauf stehen. Ein harter Filter nach der Generierung verwirft Antworten mit Ich-Spiel-Formen ("ich spiele", "ich zocke", "ich hab(e) ... gespielt", "bin gerade", "war ... weg", "bin wieder da", "wir spielen", "mein rank", "mein build" und gleichwertige Formen) mit `reject_reason = 'ich_form'`.
- REQ-04: Der Stilvertrag verlangt Witz und Frechheit auf Kosten des Spiels, der Situation oder des Bots selbst, nie auf Kosten der angesprochenen Person; keine Beleidigungen, keine Fäkal- und Sexualsprache, kein Auslachen. Ein harter Filter mit Sperrliste verwirft Antworten mit Beleidigungen (`reject_reason = 'beleidigung'`). Der Anlass-Judge liefert zusätzlich `ernst_gemeint` (bool); Troll- und Scherzaussagen ("hoffe deadlock stirbt") sowie Zugangsfragen ergeben keinen Anlass.
- REQ-05: Jeder Pitch-Prompt enthält einen Beispielblock nach dem Muster `tb-engagement/src/style_examples.rs`: feste Startbeispiele (mindestens sechs je Pfad, im Stil des Skills `community-ankuendigung`, vom Nutzer gelieferte Antworten wortgleich), und sobald mindestens vier als gut bewertete Antworten desselben Pfads vorliegen, ersetzen die jüngsten gelernten die Startbeispiele. Bis zu drei als schlecht bewertete Antworten stehen als Gegenbeispiele im Block.
- REQ-06: Beim Senden der Review-Karte wird die Discord-Message-ID an der Log-Zeile gespeichert (neue Spalte `review_message_id`). Die Karte enthält den Hinweis, mit Daumen hoch oder Daumen runter zu reagieren.
- REQ-07: Ein Timer in tb-bot (Intervall 10 Minuten, Konstante) holt über den Broker-Endpunkt `GET /internal/master/v1/discord/message-reactions` die Reaktionen aller Karten der letzten 14 Tage ohne Bewertung. Daumen hoch ergibt `bewertung = 'gut'`, Daumen runter `'schlecht'`, beides zusammen `'schlecht'`; gespeichert mit `bewertet_at`. Bewertete Antworten wirken ab dem nächsten Pitch (REQ-05). Broker-Fehler werden einmal je Lauf geloggt und blockieren nichts.
- REQ-08: Alle Pitch-Aufrufe an tb-llm (Judge, Partner, periodische Einladung) laufen mit `denken_aus` und gesetztem Token-Budget.
- REQ-09: Ein Test-Fixture mit den gesendeten Pitches aus dem Log (Stand 2026-09-09) belegt, dass die Filter aus REQ-03 und REQ-04 die vier Ich-Form-Antworten aus dem Ziel-Abschnitt verwerfen und die übrigen sauberen Antworten durchlassen; ein Test mit synthetischem `ChatMessageEvent` belegt REQ-02 für Reply, Mention und Broadcaster-Anrede.

## Invarianten (darf sich nicht ändern)

- INV-01: Mechanik der periodischen Einladung (Cooldown, Announcement, Invite-Link, Dashboard-Override, Werbefrei-Plan, Suppression-Guard) bleibt; nur Stilvertrag und Beispielblock kommen dazu.
- INV-02: Alle Limits bleiben: ein Anlass-Pitch je Zuschauer und 7 Tage, drei je Kanal und Stream, Partner-Pitch einmal je Person für immer, einmal je Kanal und Stream, fünf pro Tag; das Register-Gate vor dem Anlass-Pitch bleibt.
- INV-03: Alle KI-Aufrufe laufen über tb-llm mit Deepseek V4 Flash; kein neues Modell, kein zweiter KI-Client.
- INV-04: Bestehende Tests werden nicht gelöscht oder abgeschwächt; Fixture-Texte dürfen nur angepasst werden, wenn der Test danach dieselbe Eigenschaft prüft.
- INV-05: Kein eigener Discord-Token und kein eigener Gateway in tb-bot; Discord nur über `BrokerRelay`.
- INV-06: Jeder gesendete Anlass- und Partner-Pitch bekommt weiterhin eine Review-Karte im bestehenden Kanal.
- INV-07: Keine ENV-Variablen und keine neuen Schalter; Intervall, Fenster und Schwellen sind Konstanten im Code.
- INV-08: Bestehende harte Filter (Link, Zahlen, Superlative, Gedankenstrich, Smiley, Join-Phrase, Prompt-Injection) bleiben aktiv.

## Nicht-Ziele

- Den gezielten Zuschauer-Pitch wiederbeleben.
- Bewertung über Dashboard, Web-UI oder Discord-Buttons.
- Änderungen am Discord-Bot (Repo Deadlock-Bots); der Reaktions-Endpunkt existiert dort schon.
- Anbindung von Spielwissen (Ränge, Builds, Patch-Daten) in die Pitches.
- Änderungen am Scam-Pfad (`send_timeout_pitch`) oder am Smalltalk-Modul.

## Erlaubter Änderungsbereich

- rust/crates/tb-chat/src/promo_pitch.rs
- rust/crates/tb-chat/src/promos.rs
- rust/crates/tb-chat/src/types.rs
- rust/crates/tb-chat/src/lib.rs
- rust/crates/tb-chat/src/pitch_beispiele.rs
- rust/crates/tb-chat/src/pitch_bewertung.rs
- rust/crates/tb-transport-discord/src/relay.rs
- rust/crates/tb-transport-discord/src/backend.rs
- rust/bin/tb-bot/src/chat_wiring.rs
- rust/bin/tb-bot/src/main.rs
- rust/migrations/20260909150000_twitch_promo_pitch_log_bewertung.sql
- rust/.sqlx/
- rust/crates/tb-db/tests/fresh_schema_snapshot.txt
- .tasks/2026-09-09-pitch-stimme-lernschleife/

## Verbotene Änderungen

- Alles außerhalb des erlaubten Bereichs, insbesondere bot/dashboard_v2, rust/crates/tb-engagement, rust/crates/tb-llm, Cargo.toml und Cargo.lock.
- Bestehende Migrationen.
- Lint- und CI-Konfiguration.

## Offene Produktfragen

- keine. Vom Nutzer nachgelieferte Beispielantworten werden als Amendment ergänzt und wortgleich in die Startbeispiele übernommen.

## Amendments

