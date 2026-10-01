---
title: Was macht der Bot eigentlich?
namespace: bot
category: faq
audience: streamer
last_updated: 2026-10-02
source: manual
tip_eligible: false
---
### Welche Aufgaben übernimmt der Bot?

Der Bot prüft beim Ende eines Deadlock-Streams, ob er die Zuschauer an einen geeigneten Live-Partner weiterleiten kann. Er moderiert Werbe- und Scam-Nachrichten, erfasst Stream-Zahlen und sendet bei erfüllten Voraussetzungen Community-Hinweise. Im Dashboard stellst du Kanalschalter und optionale Funktionen ein.

KI-Auswertungen, werbefreier Chat, Raid-Vorrang und Lurker-Erinnerungen gehören zu Netzwerk Plus.

[Dashboard öffnen](https://deutsche-deadlock-community.de/twitch/auth/login?next=%2Ftwitch%2Fdashboard-v2)

### Wie funktioniert Uplink?

Uplink nimmt deinen OBS-Stream entgegen und schickt ihn an die verbundenen Plattformen. Start und Stopp machst du in OBS. Die Uplink-Hilfe erklärt die RTMPS-Verbindung, Ausgabe und Tonspuren.

[Uplink-Hilfe](https://deutsche-deadlock-community.de/twitch/dashboard-v2/uplink/index.html)

### Moderiert der Bot meinen Chat?

Der Bot prüft Nachrichten auf Werbe- und Scam-Signale. Die Moderation arbeitet unabhängig von der Spielkategorie. Sie braucht die erforderlichen Twitch-Rechte und die Moderator-Rolle. Regeln, gelernte Phrasen und bei Bedarf eine KI-Bewertung helfen bei der Erkennung.

Fehlentscheidungen sind möglich. Broadcaster und Mods können den letzten gespeicherten Auto-Ban mit `!unban` oder `!uban` zurücknehmen. `!explain` erklärt einen vorhandenen Scam-Befund.

### Welche Rechte braucht der Bot?

Twitch zeigt dir beim Verbinden die angeforderten Rechte. Auto-Raids, Clips und Chatter-Auswertungen benötigen die passenden Freigaben. Die Moderator-Rolle erlaubt konkrete Chat-Aktionen. Im Dashboard prüfst du, ob die Verbindung und die Rechte für eine Funktion vorhanden sind.

### Was passiert, wenn ich kein Deadlock mehr streame?

Nach zwei Monaten ohne Deadlock gibt der Bot seine Moderator-Rolle ab. Partnerschaft und Einstellungen bleiben erhalten. Für die automatische Rückkehr bei einem neuen Deadlock-Stream braucht er eine gültige Twitch-Verbindung. Falls sie fehlt, verbinde deinen Kanal neu. Möchtest du ganz aufhören, trenne den Bot in den Bot-Einstellungen.

[Bot-Einstellungen](https://deutsche-deadlock-community.de/twitch/verwaltung#bot)

### Was passiert, wenn ich offline bin?

Werbung und Lurker-Erinnerungen setzen einen laufenden Stream voraus. Einige Chat-Befehle und die Moderation sind auch offline verfügbar. Beim Stream-Ende prüft der Bot die Auto-Raid-Voraussetzungen. Die erfassten Analytics bleiben entsprechend deinem Zugang im Dashboard abrufbar.

### Wo sehe ich Aktionen und Einstellungen?

Bot-Nachrichten erscheinen unter dem Bot-Account im Chat. Im Dashboard findest du Aktivitäten und Einstellungen. Netzwerk Plus enthält den werbefreien Chat und unterbindet die Community-Werbung des Bots.
