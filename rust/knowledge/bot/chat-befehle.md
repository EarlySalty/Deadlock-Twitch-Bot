---
title: Twitch-Chat-Befehle
namespace: bot
category: faq
audience: streamer
last_updated: 2026-09-09
source: rust/crates/tb-chat/src/catalog.rs
tip_eligible: false
---
Der Twitch-Bot läuft vor allem automatisch. Befehle brauchst du, wenn du im Chat etwas auslösen oder nachschauen willst.

### Wo sehe ich, was der Bot kann?

`!commands` schickt einen Link zur Befehlsübersicht. `!help <thema>` verlinkt die passende Hilfe, zum Beispiel `!help raid`.

### Warum antwortet der Bot manchmal nicht auf `!rank`?

Die acht Spielstatistikbefehle lassen sich im Dashboard einzeln abschalten. Sie zeigen die Daten des Streamers, wenn dessen Steam-Account verknüpft ist. Sie funktionieren auch offline und in anderen Kategorien. Außerdem gelten die normale Chatmoderation und die Teilnahme des Kanals am Bot.

### Welche Befehle können normale Zuschauer nutzen?

- `!watchtime` zeigt deine hier erfasste Zuschauerzeit und den Anteil im laufenden Stream. Gezählt werden erfasste Anwesenheiten, auch mit Pausen dazwischen. Die Zahl ist keine lückenlose Messung deiner tatsächlichen Sehzeit.
- `!rank`, `!wins`, `!winrate`, `!mmr`, `!live`, `!lastmatch`, `!streak` und `!mostplayed` zeigen Deadlock-Statistiken des Streamers. `!live` meint ein laufendes Spiel, nicht den Twitch-Livestatus.
- `!clip` erstellt einen Clip aus dem laufenden Stream. Der Kanalschalter muss an sein und die Twitch-Verbindung passen. Die Kategorie ist egal.
- `!discord`, `!dldc` und `!dlde` zeigen den hinterlegten Discord-Link. `!invite` zeigt einen Einladungslink. Diese Befehle funktionieren auch offline.
- `!sub` zeigt den Abo-Link dieses Kanals. `!sub erinnerung an/aus/status` verwaltet deine freiwillige Abo-Erinnerung.
- `!lurk` sagt dem Chat, dass du still weiterschaust, sofern der Kanal den Befehl eingeschaltet hat.
- `!commands` verlinkt die Befehlsübersicht, `!dashboard` öffnet dein Twitch-Dashboard, `!help <thema>` die passende Hilfe und `!ping` prüft, ob der Bot antwortet.
- `!raid_status` und `!raid_history` zeigen den Raidstatus und die letzten Raids.

Kurze Wiederholungssperren verhindern doppelte Antworten oder Aktionen.

### Welche Befehle sind für Broadcaster und Mods gedacht?

- `!raid` oder `!traid` startet einen manuellen Raid zu einem passenden Deadlock-Streamer. Das geht auch noch kurz nach dem Stream. Die Raidprüfung meldet, wenn die Voraussetzungen fehlen.
- `!uban` oder `!unban` nimmt den letzten gespeicherten Auto-Ban zurück.
- `!explain` erklärt einen vorhandenen Scam-Befund.
- `!silentban` schaltet Chat-Hinweise zu Auto-Bans um. `!silentraid` schaltet Raid-Hinweise um. Die Aktionen selbst laufen weiter.
- `!title <stichwörter>` oder `!titel <stichwörter>` schlägt einen Stream-Titel vor, auch offline. Der Befehl setzt den Twitch-Titel nicht automatisch. Ein vorhandener Rang und mit `--live` verfügbare Spieldaten können den Vorschlag ergänzen; sie sind keine Voraussetzung.
- `!lurkersteuer_off` schaltet die Lurker-Erinnerung ab. Das darf nur der Broadcaster und nur bei einem Plan mit dieser Funktion.

### Welche Engagement-Befehle gibt es?

Mit `!engagement_ignore_me` nimmst du dich aus den Engagement-Antworten des Bots heraus. `!engagement_remember_me` hebt das wieder auf. Normale Chat- und Statistikaufzeichnungen werden dadurch nicht abgeschaltet. `!engagement_status` zeigt den Kanalstatus; Mods und Broadcaster können die Funktion mit `!engagement_on` und `!engagement_off` steuern.

### Was sollte ich nicht erwarten?

Der Twitch-Bot ist kein frei konfigurierbarer Nightbot-Ersatz. Eigene Fun-Commands und interne Admin-Aktionen gehören nicht zu dieser Befehlsliste. Einstellungen findest du im Dashboard.
