# Eng begrenzte Deployvorprüfung 02

## Ergebnis

**BLOCKIERT.** Für den fest geprüften Stand sind **keine Migrationen ausstehend**. Alle 180 SQLx-Versionen sind erfolgreich angewandt und ihre tatsächlichen Datenbankchecksummen stimmen mit den unveränderten Quelldateien überein. Offen bleiben die Auswirkungen der nicht atomaren Rechte-Neuvergabe bei laufenden Diensten und ein ausreichend belegter, zulässiger Rückstellweg nach der Wrapper-Abnahme. Deshalb hier kein Deploy und keine positive Ausführungsfreigabe.

Ausschließlich gpt-6.1-sol. Keine neue allgemeine Codeprüfung. Nur die beiden beauftragten Berichte und eigene temporäre Nachweise wurden geschrieben. Kein Build, Git-Schreibzugriff, Deploy, Neustart, Datenbankschreiben oder Konfigurationswechsel. Keine Geheimnisse, ENV-Dateien, Passwortdateien, Nutzerdaten oder Rohjournale gelesen. Keine Browserarbeit, Delegation oder Sessionnachrichten.

## Feste Stände und Laufzeit

Endbeobachtung am 8. Oktober 2026 um 14:59 UTC:

- Remote-main: `f04c0ef03d47ce4893ee4d17cb98eeb4c2f6c473`, zu Beginn und am Ende mit `git ls-remote` ermittelt. Das ist der geprüfte geplante SHA, nicht der lokale main und kein bereits bereitgestelltes Release.
- Laufendes Release und current: `3341098f3a953e231c0ec5731141996b10f1b9b6`. Nicht der historische SHA aus OPS-PREFLIGHT.md.
- `/usr/local/bin/deploy-twitch-release --pruefen` zweimal erfolgreich, Exit 0. Alle vier Prozesse aktiv, keine exe als gelöscht markiert, alle auf dem aktuellen Release, NRestarts jeweils 0.

| Dienst | PID | Tatsächlicher Linux-Nutzer |
| --- | --- | --- |
| Bot | 1830251 | twitchbot, UID 995 |
| Dashboard | 1830151 | twitchdash, UID 991 |
| Coaching-Watch | 1830765 | twitchaudit, UID 987 |
| Kategorieerfassung | 1830776 | twitchcollector, UID 985 |

Die Identitäten wurden zusätzlich an den UID-Metadaten der Prozesse geprüft. Alle acht vorhandenen Release-Binaries tragen in `.twitch_build` exakt den laufenden SHA ohne `-dirty`. Dieser SHA ist Vorfahr des geplanten Commits. Acht Binaries und 180 Migrationsdateien stimmen mit dem Manifest unter `/opt/deadlock/twitch/releases/3341098f3a953e231c0ec5731141996b10f1b9b6/SHA256SUMS` überein. Das belegt vorhandene Rückstellartefakte, nicht die Ausführbarkeit eines Rückstellverfahrens.

## SQLx-Abgleich und Migrationseinheit

Der vorhandene lokale Peerzugang der Sessionrolle `nathanael` funktioniert ohne Zugangsdaten. Die Rolle ist laut Rollenmetadaten Superuser. Es wurde keine neue Rolle, Anmeldung oder Berechtigung eingerichtet und nicht zu postgres oder einem Dienstnutzer gewechselt. Jede Metadatenabfrage lief mit leerer geerbter Umgebung, `--no-psqlrc`, `--no-password`, `passfile=/dev/null`, zehn Sekunden statement_timeout, `default_transaction_read_only=on` und `BEGIN READ ONLY`.

Exakte Abfrage gegen `twitch_analytics` am Socket `/var/run/postgresql`, Port 5432:

```sql
BEGIN READ ONLY;
SELECT version, success, encode(checksum, 'hex')
FROM public._sqlx_migrations ORDER BY version;
ROLLBACK;
```

Für jede Quelldatei wurden die unveränderten Bytes mit `git show` am geplanten SHA aufgelöst und mit SHA-384, dem SQLx-Prüfsummenverfahren, verglichen.

| Abgleich | Ergebnis |
| --- | --- |
| Versionierte Migrationen am geplanten SHA | 180 |
| Tatsächliche SQLx-Einträge | 180 |
| Erfolgreich und checksummengleich | 180 |
| Fehlgeschlagene Versionen | 0 |
| Fehlende Versionen | 0 |
| Abweichende Checksummen | 0 |
| Datenbankversionen ohne Releasequelle | 0 |

**Ausstehende Migrationen: keine.** Deshalb gibt es hier keine ausstehende Migration mit Datenänderung, Löschung oder Tabellensperre einzeln freizugeben. Das ist durch den Datenbankabgleich belegt und nicht aus dem laufenden SHA abgeleitet. Die aktuelle Releasequelle enthält ebenfalls exakt dieselben 180 Migrationsdateien. Bereits angewandte Dateien wurden nicht verändert.

Die installierte Einheit `/etc/systemd/system/deadlock-twitch-migrate.service` ist bytegleich zur Quelle am geplanten SHA. Sie verwendet `/usr/local/libexec/cargo-sqlx`, SQL-Dateien aus current, `--no-dotenv` und die Datenbank `twitch_analytics`. Es handelt sich um externe SQLx-Dateien, nicht um hier nachgewiesene eingebettete Migrationen. Ihr `ExecStartPost` führt auch bei null ausstehenden Migrationen `/opt/deadlock/twitch/current/ops/systemd/twitch-runtime-roles.sql` erneut aus.

## Rechte, Peerregeln und laufender alter Code

Installierter Wrapper und Installer sind bytegleich zu den Quellen am geplanten SHA. Zwischen laufendem und geplantem Commit gibt es keine geänderten Dateien unter den Migrations- und Betriebsquellen. Die gelesenen aktuellen Releasequellen, einschließlich Rollenmatrix, Kategorie-Rollenskript, Peerinstaller und Startskripten, stimmen mit diesem Stand überein. Eine Übereinstimmung der Quellen ersetzt keine Prüfung ihres produktiven Zwischenzustands.

Die erwarteten Linux-/Datenbankbindungen sind durch Prozessidentitäten, Regeln und aktuelle Verbindungsmetadaten plausibel belegt:

- Bot und Dashboard verwenden Rollen `twitchbot` beziehungsweise `twitchdash`; aktuell elf und sieben lokale Verbindungen.
- `twitchcollector` hat sechs lokale Verbindungen, `twitchcontest` drei. Die Wettbewerbsschreibrolle ist per Peermap ausschließlich dem Linux-Nutzer twitchdash zugeordnet.
- `twitchanalysis` ist ebenfalls ausschließlich twitchdash zugeordnet. Diese Rolle hat DML-Rechte auf die drei Analysezustandstabellen und SELECT/INSERT/UPDATE auf `llm_usage`, ohne DELETE dort. Sie hat die benötigten USAGE/SELECT-Sequenzrechte, kein UPDATE der Sequenz.
- `twitchaudit` ist die lokale Verbrauchsrolle des Auditnutzers. Ihre Rechte auf `llm_usage` und dessen Sequenz entsprechen denselben begrenzten Verbrauchsrechten.
- Dienstrollen sind LOGIN, ohne Superuser, Rollenvererbung, CREATE DATABASE, CREATE ROLE, Replikation oder BYPASSRLS. Keine Rollenmitgliedschaften. Globale Rolleneinstellungen sind leer; die geplanten search_paths für twitch_analytics sind bereits vorhanden. CONNECT/USAGE sind vorhanden, CREATE auf public nicht.
- Der Bot hat keinen Tabellenzugriff auf `dashboard_sessions`, das Dashboard besitzt dort SELECT/INSERT/UPDATE/DELETE. Das Dashboard ist deshalb nicht pauschal datenbankweit nur lesend. Analysezustand bleibt für Bot und Dashboard gesperrt. `_sqlx_migrations` ist für sämtliche sieben Dienstrollen gesperrt. Der Raidvorlauf ist ausschließlich für den Bot schreibbar; Funktionseigentümer, gehärteter search_path und Arrivaltrigger stimmen mit den Wrapper-Prüfkriterien überein.

Der Peerinstaller würde Haupt-HBA, Twitch-Includedatei und Identdatei vor Änderungen sichern. Aktuell sind genau die erwarteten Rejectregeln in `/etc/postgresql/16/main/pg_hba-twitch.conf` vorhanden. Bot und Dashboard haben lokale Peerregeln für twitch_analytics und deadlock. Die Analyse- und Auditregeln stehen bereits an Position 1 bis 8; andere Datenbanken und IPv4-/IPv6-Netzwerkzugänge dieser beiden Rollen werden abgewiesen. Beide Peermaps sind eindeutig, Parsefehlerzahlen jeweils 0. Dateien sind regulär, ihre Änderungszeiten liegen vor dem beobachteten Konfigurationsreload am 8. Oktober um 04:02:49 UTC.

Grenze: `pg_hba_file_rules` und `pg_ident_file_mappings` beschreiben die Dateien, nicht allein einen erfolgreichen neuen Dienstlogin. Neue Verbindungen als twitchanalysis und twitchaudit wurden in dieser Prüfung nicht hergestellt. Geschützte installierte ENV-Konfigurationen wurden nicht gelesen. Die Quellen verlangen für neue Dashboard-Schreibpools den lokalen Socket; Analyse zusätzlich genau twitch_analytics. Diese Voraussetzung muss nach einer erlaubten Ausführung tatsächlich greifen.

## Timer und Neustarts

Die Verbrauchsnachlieferung ist **bereits installiert**. Installierter Wrapper, beide normalen TOML-Konfigurationen und Service-/Timerdateien sind checksummengleich zu den geplanten Quellen. Der Aktivierungslink `/etc/systemd/system/timers.target.wants/deadlock-twitch-llm-usage-recover.timer` existiert. Ob der Timer aktuell aktiv ist und erfolgreich läuft, wurde ohne zusätzlichen erlaubten Metadatenmodus nicht erhoben.

Der Deploy installiert diese Dateien dennoch erneut, lädt die Systemdienstdefinitionen neu und führt `enable --now` aus. Konfiguriert sind zwei Minuten nach dem Systemstart und fünf Minuten nach der letzten Serviceaktivierung. Der root-One-shot liest Verbrauchsmeldungen der letzten 30 Tage aus dem Journal und führt den Rust-Helfer als twitchbot aus. Dieser aktualisiert ausschließlich passende bereits gestartete Verbrauchsversuche; bereits abgeschlossene passende Einträge gelten als erledigt. Keine neuen Modellaufrufe, INSERTs oder DELETEs in diesem Nachlieferungspfad. Die tatsächlichen zu aktualisierenden Produktionszeilen wurden nicht abgefragt.

Ein späterer Aufruf mit ausschließlich twitch-bot und twitch-dashboard als Restart-Zielen ließe Audit und Kategorieerfassung ohne gezielten Neustart. Rollenmatrix, Peerinstaller und Timerinstallation laufen trotzdem. Ohne explizite Auswahl würde der Wrapper auch Audit und Kategorieerfassung neu starten. Nach zwei gezielten Restarts wäre ein gemischter SHA-Zustand der vier Dienste beabsichtigt; der unveränderte Vier-Dienste-Prüfmodus meldet dafür Exit 1. Dies darf nicht durch zusätzliche, nicht beauftragte Neustarts grün gemacht werden.

## Konkrete Blocker und Rückweg

1. **Rechte-Neuvergabe ist nicht atomar, während die alten Dienste weiterlaufen.** `/etc/systemd/system/deadlock-twitch-migrate.service:14` startet psql ohne eine Gesamttransaktion. Das Rollenskript hat keinen umschließenden BEGIN/COMMIT. Es entzieht zunächst Tabellen-/Sequenzrechte und vergibt anschließend breite DML-Rechte, bevor einzelne Bereiche wieder eingeschränkt werden. Auch Collector-, Wettbewerb-, Analyse- und Auditrollen werden neu gesetzt. Laufende Zugriffe können deshalb Zwischenzustände mit fehlenden oder vorübergehend erweiterten Rechten sehen. Bei einem späteren Skriptfehler bleiben frühere Statements wirksam. Eine ausreichende Absicherung oder Wiederherstellung dieses konkreten Betriebsrisikos ist nicht belegt. Quellen: `/opt/deadlock/twitch/releases/3341098f3a953e231c0ec5731141996b10f1b9b6/ops/systemd/twitch-runtime-roles.sql:59-76`, die nachfolgenden Einschränkungen und `/usr/local/bin/deploy-twitch-release:360-387,620-635`.
2. **Der belegte Rückweg endet zu früh.** `/usr/local/bin/deploy-twitch-release:335-354` stellt bei Fehlern vor der Abnahme nur current zurück. Migrationen und bereits veränderte Datenbankrechte werden nicht zurückgestellt. Schon in Zeile 609 endet diese Rückstellungspflicht, also vor Timerinstallation und Dienstneustarts. Ein dortiger Fehler bekommt keinen automatischen Programm-Rollback. Das alte Release und der alte Build sind vorhanden, aber ein erneuter regulärer Wrapper-Aufruf für den alten SHA scheitert am bereits vorhandenen Buildziel gemäß Zeilen 288-290. Ein anderer vorhandener zulässiger Rückstellweg für diesen Zustand ist hier nicht belegt. Kein neuer Zugang oder alternativer Schreibweg wurde eingerichtet.

Vorhandene Teilbelege: 36 root-eigene Peer-Sicherungsverzeichnisse, zuletzt `/var/backups/twitch-contest-peer.8vVP6H1g`, Modus 0700. Deren Inhalte und Wiederherstellbarkeit wurden nicht gelesen oder getestet. Der Peerinstaller besitzt eine interne Dateirückstellung bei seinem eigenen Fehler, aber keinen Gesamtrollback für Fehler nach seinem erfolgreichen Abschluss. Zusätzlich wurden reine ACL-Metadaten von 371 Relationen und 45 Spaltenrechten gesichert. Das ist keine vollständige Rechte-/Passwortsicherung und kein erprobter Restore.

Für twitch_analytics wurde kein konkretes wiederherstellbares Datenbanksicherungsartefakt belegt. `archive_mode=off` liefert hier keinen Beleg für laufende WAL-Archivierung. Daraus folgt nicht, dass außerhalb der geprüften Pfade keine Sicherung existiert. Bei derzeit null ausstehenden Migrationen ist kein Schema-Restore als unmittelbar geplanter Schritt identifiziert; der fehlende Gesamt-Rückweg bleibt wegen Rechte- und Laufzeitänderungen relevant.

## Erforderliche Nachprüfung erst nach zulässiger Ausführung

- Remote-main unmittelbar davor erneut auflösen. Bei anderem SHA betroffene Voraussetzungen neu vergleichen; dieser Bericht ist keine Freigabe für nachfolgende Fixstände.
- Herkunft sämtlicher neuer Artefakte an genau diesem SHA prüfen. Hier wurde kein neues Release gebaut oder geprüft.
- Für die ausgewählten Dienste PID-Wechsel, exe ohne Löschmarkierung, erwarteten Release-SHA und Neustartzähler belegen. Nicht ausgewählte Dienste bleiben auf ihrem bisherigen Prozessstand.
- Nur aggregierte neue Fehlernachweise mit Journalpriorität err verwenden, ohne Rohlogs oder Nutzerdaten auszugeben.
- Frische erlaubte Peer-Verbindungen und Rechte-Metadaten für Bot, Dashboard, Wettbewerb, Analyse und Auditverbrauch positiv prüfen; unzulässige Datenbank-/Netzwerkzugänge negativ prüfen. Die eingebaute Wrapper-Prüfung deckt insbesondere Analysepeer, Reload, Rejectregeln und zentrale Rechte ab, ersetzt aber nicht jeden genannten Dienstnachweis.
- Kontofrei `/healthz` und `/readyz` am Dashboard unter `http://127.0.0.1:8769` prüfen: JSON-Inhalt statt nur HTTP 200, healthz mit `ok=true` und `status=alive`, Readiness mit funktionsfähiger Datenbank und passendem internen API-Datenbankfingerabdruck. Keine Logins, Kontoverknüpfungen, Nachrichten oder Anbieteraktionen auslösen.
- SQLx-Versionen/Checksummen erneut abgleichen und Erfolg der Verbrauchsnachlieferung nur über begrenzte Metadaten prüfen.

Ein Deploy-Livebeweis `LIVEBEWEIS[DV-1]` wurde bewusst nicht erbracht: Es gab keinen Deploy, keinen PID-Wechsel und keinen Nachher-Funktionsbeweis. Die positive Funktionswirkung der Anwendungskorrekturen wird hier nicht aus Quellcode oder Dienststatus behauptet. Es gibt keine Zusage risikofreien Betriebs; die spätere Ausführungsentscheidung bleibt bei der Hauptsession.

## Nachweise

Alle temporären Belege liegen unter `/tmp/tb-deploy-pruefung-02-emf9nl_2/`. Die begleitende JSON-Datei enthält die genaue Abfrage, SHA-Bindung, tatsächlichen Versionschecksummen und Belegpfade. Wichtige Einzeldateien:

- `/tmp/tb-deploy-pruefung-02-emf9nl_2/closing-observations.json`
- `/tmp/tb-deploy-pruefung-02-emf9nl_2/sqlx-query.json`
- `/tmp/tb-deploy-pruefung-02-emf9nl_2/sqlx-metadata.txt`
- `/tmp/tb-deploy-pruefung-02-emf9nl_2/sqlx-comparison.json`
- `/tmp/tb-deploy-pruefung-02-emf9nl_2/planned-source-manifest.json`
- `/tmp/tb-deploy-pruefung-02-emf9nl_2/runtime-metadata-executed.sql`
- `/tmp/tb-deploy-pruefung-02-emf9nl_2/runtime-metadata.json`
- `/tmp/tb-deploy-pruefung-02-emf9nl_2/service-process-identities.json`
- `/tmp/tb-deploy-pruefung-02-emf9nl_2/acl-snapshot.json`
- `/tmp/tb-deploy-pruefung-02-emf9nl_2/rollback-artifact-integrity.json`
