# Bot-Texte (Twitch-Chat, Announcement lila)

Ersatz für die Pools in `rust/crates/tb-chat/src/promos.rs` (Zeilen 308 bis 353) und Grundlage für den Themenplan. Regeln: unter 300 Zeichen, ein konkretes Angebot, eine Handlung, `{invite}` am Ende. Kein "wir haben einen Discord", kein "schau vorbei", keine Ausrufezeichen-Ketten. Umgangssprache in Dosen, keine Werbesprache.

## Thema 1: Mitspieler finden (Pool competitive + community)

- Wer noch Leute für Ranked oder Casual sucht: Im Discord hängt jedes Gesuch direkt an einem Voice, ein Klick auf Beitreten und du bist drin. {invite}
- Solo Queue nervt? Im Discord findest du Duo und Stack in der Mitspieler-Suche, mit Voice dran statt totem Textkanal. {invite}
- Nach dem Stream noch weiterzocken: Mitspieler-Suche im Discord, Post hängt am Voice, voll ist voll, sonst bist du mit einem Klick drin. {invite}

## Thema 2: Neu in Deadlock (Pool growth)

- Neu in Deadlock? Im Discord gibt es Paten, die dir Lane, Items und Movement zeigen, und eine Voice-Lane nur für Neue. {invite}
- Verstehst noch nicht, was du kaufen sollst? Frag im Discord in frag-die-community, da antwortet jemand ohne Augenrollen. {invite}
- Kein Zugang zu Deadlock? Im Discord in frag-die-community nachfragen, da hilft dir jemand mit einer Einladung. {invite}

## Thema 3: Coaching (Pool coaching)

- Coaching kostet bei uns nichts. Echte Leute aus der Community, du sagst, woran du arbeiten willst, ein Coach meldet sich. Im Discord unter Coaching. {invite}
- Feststecken im Rang? Im Discord Coaching anfragen, kostenlos, mit Leuten, die es dir am eigenen Gameplay zeigen. {invite}
- Wer besser werden will statt nur mehr zu spielen: Kostenloses Coaching im Discord, Anfrage dauert eine Minute. {invite}

## Thema 4: Rang verknüpfen

- Steam im Discord verknüpfen und du bekommst deine Rang-Rolle automatisch, die zieht beim Aufstieg mit. Rank-ups feiern wir in #rank-ups. {invite}
- Für Ranked-Gesuche in der Mitspieler-Suche brauchst du den verknüpften Rang. Geht im Discord im Rang-Kanal in zwei Minuten. {invite}

## Thema 5: Scrims und Custom Games

- Bei uns laufen Scrims mit festen Teams, alle zwei Wochen, mit Voice und Absprachen. Wer mitspielen will, schreibt im Discord in den Scrim-Kanal. {invite}
- Custom Games mit Leuten aus dem Discord: Ping-Rolle holen, dann weißt du, wann eine Lobby aufgeht. {invite}

## Thema 6: Patchnotes

- Jeder Deadlock-Patch landet sofort im Discord im Patchnotes-Kanal. Ping-Rolle holen, dann liest du ihn in der Queue statt im Match zu merken, dass was anders ist. {invite}

## Zuschauer-Sprung (Pool hype, nur bei Viewer-Spike)

- Viele neue Gesichter hier. Wer nach dem Stream Mitspieler oder kostenloses Coaching sucht: Discord. {invite}
- Willkommen alle, die gerade reinkommen. Mitspieler-Suche mit Voice und Paten für Neue gibt es im Discord. {invite}

## Streamer-Portal (Pool partner, nur in Kanälen ohne Partnerstatus)

- Du streamst Deadlock? Auto-Raids in beide Richtungen, Go-Live-Post im Discord und Chat-Schutz, 0 Euro für immer: https://deutsche-deadlock-community.de/streamer
- Am Ende deines Streams raidet der Bot automatisch den nächsten Deadlock-Streamer, und die Raids der anderen kommen zu dir. Einloggen mit Twitch reicht: https://deutsche-deadlock-community.de/streamer
- Streamer-Netzwerk für Deadlock: kostet nichts, in einer Minute drin, in einer Minute wieder raus. https://deutsche-deadlock-community.de/streamer

## Feste Antworten (neu, `standard_replies.rs`)

Auslöser: Regex auf "discord", "server", "community" plus Fragezeichen oder "gibt es", "habt ihr", "link". Antwort:

- @{chatter} Ja, hier: {invite} Mitspieler-Suche mit Voice, kostenloses Coaching, Paten für Neue.

Cooldown je Kanal 120 Sekunden, je Chatter einmal pro Stream.

## Raid-Ankunft (an die bestehende Raid-Begrüßung hängen)

- Willkommen vom Raid. Heute im Fokus: {thema_kurz} {invite}

`{thema_kurz}` ist die Kurzfassung des Wochenthemas, ein Satz.

## `!thema`

Gibt den ersten Text des aktuellen Themas als Announcement aus. Für den Streamer und Mods ohne Cooldown, für Zuschauer 120 Sekunden je Kanal.

## LFG-Pitch (bestehend, Text tauschen)

Heute: "@{chatter} Schau gerne mal in unsere Community rein: {invite} da findest du jederzeit passende Mitspieler :)"

Neu: "@{chatter} Im Discord hängt die Mitspieler-Suche direkt am Voice, ein Klick und du hast Leute: {invite}"
