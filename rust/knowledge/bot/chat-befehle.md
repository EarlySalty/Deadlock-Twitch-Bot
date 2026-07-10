---
title: Twitch-Chat-Befehle
namespace: bot
category: faq
audience: streamer
last_updated: 2026-07-10
source: rust/crates/tb-chat/src/catalog.rs
tip_eligible: false
---
Der Twitch-Bot läuft vor allem automatisch. Befehle brauchst du nur, wenn du im Chat bewusst etwas auslösen oder nachschauen willst.

### Wo sehe ich, was der Bot kann?

`!commands` schreibt die Befehle direkt in den Chat, dazu einen Link zur vollen Übersicht. `!help <thema>` erklärt ein einzelnes Feature, zum Beispiel `!help raid`.

### Warum antwortet der Bot manchmal nicht auf `!rank`?

Die Deadlock-Befehle laufen nur, solange dein Kanal auch Deadlock streamt. Steht die Kategorie auf Just Chatting oder ist der Stream vorbei, bleibt der Bot bei diesen Befehlen still. Das betrifft `!rank`, `!wins`, `!winrate`, `!mmr`, `!live`, `!lastmatch`, `!streak`, `!mostplayed`, außerdem `!clip` und die Einladungslinks.

### Welche Befehle können normale Zuschauer nutzen?

Nur bei laufendem Deadlock:

- `!rank`, `!wins`, `!winrate`, `!mmr`, `!live`, `!lastmatch`, `!streak` und `!mostplayed` zeigen Deadlock-Stats des Streamers, wenn dessen Steam-Account verknüpft ist.
- `!clip` erstellt einen Clip aus dem aktuellen Stream, wenn die Twitch-Autorisierung passt.
- `!discord`, `!invite`, `!dldc` oder `!dlde` posten den Einladungslink zur Deutschen Deadlock Community.

Immer verfügbar:

- `!commands` zeigt die Befehlsliste.
- `!help <thema>` zeigt Hilfe zu einem Thema.
- `!ping` prüft, ob der Bot antwortet.

### Welche Befehle sind für Broadcaster und Mods gedacht?

Diese Befehle hängen nicht an der Kategorie, sie funktionieren also auch nach Stream-Ende.

- `!raid` oder `!traid` startet einen manuellen Raid zu einem passenden Deadlock-Streamer. Das geht bewusst auch noch kurz nach dem Stream, denn genau dann raidet man. Ob dein letzter Stream Deadlock war, prüft der Raid selbst und sagt es dir.
- `!raid_status` zeigt Auto-Raid-Status und grobe Raid-Statistik.
- `!raid_history` zeigt die letzten Raids.
- `!uban` oder `!unban` nimmt den letzten Auto-Ban zurück.
- `!explain` erklärt, warum der Bot jemanden als Scam eingestuft hat.
- `!silentban` schaltet Chat-Hinweise zu Auto-Bans um.
- `!silentraid` schaltet Chat-Hinweise zu Raids um.
- `!title <keywords>` schlägt einen Stream-Titel vor. Sinnvollerweise vor dem Stream, deshalb läuft er auch offline.
- `!lurkersteuer_off` deaktiviert die Lurker-Erinnerung, wenn dein Plan das Feature hat.

### Welche Engagement-Befehle gibt es?

Der Bot kann für Community-Auswertungen merken, welche Zuschauer wiederkehren. Wenn du das nicht willst, nutze `!engagement_ignore_me`. Mit `!engagement_remember_me` kannst du später wieder teilnehmen. Beides geht jederzeit, unabhängig davon, was gerade gestreamt wird. Mods und Broadcaster können das Engagement-Tracking mit `!engagement_status`, `!engagement_on` und `!engagement_off` für den Kanal prüfen oder steuern.

### Was sollte ich nicht erwarten?

Der Twitch-Bot ist kein frei konfigurierbarer Nightbot-Ersatz. Eigene Fun-Commands, Filterlisten oder interne Admin-Aktionen laufen nicht über diese öffentliche Befehlsliste. Das meiste wird im Dashboard eingestellt oder passiert automatisch im Hintergrund.
