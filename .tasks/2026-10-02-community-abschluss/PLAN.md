# Abschlussplan für Twitch #1035

Der Twitch-Anteil umfasst die Punkteaggregation, interne Punkte- und Scout-Endpunkte sowie Clip-Einreichungen aus Chat und Social-Studio. Grundlage ist der ursprüngliche Plan zur Community-Streamer-Brücke. Der neue Arbeitsbranch übernimmt den belegten Anteil vom Herkunftskopf und den bestätigten uncommitteten Replay-Fix auf den heutigen Main. Die Herkunft bleibt als Sicherung erhalten.

1. Producer- und Consumerverträge gemeinsam mit dem vorhandenen Discord-Worker prüfen.
2. Watchtime am Sessionende, serialisierte Aggregation, Bannprüfung und dauerhaftes Clip-Replay mit bestehenden Tests nachweisen.
3. Alte Testfehler gegen eine genaue heutige Baseline abgleichen, falls sie erneut auftreten.
4. Social-Studio lokal bauen und die neue Aktion samt Antwortzuständen visuell prüfen.
5. Den vorhandenen Gate nach jedem eigenen Fix auf den aktuellen Commit anwenden. Historische BLOCKs sind kein frisches Urteil. Keine Reviewer- oder Fixerthreads anlegen.
6. C9-Integration abwarten, Consumer nachziehen und das Communitypaket gemeinsam erneut prüfen.
7. Nach gemeinsamer Abnahme und Gate-ALLOW Consumer vor Producer unter den jeweiligen Repo- und Deploysperren ausrollen. Hostbuilds serialisieren. Migrationen und produktiven Quellstand nachweisen.
8. Nach der gemeinsamen Liveprüfung die eigene entbehrliche Arbeit aufräumen. Die fremde Herkunft bleibt erhalten.

Arbeitsworktree: `/home/nathanael/.worktrees/sol-abschluss-51eb094dc5242b90`.
Integrationsbranch: `sol/abschluss/51eb094dc5242b90`.
Community-Consumer: vorhandener Thread `c0b1d111-e402-4ed3-bb0b-68958a1ba699`.
Hauptthread: `eea28c5f-5eef-418c-ad9c-e5c7faf27b60`.

Die letzte bestätigte Testfolge lautet Discord, Docs, Bots-C9. Der nächste Twitch-Fixlauf benötigt danach einen ausdrücklich zugewiesenen Slot. Ein freier Lock allein ist keine Übergabe.

Vor dem späteren Twitch-Deploy ist der Migrationsbestand neu abzugleichen. Das Paket enthält im aktuellen Manifest eine fremde pending Main-Migration für token_storage zusätzlich zu den drei Communitymigrationen. Bei abweichender Zuständigkeit oder noch ungeklärtem Auslieferungsstand hält die Gruppe den Migrationslauf zurück. Keine selektive versteckte DDL außerhalb der geprüften Launcherstrecke.

Für den Wechsel auf den internen Schreibpfad werden unter dem Repo-Deploylock zuerst die Dashboard-Einreichungen angehalten, danach das vollständig geprüfte Release aktiviert und die pending Migrationen samt finaler Rollenmatrix ausgeführt. Die neue Producer-API startet und wird authentifiziert geprüft, bevor das neue Dashboard wieder Einreichungen annimmt. Der Consumer läuft zuvor bereits mit dem gemeinsam abgenommenen Vertrag. Ein laufendes altes Dashboard darf nicht gegen bereits entzogene Schreibrechte einreichen. Der vorhandene Launcher und seine tatsächlich gestarteten Dienste werden vor Ausführung erneut geprüft; keine parallele Auslieferung. Ein Fehler hält den weiteren Gruppenwechsel an und wird mit dem Hauptthread geklärt.

Die Rollenregression bildet die reale Reihenfolge in einer eigenen Wegwerf-Datenbank ab: Migrationen, Rollenmatrix, Rollenmatrix erneut, echte SELECT-/INSERT-/UPDATE-/DELETE-Versuche als twitchdash sowie Producer-Schreibversuch. Kein produktives DDL vor Gruppen-Gate-ALLOW. Der neue Communityblock in ops/systemd/twitch-runtime-roles.sql muss beim Nachziehen fremder Integrationen nach den allgemeinen Grants erhalten bleiben.
