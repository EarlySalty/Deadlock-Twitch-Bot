# Bedingte Deployfreigabe und konkrete Vorprüfung

## Neue Nutzerentscheidung

Am 8. Oktober 2026 antwortete der Nutzer auf die Erklärung, dass der Deploy ausstehende Datenbankänderungen ausführt und Dienstrechte neu setzt:

> genau prüfen wenn nxi damit kaputt geht und es positiv ist machen

Damit besteht eine bedingte Freigabe zur Ausführung nach belegter positiver Prüfung der konkreten Auswirkungen. Das ist keine pauschale Zustimmung zu unbekannten Migrationen und keine Behauptung garantierter Fehlerfreiheit. Bis die Prüfung die Bedingungen trägt, keine Migration, Rechteänderung, Wrapperausführung oder Neustarts.

Der Nutzer möchte weiter keine neuen allgemeinen Reviews. Diese eng begrenzte Deployvorprüfung ist ausdrücklich neu beauftragt. Bestehende Fixketten weiterführen, keine zusätzliche breite Fehlerwelle. Alle sonstigen Grenzen, insbesondere Secrets/ENV, echte Konten, ai-coach, Schutzmechanismen und fremde Arbeit, bleiben bestehen.

## Auftrag an die lesende Prüfrolle

Modell ausschließlich gpt-6.1-sol, keine Delegation. Exklusiv DEPLOY-PRUEFUNG-02.md und DEPLOY-PRUEFUNG-02.json im Taskordner schreiben. Eigene temporäre Belege zulässig. Keine sonstigen Taskdateien oder Anwendungscode ändern. Kein Git-Mutieren, kein Deploy, Build, Dienstneustart, Datenbankschreiben oder Bereinigen. Diese Rolle meldet nachprüfbare Voraussetzungen und Blocker; die Hauptsession entscheidet anhand der Nutzerbedingung über eine spätere Ausführung.

1. Frisches Remote-main und tatsächlich laufendes Release lesend ermitteln. Der bestehende Wrapper unterstützt --pruefen. Nicht von Dateialter oder dem historischen SHA im alten Bericht ausgehen. Rolle-deploy-verifizierer und deploy-restart-selbstdienst beachten. Kein generischer sudo-/systemctl-Ersatz für blockierte Aktionen.
2. Konkrete installierte Migrationseinheit und geplante versionierte Migrationen vergleichen. Nur Metadaten über angewandte SQLx-Migrationsversionen, Erfolgsstatus und Checksummen abfragen, wenn ein vorhandener zulässiger rein lesender Zugangsweg ohne Secrets verfügbar ist. Keine Nutzerdaten, ENV-Dateien, Passwortdateien oder Zugangsdaten lesen. Kein neuer Zugangsweg und keine Sicherheitsumgehung; fehlenden zulässigen Metadatenzugang als Blocker melden.
3. Ausstehende Migrationen am fest aufgelösten geplanten SHA einzeln benennen. SQL-Inhalt auf Datenänderungen, Löschungen, Tabellensperren, Abhängigkeiten und Auswirkungen auf aktuell laufenden alten Code prüfen. Bereits angewandte Migrationen nicht ändern. Keine Ausführung der Migrationen, auch nicht testweise auf Produktion. Unbekannten Datenbankstand nicht durch Null ausstehende Migrationen ersetzen.
4. Konkrete Wirkung des Rollenskripts und der Peerregeln auf tatsächlich verwendete Dienstidentitäten und Rechte prüfen. Der Bot-/Dashboardbetrieb, Analyse-/Wettbewerbs-/Verbrauchsrollen und neue Verbindungen müssen zu den Regeln passen. Zusätzlichen Verbrauchsnachlieferungstimer und ausgewählte Neustarts als Seiteneffekte einbeziehen. Unbekannte installierte Konfiguration, Rollen oder Pfade ausdrücklich offen lassen.
5. Vorhandene Sicherungs- und Rückstellmöglichkeiten anhand zugänglicher Metadaten belegen, keine produktiven Daten exportieren. Der Wrapper stellt den Programmverweis zurück, aber nicht bereits angewandte Migrationen. Kein vollständig geprüfter Rollback ohne echten Beleg. Notwendige positive Funktions- und Rechteprüfungen für nach einem späteren Deploy benennen, ohne echte Kontoaktionen.

## Ergebnisvertrag

Bericht für den Nutzer auf Deutsch, ohne Gedankenstriche. Pro ausstehender Migration: Nummer, Quelldatei, tatsächliche Wirkung, Betriebsrisiko, Voraussetzungen. Bei null ausstehenden Migrationen die genaue Metadatenabfrage und SHA-Bindung belegen. Rechte- und Konfigurationsänderungen bleiben auch dann separat zu prüfen.

Urteil ausschließlich VORAUSSETZUNGEN_BELEGT oder BLOCKIERT mit konkreten offenen Punkten. VORAUSSETZUNGEN_BELEGT bedeutet vertretbar abgesicherte konkrete Änderungen, nicht garantiert risikofrei. Ohne belegten Serverstand oder ausreichenden Rückweg BLOCKIERT. Keine pauschale Aufforderung an den Nutzer, unbekannte Änderungen freizugeben.

Quellen: DEPLOY-ERKLAERUNG.md und OPS-PREFLIGHT.md, installierter Wrapper /usr/local/bin/deploy-twitch-release, versionierte Dateien ops/systemd/deadlock-twitch-migrate.service, twitch-runtime-roles.sql, install-twitch-contest-peer sowie Verbrauchsnachlieferungstimer. Vor Codesuche code-suche und Graphify. Keine Browserarbeit; falls später ausdrücklich nötig ausschließlich Moli nach dessen Leitfaden, niemals Brave.

Die serielle Fixintegration kann main weiterbewegen. Prüfung an festen SHA binden und nach späteren Änderungen die betroffenen Voraussetzungen neu abgleichen. Aktuelle Prüfung erlaubt keinen konkurrierenden Main-Push und keinen vorzeitigen Deploy, solange neue Fixstände noch integriert werden.
