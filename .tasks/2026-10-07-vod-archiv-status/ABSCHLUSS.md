# VOD-Archivstatus: Abschluss

## Ergebnis und Ursache

Der Archivtab zeigt den Status mit Icon links beim Titel, unterscheidet YouTube-Abschluss, Drive-Sicherung, laufenden Transfer, Warten, Teilerfolg, Fehler und unklaren Altbestand. Ausblenden ist nachgeordnet. Ort: https://deutsche-deadlock-community.de/twitch/social-media?view=archiv, Tab Archiv.

Die bisherige API lieferte uploaded_at nicht aus. Die Oberfläche deutete fehlende historische Versuchszeiten als „Noch kein Versuch“ und bewertete den Rohstatus unabhängig von den gespeicherten Teilbestätigungen. Die Reparatur nutzt den bestehenden Rust-Store, Worker, Handler und Archivtab. Keine Migration, kein neuer Dienst und kein neuer Plattformclient.

YouTube-Abschluss verlangt einen abgeschlossenen Elternzustand, vorhandene Teile und für jeden Teil done plus eine nicht leere Video-ID. Ein Link reicht nicht. Drive-Sicherung verlangt den passenden Zustand, uploaded_at und eine gültige Drive-Zieladresse. uploaded_at ist der Abschlusszeitpunkt; last_attempt_at entsteht beim tatsächlichen Download-/Uploadübergang. Fehlende historische Zeitpunkte werden nicht aus updated_at ersetzt. Der bestehende YouTube-Client und seine begrenzten Verarbeitungs-/Ablehnungsprüfungen bleiben erhalten. Gespeicherter Abschluss bestätigt weder heutige Erreichbarkeit noch öffentliche Sichtbarkeit.

## Prüfungen und Grenzen

- Eigene isolierte PostgreSQL-Daten: 50 Archivtests und 5 API-Archivtests bestanden, jeweils 0 failed und 0 ignored. Tatsächlich ausgeführte SQL-Übergänge separat in db-execution-proof.log bewiesen. Der versehentliche Filterlauf mit 0 Tests zählt nicht.
- Archiv-Renderprüfungen: 5 passed, 0 failed, 0 skipped. Voller Frontendlauf: 439 passed, 5 failed; Ausgangsstand: 434 passed, dieselben 5 failed. Voller Rust/API-Lauf: 1319 passed, 22 failed; Ausgangsstand: 1318 passed, dieselben 22 failed. Die vollen Suites sind nicht grün.
- Rustfmt und Diffprüfung bestanden. Striktes Archiv-Clippy bestanden. Striktes API-Clippy bleibt bei exakt denselben 25 Fehlerstellen wie der separat gemessene Ausgangsstand. Dashboard-, Admin- und Website-Produktionsbuilds sowie acht Rust-Releasebinaries erfolgreich.
- Moli: gebündelte Desktop-/Mobilprüfung und eine Bestätigungsrunde. Titel/Status beginnen bei x=305 beziehungsweise x=29; Dokumentbreiten 1440 und 390, acht unterscheidbare Icons, keine Seitenfehler. Der anfängliche Nullkoordinatenbefund ist offengelegt und durch die erlaubte Bestätigung geklärt. Die Screenshots verwenden das gebaute Produktionsbundle mit echter isolierter Handlerantwort.
- Lesender Echtbestand vor und nach Deploy: 82 VODs, 76 bestätigte YouTube-Abschlüsse mit Abschlusszeit, 5 unklare historische Abschlüsse, 1 nicht verfügbar, 82 ohne Versuchszeit. Keine echten VODs erneut hochgeladen, ausgeblendet, gelöscht oder kopiert; keine Sichtbarkeit geändert und keine öffentliche Testnachricht gesendet.

## Gate und Historie

Produktänderung: e5c04f4d8c8ce42cd7faa27ab43ab1499b9f71cf. Nachweiskommitierungen: 850a7e5f, d29bd6ee5a09ee83d0925a44447b6f5fa59e7498 und b0bd68248c3accc1771e938e6166c3a122ac154e. Diese Stände sind in origin/main integriert. Vier reguläre Urteile desselben Modells gpt-6.1-sol waren ALLOW. Kein BLOCK, kein Fixer und kein zusätzlicher Reviewer-Thread. Die optionale DB-Testkonfiguration bleibt als NIT dokumentiert; dieser Lauf ist tatsächlich nachgewiesen.

Der erfolgreiche Integrationspush war ein einzelner Git-Schritt. Zwei vorangehende Hook-Ablehnungen betrafen die Befehlsform beziehungsweise den erkannten Testnachweis. Nach Status-/HEAD-Prüfung und einem grünen Vordergrundtest erfolgte der reguläre dritte Anlauf. Kein Gate wurde umgangen. Der abschließende Nachweiskommit enthält keine Produktänderung und ist nicht der Quell-SHA der bereits gebauten Binaries.

## Release und Live-Beweis

Gebauter und ausgelieferter Quell-SHA: b0bd68248c3accc1771e938e6166c3a122ac154e. Acht ELF-Metadaten enthalten exakt diesen SHA ohne dirty. Rust-Releasebau im eigenen Worktree mit cargo-slot und drei Jobs: Exit 0, 14m 11s. Die kurzzeitige Registeränderung hinterließ keine schmutzige Buildrevision. Der saubere Standalone-Clone wurde vom serialisierten Deploy-Wrapper nach /opt/deadlock/twitch/builds/b0bd68248c3accc1771e938e6166c3a122ac154e verschoben. Current zeigt auf /opt/deadlock/twitch/releases/b0bd68248c3accc1771e938e6166c3a122ac154e. Wrapper und lesende Nachprüfung: Exit 0.

| Prozess | PID vorher | PID nachher |
|---|---:|---:|
| Bot | 1807853 | 3022811 |
| Dashboard | 1807722 | 3022680 |
| Coaching-Watch | 1809122 | 3023797 |
| Collector | 1809132 | 3023840 |

Die vier Prozesse sind aktiv, ihre ausführbaren Dateien liegen im neuen Release, keine trägt deleted, NRestarts jeweils 0. Journal seit jeweiligem tatsächlichem Start mit -p err: 0 Einträge, lesbar, Exit 0. Neue Inhaltsanker im tatsächlich installierten Dashboard und Bot sind nachgewiesen. Der zusätzliche Watchdog ist laut Quell- und Live-Unit Type=oneshot, sein Timer aktiv/waiting und der letzte Lauf Exit 0; ExecStart nutzt current. Er wurde nicht als Test gestartet und hat keinen dauerhaft zu ersetzenden Prozess.

Sieben lokale Dashboard-Artefakte entsprechen den Hashes der Moli-Bestätigung. Die sechs öffentlich ausgelieferten JS-/CSS-Artefakte sind HTTP 200 mit passendem Content-Type und ebenfalls bytegleich. Die Archivseite liefert ohne Anmeldung 303 nach /twitch/auth/login; die Weiterleitung wurde beim Grenznachweis nicht verfolgt. Beide Archiv-API-Aliase liefern ohne Sitzung JSON-401. Ein erster HTML-Vergleich folgte der Anmeldeweiterleitung und war deshalb kein gültiger Indexvergleich. Keine Anmeldeparameter wurden in die Akte übernommen.

Es gab keine angemeldete Produktions-DOM-Prüfung und keinen neuen externen YouTube-/Drive-Verfügbarkeitstest. Der Funktionsbeweis umfasst tatsächliche isolierte SQL-/Handlerpfade bis zur gebauten Anzeige sowie identischen live ausgelieferten Code und Assets, nicht eine erfundene Produktionssitzung.

## Eigentum und Abschlussmechanik

Worker: 9b165f9b-1fc6-4d0a-ade0-8b01afcc86ea. Die finale Registerpflege wurde einmalig vom Hauptorchestrator übertragen. Eigenes Moli, eigener Fixture-Server und eigene Testdatenbank sind beendet; Testkonfiguration gelöscht. Baselineworktree und gestoppte Wegwerfdatenbank sind entfernt, Baseline-SHA vor Entfernung als Vorfahr mit Exit 0 geprüft. Auftragsakte, Register und Nachweise werden vor Entfernung des Hauptworktrees committed und nach origin/main gesichert.

Zum Zeitpunkt dieses Nachweiskommit sind Hauptworktree und Fixbranch noch vorhanden. Nach erfolgreicher Sicherung folgen frischer Fetch, Vorfahrenprüfung mit Exit-Code, Entfernung des eigenen Remote-/Lokalbranches und eigenen Hauptworktrees. Fremde Worktrees, gemeinsamer Checkout und root-eigene Release-/Buildverzeichnisse bleiben unangetastet. Der letzte sichtbare Abschlussbericht nennt das tatsächlich erreichte Cleanup-Ergebnis. Self-settle wird erst danach als letzter Werkzeugschritt vorgemerkt; sein Nachläufer greift nach Turn-Ende.

BESTAND[BS-1]: ja | Fundort: rust/crates/tb-vod-archive/src/store.rs:528 | Anknüpfung: vorhandene Teilbestätigungen, Abschlusszeiten, Statusübergänge und Plattformclients
TESTNACHWEIS[TW-1]: 60 passed, 0 ignored | Baseline: 22 Rust/API und 5 Frontend rot
WIRKUNGSPRUEFUNG[WP-1]: 0 Befunde | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 3/3 geprüft
TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: Archivtab und interne Auftragsakte
MERGEPROTOKOLL[MS-1]: 1 Git-Schritte einzeln | Anläufe: 3 | Gate: ALLOW, No merge-blocking defect established in the supplied diff.
LIVEBEWEIS[DV-1]: PID 1807853->3022811, 1807722->3022680, 1809122->3023797, 1809132->3023840 | exe ohne (deleted) | journal -p err leer | Anker "display_status" in Binary | Funktion: echte isolierte SQL-/Handlerantwort korrekt angezeigt, live Code und Assets identisch, Anmeldung bleibt nötig | Ort: https://deutsche-deadlock-community.de/twitch/social-media?view=archiv, Archivtab
