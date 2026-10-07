# Vertrag

## Zugriff

Die Liste und jede Aktion verwenden die bestehende serverseitige Social-Media-Freigabe. Bei Partnern ist die Twitch-ID der Sitzung maßgeblich. Eine fremde angeforderte Kanal-ID wird abgewiesen. Verwaltungssitzungen können ohne Kanalfilter den sichtbaren Bestand abrufen. Der Loginname ist keine Zugriffsgrenze.

## Daten und Aktionen

Ein neuer Migrationsschritt ergänzt Ausblendezeit, ausdrückliche Drive-Wahl, Drive-Link und letzten Bearbeitungsversuch. Die Liste enthält keine lokalen Dateipfade, Upload-Sitzungen oder rohen Fehlermeldungen. Einträge ohne bestätigte YouTube-Teile werden trotz historischem Fertigstatus als unbestätigt dargestellt und nicht automatisch nochmals hochgeladen.

Erneuter Versuch und Drive-Wahl erhalten fertige Teile und setzen fehlgeschlagene Teile zurück. Ausblenden entfernt die Listenanzeige, nicht die Sicherung. Ein PostgreSQL-Advisory-Lock verhindert Aktionen während der aktiven Workerbearbeitung.

## Sicherung

Ohne eigenen geeigneten YouTube-Zugang bleibt der lokale Download zulässig. Der Worker nutzt den gemergten Verbindungszustand aus Aufgabe A und akzeptiert keinen globalen Kontorückfall. Die Liste beachtet denselben gespeicherten Neuverbindungszustand und den Ablauf der erneuerbaren Verbindung.

Drive ist eine ausdrücklich gewählte Alternative. Dateigröße und Ziellink müssen vor dem lokalen Aufräumen bestätigt sein. YouTube benötigt bestätigte Verarbeitung der fertigen Teile. Aufgeräumt wird am gespeicherten Medienpfad innerhalb der kanonischen Archivwurzel, nicht an einem aus dem möglicherweise geänderten Login geratenen Ordner.

## Betrieb und Abschluss

Der Bot hat einen Archiv-Laufzeitpfad mit den vorhandenen Kanaloptionen. Das Archiv bleibt eine Unterseite des bestehenden Social-Media-Managers. Nach der Korrektur vom 7. Oktober ist das Ziel `/twitch/social-media?view=archiv`. Aufgabe P (`feat/twitch-pfade`) übernimmt das gemeinsame Twitch-Präfix. Beim abschließenden Fetch steht P noch nicht auf main; die bereits integrierte SPA-Einbindung unter `/social-media?view=archiv` bleibt deshalb für P erhalten. Es wird keine zweite Archivroute aufgebaut. Archiveinstellungen stammen aus der normalen typisierten Betriebsdatei, Zugangsdaten aus den vorhandenen verschlüsselten Quellen. Die SQLite-Historie wird nicht nochmals importiert. Der alte Dienst und die alten Python-Anwendungsdateien wurden nach erfolgreicher Live-Prüfung entfernt; der übrige Nebenbestand bleibt bis zur eigenen Löschfreigabe bestehen.
