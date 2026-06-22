---
title: Statistik-Befehle & Stream-Overlay
namespace: bot
category: faq
audience: streamer
last_updated: 2026-06-22
source: manual
tip_eligible: false
---
Deine Deadlock-Stats im Chat und als einblendbares OBS-Overlay — welche Befehle es gibt, was sie zeigen und wie du das Overlay einrichtest.

### Welche Statistik-Befehle gibt es im Chat?

Sobald dein Steam-Account verknüpft ist, kennt der Bot diese Befehle im Chat:

- `!rank` — dein aktueller Deadlock-Rang.
- `!wins` — deine Karriere-Siege.
- `!winrate` — Siegquote über deine letzten Spiele (mit Siege/Niederlagen).
- `!lastmatch` — dein letztes Match: Sieg oder Niederlage, gespielter Hero, KDA.
- `!streak` — deine aktuelle Sieges- oder Pechsträhne.
- `!mostplayed` — dein meistgespielter Hero der letzten Spiele.
- `!mmr` (auch `!climb`) — aktueller Rang plus Trend der letzten Tage.
- `!live` — ob du gerade in einem laufenden Deadlock-Match bist (inkl. Hero und Spielminute).

### Was brauche ich, damit die Statistiken funktionieren?

Einen über den Discord verknüpften Steam-Account. Die Verknüpfung richtest du im Verwaltungs-Dashboard im Einrichtungs-Assistenten ein (Schritt „Steam-Account verknüpfen"). Ohne Verknüpfung weisen die Befehle freundlich darauf hin, statt eine Fehlermeldung zu zeigen.

- Die Verknüpfung läuft über Steam-Login (OpenID, kein Passwort) und eine Freundschaftsanfrage des Bots.
- Es gibt bewusst keine Verknüpfungs-Befehle im Chat — alles läuft über das Verwaltungs-Dashboard.

[Verwaltungs-Dashboard öffnen](https://deutsche-deadlock-community.de/twitch/verwaltung)

### Woher kommen die Zahlen — und warum ist Winrate „letzte Spiele"?

Die Zahlen kommen aus deiner echten Match-Historie, die der Bot direkt über den Deadlock-Spiel-Dienst abruft — kein externer Dienst, keine geschätzten Werte. Winrate, Serie und Lieblings-Hero beziehen sich auf ein Fenster deiner jüngsten gewerteten Spiele, nicht auf die gesamte Karriere.

- Ungewertete oder abgebrochene Spiele werden für Winrate und Serie ausgeklammert.
- `!wins` zeigt die Karriere-Siege; eine verlässliche Gesamt-Match-Zahl liefert die Quelle über diesen Weg nicht, daher die bewusste Beschränkung auf „letzte Spiele".
- Der Rang-Trend (`!mmr`) baut sich auf, je länger dein Account verknüpft ist — anfangs steht er auf „stabil", bis sich dein Rang das erste Mal ändert.

### Wie blende ich meine Stats im Stream ein (OBS-Overlay)?

Es gibt eine eigene Overlay-Seite mit Baukasten: Dort stellst du dir dein Overlay zusammen und bekommst eine fertige URL für OBS. Erreichbar über den Eintrag „Stream-Overlay" in der Seitenleiste des Dashboards oder direkt unter der Adresse unten.

1. Öffne die Overlay-Seite: `deutsche-deadlock-community.de/twitch/overlay`.
2. Wähle einen Stil (Dunkel, Hell oder Akzent) und ein Layout (Box-Karte oder schlanke Leiste).
3. Schalte ein, was angezeigt werden soll — Rang, Winrate, heutige Bilanz, Serie, K/D, letztes Match, meistgespielter Hero, Match-Verlauf, Live-Match — und wähle die Ecke im Bild.
4. Stell bei Bedarf die Hintergrund-Deckkraft und die Länge des Match-Verlaufs ein.
5. Kopiere die angezeigte Overlay-URL, füge in OBS eine Browser-Quelle hinzu und trage sie ein (Box 360×200, Leiste 520×90).
6. Zieh die Quelle an die gewünschte Stelle — sie aktualisiert sich automatisch.

- Das Overlay ist transparent und fügt sich in deine Szene ein; in der Live-Vorschau siehst du jede Änderung sofort.
- Rang-Abzeichen und Hero-Bilder sind die offiziellen Deadlock-Spielgrafiken.
- Werte ohne Daten (z. B. die heutige Bilanz vor dem ersten Match) werden einfach ausgeblendet, statt leer dazustehen.
- Du kannst jederzeit umstellen, was angezeigt wird — einfach eine neue URL aus dem Baukasten kopieren.

[Overlay-Baukasten öffnen](https://deutsche-deadlock-community.de/twitch/overlay)
