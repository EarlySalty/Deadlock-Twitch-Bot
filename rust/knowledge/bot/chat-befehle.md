---
title: Twitch-Chat-Befehle
namespace: bot
category: faq
audience: streamer
last_updated: 2026-07-08
source: rust/crates/tb-chat/src/catalog.rs
tip_eligible: false
---
Der Twitch-Bot läuft vor allem automatisch. Befehle brauchst du nur, wenn du im Chat bewusst etwas auslösen oder nachschauen willst.

### Welche Befehle können normale Zuschauer nutzen?

- `!commands` zeigt den Link zur Befehlsübersicht.
- `!help <thema>` zeigt Hilfe zu einem Thema, zum Beispiel `!help raid`.
- `!ping` prüft, ob der Bot antwortet.
- `!clip` erstellt einen Clip aus dem aktuellen Stream, wenn die Twitch-Autorisierung passt.
- `!invite`, `!dldc` oder `!dlde` posten den Einladungslink zur Deutschen Deadlock Community.
- `!rank`, `!wins`, `!winrate`, `!mmr`, `!live`, `!lastmatch`, `!streak` und `!mostplayed` zeigen Deadlock-Stats des Streamers, wenn dessen Steam-Account verknüpft ist.

### Welche Befehle sind für Broadcaster und Mods gedacht?

- `!raid` oder `!traid` startet einen manuellen Raid zu einem passenden Deadlock-Streamer.
- `!raid_status` zeigt Auto-Raid-Status und grobe Raid-Statistik.
- `!raid_history` zeigt die letzten Raids.
- `!uban` oder `!unban` nimmt den letzten Auto-Ban zurück.
- `!silentban` schaltet Chat-Hinweise zu Auto-Bans um.
- `!silentraid` schaltet Chat-Hinweise zu Raids um.
- `!title <keywords>` schlägt einen Stream-Titel vor.
- `!lurkersteuer_off` deaktiviert die Lurker-Erinnerung, wenn dein Plan das Feature hat.

### Welche Engagement-Befehle gibt es?

Der Bot kann für Community-Auswertungen merken, welche Zuschauer wiederkehren. Wenn du das nicht willst, nutze `!engagement_ignore_me`. Mit `!engagement_remember_me` kannst du später wieder teilnehmen. Mods und Broadcaster können das Engagement-Tracking mit `!engagement_status`, `!engagement_on` und `!engagement_off` für den Kanal prüfen oder steuern.

### Was sollte ich nicht erwarten?

Der Twitch-Bot ist kein frei konfigurierbarer Nightbot-Ersatz. Eigene Fun-Commands, Filterlisten oder interne Admin-Aktionen laufen nicht über diese öffentliche Befehlsliste. Das meiste wird im Dashboard eingestellt oder passiert automatisch im Hintergrund.
