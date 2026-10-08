# Was der Deploy zusätzlich zum Programmwechsel verändert

## Nachtrag zur Entscheidung

Nach dieser Erklärung hat der Nutzer die konkrete Prüfung beauftragt und die Ausführung bei belegter positiver Wirkung ohne ungeklärte Schäden bedingt erlaubt. BRIEFING-DEPLOYPRUEFUNG-02.md bindet die lesende Vorprüfung. Bis ihr Ergebnis die Bedingungen trägt, bleibt die Ausführung gesperrt. Die Empfehlung unten beschreibt den Stand vor dieser Antwort; eine garantierte Fehlerfreiheit oder pauschale Freigabe unbekannter Änderungen wird weiterhin nicht behauptet.

Stand: 8. Oktober 2026. Lesende Prüfung des installierten Wrappers /usr/local/bin/deploy-twitch-release und der zugehörigen versionierten Systemdateien. Nichts davon wurde ausgeführt. Die genaue Liste der auf dem Server noch ausstehenden Migrationen ist nicht ermittelt.

## Bedeutung für den Betrieb

Der vorgeschriebene Deploy macht mehr als die neuen Botdateien bereitzustellen:

1. **Datenbank auf den Stand des Releases bringen.** Eine eigene Migrationseinheit läuft als PostgreSQL-Systemnutzer. Sie führt noch nicht angewandte SQLx-Migrationen aus dem neuen Release aus. Je nach Inhalt können Tabellen, Spalten, Indizes oder Daten verändert werden. Welche konkreten Änderungen diesmal anstehen, ist ohne Abgleich mit der Datenbank unbekannt. Selbst wenn keine neue Migration ansteht, führt die Einheit anschließend die Rollen- und Rechtezuordnung erneut aus.
2. **Datenbankrechte der Dienste setzen.** Das Rollenskript entzieht vorhandene Tabellen- und Sequenzrechte und vergibt die im Release vorgesehenen Rechte neu. Für die Bot- und Dashboardrollen werden außerdem Rollenattribute, Einstellungen und die passwortbasierte Anmeldung verändert. Das soll die vorgesehenen begrenzten Dienstrechte herstellen. Eine unpassende Zuordnung kann benötigte Datenbankzugriffe unterbrechen.
3. **Lokale Datenbankanmeldung konfigurieren.** Der Peerinstaller schreibt PostgreSQL-Zugangsregeln und Zuordnungen zwischen Linux-Dienstnutzern und Datenbankrollen. Dazu gehören lokale Zugänge für Bot und Dashboard sowie begrenzte Schreibrollen für Wettbewerb, Analyse und Verbrauchserfassung. Für bestimmte Rollen wird Netzwerkzugang ausdrücklich abgewiesen. Die PostgreSQL-Konfiguration wird neu geladen; es ist kein pauschaler Datenbankneustart vorgesehen. Fehler können neue Verbindungen der betroffenen Dienste verhindern. Vor der Änderung werden die betroffenen Regeldateien gesichert; bei Fehler im Installer ist eine Rückstellung vorgesehen.
4. **Verbrauchsnachlieferung aktivieren und Dienste neu starten.** Der Wrapper installiert Dateien für die Nachlieferung unvollständiger KI-Verbrauchserfassung und aktiviert einen Timer. Dieser ist mit zwei Minuten nach dem Systemstart und fünf Minuten nach der letzten Aktivierung konfiguriert. Anschließend startet der Wrapper die ausgewählten Dienste neu. Ohne ausdrückliche Auswahl betrifft das neben Bot und Dashboard auch weitere Twitch-Dienste.

## Was eine Freigabe bedeuten würde

Eine Freigabe des regulären Deploys erlaubt damit Änderungen an der produktiven Datenbankstruktur beziehungsweise ihren Daten, an Dienstrechten, an PostgreSQL-Zugangsregeln und an Systemdiensten. Sie wäre mehr als die Freigabe für einen Neustart. Die Auswahl einzelner Neustartziele unterdrückt die Datenbank- und Konfigurationsschritte nicht.

Die geprüften Botfixes selbst erhalten dadurch keine neue Migration. Der Wrapper übernimmt aber den gesamten freigegebenen Release-Stand und kann bereits vorhandene, auf diesem Server noch nicht angewandte Änderungen ausführen. Eine automatische Garantie, dass diesmal nichts an Daten oder Zugangsrechten geändert wird, ist nicht belegt.

Bei einem Fehler vor der Release-Abnahme versucht der Wrapper, den bisherigen Programmverweis wiederherzustellen. Bereits ausgeführte Migrationen bleiben ausdrücklich erhalten. Ein Programm-Rollback ist somit kein Datenbank-Rollback.

## Empfehlung und offene Entscheidung

Vorerst keinen pauschalen Deploy freigeben. Für eine spätere Entscheidung zuerst lesend die auf dem Server noch ausstehenden Migrationsnummern und ihre Inhalte dem vorgesehenen Release zuordnen sowie die installierte Migrationseinheit und Rechtezuordnung vergleichen. Danach lassen sich die konkreten Änderungen, notwendige Sicherungen und ein Rückweg benennen. Diese vertiefte Vorprüfung ist hier noch nicht ausgeführt.

Bis dahin können die bestätigten Fixes geprüft und nach main übernommen werden. Sie sind dann vorbereitet, wirken aber noch nicht nachgewiesen im laufenden Bot. Die Frage des Nutzers nach der Bedeutung des Deploys ist keine Zustimmung zu diesen Änderungen.

## Quellnachweise

- Installierter Wrapper: /usr/local/bin/deploy-twitch-release:360-387 startet Installer, Migrationseinheit und Peerinstaller.
- Installierter Wrapper: /usr/local/bin/deploy-twitch-release:335-354 stellt bei früheren Fehlern den Programmverweis zurück, erhält aber Migrationen.
- Installierter Wrapper: /usr/local/bin/deploy-twitch-release:611-635 installiert Verbrauchsnachlieferung, aktiviert den Timer und startet die ausgewählten Dienste.
- Versionierte Migrationseinheit: ops/systemd/deadlock-twitch-migrate.service:10-14, PostgreSQL-Nutzer, SQLx-Migrationen und anschließendes Rollenskript.
- Versioniertes Rollenskript: ops/systemd/twitch-runtime-roles.sql:22-27,51-75,362-402,472-493, Rollenattribute sowie Entzug und Vergabe von Rechten.
- Versionierter Peerinstaller: ops/systemd/install-twitch-contest-peer:45-73,113-160, Dateisicherung, lokale Rollenbindungen, Ablehnungsregeln und Konfigurationsreload.
- Versionierter Timer: ops/systemd/deadlock-twitch-llm-usage-recover.timer:3-6.

Die versionierten Quelldateien wurden aus dem eigenen Artefakt-Worktree gelesen. Das ist kein Nachweis, dass die laufend installierte Migrationseinheit oder jede Serverkonfiguration aktuell bytegleich ist. Es wurden keine ENV-Dateien, Geheimnisse oder produktiven Nutzerdaten gelesen.
