# Community-Werbung im Stream: Ein Thema pro Stream

Stand 2026-09-02. Bestand gegen Code (Deadlock-Twitch-Bot main), Discord (Server-Übersicht) und Docs geprüft. Nichts hier bewirbt etwas, das es nicht gibt.

## Kernidee

Nicht "wir haben einen Discord", sondern pro Stream genau ein konkretes Angebot, das den Zuschauer in dem Moment betrifft, in dem er es hört. Drei Kanäle spielen dasselbe Thema:

1. **Nani sagt es einmal** (30 bis 45 Sekunden, Sprechzettel, nicht ablesen), am Match-Ende oder wenn im Chat der passende Satz fällt ("kann ich mit?", "bin neu", "wie werde ich besser").
2. **Der Bot schreibt es dazu**, mit Link, im selben Moment (`!thema`) und danach dosiert im Hintergrund (bestehende Promo-Engine, gleiches Thema statt Zufallspool).
3. **OBS zeigt es** als Lower Third für 10 Sekunden, während Nani spricht.

Warum ein Thema: Ein Zuschauer, der drei Angebote hört, merkt sich keins. Einer, der hört "die Mitspieler-Suche hängt am Voice, ein Klick und du bist drin", weiß, was er nach dem Stream macht.

## Die Themen (Rotation über sechs Wochen)

| KW-Slot | Thema | Auslöser im Stream | Zielgruppe |
|---|---|---|---|
| 1 | Mitspieler finden (Voice-Lanes + Mitspieler-Suche mit Beitreten-Knopf) | "kann ich mit?", Match-Ende, Solo-Queue-Gejammer | jeder Zuschauer |
| 2 | Neu in Deadlock (Paten, Neue-Spieler-Lane, frag-die-community) | Neuling im Chat, Nani erklärt gerade etwas Grundlegendes | Einsteiger |
| 3 | Coaching, kostenlos, echte Leute (Coaching-Anfrage per Panel oder Befehl) | Fehler-Analyse im Stream, "wie werde ich besser" | Aufsteiger |
| 4 | Rang verknüpfen (Steam-Verknüpfung, Rang-Rolle, #rank-ups) | Rank-up im Stream, Rang-Talk | Ranked-Spieler |
| 5 | Scrims und Custom Games (feste Teams, Ansprechpartner Leo, Ping-Rolle) | Custom-Lobby, Teamplay-Momente | Leute mit Team-Bock |
| 6 | Patchnotes-Kanal mit Ping-Rolle | Patch-Tag, "was hat sich geändert" | jeder |

Zusätzlich, nicht in der Rotation, sondern anlassbezogen:

- **Streamer-Portal** (0 EUR für immer: Auto-Raid-Netzwerk in beide Richtungen, Go-Live-Post im Discord, Chat-Schutz, Dashboard): immer dann, wenn ein anderer Streamer im Chat sitzt oder Nani über Raids spricht. Der Bot spricht kleine Deadlock-Kanäle ohnehin per Partner-Outreach an (8 pro Tag, 30 Tage Cooldown).
- **Uplink** (Beta, Warteliste): nur wenn Nani selbst über Uplink streamt und Plätze frei sind.

Bei DachLock-Streams mit mehreren Streamern gilt: alle sprechen dasselbe Wochenthema, jeder in eigenen Worten. Der Sprechzettel ist ein Gerüst, kein Text.

## Was heute schon da ist (Bot)

- **Promo-Engine** `tb-chat/src/promos.rs`: schreibt in Partnerkanälen (nicht `promo_disabled`, Plan ohne "werbefrei") bei Chat-Aktivität, im 60-s-Takt und bei Zuschauer-Sprung. Cooldown 45 bis 180 Minuten je nach Chat-Tempo, frühestens 10 Minuten nach Stream-Start, mindestens 2 neue Chatter seit der letzten Promo. Texte kommen aus festen Pools im Code (competitive, community, growth, coaching, hype, partner), Zufall mit Schutz nur gegen den unmittelbar letzten Text. Jede Promo geht als **lila Twitch-Announcement** raus (`promos.rs:1180`), fällt also im Chat auf.
- **Begrüßung** `tb-chat/src/standard_replies.rs`: antwortet auf Grußworte einmal je Chatter und Stream, pro Kanal abschaltbar. Erst-Chatter-Ereignis (`channel.chat.user_first_message`) und Raid-Ankunft werden erfasst, lösen aber keinen Community-Hinweis aus.
- **Zugangsfrage** `tb-chat/src/invite_question.rs`: "wie komme ich an Deadlock?" bekommt eine Antwort mit Verweis auf frag-die-community. Die reine Frage "gibt es einen Discord?" stuft der Judge absichtlich als nein ein und bleibt unbeantwortet.
- **Globaler Event-Modus** `tb-analytics/src/promo_mode.rs`: Admin setzt `custom_event` mit einem Text und Zeitfenster, der Text ersetzt den Pool. Das ist der Hebel für "Thema der Woche", heute aber nur ein Text ohne Rotation.
- **Streamer-eigener Promo-Text** (`streamer_plans.promo_message`, max. 500 Zeichen, muss `{invite}` enthalten).
- **LFG-Pitch** `tb-chat/src/lfg_pitch.rs`: antwortet auf "kann ich mit?", "noch Platz?" mit Discord-Link, Regex plus Judge, nur im eigenen Kanal (seit Lurker-Pitch-Slice), 6 h Cooldown je Chatter.
- **Lurker-Pitch**: stille Stammzuschauer bekommen einmalig eine Einladung, hinter `lurker_pitch_enabled` (Default aus).
- **Befehle**: `!discord`, `!invite`, `!dldc`/`!dlde` (Einladung des Streamers), `!rank`, `!live`. Alle Befehle stehen fest im Code (`catalog.rs`, `commands.rs`), es gibt kein Custom-Command-System.
- **Match-Kontext** `tb-engagement/src/match_context.rs`: Held, Match-ID, "Match läuft" aus der Deadlock-API. Ein Match-Ende-Signal gibt es nicht, es lässt sich aber aus dem Wechsel von "läuft" auf "läuft nicht" ableiten.
- **Einladungscodes je Streamer** (`discord_invite_codes`) plus `InviteTracker` im Discord-Bot: Beitritte lassen sich der Einladung zuordnen. Damit ist messbar, welches Thema Leute bringt.
- **Werbepausen-Manager** (seit 2026-09-01): kennt ruhige Chat-Fenster und die Werbe-Termine. Kein Community-Promo, aber derselbe "ruhiger Moment"-Sensor.

## Was fehlt (Bauliste, Reihenfolge = Priorität)

1. **Texte tauschen** (klein, sofort): Die Pool-Texte sind trocken und teils falsch geschrieben ("frag die community hilft jemand"). Neue Texte in `BOT-TEXTE.md`, je Thema drei Varianten. Dazu: Partner-Pool nur in Kanälen ausspielen, deren Streamer noch kein Partner ist. Nur Code-Konstanten plus ein Filter, kein Schema.
2. **`!thema`-Befehl plus Discord-Frage** (klein): Streamer oder Mod tippt `!thema`, der Bot postet sofort den Text des aktuellen Themas als Announcement. Zuschauer bekommen mit `!thema` denselben Text (Cooldown 2 Minuten je Kanal). Außerdem ein fester Regex-Pfad in `standard_replies.rs` für "gibt es einen Discord/Server?" mit Einladung, ohne Judge.
3. **Themenplan statt Einzeltext** (mittel): `custom_event` wird zu einer Liste: Thema, Textvarianten, Slot (Kalenderwoche modulo Anzahl) oder festes Zeitfenster, Gedächtnis "jedes Thema höchstens einmal je Stream". Admin-Seite bestehend (`admin_promo_mode.rs`), Rotation rechnet der Bot. Pool bleibt Fallback.
4. **Einladung je Thema und Sende-Protokoll** (mittel): pro Thema ein eigener Discord-Einladungscode (Discord-Bot legt an), Tabelle "Thema, Kanal, Zeitpunkt, Auslöser" statt nur Cooldown, Auswertung "Beitritte je Thema je Woche" im Admin-Dashboard. Ohne das bleibt die Rotation Bauchgefühl.
5. **Match-Ende und Raid-Ankunft als bevorzugte Momente** (mittel): Match-Ende aus dem Wechsel in `twitch_channel_match_state` ableiten, fällige Promo bis dahin aufschieben (maximal 15 Minuten). Bei Raid-Ankunft einmal die Kurzfassung des Themas an die bestehende Raid-Begrüßung hängen.
6. **OBS-Overlay "Heute im Fokus"** (mittel): Browser-Quelle aus dem Streamer-Dashboard, zeigt den Thementitel als Lower Third, Einblendung per `!thema` oder Knopf im Dock. Bis dahin: sechs statische PNGs (Bauliste Visuals).
7. **Streamer-Sicht im v2-Dashboard** (mittel): Der eigene Promo-Text liegt heute in der Abo-Seite. Unter Verwaltung gehört eine Karte hin: aktuelles Thema, was der Bot zuletzt in meinem Chat geschrieben hat, Themen an/aus.
8. **Partnerkanäle mit Opt-in** (später): LFG-Pitch und Themen-Promo in fremden Partnerkanälen nur, wenn der Streamer es dort einschaltet. Im Lurker-Pitch-Contract ausdrücklich als eigener Slice vorgesehen.

Punkte 1 und 2 zusammen: ein Nachmittag. Punkte 3 bis 5: je ein Tag mit Tests. Punkte 6 und 7: je ein Tag Frontend.

## Visuals (Bauliste)

- **Lower Third je Thema** (6 Stück, 1920x1080 mit Transparenz, Gold auf Dunkel, Schrift wie Dashboard): Titel plus ein Satz plus Kurzlink. Pipeline: `deadlock-brand-asset-generation` (Pillow). Einblenden per OBS-Hotkey, 10 Sekunden.
- **Echter Screenshot der Mitspieler-Suche** mit dem Beitreten-Knopf, als Bild-Quelle für Thema 1. Ein echter Post überzeugt mehr als jede Grafik.
- **Kurzlink**: `deutsche-deadlock-community.de/discord` als Weiterleitung auf die aktuelle Einladung (Caddy, fünf Minuten). Dann steht auf jedem Visual derselbe Link.
- **Twitch-Panel "Community"** unter dem Stream mit den sechs Themen in je einer Zeile, gleicher Link.
- **QR-Code** nur für DachLock-Events mit Zuschauern am Handy, sonst weglassen.

## Dosierung (damit es nicht nervt)

- Nani: ein Thema pro Stream, einmal ausführlich, höchstens einmal kurz wiederholt bei Raid-Ankunft.
- Bot: höchstens eine Themen-Promo pro 45 Minuten und nie im laufenden Teamfight (Bauliste 5). LFG-Pitch reagiert nur auf echte Anlässe.
- Raid-Ankunft: Begrüßung plus Kurzfassung des Themas, einmal.
- Kein Thema mehr als zwei Wochen am Stück, sonst rotiert es weiter, auch wenn es gut lief.

## Messen

Pro Woche drei Zahlen im Admin-Dashboard: Beitritte je Einladung (Thema), Klicks auf `!thema`, Zuschauer die nach dem Stream in einer Voice-Lane gelandet sind (Discord-Bot kennt die Lanes). Nach sechs Wochen: die zwei schwächsten Themen tauschen oder neu formulieren.

## Offene Entscheidungen

- Streamer-Portal als siebter Rotations-Slot oder nur anlassbezogen? Vorschlag: anlassbezogen, weil die Zielgruppe im Chat klein ist.
- Uplink überhaupt im Stream erwähnen, solange die Warteliste manuell freigeschaltet wird? Vorschlag: erst nach der Ankündigung in #streamer-updates.
- Scrims bewerben, bevor der Onboarding-Weg (Leo als Ansprechpartner) im Discord sichtbar ist? Vorschlag: Slot 5 erst ab KW 5 fahren und vorher einen Pin in #scrim-channel setzen.
