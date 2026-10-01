# Reparatur nach dem Titel-Studio-Review

## Status

Folgeauftrag des Nutzers: „Fix die Mängel, deploy alles“. Die Umsetzung wurde selbst ausgeführt, ohne einen Coding-Auftrag weiterzugeben. Grundlage sind `AUFTRAG.md` einschließlich der drei Nachträge und die 21 Befunde in `REVIEW.md`.

Die nachstehenden Korrekturen sind implementiert und durch gezielte Tests geprüft. Das gemeinsame Deployment ist noch nicht abgeschlossen. Für den erforderlichen Merge-Kritiker liegt keine belastbare Freigabe vor; der ausdrückliche Aufruf des bestehenden Gate-Einstiegs wurde vom Werkzeugzugang blockiert. Es wurde kein neues Release aktiviert und keine Produktionsmigration angewendet.

Wichtig zum Git-Zustand: Der Steam-Commit `f4ac4a868f9162ef9d9cbfd5874af1854b05b2a3` wurde bereits auf den Feature-Branch und auf das dortige Remote-`main` gepusht. Der erfolgreiche Git-Push ist kein Nachweis eines bestandenen Merge-Gates. Der lokale Steam-`main` blieb für die noch ausstehende Prüfung auf seinem vorherigen Stand `02f1367cd9729e29052197c81ef7ac55e725eb40`. Die Twitch-Reparatur wird auf `feat/titel-studio-costream` gesichert, nicht als freigegebenes Live-Release ausgegeben.

## Übernommener Arbeitsstand

Der frühere Review-Commit ist `b861ab93ff6e9ae3378316b1949693d559dc7ecc`. Der Intent-Thread hat den vorgemerkten Paket-B-Stand anschließend in `db20505b384fa057300c50150c029ff4a37404de` gesichert. Dessen Commit-Nachricht bezeichnet ihn ausdrücklich als Stand vor den Review-Fixes. Die in `REPORT-B.md` dokumentierte parallele Sitzung hat ihre weiteren Arbeiten und Prüfprozesse eingestellt. Der Bericht bleibt als historischer Nachweis erhalten.

## Zusätzlich gefundene Produktionsursache

Die direkten Joins in den Vorarbeiten hätten im laufenden Twitch-Dienst nicht funktioniert: Eine gezielte Katalogabfrage auf `twitch_analytics` ergab, dass `core.steam_links` und `voice.deadlock_party_members` dort nicht existieren. Der Verweis auf ihre Verwendung in einer anderen Quelldatei war kein Beleg für die Verfügbarkeit im selben Datenbankpool.

Die zentrale Datenbank bleibt beim vorhandenen Steam-Dienst. Dort ergänzt der neue, token-geschützte Leseweg `GET /internal/title-context?discord_id=<id>` den benötigten Kontext. Die Antwort enthält den Aufnahmezeitpunkt, eine bekannte Party-Größe und Discord-IDs aus Party beziehungsweise Sprachkanal. Der Twitch-Bot führt die Zuordnung zu Twitch-IDs und die Prüfung seines Live-Stands in seiner eigenen Datenbank aus.

Betroffene Steam-Dateien im Commit `f4ac4a8`:

- `rust/crates/steam-persistence/src/title_context.rs`: zentrale Leseabfragen, Frische, Party-ID und Größentyp.
- `rust/crates/steam-web/src/routes/title_context.rs`: geschützter Endpunkt und Integrationstests.
- Die jeweiligen Modul- und Router-Einbindungen sowie die interne Sichtbarkeit des vorhandenen Authentifizierungsprüfers.

Damit wurden weder zusätzliche Datenbankrechte noch ein neuer Dienst, ein Discord-Gateway oder Tabellenkopien eingeführt. Deadlock-Bots blieb unverändert. Die vorhandenen internen Token werden weiterverwendet; es gibt keinen neuen Konfigurationsschalter.

## Abgleich mit den Review-Befunden

| Befund | Korrektur und Nachweis |
|---|---|
| R01, Steam-Party fehlt | Der gemeinsame Einstieg `steam_lookup::detect_co_streamers_all` kombiniert Shared Chat, Party und Voice in dieser Reihenfolge. Die Party stammt aus dem geschützten Steam-Leseweg. Der getrennte Datenbanktest enthält absichtlich keine zentralen Tabellen im Twitch-Pool. |
| R02, Live-Stand unzureichend | Shared-Chat-Teilnehmer werden mit dem bestehenden `/streams`-Aufruf auf laufende Streams geprüft; auch der anfragende Kanal muss darin live sein. Lokale Kandidaten benötigen `is_live = 1` und einen gültigen, weniger als zehn Minuten alten `last_seen_at`. Fehlende, ungültige und zukünftige Zeitstempel werden verworfen. |
| R03, Namen statt IDs dedupliziert | Die Zusammenführung hält die Twitch-ID bis zum gemeinsamen Zweierlimit und entfernt darüber die eigene ID sowie Duplikate. Ein geänderter Login kann dieselbe ID nicht zweimal einschleusen. |
| R04, INT4/INT8-Fehler | Der Steam-Leser liest `party_size` als `Option<i32>`, akzeptiert 1 bis 6 und liefert daraus den Größenhinweis. Der Test verwendet das echte zentrale Migrationsschema und bestätigt ein Duo. |
| R05, fremde Unicode-/Präfix-Mentions | `strip_title_mentions` entfernt Modell-Mentions einschließlich Unicode-Buchstaben, kombinierender Zeichen, Formatzeichen und der beiden geprüften breiten @-Varianten. Danach werden bestätigte ASCII-Logins neu aufgebaut. Tests umfassen Groß-/Kleinschreibung, @ im Wort, Unicode, Zero-Width-Zeichen und überlange Namenspräfixe. |
| R06, kurzer Login durch längeren erfüllt | Der alte Teilstringtest entfällt. `anna` und `annabelle` werden als getrennte bestätigte Logins aufgebaut, jeweils einmal in kanonischer Schreibweise. |
| R07, Suffix umgeht Verbotsliste | Der fertige Titel einschließlich `mit @login` wird gegen die Verbotsliste geprüft. Ein Konflikt verwirft den Vorschlag, statt das verbotene Wort ungeprüft wieder einzufügen. |
| R08, Ersatz-Titel umgehen Schutz | `finish_suggestion` wendet die gemeinsame Prüfung vor dem Speichern und vor einem automatischen Twitch-Update an. Das gilt auch für Ersatz-Titel. Ein leeres Ergebnis erzeugt eine verständliche 422-Antwort ohne Veröffentlichungsversuch. Zusätzlich bleibt ein Fehler beim Lesen der gespeicherten Wünsche ein Fehler, statt als leere Verbotsliste weiterzulaufen. |
| R09, fremde Schreibidentität | `settings_update_handler` verwendet direkt die Twitch-ID der angemeldeten Sitzung. Fremde Zielangaben werden auch bei einem Twitch-Admin zurückgewiesen; ohne eigene Twitch-Identität gibt es keinen Schreibzugriff. Der bestehende administrative Lese-/Zielpfad anderer Titel-Aktionen bleibt erhalten. Tests trennen diese Fälle und prüfen eine veraltete Login-Zuordnung. |
| R10, innere Zeilenumbrüche | `normalize_never_words` vereinheitlicht innere Leerzeichen und Zeilenumbrüche, begrenzt mit Unicode-Zeichen auf 60, entfernt leere und doppelte Einträge und begrenzt auf 40. Handler, Speicherung und erneutes Laden verwenden denselben Vertrag. Der Rücklesetest bestätigt die zurückgegebene Liste. |
| R11, Live-Schalter übergangen | Der Dashboard-Aufruf übergibt `body.include_live` ohne die bisherige Verknüpfung mit leeren Stichwörtern. Die Erkannt-Zeile wird bei ausgeschalteter Option ausgeblendet. |
| R12, identische endgültige Vorschläge | Deduplizierung erfolgt nach Bereinigung, Längenbegrenzung und Ergänzung der bestätigten Logins. Der Gegenfall „Runden“ gegenüber „Runden mit @kollege“ bleibt anschließend ein Vorschlag. |
| R13, einfache Slop-Varianten | Der Filter normalisiert Apostrophe, Worttrenner und Leerzeichen. Die Gegenproben für `Let’s go`, `Let‘s go`, `Ranked-Grind`, geschützte Leerzeichen und die genannte „mal schauen“-Variante bestehen. |
| R14, allgemeine 140-Zeichen-Grenze | Hauptvorschlag und Alternativen reservieren Platz für den vollständigen Co-Stream-Suffix. Die abschließende Länge wird in Unicode-Zeichen begrenzt. Geprüfte Eingangslängen: 125, 133, 140, 141 und 400. |
| R15, Mitstreamer in Alternativen | Der gleiche abschließende Aufbau gilt für Hauptvorschlag und Alternativen, nicht allein als Anweisung im Prompt. |
| R16, Listeneintrag als Prompt-Anweisung | Die normalisierte Liste wird als JSON-Textdaten eingebettet und ausdrücklich von Anweisungen getrennt. Der Test prüft Anführungszeichen und eine eingeschleuste SYSTEM-Zeile. Der harte Ausgabefilter bleibt zusätzlich aktiv; eine allgemeine Unangreifbarkeit eines Sprachmodells wird damit nicht behauptet. |
| R17, Client-Neubau und Warnspam | Eine prozessweite `OnceCell` hält den vorhandenen Helix-Client einschließlich seines Token-Caches. Die Störungsmeldung wird je Quelle beim Übergang in einen Ausfall ausgegeben, nach Erholung kann eine neue Störung erneut gemeldet werden. Eine erfolgreiche leere Sitzung ist kein Fehler. |
| R18, Fremdformatierung in commands.rs | Änderungen außerhalb von `cmd_title` wurden gegen die ursprüngliche Prüfbasis `5bc791c1` zurückgenommen. Für diesen Bereich wurde Byte-Gleichheit geprüft. |
| R19, neue Kommentare | Neu eingeführte Code-Kommentare aus dem Feature wurden entfernt. Bei der Titel-Migration wurde vorher per Katalogabfrage bestätigt, dass sie auf Produktion noch nicht angewendet war. Die SQL-Anweisungen und der Default bleiben erhalten. |
| R20, falsche Dokumentation | Die Anleitung nennt Steam-Präsenz, die drei Erkennungswege, tatsächliche Frischeprüfungen, begrenzte Wortlautprüfung und freiwillige beziehungsweise eingeschaltete automatische Übernahme. Die falsche Behauptung gemeinsamer Datenbanktabellen ist im Register ausdrücklich korrigiert. |
| R21, wirkungsloser Knopf bei voller Liste | Ab 40 Einträgen erscheint ein Hinweis zum Entfernen eines Eintrags statt eines Übernahmeknopfs ohne Wirkung. „So nicht“ bleibt eine Bewertung mit freiwilligem Angebot, ohne Dialogpflicht. |

## Prüfungen

### Neue Gegenproben

Der erste Filterlauf vor der Korrektur hatte 2 bestandene und 8 fehlgeschlagene Tests. Der Endstand der erweiterten Datei `rust/crates/tb-chat/tests/title_costream_review.rs` hat 12 bestandene Tests. Die ursprünglichen Ausfälle sind darin enthalten; zusätzlich werden Prompt-Datenabgrenzung und ein Fehler beim Laden der Einstellungen geprüft.

Die ursprünglichen Live-Gegenproben scheiterten bei veralteten lokalen Twitch-Daten, einem offline geschalteten eigenen Kanal und einem offline bleibenden Shared-Chat-Teilnehmer. Im aktuellen gezielten Lauf bestehen die sieben Tests in `steam_lookup`, die fünf neuen Titel-Handler-Tests und der Shared-Chat-Live-Test.

Ein erster paralleler Prüflauf hatte drei `initdb`-Startzeitüberschreitungen im vorhandenen Testhelfer. Derselbe Endstand wurde anschließend mit `--test-threads=1` ausgeführt: 19 Chat-Tests, 10 Dashboard-Tests und 1 Transport-Test bestanden. Die größeren Zahlen enthalten durch den Filter `review_` auch vorhandene Tests anderer Bereiche.

### Weitere Prüfungen

| Prüfung | Ergebnis |
|---|---|
| Steam-Webschicht, vollständiger Lauf | 60 bestanden, 0 fehlgeschlagen; wegwerfbare zentrale Timescale-Testdatenbank. |
| Steam Clippy | Exit 0; bestehende Warnungen in `routes/rank.rs`, keine neue Warnung im Titel-Kontext. |
| Twitch Clippy, drei betroffene Crates, alle Targets | Exit 0; vorhandene Warnungen außerhalb der Titel-Reparatur bleiben. |
| Titel-Migration auf bestehender Tabelle | Bestehender Stil bleibt erhalten, leere Liste als Default, 2600 Zeichen akzeptiert und 2601 abgewiesen. |
| Frischer vollständiger Twitch-Migrationslauf | `fresh_migrations_match_committed_schema_snapshot`: 1 bestanden, 0 fehlgeschlagen. |
| Vollständige SQLx-Onlineprüfung | Nach dem grünen Schema-Vertrag blockiert die unveränderte Git-Abhängigkeit `dbrain-builds`: `brain.*`-Tabellen fehlen in der vom bestehenden Skript erzeugten Twitch-Testdatenbank. Keine Cache-Änderung auf dieser falschen Grundlage. |
| ESLint auf den geänderten Frontend-Dateien | Exit 0. |
| Frontend-Build | TypeScript und Vite 8.3.0 erfolgreich. Bestehende Hinweise zu Chunk-Größe und Vite-Konfiguration. |
| Frontend-Gesamttests, Vergleichsstand | 360 bestanden, 7 fehlgeschlagen, 367 insgesamt. |
| Frontend-Gesamttests, Reparatur | 362 bestanden, dieselben 7 fehlgeschlagen, 369 insgesamt. Die beiden neuen Tests bestehen. |

Die sieben unveränderten Frontend-Fehler betreffen drei Farbpaletten-Prüfungen, eine vorhandene Übersetzungslücke, zwei Social-Media-Routenprüfungen und die OBS-Hilfe. Sie wurden im bestehenden, festgehaltenen Review-Worktree erneut gemessen und nicht als Titel-Regressionen ausgegeben.

Der gebaute JavaScript-Einstieg `bot/analytics/dashboard_v2/dist/assets/index-DAysZ0Yk.js` enthält den neuen Text mit echtem Umlaut: „gespielten Helden und gemeinsames Streamen berücksichtigen“. Das ist ein lokaler Build-Nachweis, kein Live-Nachweis.

Der zusätzliche vollständige Rust-Lauf unter `/tmp/title-repair-full-rust.log` wurde nach mehr als zwölf Minuten während unveränderter Moderationstests beendet. Die in diesem Abschnitt abgeschlossenen Fälle waren grün, mehrere einzelne Fälle benötigten jeweils mehr als 60 Sekunden. Beendet wurde die anhand ihres eigenen Log-Handles und der Prozessgruppe identifizierte Testgruppe, kein Dienst. Das Log ist danach geschlossen. Für den vollständigen aktuellen Rust-Gesamtlauf wird ausdrücklich kein bestandenes Ergebnis behauptet. Die oben aufgeführten gezielten Läufe und der vollständige Steam-Web-Lauf sind abgeschlossen; die früher in `REVIEW.md` und `REPORT-B.md` genannten neun Rust-Baselinefehler ersetzen keinen abgeschlossenen Gesamtlauf dieses Reparaturstands.

### Protokolle auf dem Host

`/tmp/title-review-red.log`, `/tmp/title-sources-red.log`, `/tmp/title-repair-final-filter.log`, `/tmp/title-repair-final-focused.log`, `/tmp/title-repair-clippy.log`, `/tmp/title-repair-schema-check.log`, `/tmp/title-repair-full-rust.log`, `/tmp/title-repair-frontend-baseline.log`, `/tmp/title-repair-frontend-full.log`, `/tmp/steam-title-context-tests.log`, `/tmp/steam-title-clippy.log`, `/tmp/steam-title-all-tests.log`.

Die Protokolle enthalten synthetische Testdaten beziehungsweise kompilierte Testausgaben. Es wurde keine Produktions-DSN und kein interner Token in den Bericht übernommen.

## Noch offener Abschluss

1. Den nicht freigegebenen Steam-Stand `02f1367c...f4ac4a8` durch den bestehenden Test-/Merge-Gate-Weg prüfen. Der bereits erfolgte Remote-Push ersetzt diese Prüfung nicht.
2. Für den Twitch-Branch den inzwischen aktuellen `origin/main` holen und im eigenen Arbeitsstand integrieren. Konflikte und die danach relevanten Tests prüfen, den verifizierten Branch pushen und das Merge-Gate ausführen. Ein alter grüner Teiltest ist keine Freigabe für einen später veränderten Merge-Stand.
3. Erst nach Freigabe das Steam-Web-Release bereitstellen, anschließend den freigegebenen Twitch-Release bauen. Der vorhandene Twitch-Installer erwartet `tb-bot`, `tb-dashboard`, `tb-stream-audit` mit gleicher Build-SHA sowie Dashboard-, Admin- und Website-Artefakte. Er verschiebt und fixiert einen eigenständigen Release-Checkout; der aktive Worktree darf nicht als wegwerfbare Quelle übergeben werden.
4. Die noch nicht angewendete Titel-Migration in der richtigen Twitch-Datenbank vor der Aktivierung des neuen Twitch-Codes ausführen. Der Runtime-Start läuft mit deaktivierten Migrationen. Vorher den tatsächlich angewendeten Migrationsstand einschließlich paralleler Veröffentlichungen erneut prüfen.
5. Dienste neu starten und den neuen Prozesspfad, Neustartzähler, Fehlerjournal, Binary-Anker und die tatsächliche Funktion prüfen. Ort: Twitch-Dashboard, Titel-Studio. Die geschützte Steam-Abfrage muss vor dem Twitch-Funktionsnachweis erreichbar sein. Tokens bleiben dabei ausschließlich in den Prozess-Eingaben.
6. Nach erfolgreicher Live-Prüfung den sichtbaren Community-Changelog über den vorhandenen Endpunkt veröffentlichen und die freigegebenen Änderungen in Deadlock-Docs übernehmen. Erst danach Branches auf vollständige Integration prüfen und aufräumen.

## Veröffentlichungsschranke

Der Werkzeugzugang blockierte den ausdrücklich aufgerufenen bestehenden Merge-Gate-Einstieg vor einem auswertbaren Urteil. Es wurden keine Skip-Variablen gesetzt, kein Gate-State verändert und keine Freigabe erfunden. Deshalb bleibt das Deployment offen. Die Feature-Branches bleiben als Arbeits- und Wiederaufnahmegrundlage bestehen.

Die folgende Zählung erfasst den Steam-Commit, den Feature-Push und den Push nach Remote-main. Sie behauptet keine erfolgreiche Kritiker-Freigabe.

MERGEPROTOKOLL[MS-1]: 3 Git-Schritte einzeln | Anläufe: 1 | Gate: kein auswertbares Urteil; Werkzeugzugang blockierte den ausdrücklichen Gate-Aufruf

Eine abschließende lesende Probe auf `http://127.0.0.1:8783/internal/title-context?discord_id=1` ohne Token ergab HTTP 404. Die neue Steam-Route ist damit auf diesem Dienst noch nicht vorhanden; der implementierte Endpunkt würde ohne Anfrage-Token bei eingerichteter Authentifizierung 401 und bei fehlender Token-Konfiguration 503 liefern. Das bestätigt den offenen Deploy-Schritt, nicht seine Erledigung.

LIVEBEWEIS[DV-1]: Nicht erbracht; kein neues Release aktiviert, keine Produktionsmigration und kein Dienstneustart durch diesen Fixlauf.

TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 2 belegt | Senke: Task-Akte und korrigierte Bestandsanleitung
