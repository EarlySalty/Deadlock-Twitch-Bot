---
title: Twitch-Chat-Befehle
namespace: bot
category: faq
audience: streamer
last_updated: 2026-09-18
source: rust/crates/tb-chat/src/catalog.rs
tip_eligible: false
---
Der Twitch-Bot läuft vor allem automatisch. Befehle brauchst du, wenn du im Chat etwas auslösen oder nachschauen willst.

### Wo sehe ich, was der Bot kann?

`!commands` schickt einen Link zur Befehlsübersicht. `!help <thema>` verlinkt die passende Hilfe, zum Beispiel `!help raid`.

### Warum antwortet der Bot manchmal nicht auf `!rank`?

Die acht Spielstatistikbefehle lassen sich im Dashboard einzeln abschalten. Ohne Ziel zeigen sie die Daten des Streamers, mit @user die Daten der genannten Person. Dafür wird deren Steam-Verknüpfung verwendet; !rank bietet zusätzlich einen öffentlichen API-Fallback. Sie funktionieren auch offline und in anderen Kategorien. Außerdem gelten die normale Chatmoderation und die Teilnahme des Kanals am Bot.

### Twitch und Steam direkt verbinden

`!connect` verlinkt unsere Seite `/twitch/connect`. Dort bestätigst du zuerst dein Twitch-Konto und meldest dich anschließend bei Steam an. Kein Discord-Konto und keine Streamer-Partnerschaft nötig. Gespeichert werden die stabile Twitch-ID und die von Steam bestätigte Steam-ID; ein gleicher Anzeigename ist nicht erforderlich.

Danach nutzt `!rank @deinname` diesen Account über die Deadlock API. Die Verbindung bestätigt den Account, nicht die Verfügbarkeit oder Aktualität von Rangdaten. Ohne @name zeigt `!rank` weiterhin den Rang des Streamers. Weitere Spielstatistiken benötigen vorerst zusätzlich die bestehende Discord-/Steam-Verknüpfung zum selben Steam-Account. Watchtime braucht keine Steam-Verknüpfung.

`!unconnect` (auch `!disconnect`) entfernt deine direkte Steam-Zuordnung und löscht daraus die Steam-ID. Nur deine Twitch-ID bleibt mit einem Abschaltvermerk gespeichert: Die automatische Zuordnung über Discord und Namenssuche bleibt deaktiviert, bis du selbst erneut verbindest. Der Befehl funktioniert ausschließlich für das eigene Konto, niemals mit @user. Auf der Kontoseite gibt es dafür ebenfalls „Verknüpfung entfernen“. Die separate Discord-Verknüpfung und öffentliche Steam-/Deadlock-Daten werden dadurch nicht gelöscht. Ein bereits gestarteter Steam-Rücksprung kann die Trennung nicht rückgängig machen.

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

### Kann ich die Werte einer anderen Person abfragen?

Ja: `!watchtime @username` zeigt die erfasste Zuschauerzeit dieser Person **im aktuellen Kanal**. Ohne Namen zeigt `!watchtime` weiterhin deine eigene Zeit. Ein Twitch-Name ohne `@` funktioniert ebenfalls; pro Aufruf ist genau ein Ziel erlaubt.

Auch `!rank`, `!wins`, `!winrate`, `!mmr`, `!live`, `!lastmatch`, `!streak` und `!mostplayed` akzeptieren `@username`, ebenso ihre Aliase `!climb`, `!last` und `!main`. Ohne Ziel bleiben sie auf den Streamer bezogen. Die Einstellungen des aktuellen Kanals gelten weiterhin.

Eine direkte Verbindung über `!connect` hat Vorrang: `!rank @username` verwendet diesen bestätigten Account über die Deadlock API. Ohne direkte Verbindung nutzt der Befehl zuerst die bestehende Discord-/Steam-Zuordnung und unseren Steam-Bot. Fehlen dort Rangdaten oder die Bot-Freundschaft, wird der öffentliche Rang des bestätigten Accounts über die Deadlock API abgefragt. Gibt es keine Verknüpfung, sucht der Bot nach dem Twitch-Namen auf Steam. Ein eindeutiger exakter Namensfund wird ausdrücklich als **unbestätigte Twitch-Zuordnung** markiert. Bei mehreren oder nur ähnlichen Treffern gibt es Vorschläge, keinen geratenen Rang. Über `!rank steam:<Account-ID>` oder `!rank steam:<SteamID64>` lässt sich ein Steam-Account eindeutig auswählen. Das speichert keine neue Verknüpfung.

API-Ränge werden als Stand des letzten erfassten Ranked-Matches gekennzeichnet. Fehlende Rangdaten sind kein Beweis dafür, dass jemand noch nie gerankt war. Die anderen Spielstatistiken brauchen weiterhin die hinterlegte Steam-Verknüpfung.

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
