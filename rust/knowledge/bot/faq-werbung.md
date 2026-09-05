---
title: Chat-Werbung des Bots
namespace: bot
category: faq
audience: streamer
last_updated: 2026-09-05
source: manual
tip_eligible: false
---
Was der Bot in deinen Chat schickt, wann, und wie du das komplett abstellst.

### Welche Werbung schickt der Bot in meinen Chat?

Zwei Dinge, beide nur rund um den Community-Discord, keine externen Sponsoren, keine fremden Produkte, kein Spam. Erstens antwortet der Bot einem Zuschauer, der gerade eine passende Situation schreibt (keine Mitspieler, findet Deadlock zu unpopulär, alles zu tryhard, Solo-Queue-Frust, neu im Spiel, sucht Hilfe). Die Antwort geht zuerst auf das Gesagte ein und erwähnt danach höchstens in einem Satz die Community. Zweitens gibt es die periodische Einladung mit dem Discord-Link.

- Anlass-Antworten sind frei geschrieben, keine fertigen Standard-Sprüche mehr.
- In der Anlass-Antwort steht nie ein Link und kein "komm auf Discord".
- Den Discord-Link bekommt ein Zuschauer nur in der periodischen Einladung oder wenn er selbst nach dem Discord fragt (`!discord`, `!invite`).
- Keine Werbung für externe Produkte oder Drittanbieter.
- Den Text der periodischen Einladung kannst du im Dashboard durch deinen eigenen ersetzen.

[Dashboard öffnen](https://deutsche-deadlock-community.de/twitch/auth/login?next=%2Ftwitch%2Fdashboard-v2)

### Wann genau wird das gepostet?

Beides greift nur, wenn dein Stream läuft. Die Anlass-Antwort kommt kurz nachdem ein Zuschauer eine passende Situation schreibt, spricht die Person mit ihrem Namen an und ist streng gedeckelt: pro Zuschauer höchstens einmal in sieben Tagen und pro Stream nur wenige Antworten mit Abstand. Die periodische Einladung braucht eine gewisse Chat-Aktivität und hat eigene Cooldowns, damit nichts spammt.

- Die Anlass-Antwort trifft nur echte Situationen; ohne passenden Anlass schweigt der Bot.
- Cooldowns und Limits verhindern, dass dieselben Zuschauer wiederholt angeschrieben werden.
- Bei aktiven Sonder-Events kann der Bot in der periodischen Einladung stattdessen einen Aktions-Text einblenden.

### Wie schalte ich die Chat-Werbung komplett ab?

Mit dem Werbefrei-Plan (3,99 €/Monat) oder einem der Bundles, die ihn enthalten. Sobald der Plan aktiv ist, sendet der Bot in deinem Chat keinerlei Werbung mehr — auch nicht, wenn andere Trigger eigentlich greifen würden.

- Werbefrei: 3,99 €/Monat, einziger Effekt ist Werbung-aus.
- Werbefrei + Raid Boost (Combo): 5,99 €/Monat.
- Großes Bundle (Erweitert + Raid Boost + Werbefrei): 11,49 €/Monat.
- Plan im Dashboard buchen, Effekt greift sofort.

[Pläne ansehen](https://deutsche-deadlock-community.de/twitch/auth/login?next=%2Ftwitch%2Fabbo)

### Gilt 'Werbefrei' auch bei Sonder-Events vom Admin?

Ja. Wenn ein Admin global einen Aktions-Text aktiviert (z. B. zu einem Community-Event), gilt das ausdrücklich nicht für Streamer mit Werbefrei-Plan. Der Plan überschreibt jeden globalen Werbe-Override — komplett kein Bot-Werbungstext in deinem Chat, ohne Ausnahme.

- Werbefrei-Streamer bekommen auch bei aktivem globalem Sonder-Text nichts gesendet.
- Die Sperre greift in jedem Trigger-Pfad — Chat-Aktivität, Viewer-Anstieg oder Zeitplan.
- Du kannst dich darauf verlassen, dass 'Werbefrei' wirklich werbefrei ist.

### Kann ich nur den Werbe-Text anpassen, ohne Werbefrei zu buchen?

Ja. Im Dashboard kannst du den Text der periodischen Einladung durch einen eigenen ersetzen. Dann postet der Bot in der periodischen Einladung deinen Text statt des frei geschriebenen. Das ist kostenlos und für alle Pläne verfügbar. Den Discord-Link kannst du als Platzhalter einbauen. Die Anlass-Antworten auf einzelne Zuschauer bleiben davon unberührt und tragen weiterhin keinen Link.

- Eigener Werbe-Text im Dashboard hinterlegbar.
- Platzhalter {invite} wird beim Senden durch den Discord-Link ersetzt.
- Wenn ein Admin gerade einen Aktions-Text aktiviert hat, hat dieser kurzzeitig Vorrang vor deinem eigenen.

[Dashboard öffnen](https://deutsche-deadlock-community.de/twitch/auth/login?next=%2Ftwitch%2Fdashboard-v2)
