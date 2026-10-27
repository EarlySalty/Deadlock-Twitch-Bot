# Live-Stichprobe und Beleggrenzen

7. Oktober 2026, etwa 09:22 Uhr Ortszeit. Hauptsession, ausschließlich lesend.

## Erreichbarkeit

- `https://deutsche-deadlock-community.de/streamer/`: HTTP 200, öffentliches serverseitig gerendertes HTML gelesen. Titel: „Deadlock Partner-Netzwerk für Twitch-Streamer · Deutsche Deadlock Community“.
- `/twitch/verwaltung` und `/twitch/hilfe`: ohne Anmeldung Weiterleitung zum Twitch-Login. Keine Anmeldung durchgeführt, kein Konto verwendet und kein Schalter betätigt.
- Browser-Sichtprüfung nicht möglich: Das Vorschauwerkzeug meldet `No preview automation host is available`. Daher keine Aussage über visuelles Rendering, Bedienbarkeit oder angemeldete Live-Zustände.
- Bot und Dashboard melden über systemd `ActiveState=active`. Der tatsächliche Binary-Pfad unter `/proc/<pid>/exe` ist für die Session nicht lesbar (`Permission denied`). Der Produktions-SHA wurde damit nicht verifiziert. Kein Neustart und keine Rechteänderung.

## Ergänzende Nachweise

- Die Landingpage verlinkt als „Bot FAQ“ auf `/twitch/faq`. Diese Route liefert HTTP 200 und den Titel „Empfang · Fragen zum Twitch-Bot“, der eigentliche Inhalt wird per JavaScript geladen. Ohne Browser-Host wurde dieser Inhalt nicht live geprüft. `/twitch/hilfe` ist davon getrennt.
- Der lesbare Release-Zeiger `/opt/deadlock/twitch/current` zeigt auf `releases/e0b0dbaf662d7680c4ceaa210bf15f1443693cd8`, Commit vom 7. Oktober 2026 um 07:09 Uhr. Ein Symlink belegt nicht, welches Binary der laufende Prozess geladen hat.
- Zwischen diesem Zeiger und dem Audit-SHA unterscheiden sich 31 Dateien außerhalb der Task-Akten. Die Änderungen betreffen überwiegend Social Media und TikTok, außerdem zentrale Start-/Routing-Dateien und das Wörterbuch. Deshalb Social-Media-Befunde besonders ausdrücklich als Prüfung des Repository-Stands behandeln, nicht als Produktionsbefund.

## Tatsächlich ausgelieferte öffentliche Texte

Die folgenden Wortlaute wurden aus dem HTML der öffentlichen Streamer-Landingpage extrahiert, nicht nur aus einer Quelldatei:

1. Hero: „Der Bot ist nur der Schlüssel. Sobald du ihn aktivierst, bist du Partner im deutschen Deadlock-Netzwerk. Gehst du offline, reicht das Netzwerk deine Zuschauer an den nächsten deutschen Deadlock-Stream weiter.“
2. Einstieg: „Dein Kanal wird Teil der deutschen Deadlock-Community. Der Bot managt ihn ab dann komplett.“
3. Erläuterung: „Raids, Live-Ankündigung, Chat-Schutz, Auswertung. Du richtest nichts ein.“
4. Raid-Abschnitt: „Der Auto-Raid hält die Deadlock-Community in Bewegung. Endet ein Partner-Stream nach einer Deadlock-Session, sucht das Netzwerk automatisch nach einem passenden Live-Partner und leitet die Community direkt weiter.“
5. Folgesatz: „Wer lieber selbst entscheidet, kann natürlich weiterhin manuell raiden.“
6. Vorteilskarte: „Endet dein Stream, landen deine Zuschauer beim passenden Live-Partner. Gehen andere offline, landen ihre bei dir.“
7. Schutzkarte: „Scam- und Spam-Konten fliegen aus deinem Chat, bevor jemand sie sieht. Dazu die Ban-Liste des ganzen Netzwerks, alles pro Kanal abschaltbar.“
8. Sicherheitsabschnitt, in einer zweiten HTTP-Stichprobe bestätigt: „Keine Broadcaster-Rechte“, „Der Bot moderiert, mehr nicht. Streamtitel, Kategorie oder Kanal-Einstellungen kann er technisch gar nicht anfassen.“
9. Raid-Erklärung: „Volle Kontrolle“ und „Automatisierung, wenn sie hilft; manuelle Raids bleiben jederzeit Teil eures eigenen Ablaufs.“

## Aussagekraft

Die öffentliche Seite vermischt ein starkes Versprechen vollständiger Kanalverwaltung mit engeren Erläuterungen einzelner Automatiken. Die Raid-Karte lässt die Deadlock-Bedingung weg, die im längeren Raid-Abschnitt genannt wird. Das sind belegte Kommunikationsunterschiede. Ob und wann die Mechanik die Versprechen einlöst, wird separat anhand des fixierten Quellstands geprüft.

Der Audit-Snapshot ist `origin/main` bei `9315b3cf7e4a6feeffc32b603db26eca78b03a2c`. Code-Befunde sind nicht automatisch Live-Befunde. Das HTML belegt ausgelieferte Texte, aber weder Vollständigkeit der nachgeladenen Inhalte noch die Funktionswirkung eines Schalters.
