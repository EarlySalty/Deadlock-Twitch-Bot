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

Ohne Twitch-Plan stellst du die Werbeminuten pro Stunde ein (1 bis 8, Standard 3). Daraus macht der Bot gleichmäßige Blöcke. 3 Minuten werden sechs Blöcke von 30 Sekunden, etwa alle 10 Minuten. Nach jeder Werbung gilt die Sperrzeit von Twitch, üblich 8 Minuten. Passt die Menge nicht in 30-Sekunden-Blöcke mit diesem Abstand, wachsen die Blöcke auf 60 Sekunden. Mit der üblichen Sperrzeit von 8 Minuten passen bei 8 Minuten Budget 7 Blöcke in die Stunde, weil der Abstand den achten Block nicht mehr zulässt.

Läuft bei dir ein Twitch-Plan, entscheidet allein Twitch über die Menge. Das Budgetfeld ist dann nur Anzeige.

Der Mindestabstand in den Feineinstellungen gilt für Pausen und für Werbung, die du selbst startest. Die automatische Verteilung nach deinem Budget richtet sich nach Budget und Twitch-Sperrzeit, nicht nach diesem Abstand.

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
