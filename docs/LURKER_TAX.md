# Lurker Steuer

Die Lurker Steuer ist eine Chat-Automation für bezahlte Pläne. Sie erinnert bekannte aktuell anwesende Lurker sanft im Twitch-Chat, ohne Punktestände oder Reward-Kosten zu behaupten.

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
- Wenn der zentrale Bot-Scope `moderator:read:chatters` fehlt oder noch nicht geladen ist, zeigt die Karte einen Readiness-Hinweis; ohne diesen Zugriff feuert das Feature nicht

## Laufzeitlogik

Die Laufzeit hängt am bestehenden Promo-/Announcement-Loop in [`bot/chat/promos.py`](../bot/chat/promos.py).

Ein Reminder wird nur gesendet, wenn alle Bedingungen erfüllt sind:

- der Stream ist live
- es gibt eine aktive Session
- der Streamer hat einen bezahlten Plan
- `lurker_tax_enabled = true`
- der zentrale Bot-Zugriff fuer `moderator:read:chatters` ist verfuegbar
- es gibt frische Präsenzdaten in `twitch_session_chatters`

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

- der Reminder nennt klar den Feature-Namen `Lurker Steuer`
- die Formulierung bleibt weich und direkt, ohne Shaming oder Druck
- es gibt keine Aussage wie "du hast genug Punkte"
- V1 behauptet weder Reward-Titel noch Reward-Kosten

## V1-Annahmen

- V1 nutzt keine exakte Channel-Points-Balance, weil dafür keine verlässliche Twitch-Datenquelle eingeplant ist
- V1 arbeitet ohne Reward-Titel oder Reward-Kosten; `Lurker Steuer` ist ein generischer user-facing Name
- die Direkt-Erwähnungs-Dedupe lebt nur im Runtime-State pro Live-Session; ein Bot-Neustart kann diese Session-Dedupe verlieren

## Chat-Command

Der Broadcaster kann das Feature im Chat deaktivieren:

- `!lurkersteuer_off`
- Alias: `!lurkersteuer_aus`
- Alias: `!lurker_tax_off`

Die Reaktivierung läuft in V1 über den Abo-Bereich.
