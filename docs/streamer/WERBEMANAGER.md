# Werbemanager (Twitch-Werbung)

Stand: `2026-09-18`

Der Werbemanager wählt den klugen Zeitpunkt für die Twitch-Werbung deines Streams. Er ersetzt nicht den Twitch-Werbungs-Manager, sondern legt die Werbung in ruhige Momente. Du findest ihn im Dashboard unter Verwaltung, Tab Werbung. Er regelt nicht die Chat-Einladung des Bots; dafür gibt es die FAQ zur Chat-Werbung im Wissensbestand.

## So arbeitet er

Der Bot erkennt selbst, woher die Werbemenge kommt:

- Hast du bei Twitch einen Werbeplan aktiv, ist Twitch die Quelle. Der Bot legt nichts obendrauf, sondern bewegt die geplante Werbung: er zieht sie in ein gutes Fenster vor oder schiebt sie mit einer Pause aus einem schlechten Moment heraus.
- Hast du keinen Twitch-Plan, verteilt der Bot dein eigenes Budget selbst: kurze Blöcke von 30 Sekunden, gleichmäßig über die Stunde.

Gute Fenster sind die Queue, das Menü und die erste Minute eines Matches. Aus dem Match ab Minute 1 hält er Werbung heraus, ebenso nach einem Raid und kurz nach einem neuen Erstchatter. Nach einem Match wartet er eine Minute; schreibt der Chat in dieser Minute, verschiebt er, bleibt es ruhig, schaltet er.

## Zwei Strategien

- Nur Twitch-Pausen nutzen: Der Bot verschiebt die geplante Twitch-Werbung mit den vorhandenen Pausen, startet aber nie selbst welche.
- Match schützen und Queue nutzen: Der Bot zieht vor, verschiebt und startet in guten Fenstern, je nachdem, was der Plan gerade hergibt.

Ist der Schalter aus, liest der Bot nur mit und zeigt den Status, er greift nicht ein.

## Budget

Ohne Twitch-Plan stellst du die Werbeminuten pro Stunde ein (1 bis 8, Standard 3). Daraus macht der Bot gleichmäßige Blöcke, zum Beispiel 3 Minuten als sechs Blöcke von 30 Sekunden, etwa alle 10 Minuten. Passt eine sehr dichte Menge nicht in 30-Sekunden-Blöcke mit dem Twitch-Mindestabstand, wachsen die Blöcke auf 60 Sekunden, damit dein Budget nicht verfehlt wird.

Läuft bei dir ein Twitch-Plan, entscheidet allein Twitch über die Menge. Das Budgetfeld ist dann nur Anzeige.

## Verlauf

Jede Aktion und jede Verschiebung steht mit Uhrzeit und Grund im Verlauf, dazu die Bilanz des Streams: gelaufene Blöcke, davon in einem guten Fenster, genutztes Budget und die Zahl der Verschiebungen. Der Verlauf hält 30 Tage.

## Voraussetzung für die Queue-Steuerung

Hinterlege deine SteamID64 im Dashboard unter Verwaltung, Tab Bot & Schutz, Abschnitt KI-Engagement. Der Steam-Bot liest damit deinen Match-Status: im Match, in der Queue oder im Menü, nicht im Spiel. Der Status gilt nur frisch (rund drei Minuten). Fehlt die Anbindung oder ist der Status zu alt, nutzt der Bot ruhige Chat-Phasen und macht nichts Unerwartetes.

## Grenzen

- Wie viel Werbung insgesamt läuft, bleibt deine Twitch-Einstellung. Der Bot kann sie lesen, pausieren und Werbung starten, aber die Menge bei Twitch nicht ändern.
- Twitch erlaubt es nicht, geplante Werbung ganz abzuschalten. Ohne verfügbare Pausen läuft sie, auch im Match. Der Bot hält sie so lange wie möglich heraus.
- Der Startschutz verhindert Werbung in den ersten Minuten nach Streamstart.
- Abonnenten und Turbo-Nutzer sehen keine Werbung.

## Manuelle Aktionen

Pause und Werbung lassen sich im Dashboard auch von Hand anstoßen. Beides wirkt nur während eines laufenden Streams und braucht die passenden Twitch-Berechtigungen, die der Bot beim Verbinden anfragt.
