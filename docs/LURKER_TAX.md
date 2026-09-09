# Lurker Steuer

Die Lurker Steuer ist eine Chat-Automation für bezahlte Pläne. Sie erinnert bekannte aktuell anwesende Lurker sanft im Twitch-Chat, genau eine Kanalpunkte-Belohnung mit dem Standardnamen "Lurker Steuer" einzulösen. Wer sie einlöst, bekommt eine kurze Dankes-Antwort und wird in dieser Session nicht mehr erinnert. Der Bot behauptet keine Punktestände und keine Kosten.

## Belohnung als Vorlage

- Standardname der Belohnung ist "Lurker Steuer". Die Erkennung ist unabhängig von Groß- und Kleinschreibung und akzeptiert auch "Lurker Steuern" (Präfixvergleich auf dem normalisierten Titel).
- Den Preis legt der Streamer selbst fest, zum Beispiel 10 Punkte. Der Bot legt die Belohnung nicht selbst an.
- Die Erinnerung wird nur gesendet, wenn diese Belohnung im Kanal existiert und aktiv ist (Helix `GET channel_points/custom_rewards` mit dem bestehenden Streamer-Token, Scope `channel:read:redemptions` liegt im Basisprofil).

## Verfügbarkeit

- `raid_boost`
- `analysis_dashboard`
- `bundle_analysis_raid_boost`
- `raid_free` bleibt ausgeschlossen

Die bestehende Extended-Grenze für Analytics bleibt unverändert. Das Feature nutzt eine eigene Paid-Plan-Prüfung.

## Dashboard / Settings

- Seite: `GET /twitch/abbo`
- Speichern: `POST /twitch/abbo/lurker-tax-settings`
- Speicherung: `streamer_plans.lurker_tax_enabled`

Verhalten im Abo-Bereich:

- `raid_free`: gesperrte Teaser-Karte mit Upgrade-Hinweis
- Bezahlplan: Toggle für aktiv/inaktiv
- Vorlage-Karte: Name "Lurker Steuer", Preisvorschlag 10 Punkte, Kurzanleitung (Twitch Creator-Dashboard, Zuschauerbelohnungen, Punkte und Belohnungen, Individuelle Belohnung hinzufügen) plus Status "Belohnung gefunden" oder "Belohnung fehlt"
- Readiness: die Karte spiegelt dieselbe Bedingung wie die Laufzeit. Der Scope `moderator:read:chatters` ist eine Kapabilität des zentralen Bots und wird in `twitch_bot_capabilities` gespiegelt. Der Warnhinweis sagt bei echtem Fehlen, dass sich der Betrieb darum kümmert, statt dem Streamer ein wirkungsloses Neu-Verbinden zu empfehlen

## Laufzeitlogik

Die Laufzeit hängt am bestehenden Promo-/Announcement-Loop in [`bot/chat/promos.py`](../bot/chat/promos.py).

Ein Reminder wird nur gesendet, wenn alle Bedingungen erfüllt sind:

- der Stream ist live
- es gibt eine aktive Session
- der Streamer hat einen bezahlten Plan
- `lurker_tax_enabled = true`
- der zentrale Bot-Zugriff fuer `moderator:read:chatters` ist verfuegbar
- die Belohnung "Lurker Steuer" existiert im Kanal und ist aktiv
- es gibt frische Präsenzdaten in `twitch_session_chatters`

Wer in der laufenden Session die Belohnung bereits eingelöst hat (`twitch_channel_points_events` mit passendem Reward-Titel), fällt aus der Kandidatenliste. Bei einer Einlösung (`channel.channel_points_custom_reward_redemption.add` mit passendem Titel) antwortet der Bot einmal je Zuschauer und Session mit einem kurzen Dank.

## Kandidatenlogik

Ein Kandidat gilt als aktuell anwesender bekannter Lurker, wenn im aktiven Stream:

- `seen_via_chatters_api = true`
- `messages = 0`
- `last_seen_at` höchstens 5 Minuten alt ist

Zusätzlich muss die Historie auf demselben Kanal erfüllen:

- mindestens 3 frühere Lurk-Sessions auf beendeten Streams
- mindestens 240 Minuten konservative Watchtime-Schätzung

Die Watchtime-Schätzung pro Session ist:

- `last_seen_at - first_message_at`

Es werden nur frühere Sessions summiert, in denen der Viewer als Lurker erkannt wurde.

## Versandregeln

- Sortierung nach geschätzter Lurk-Watchtime absteigend
- maximal 2 Usernamen pro Reminder
- pro Live-Session wird derselbe Viewer nur einmal direkt erwähnt
- wenn keine neuen Kandidaten übrig sind, wird der Zyklus übersprungen
- Lurker Steuer und bestehende Promo-/Discord-Nachrichten teilen sich denselben 60-Minuten-Cooldown

## Reminder-Copy

- Ein Name: `Hey @xy, schön dass du da bist! Vergiss nicht, deine Lurker Steuer zu zahlen: Belohnung 'Lurker Steuer' einlösen.`
- Zwei Namen: beide Erwähnungen in einem Satz, `Hey @a und @b, schön dass ihr da seid! Vergesst nicht, eure Lurker Steuer zu zahlen: Belohnung 'Lurker Steuer' einlösen.`
- Dank: `@xy hat die Lurker Steuer bezahlt. Vorbildlich, danke!`
- echte Umlaute, keine Gedankenstriche, keine Aussage über Punktestände oder Kosten

## Annahmen

- der Bot nutzt keine exakte Channel-Points-Balance, sondern nennt nur die Belohnung zum Einlösen
- die Direkt-Erwähnungs-Dedupe und die Dank-Dedupe leben nur im Runtime-State pro Live-Session; ein Bot-Neustart kann diese Session-Dedupe verlieren

## Chat-Command

Der Broadcaster kann das Feature im Chat deaktivieren:

- `!lurkersteuer_off`
- Alias: `!lurkersteuer_aus`
- Alias: `!lurker_tax_off`

Die Reaktivierung läuft in V1 über den Abo-Bereich.
