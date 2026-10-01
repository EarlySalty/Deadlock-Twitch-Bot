# Werbemanager (Twitch-Werbung)

Stand des PR: `2026-09-24`. Noch nicht produktiv ausgeliefert.

Der Werbemanager wählt den klugen Zeitpunkt für die Twitch-Werbung deines Streams. Er ersetzt nicht den Twitch-Werbungs-Manager, sondern legt die Werbung in ruhige Momente. Du findest ihn im Dashboard unter Verwaltung, Tab Werbung. Er regelt nicht die Chat-Einladung des Bots; dafür gibt es die FAQ zur Chat-Werbung im Wissensbestand.

## So arbeitet er

Der Bot erkennt selbst, woher die Werbemenge kommt:

- Hast du bei Twitch einen Werbeplan aktiv, ist Twitch die Quelle. Der Bot legt nichts obendrauf, sondern bewegt die geplante Werbung: er zieht sie in ein gutes Fenster vor oder schiebt sie mit einer Pause aus einem schlechten Moment heraus.
- Hast du keinen Twitch-Plan, verteilt der Bot dein eigenes Budget selbst: kurze Blöcke von 30 Sekunden, gleichmäßig über die Stunde.

Gute Fenster sind eine bestätigte Queue oder das Menü. Sobald ein laufendes Match erkannt ist, startet der Bot keinen eigenen Werbeblock und zieht keinen vor. Bei einer anstehenden Twitch-Werbung nutzt er verfügbare Pausen. Ohne frischen Matchstatus bleiben eigene Werbestarts gesperrt; Chat-Ruhe ersetzt diesen Nachweis nicht. Nach einem Match wartet er eine Minute und berücksichtigt danach die Chat-Aktivität.

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

Das über die Kontoverknüpfung gewählte Steam-Konto hat Vorrang. Ohne direkte Zuordnung kann der Bot die bestehende SteamID64 unter Verwaltung, Bot & Schutz nutzen. Eine Discord-Verknüpfung allein reicht für die Queue-Steuerung nicht aus. Eine getrennte direkte Verknüpfung wird respektiert. Der Matchstatus kommt über die Steam-Bot-Schnittstelle mit dem ursprünglichen Messzeitpunkt, nicht aus der Twitch-Datenbank. Ältere Daten als drei Minuten, Quellenfehler und unvollständige Antworten bestätigen kein Werbefenster. Das Dashboard zeigt fehlende oder veraltete Daten gesondert an.

## Grenzen

- Wie viel Werbung insgesamt läuft, bleibt deine Twitch-Einstellung. Der Bot kann sie lesen, pausieren und Werbung starten, aber die Menge bei Twitch nicht ändern.
- Der Bot kann einen aktiven Twitch-Werbeplan über diese Schnittstelle nicht abschalten. Ein Snooze verschiebt die nächste Werbung um fünf Minuten. Ohne verfügbare Pausen kann Twitch Werbung auch während eines Matches ausspielen. Werbung zu Queue-Beginn garantiert daher kein vollständig werbefreies Match.
- Der Worker prüft im 25-Sekunden-Takt. Der effektive Vorlauf für Verschiebungen beträgt mindestens 60 Sekunden, auch bei einer kürzeren Einstellung.
- Der Startschutz verhindert Werbung in den ersten Minuten nach Streamstart.
- Abonnenten und Turbo-Nutzer sehen keine Werbung.

## Manuelle Aktionen

Pause und Werbung lassen sich im Dashboard auch von Hand anstoßen. Beides wirkt nur während eines laufenden Streams und braucht die passenden Twitch-Berechtigungen, die der Bot beim Verbinden anfragt.
