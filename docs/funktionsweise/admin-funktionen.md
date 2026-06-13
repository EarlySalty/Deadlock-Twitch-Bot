# Admin-Funktionen

## Worum es geht

Das Admin-Dashboard ist die Betreiber-Oberfläche des Bots. Hier verwalten die Betreiber das gesamte Partner-Netzwerk: sie nehmen Streamer auf, verifizieren sie, behalten Monitoring und Abrechnung im Blick, steuern globale Einstellungen und greifen bei Bedarf bei der Moderation ein. Es ist klar getrennt vom Streamer-Dashboard: Streamer sehen und steuern dort nur ihren eigenen Kanal, während das Admin-Dashboard den Überblick und die Steuerung über alle Kanäle hinweg bietet.

## Was der Bot tut

Das Admin-Dashboard bündelt mehrere Aufgabenbereiche an einer Stelle:

- **Streamer- und Partnerverwaltung:** Eine Liste aller registrierten Streamer mit ihrem aktuellen Status. Pro Streamer gibt es eine Detailansicht, von der aus sich alle weiteren Aktionen steuern lassen.
- **Monitoring:** Übersicht über den Systemzustand, den Status der Live-Erkennung, allgemeine Datenbank-Kennzahlen und aufgetretene Fehler. So lässt sich auf einen Blick erkennen, ob der Bot rund läuft.
- **Konfiguration:** Zentrale Stellschrauben für das Verhalten des Bots, der Chat-Moderation und der Raid-Funktion.
- **Community-Steuerung:** Moderations-Aktionen im Chat von Partner-Kanälen, Einblick in Engagement und Raid-Aktivität sowie eine Marktübersicht zum Anteil des Partner-Netzwerks an der Deadlock-Kategorie auf Twitch.
- **Billing-Überblick:** Abo-Status der Streamer, Affiliate-/Partnervergütungen und Gutschriften. Inklusive der Möglichkeit, den Abo-Plan eines Streamers manuell zu überschreiben.
- **Inhalte:** Verwaltung von Ankündigungen, Changelog-Einträgen, rechtlichen Seiten und der Roadmap.

## Wann es passiert

- Das Admin-Dashboard ist jederzeit für berechtigte Betreiber erreichbar. Aktionen passieren genau dann, wenn ein Admin sie auslöst — es ist eine manuelle Steuer- und Übersichtsoberfläche, kein automatischer Hintergrundprozess.
- Die meisten Bereiche sind reine Ansichten (Monitoring, Listen, Billing-Überblick). Schreibende Aktionen — etwa einen Streamer verifizieren, einen Plan überschreiben, eine Chat-Aktion senden — finden nur statt, wenn der Admin sie ausdrücklich bestätigt.
- Der Zugang ist auf Betreiber beschränkt. Normale Streamer-Konten haben keinen Zugriff auf diese Oberfläche und können keine Aktionen gegen fremde Kanäle ausführen.

## Was Streamer/Viewer sehen

Für normale Streamer und Viewer ist das Admin-Dashboard unsichtbar — es ist ausschließlich für die Betreiber gedacht und liegt hinter einem geschützten Zugang. Was Streamer indirekt davon merken:

- Wenn ein Admin sie aufnimmt und verifiziert, beginnt der Bot, ihren Kanal zu betreuen (Monitoring, Live-Ankündigungen, Raids, Analytics, je nach Plan).
- Wenn ein Admin ihren Abo-Plan manuell überschreibt — etwa für Kulanz, einen Bonusmonat oder zur Reparatur nach einem Abrechnungsproblem — ändert sich entsprechend ihr Funktionsumfang.
- Wenn ein Admin sie archiviert oder entfernt, hört die Betreuung des Kanals auf.

## Was Streamer einstellen können

Im Admin-Dashboard stellen ausschließlich Betreiber etwas ein, keine einzelnen Streamer. Die wichtigsten Einstell- und Steuermöglichkeiten der Admins:

- **Streamer aufnehmen** per Twitch-Link oder Login-Name, manuell verifizieren, archivieren oder ganz entfernen.
- **Discord-Verknüpfung** eines Streamers setzen oder korrigieren.
- **Abo-Plan manuell überschreiben** oder einen bestehenden Override wieder entfernen, sodass wieder der reguläre Abo-Status greift.
- **Globale Ankündigungs- und Promo-Modi** steuern — also ob und wie Live-Ankündigungen und Promo-Nachrichten ausgespielt werden. Diese Schalter sind bewusst zentral bei den Admins und nicht in der Streamer-Selbstverwaltung.
- **Bot-, Chat- und Raid-Konfiguration** anpassen.
- **Chat-Moderationsaktionen** für Partner-Kanäle auslösen.
- **Inhalte pflegen:** Ankündigungen, Changelog, Roadmap und rechtliche Seiten (Impressum, Datenschutz, AGB).

## Grenzen & Sonderfälle

- **Strikte Trennung Admin/Streamer:** Ein normales Streamer-Konto kommt nicht ins Admin-Dashboard und kann keine fremden Kanäle steuern. Diese Trennung ist bewusst und wird abgesichert.
- **Manuelle Plan-Overrides sparsam einsetzen:** Die reguläre Abrechnung bleibt die primäre Quelle für den Plan eines Streamers. Ein manueller Override ist für Support-Fälle, Kulanz oder Reparaturen gedacht; wird er entfernt, greift wieder der normale Abo-Status.
- **Die Datenbank-Ansicht im Monitoring ist nur lesend:** Über das Dashboard lassen sich Kennzahlen und Zustände einsehen, aber keine Daten direkt verändern.
- **Saubere Partnerpflege ist die Grundlage:** Billing, Raids, Analytics und weitere Funktionen bauen auf einem korrekt gepflegten Streamer-Bestand auf. Falsche oder verwaiste Einträge sollten archiviert oder entfernt werden.
- **Sichtbarkeit einzelner Bereiche:** Manche Bereiche sind reine Übersichten ohne Eingriffsmöglichkeit; die schreibenden Aktionen sind auf das beschränkt, was im jeweiligen Bereich angeboten wird.

## Häufige Fragen

**Wer hat Zugriff auf das Admin-Dashboard?**
Nur die Betreiber des Bots. Normale Streamer oder Viewer haben keinen Zugang. Wer kein berechtigter Admin ist, sieht die Oberfläche gar nicht.

**Kann ich als Streamer dort meinen eigenen Kanal verwalten?**
Nein. Streamer verwalten ihren Kanal über das separate Streamer-Dashboard. Das Admin-Dashboard ist ausschließlich für die Betreiber, die das gesamte Netzwerk im Blick haben.

**Wie wird ein neuer Streamer aufgenommen?**
Ein Admin fügt ihn über seinen Twitch-Link oder Login-Namen hinzu und verifiziert ihn anschließend. Danach betreut der Bot den Kanal entsprechend des Plans.

**Was passiert, wenn ein Streamer archiviert wird?**
Die Betreuung des Kanals endet — der Bot überwacht ihn nicht mehr. Archivieren ist für inaktive oder fehlerhafte Einträge gedacht, ohne sie komplett zu löschen.

**Was bedeutet ein manueller Plan-Override?**
Ein Admin kann den Abo-Plan eines Streamers von Hand setzen, etwa für Kulanz, einen Bonusmonat oder zur Reparatur nach einem Abrechnungsproblem. Wird der Override entfernt, gilt wieder der reguläre Abo-Status.

**Kann ein Admin über das Dashboard direkt in der Datenbank etwas ändern?**
Nein. Die Datenbank-Ansicht ist rein lesend und dient nur dem Einblick in Kennzahlen und Zustände.

**Welche Moderationsmöglichkeiten haben Admins?**
Admins können Moderationsaktionen im Chat von Partner-Kanälen auslösen. Davon unabhängig moderiert der Bot Partner-Kanäle ohnehin automatisch.

**Was zeigt der Billing-Überblick?**
Den Abo-Status der Streamer, Affiliate- beziehungsweise Partnervergütungen und Gutschriften — plus die Möglichkeit, einen Plan manuell zu überschreiben oder einen Override zu entfernen.
