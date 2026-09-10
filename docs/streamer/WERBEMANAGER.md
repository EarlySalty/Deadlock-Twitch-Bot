# Werbemanager (Twitch-Werbung)

Stand: `2026-09-10`

Der Werbemanager steuert die Twitch-Werbepausen deines Streams. Er ist im Dashboard unter Verwaltung, Tab Werbung (Experimentell) zu finden. Er regelt nicht die Chat-Einladung des Bots; dafür siehe die FAQ zur Chat-Werbung im Wissensbestand.

## Was er kann

- Werbeplan lesen: Der Bot kennt die nächste von Twitch geplante Werbung, die verfügbaren Verschiebepausen (Snoozes) und die Zeit seit der letzten Werbung.
- Verschieben: Jede der bis zu drei Twitch-Pausen pro Stream schiebt die nächste Werbung um rund fünf Minuten. Neue Pausen kommen zeitgesteuert nach.
- Werbung starten: Der Bot kann die fällige Werbung selbst starten und damit den Moment wählen.
- Drei Strategien:
  - Nur überwachen: Der Bot liest den Plan und tut nichts.
  - Werbung möglichst verschieben: Fällige Werbung wird verschoben, solange Pausen übrig sind.
  - Intelligent steuern: Mit Steam-Anbindung hält der Bot Werbung aus deinen Deadlock-Matches und startet sie in deiner Queue. Ohne frischen Steam-Status nutzt er ruhige Chat-Phasen.

## Voraussetzung für die Queue-Steuerung

Hinterlege deine SteamID64 im Dashboard unter Verwaltung, Tab Bot & Schutz, Abschnitt KI-Engagement. Der Steam-Bot liest damit deinen Match-Status. Er kennt drei Zustände: im Match, in der Queue oder im Menü, nicht im Spiel.

Der Match-Status gilt nur frisch (rund drei Minuten). Ist er zu alt oder fehlt die Steam-Anbindung, fällt der Bot auf die Chat-Ruhe-Logik zurück und es passiert nichts Unerwartetes.

## Grenzen

- Twitch erlaubt die Abschaltung geplanter Werbung nicht. Ohne verfügbare Pausen läuft die geplante Werbung, auch im Match. Der Bot hält sie so lange wie möglich raus und macht danach in deiner Queue weiter.
- Der Startschutz verhindert Werbung in den ersten Minuten nach Streamstart.
- Der Mindestabstand gilt auch für Werbung, die Twitch selbst gestartet hat.

## Manuelle Aktionen

Im Dashboard lassen sich Pause und Werbung auch von Hand anstoßen. Beides wirkt nur während eines laufenden Streams und braucht die passenden Twitch-Berechtigungen, die der Bot beim Verbinden anfragt.
