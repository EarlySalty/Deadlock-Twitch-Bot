---
title: Statistik-Befehle & Stream-Overlay
namespace: bot
category: faq
audience: streamer
last_updated: 2026-09-18
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

Alle diese Befehle akzeptieren optional `@username`: etwa `!rank @username` oder `!winrate @username`. Ohne Ziel sind die Daten des Streamers gemeint. Bei `!rank me` wird ausdrücklich der Absender im Chat verwendet. `!watchtime @username` zeigt die erfasste Zuschauerzeit dieser Person im aktuellen Twitch-Kanal; ohne Ziel die eigene Zeit.

### Was brauche ich, damit die Statistiken funktionieren?

Für `!rank @deinname` reicht jetzt `!connect`: Auf unserer Website bestätigst du Twitch und Steam. Kein Discord nötig. Wir speichern die bestätigte Steam-ID; der Rang kommt dann aus der Deadlock API, soweit dort Daten verfügbar sind.

`!unconnect` oder `!disconnect` entfernt deine direkte Zuordnung und stoppt die automatische Twitch-zu-Steam-Auflösung. Nur du selbst kannst mit einem neuen `!connect` wieder aktivieren. Alternativ kannst du die Verbindung auf `/twitch/connect` entfernen.

Die anderen Spielstatistikbefehle und das Overlay verwenden weiterhin die bestehende Discord-/Steam-Verknüpfung. Ist zusätzlich eine direkte Verbindung vorhanden, dürfen die Chat-Befehle nicht stillschweigend einen anderen Steam-Account verwenden.

### Woher kommen die Zahlen — und warum ist Winrate „letzte Spiele"?

Die Match-Statistiken kommen aus deiner Match-Historie über unseren Steam-Bot. Bei `!rank` kann die öffentliche Deadlock API einspringen; solche Antworten nennen ausdrücklich den Stand des letzten erfassten Ranked-Matches. Ein Rang aus einem gleichnamigen Steam-Profil beweist nicht, dass dieses Profil zur Twitch-Person gehört. Winrate, Serie und Lieblings-Hero beziehen sich auf ein Fenster deiner jüngsten gewerteten Spiele, nicht auf die gesamte Karriere.

- Ungewertete oder abgebrochene Spiele werden für Winrate und Serie ausgeklammert.
- `!wins` zeigt die Karriere-Siege; eine verlässliche Gesamt-Match-Zahl liefert die Quelle über diesen Weg nicht, daher die bewusste Beschränkung auf „letzte Spiele".
- Der Rang-Trend (`!mmr`) baut sich auf, je länger dein Account verknüpft ist — anfangs steht er auf „stabil", bis sich dein Rang das erste Mal ändert.

### Wie blende ich meine Stats im Stream ein (OBS-Overlay)?

Es gibt eine eigene Overlay-Seite mit Baukasten: Dort stellst du dir dein Overlay zusammen und bekommst eine fertige URL für OBS. Erreichbar über den Eintrag „Stream-Overlay" in der Seitenleiste des Dashboards oder direkt unter der Adresse unten.

1. Öffne die Overlay-Seite: `deutsche-deadlock-community.de/twitch/overlay`.
2. Wähle einen Stil (Dunkel, Hell oder Akzent) und ein Layout (Box-Karte, schlanke Leiste oder „Freie OBS-Leinwand").
3. Schalte ein, was angezeigt werden soll — Rang, Winrate, heutige Bilanz, Serie, K/D, letztes Match, meistgespielter Hero, Match-Verlauf oder Live-Match. Bei Karte und Leiste wählst du zusätzlich die Ecke im Bild.
4. Stell bei Bedarf die Hintergrund-Deckkraft und die Länge des Match-Verlaufs ein.
5. Kopiere die angezeigte Overlay-URL, füge in OBS eine Browser-Quelle hinzu und trage sie ein. Für Karte und Leiste gelten die empfohlenen Größen; bei der freien Leinwand nimmst du deine eingestellte Leinwandgröße.
6. Zieh die Quelle an die gewünschte Stelle — sie aktualisiert sich automatisch.

Für eine frei anpassbare OBS-Leinwand wählst du „Freie OBS-Leinwand". Stell zuerst die Leinwandgröße ein. Danach kannst du die Quellen in der Vorschau per Drag-and-drop verschieben, am Eckgriff in der Größe ändern oder X, Y, Breite und Höhe exakt eintragen. Klicke anschließend auf „Layout speichern" und kopiere die fertige URL in OBS. In OBS verwendest du für die Browser-Quelle dieselbe Breite und Höhe wie für die Leinwand.

- Das Overlay ist transparent und fügt sich in deine Szene ein; in der Live-Vorschau siehst du jede Änderung sofort.
- Rang-Abzeichen und Hero-Bilder sind die offiziellen Deadlock-Spielgrafiken.
- Werte ohne Daten (z. B. die heutige Bilanz vor dem ersten Match) werden einfach ausgeblendet, statt leer dazustehen.
- Du kannst jederzeit umstellen, was angezeigt wird — einfach eine neue URL aus dem Baukasten kopieren.

[Overlay-Baukasten öffnen](https://deutsche-deadlock-community.de/twitch/overlay)

### Twitch und Steam direkt verbinden

Mit `!connect` bekommst du den Link zu unserer Kontoseite. Dort bestätigst du zuerst dein Twitch-Konto und kannst anschließend mehrere Steam-Konten verbinden. Eines davon ist das Standardkonto; auf der Kontoseite kannst du den Standard wechseln oder einzelne Konten entfernen. Kein Discord-Konto und keine Streamer-Partnerschaft sind erforderlich. Die direkte, bestätigte Zuordnung wird für `!rank me` und `!rank @deinname` verwendet, unabhängig von unterschiedlichen Twitch- und Steam-Namen. Der Rang kommt dabei aus der Deadlock API, sofern öffentliche Rangdaten vorhanden sind. Der Login selbst garantiert keine Rangdaten und erzeugt keine Steam-Bot-Freundschaft.

`!unconnect` (auch `!disconnect`) entfernt nur deine eigene direkte Steam-Zuordnung. Die Steam-ID wird aus dieser Zuordnung gelöscht; deine Twitch-ID bleibt mit einem Abschaltvermerk gespeichert, damit Discord- und Namens-Fallback sie nicht automatisch wieder ersetzen. Ein bereits begonnener Steam-Login kann diese Trennung nicht rückgängig machen. Mit einem neu gestarteten `!connect` kannst du wieder verbinden. Auf der Kontoseite gibt es dieselbe Funktion als „Verknüpfung entfernen“. Bestehende Discord-/Steam-Verbindungen werden nicht gelöscht.

Die übrigen Spielstatistikbefehle verwenden weiterhin den bisherigen Discord-/Steam-Datenweg. Bei einer direkten Verbindung zu einem anderen Steam-Konto zeigen sie keine fremden Altdaten; der Bot erklärt die noch fehlende zusätzliche Verbindung. `!watchtime` benötigt kein Steam-Konto.
