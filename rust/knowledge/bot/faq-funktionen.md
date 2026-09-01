---
title: Was macht der Bot eigentlich?
namespace: bot
category: faq
audience: streamer
last_updated: 2026-06-22
source: manual
tip_eligible: false
---
Die wichtigsten Aufgaben des Bots auf einen Blick — was er für dich erledigt und was nicht.

### Welche Hauptaufgaben übernimmt der Bot für mich?

Der Bot kümmert sich um fünf Dinge: Er leitet beim Stream-Ende deine Zuschauer automatisch an einen live Deadlock-Partner weiter (Auto-Raid), hält automatisch nervige Werbe-Bots aus deinem Chat (die dir mehr Viewer oder Follower verkaufen wollen), trackt deine Stream-Zahlen für dein Dashboard, schickt bei Bedarf eine dezente Discord-Einladung in deinen Chat und bringt optionale Extras wie Lurker-Erinnerungen oder KI-Stream-Reports mit.

- Auto-Raid läuft ohne Setup — der Bot wählt den passenden Partner aus.
- Die Chat-Moderation gegen diese Werbe-Bots läuft automatisch im Hintergrund, ohne dass du Filterlisten oder Befehle pflegen musst.
- Analytics werden im Hintergrund erfasst, du musst nichts konfigurieren.
- Chat-Werbung kannst du im Dashboard steuern oder mit dem Werbefrei-Plan komplett abschalten.
- Stream-Reports (KI) und Lurker-Tax sind optionale Premium-Features.

[Dashboard öffnen](https://deutsche-deadlock-community.de/twitch/auth/login?next=%2Ftwitch%2Fdashboard-v2)

### Wie funktioniert Uplink?

Uplink nimmt deinen OBS-Stream entgegen und schickt ihn an die verbundenen Plattformen. Start und Stop machst du in OBS. Die [Uplink-Hilfe](https://deutsche-deadlock-community.de/twitch/dashboard-v2/uplink/index.html) erklärt den Ablauf und die Einrichtung.

### Moderiert der Bot auch meinen Chat?

Ja. Der Bot räumt automatisch die nervigen Werbe-Bots aus dem Chat, die dir mehr Viewer oder Follower verkaufen wollen — die kennt jeder Streamer, und sie sehen im Chat einfach mies aus. Das läuft im Hintergrund, ohne dass du Wörter sperren oder Mod-Regeln pflegen musst.

- Erkennt gezielt diese Werbe-Bots, nicht pauschal alles — normale Chatter und Links bleiben unangetastet.
- Anders als klassische Mod-Bots (Nightbot & Co.) musst du keine Befehle oder Filterlisten einrichten.
- Auf Treffsicherheit ausgelegt: ein versehentlicher Bann ist praktisch ausgeschlossen.
- Die Moderation ist aktiv, sobald dein Kanal verbunden ist — unabhängig davon, welches Spiel du gerade streamst.

### Welche Rechte braucht der Bot in meinem Kanal?

Der Bot meldet sich per Twitch-Login an und fordert nur die Rechte an, die seine einzelnen Funktionen brauchen. Auto-Raid, Clips und optionale Chatter-Auswertungen verwenden getrennte, zweckgebundene Twitch-Berechtigungen; die Moderator-Rolle des Bot-Accounts ist für konkrete Chat-Aktionen nötig.

- Du autorisierst den Bot über deinen Twitch-Account — kein extra Passwort.
- Als Mod kann der Bot Ankündigungen senden und erkannte Werbe-Bots entfernen. Ohne diese Rolle funktionieren diese Chat-Aktionen nicht.
- Du kannst die Verbindung jederzeit in deinen Twitch-Einstellungen widerrufen.

[Bot für deinen Kanal aktivieren](https://deutsche-deadlock-community.de/twitch/raid/auth?scope_profile=base&source=website_onboarding&ts=1782086400000)

### Was passiert, wenn ich kein Deadlock mehr streame?

Streamst du zwei Monate lang kein Deadlock, gibt der Bot seine Moderator-Rechte in deinem Kanal von allein ab. Deine Partnerschaft und deine Einstellungen bleiben dabei bestehen. Streamst du wieder Deadlock, moddet er sich automatisch zurück, ohne dass du etwas tun musst.

- Du bekommst eine Discord-Nachricht, wenn der Bot sich entmoddet. Kommt er später zurück, läuft das still im Hintergrund.
- Voraussetzung fürs Zurückkommen ist eine gültige Twitch-Verbindung; ist die abgelaufen, verbinde deinen Kanal einmal neu.
- Willst du dauerhaft aufhören, trenn den Bot lieber ganz: das geht im Dashboard unter Bot-Einstellungen.

[Bot-Einstellungen](https://deutsche-deadlock-community.de/twitch/verwaltung#bot)

### Was passiert, wenn ich offline bin?

Der Bot bleibt erreichbar, schickt aber keine Chat-Aktionen mehr — Werbung, Lurker-Erinnerungen und ähnliches greifen nur, wenn dein Stream läuft. Sobald dein Stream endet, prüft der Bot, ob er deine Zuschauer per Auto-Raid weiterleiten kann.

- Während du offline bist, ruhen alle Chat-Trigger.
- Der Auto-Raid wird genau einmal pro Stream-Ende ausgelöst.
- Deine Analytics-Daten werden weiter im Dashboard zugänglich gemacht.

### Bekomme ich mit, was der Bot in meinem Chat sendet?

Ja — jede vom Bot gesendete Nachricht steht klar erkennbar in deinem Chat mit dem Bot-Account als Absender. Im Dashboard kannst du außerdem die letzten Aktivitäten einsehen und ggf. Werbung deaktivieren oder den Text anpassen.

- Alle Bot-Nachrichten laufen unter einem dedizierten Bot-Account.
- Im Dashboard siehst du jüngste Aktionen und kannst Einstellungen ändern.
- Mit dem Werbefrei-Plan unterbindest du die Discord-Einladungen komplett.

[Dashboard öffnen](https://deutsche-deadlock-community.de/twitch/auth/login?next=%2Ftwitch%2Fdashboard-v2)
