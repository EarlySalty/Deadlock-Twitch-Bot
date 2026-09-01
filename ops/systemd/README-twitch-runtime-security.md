# Gehärtete Twitch-Laufzeit

Die produktiven Prozesse laufen als getrennte Systemnutzer:

- `twitchbot`
- `twitchdash`
- `twitchaudit`
- `twitchdownload`

Alle vier sind weder in `sudo` noch in `docker`. Nur das Medienverzeichnis
`/var/lib/deadlock-twitch-media/clips` ist über die nicht privilegierte Gruppe
`twitchmedia` zwischen Bot und Dashboard geteilt. `twitchdownload` besitzt eine
eigene primäre Gruppe, ist in keiner weiteren Gruppe und hat keinerlei
Medienpfad-, Datenbank- oder Credential-Rechte. Sonstiger Zustand und Logs sind
getrennt. Der Code liegt
als root-eigenes, nicht beschreibbares Release unter `/opt/deadlock/twitch`.
`UMask=0027` lässt Dateien in den setgid-Medienordnern für beide Dienste lesbar,
aber nicht gegenseitig beschreibbar. Der Clips-Root samt Quellen und
`rendered/` gehört `twitchbot:twitchmedia` (2750), `uploads/` samt
Streamer-Unterverzeichnissen `twitchdash:twitchmedia` (2770). Nur der feste
interne Ordner `uploads/.dashboard-work` bleibt `twitchdash`-eigen und 0700;
dort liegende reguläre Upload-Tempdateien bleiben 0640. So kann das Dashboard
nur Uploads anlegen und fertige Render lesen; der Bot kann veröffentlichte
Uploadpfade für die Retention löschen, aber das Dashboard keine Renderziele
vorpflanzen. Exakt `clips/.preparation-work` und
`clips/.retention-quarantine` sind zusätzlich `twitchbot`-eigene 0700-
Verzeichnisse auf demselben Dateisystem. Darin sind nur die fest benannten,
regulären 0640-Arbeitsdateien mit Linkzahl eins erlaubt. Retention-Quellen
behalten beim atomaren Verschieben ihren Bot- oder Dashboard-Eigentümer;
Render-Quarantäne und Preparation-Artefakte gehören immer dem Bot. Die privaten
Zustandswurzeln gehören weiterhin jeweils nur einem Dienstkonto.
Unter `uploads/` ist genau eine Ebene aus kleingeschriebenen sicheren
Streamer-Slugs erlaubt; jede reguläre Datei darin heißt ausschließlich
`manual:<serverseitige-UUIDv4>.mp4`. Clientseitige Clip-IDs, weitere Ebenen und
abweichende Dateinamen werden fail-closed abgelehnt. Der einzige interne
Dashboard-Tempname bleibt `.dashboard-work/.upload-<32-kleine-Hexzeichen>.tmp.mp4`.
Bot und Coaching-Audit teilen ausschließlich den root-eigenen, nicht
beschreibbaren STT-Werkzeugbaum über die Gruppe `twitchstt`; Daten- und
Secret-Verzeichnisse sind darüber nicht erreichbar.

Schemaänderungen laufen ausschließlich über `deadlock-twitch-migrate.service`
als lokaler PostgreSQL-Systemnutzer. Bot und Dashboard starten mit
`TB_DB_MIGRATE=0` und verwenden eigene, nicht privilegierte Datenbankrollen.
Der Bot hat keinen Zugriff auf Web-Sessions, Admin-Audits, Affiliate-PII und
Billing-Tabellen; das Dashboard darf EventSub-Transporttabellen nur lesen.
Neue Tabellen erhalten erst durch die geprüfte Rollenmatrix Laufzeitrechte.
Der Migrator bleibt absichtlich ein kurzlebiger, gehärteter `postgres`-One-shot:
die vorhandenen Tabellen und Timescale-Objekte gehören bereits `postgres`.
Eine erzwungene Eigentumsumschreibung wäre beim Live-Cutover riskanter, ohne
einen dauerhaften Angriffsprozess zu beseitigen.

Das Infisical-Bootstrap-Credential wird mit `systemd-creds` hostgebunden
verschlüsselt und über `LoadCredentialEncrypted=` nur in den jeweiligen
Mount-Namespace gereicht. Der inhaltliche Umbau auf getrennte Infisical-Pfade
ist bewusst nicht Teil dieses Schritts.
Die rclone-Konfiguration des Coaching-Audits liegt ebenfalls nur als
hostgebunden verschlüsseltes systemd-Credential vor; der Audit-Prozess erhält
die entschlüsselte Laufzeitkopie ausschließlich in seinem privaten
Credential-Verzeichnis.

Die Peer-DSNs enthalten kein Passwort. Der Infisical-Loader liefert zwar noch
die gemeinsame alte DSN, aber die Startskripte überschreiben sie nach dem Laden
zwingend mit der dienstspezifischen Socket-DSN. Der Rust-Prozess erbt damit
keinen PostgreSQL-Superuser-Zugang. Das gemeinsame Legacy-Secret zeigt nur noch
auf die eingeschränkte Übergangsrolle `twitchlegacy`; sie besitzt breite DML-
Kompatibilität für noch nicht getrennte lokale Dienste, aber keinerlei DDL-,
Superuser-, Rollen- oder Dateisystemrechte. Die fachliche Aufteilung dieser
Restdienste folgt zusammen mit den späteren getrennten Infisical-Bereichen.

Für die Social-Clip-Materialisierung läuft der netzfähige yt-dlp-Prozess
ausschließlich im socket-aktivierten `tb-media-downloader` unter
`twitchdownload`. Der Bot übergibt über einen
`SOCK_SEQPACKET`-Socket genau ein bereits sicher geöffnetes, leeres Ausgabe-FD;
das eng begrenzte Protokoll prüft Credentials, Frame, FD-Typ, Linkzahl und
Offset. Der Helfer erhält weder Infisical noch DB- oder Provider-Konfiguration
und sieht den Medienbaum nicht. Seine versionierte Resolverdatei enthält nur
die vier [offiziellen Cloudflare-Resolver](https://developers.cloudflare.com/1.1.1.1/ip-addresses/).
Eine eigene nftables-Tabelle erlaubt für genau die UID `twitchdownload` nur DNS
zu diesen vier Adressen sowie öffentliches TCP 80/443; lokale, private,
Link-local-, Metadaten-, Multicast-, reservierte, IPv4-gemappte und NAT64-Ziele
werden vorher verworfen. Einzige Protokollausnahme sind die vier notwendigen
IPv6-Neighbor-Discovery-Typen zu Link-local/Multicast mit Hop-Limit 255; ohne
sie wäre auch der erlaubte öffentliche IPv6-Pfad technisch unerreichbar. Die
systemd-Adressfilter bleiben Defense-in-depth:
auf diesem cgroup-v1-Host wirken sie allein nachweislich nicht. Vor jeder
Accept-Instanz lädt deshalb eine kurze root-eigene Refresh-Unit den exakten
versionierten Regelsatz erneut. Sie verwendet keine Instanzdaten in
privilegierten Argumenten.

## Installation und Deploy

1. Frontends und die vier Rust-Release-Binaries (`tb-bot`, `tb-dashboard`,
   `tb-stream-audit`, `tb-media-downloader`) in einem eigenständigen Clone
   mit der separaten Build-Identität `twitchbuild` bauen und testen. Das zuvor
   offizielle, eigenständige Linux-Artefakt `yt-dlp_linux` der fest gepinnten
   Version `2026.08.19` außerhalb des Build-Helfers von
   `https://github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19` beziehen und gegen
   `SHA2-256SUMS` und `SHA2-256SUMS.sig` derselben unveränderlichen Release
   prüfen. Der Signaturschlüssel `public.key` kommt aus dem zugehörigen
   Release-Commit `3a08beaf031ab68f966401ead017ac81fe8486cf`, nicht aus einem
   beweglichen Branch; sein vorab abzugleichender
   Fingerprint ist `AC0C BBE6 848D 6A87 3464 AF4E 57CF 6593 3B5A 7581`.
   `ops/systemd/yt-dlp-linux-2026.08.19.sha256` hält daraus genau den Eintrag für
   `yt-dlp_linux` fest; Build-Helfer und root-seitiger Installer setzen ihn
   unabhängig voneinander zwingend durch.
   Das geprüfte, ausführbare Artefakt ausdrücklich mit
   `rust/scripts/stage-yt-dlp-release.sh <artefaktpfad>` nach
   `rust/target/release/yt-dlp` übernehmen; der Helfer lädt nichts herunter und
   sucht weder in `PATH` noch in `HOME`. Python-/pipx-Launcher werden durch die
   feste Standalone-Prüfsumme ausgeschlossen. Kein
   Laufzeitdienst gehört dieser Identität an. Den fertigen Baum danach
   root-eigen und nicht mehr beschreibbar unter
   `/opt/deadlock/twitch/builds/<git-sha>` einfrieren. Arbeitsbaum und HEAD
   werden noch als `twitchbuild` geprüft; root führt auf dem Checkout bewusst
   kein `git status` aus.
   Auf dem Zielhost muss außerdem das root-eigene, nicht schreibbare und weder
   setuid- noch capability-behaftete `/usr/bin/bwrap` mindestens in Version
   `0.9.0` installiert sein. Der Installer prüft Pfad, Typ, Eigentümer,
   Linkzahl, Modus, Capabilities, Version und die tatsächlich benötigten
   FD-Bind-/Namespace-Optionen. Vor dem Rechtecutover muss außerdem je eine
   echte, unprivilegierte Read-only-Input-/Read-write-Output-FD-Probe als
   `twitchbot` und `twitchdash` gelingen; sonst bricht der Installer ab. Bot-
   und Dashboard-Unit erlauben Bubblewrap nur die Namespace-Typen
   `user`, `mnt`, `pid` und `net`; das Dashboard braucht sie ausschließlich für
   die fail-closed Medienprüfung manueller Uploads. Die übrigen Dienste behalten
   das vollständige Namespace-Verbot. Die Sandbox bindet aus
   `/etc/alternatives` nur die zwei für FFmpeg/ffprobe nachgewiesenen,
   root-verwalteten BLAS-/LAPACK-Symlinks ein. Vor dem Bubblewrap-Exec werden
   sämtliche fremden Dateideskriptoren auf `CLOEXEC` gesetzt und nur die exakt
   benötigten Medien-FDs wieder freigegeben. Harte AS-/CPU-/Dateigrößen-/FD-
   Grenzen, feste Encoder-Threads und `TasksMax=64` pro Dienst begrenzen auch
   fehlerhafte oder bösartige Medienparser.
2. Noch vor dem Offline-Cutover die festen Hostvoraussetzungen herstellen.
   `twitchdownload` wird als Systemkonto mit eigener leerer Primärgruppe,
   Home `/nonexistent`, Shell `/usr/sbin/nologin` und ohne Zusatzgruppen
   angelegt. Das Konto darf insbesondere nicht Mitglied von `twitchmedia`
   sein. Nur auf einem vorher als nicht installiert bestätigten Host das
   signierte Ubuntu-Paket `nftables:amd64=1.0.9-1ubuntu0.1` installieren. Das
   vorab aus den Ubuntu-Paketmetadaten geprüfte Deb hat SHA-256
   `d648859fccde3c17b8b9b89993427564f91f394a39306db1f67bd5ea3ac42c6e`.
   Sein Postinst startet bei einer Neuinstallation keinen Dienst; bei einem
   Upgrade kann es dagegen einen zuvor installierten Dienst neu starten.
   Deshalb ist ein vorhandenes oder abweichendes Paket ein Abbruchgrund und
   kein automatischer Upgradepfad. Nach der Installation müssen
   `/usr/sbin/nft` exakt Version `1.0.9`, `nftables.service` `disabled` und
   `inactive` sowie die Paketdateien unverändert sein. Den generischen Dienst
   niemals starten, enablen, stoppen oder disablen: sein `ExecStop` kann den
   gesamten nft-Regelsatz einschließlich fremder Regeln leeren. Der Installer
   prüft Paket, dynamisch gelinkte Pakete, Binärpfad, Version,
   `meta skuid`-Unterstützung und den Managerzustand erneut und bricht
   fail-closed ab; es gibt keinen iptables-Fallback.
3. Die Regeln aus `pg_hba-twitch.conf` vor den allgemeinen Local- und
   Host-Regeln einfügen und PostgreSQL neu laden. Positiv gegen
   `twitch_analytics` sowie negativ gegen `postgres` und eine weitere lokale
   Datenbank prüfen. Dadurch können weder die Peer-Rollen noch `twitchlegacy`
   eine andere lokale Datenbank öffnen.
4. Den geprüften Installer root-eigen nach
   `/usr/local/sbin/install-twitch-release` installieren, das aktuelle Ziel von
   `/opt/deadlock/twitch/current` als Rollback-SHA protokollieren. Bot- und
   Dashboard-Systemunit persistent disablen und zusammen mit dem
   Downloader-Socket unter `/run/systemd/system` exakt auf `/dev/null` maskieren;
   erst danach Bot, Dashboard, Coaching-Audit, den Downloader-Socket und alle
   vorhandenen Downloader-Instanzen vollständig stoppen. Die Runtime-Masken
   verhindern manuelle Aktivierung im Offline-Fenster; die entfernten
   `*.wants`-Symlinks verhindern nach einem Reboot den automatischen Start,
   obwohl `/run` dann leer ist. Der Installer verlangt für Bot, Dashboard und
   Socket gleichzeitig `masked-runtime`, `inactive` und keinerlei persistenten
   Enable-Symlink. Er verändert diesen Zustand nicht selbst. Für den Medienrechte-
   Cutover müssen insbesondere **Bot und Dashboard gleichzeitig gestoppt** sein;
   der Installer bricht ab, solange unter `twitchbot` oder `twitchdash` noch ein
   Prozess lebt. Beim ersten Cutover auch
   die drei alten User-Dienste stoppen und **persistent** deaktivieren; deren
   Enable-Symlinks dürfen auch nach einem Reboot nicht zurückkehren. Verifizieren,
   dass kein
   `tb-bot`-, `tb-dashboard`- oder `tb-stream-audit`-Prozess und kein zugehöriges
   PostgreSQL-Backend mit der Superuser-Rolle mehr lebt. `current` darf niemals
   bei laufenden Prozessen gewechselt werden. Unmittelbar vor dem Stop zusätzlich
   read-only bestätigen, dass die Live-Queue keine Zeile im Zustand
   `processing` enthält; ein unerwarteter Treffer wird zuerst reconciled und
   nicht durch den Deploy überfahren.
5. Ausschließlich die root-eigene Installer-Kopie als
   `sudo /usr/local/sbin/install-twitch-release <checkout> <git-sha>` ausführen,
   niemals das Skript direkt aus dem Build-Checkout. Seine feste GNU-`env -S`
   Shebang leert die Umgebung technisch vor dem Start von `/usr/bin/bash` und
   verhindert damit insbesondere geerbte `BASH_ENV`, Shell-Funktionen und
   Tool-Optionen. Der Installer
   prüft SHA, Eigentümer, Dateitypen, yt-dlp-Pin, Bubblewrap, das isolierte
   Downloader-Konto, nftables und alle Artefakte,
   übernimmt ausführbare Quellen direkt aus Git, schreibt ein Gesamtmanifest
   und kopiert root-eigen nach `/opt/deadlock/twitch/releases/<sha>`. Erst danach
   wechselt er `current` atomar. Vorher versiegelt er ausschließlich den exakten Pfad
   `/var/lib/deadlock-twitch-media/clips`, lehnt Symlinks, Sonderdateien,
   Hardlinks, fremde Mounts und unerwartete Unterverzeichnisse ab und setzt die
   getrennten Writer-Rechte. Einzige enge Ausnahme vom Hardlink-Verbot ist ein
   vollständig innerhalb des Baums nachgewiesenes Zweierpaar aus exakt einer
   serverbenannten Temp-MP4 unter `uploads/.dashboard-work` und exakt einem
   finalen MP4-Pfad unter `uploads/<streamer>/`; das bewahrt den forensischen
   Stand eines unklaren DB-Commits. Der Installer löscht solche Orphans nicht.
   Er lädt danach atomar nur die eigene nft-Tabelle, prüft als
   `twitchdownload` echte negative Loopback-/Privatnetz- und positive
   Cloudflare-DNS-/Twitch-HTTP(S)-Verbindungen und lässt die Tabelle bei einem
   Fehler fail-closed stehen. Vor dem Laden darf die Tabelle fehlen oder nur
   exakt ihre eine vorgesehene Base-Chain enthalten; zusätzliche Chains, Sets,
   Maps oder Objekte brechen vor jeder Mutation ab. Nach dem Laden muss die
   JSON-Livesicht exakt eine `output`-Chain mit `filter`/`output`/Priorität
   `filter`/Policy `accept`, zwölf Regeln und keinerlei weitere Objekte zeigen.
   Aus dem versiegelten Release ersetzt er atomar
   genau die Bot- und Dashboard-Unit, Downloader-Socket und -Template sowie
   Basis- und per-Instanz-Firewall-Unit unter `/etc/systemd/system/`. Erst wenn
   alle sechs Dateien erfolgreich ersetzt sind, folgt genau ein
   `systemctl daemon-reload`; gestartet oder enabled wird nichts. Danach wird
   `current` atomar gewechselt. Die Dienste bleiben bis nach Migration und
   Prüfung gestoppt.
6. Weiterhin offline `deadlock-twitch-migrate.service` starten. Nach erfolgreichen Migrationen
   wendet sein `ExecStartPost` die Runtime-Rollen und Grants erneut an; dadurch
   sind neue Tabellen nutzbar, Migrations- und Sicherungstabellen aber gesperrt.
   Vor einem Dienststart prüfen: alle Settings stehen zunächst auf
   `release_mode = 'prepare_only'`, jede vorhandene Clip-Zeile besitzt genau eine
   Preparation-Zeile, der Insert-Trigger ist aktiv und die vier Provider-Lease-
   Spalten samt Partial-Unique-Indizes sind vorhanden. Eine vor der Migration
   vorhandene Legacy-Zeile `processing` ohne Provider-Marker muss danach mit
   nicht leerem `provider_started_at` im Zustand `reconciliation_required`
   stehen, darf weder geclaimt noch automatisch erneut versucht werden und
   trägt den stabilen Fehlercode `legacy_processing_requires_reconciliation`.
   Beim ersten Lauf mit dem
   Bestand vom 1. September 2026 müssen exakt 125 Layout-Snapshots gesichert und
   exakt diese 125 Overrides auf `NULL` normalisiert sein (115 globaler Default,
   10 Kanaldefault). Bot und Dashboard müssen die vorgesehenen Rechte auf
   Preparation, Queue und Settings besitzen, aber keinerlei Recht auf
   `social_media_layout_override_migration_backup_20260901`.
7. Das gemeinsame `TWITCH_ANALYTICS_DSN`-Secret auf `twitchlegacy` umstellen,
   alle verbleibenden Verbraucher neu starten und über `pg_stat_activity`
   verifizieren, dass kein TCP-Backend mehr als `postgres` verbunden ist. Erst
   danach das TCP-Passwort der Rolle `postgres` entfernen.
8. Das verschlüsselte rclone-Credential anlegen und als `twitchaudit` mit einer
   reinen Leseprobe prüfen. Nur bei Erfolg zuerst
   die drei Runtime-Masken entfernen und
   `deadlock-twitch-media-downloader.socket` sowie die neuen Bot-/Dashboard-
   Systemunits enablen. Zuerst nur den Downloader-Socket starten. Dabei müssen
   Basis-Firewall und Socket erfolgreich aktiv werden, der Socket
   `root:twitchbot`/0660 gehören und der generische `nftables.service` weiterhin
   disabled/inactive bleiben. Eine echte Accept-Instanz muss über den geerbten
   Seqpacket-FD trotz `InaccessiblePaths=/run` und
   `RestrictAddressFamilies=AF_INET AF_INET6` vollständig antworten; sie darf
   weder AF_UNIX neu öffnen noch Credentials, DB- oder Medienpfade sehen. Die
   negative Loopback-/Privatnetzprobe und die positive öffentliche
   DNS-/Twitch-Probe werden unter `twitchdownload` wiederholt. Erst danach Bot,
   Dashboard und Coaching-Audit starten. Bot, Dashboard, MCP, Audit, STT,
   Medienpfade und Datenbankrechte live prüfen; die Social-Media-Queue muss im
   Testbetrieb Clips aufbereiten, aber wegen `release_mode = 'prepare_only'`
   keinen Upload claimen. Der offizielle Twitch-Clip-Test muss über
   Seqpacket/SCM_RIGHTS in das gehaltene FD laden und anschließend ISO-BMFF-
   sowie sandboxed-ffprobe-Prüfung bestehen.

Für jeden Rollback zuerst Bot, Dashboard und Downloader-Socket persistent
disablen und erneut runtime-maskieren. Dann den Downloader-Socket stoppen, alle daraus bereits
erzeugten Instanzen anhand der von systemd aufgelisteten exakten Unitnamen
beenden und anschließend Bot, Dashboard und Audit stoppen. Keine Shell-Globs
oder breiten Prozesskills als Zielauflösung verwenden. Nach einer
fehlgeschlagenen Migration kann `current` bei weiterhin gestoppten Diensten
atomar auf die protokollierte SHA zurückgesetzt werden, sofern SQLx die Migration
nicht eingetragen hat. Die sechs systemweiten Unitdateien bleiben dabei zunächst
auf dem neuen, gehärteten Stand; sie referenzieren den atomaren `current`-Link.
Ist eine davon mit dem alten Release nicht kompatibel, muss ihre zuvor gesicherte
root-eigene Version ebenfalls per regulärer Tempdatei im selben
`/etc/systemd/system`-Verzeichnis atomar zurückgesetzt und danach genau einmal
`daemon-reload` ausgeführt werden. Downloader-Socket und -Instanzen bleiben bis
zur erneuten Gesamtprüfung gestoppt.

Nach einer **erfolgreichen** Migration ist ein blinder Bot-Rollback verboten:
ältere Binaries kennen `release_mode` und die Provider-Leases nicht und könnten
trotz `prepare_only` erneut veröffentlichen. Dann bleiben Bot und Upload-Pipeline
gestoppt, bis ein Forward-Fix bereitsteht. Provider-Zeilen mit
`provider_started_at` oder `reconciliation_required` werden nach einem Crash nie
automatisch erneut geclaimt; zuerst wird der externe Providerzustand manuell
abgeglichen. Alte User-Dienste und PostgreSQL-Superuser-Zugänge werden in keinem
Rollback reaktiviert.
Die getrennten Medienrechte werden bei einem Code-Rollback niemals wieder auf
einen gemeinsam schreibbaren Baum oder `UMask=0007` aufgeweicht. Kommt ein alter
Dienst damit nicht zurecht, bleibt er gestoppt und wird vorwärts repariert.
Ein `upload_commit_uncertain` wird ebenfalls nie durch Löschen oder automatischen
Retry „bereinigt“: DB-Zeile, Final-Link und die erhaltene Temp-Inode unter
`.dashboard-work` werden manuell abgeglichen. Erst nach eindeutigem Ergebnis darf
der Orphan gezielt entfernt oder der Upload erneut freigegeben werden; bis dahin
blockiert er bewusst denselben Zielnamen.

Die eigene Tabelle `inet deadlock_twitch_downloader` darf bei einem Rollback
stehen bleiben: ohne laufende `twitchdownload`-Prozesse verändert sie keinen
anderen Dienst und bleibt für einen versehentlichen Helperstart fail-closed.
Sie darf nur nach nachweislich gestopptem Socket und gestoppten Instanzen gezielt
entfernt werden. Weder dafür noch für Paket-Rollback darf
`systemctl stop nftables.service` verwendet werden. Ein root-privilegierter
fremder nftables-Manager kann die Tabelle während einer bereits laufenden
Downloadinstanz weiterhin löschen; das liegt außerhalb der Dienstgrenze. Der
per-Instanz-Refresh schließt diesen Zustand vor jeder neuen Verbindung, aber
nicht mitten in einem laufenden Root-Eingriff. Deshalb sind globale
nft-Regelsatz-Mutationen während des Betriebs untersagt und werden überwacht.

Alte Klartextkopien des Infisical-Bootstrap-Tokens und der rclone-Konfiguration
dürfen erst nach einer erfolgreichen Live-Prüfung entfernt werden. Danach die
alten Bootstrap- und Google/rclone-Tokens serverseitig widerrufen, neue
Credentials hostgebunden verschlüsseln und die Dienste erneut prüfen. Die
Systemdienste verwenden ausschließlich `LoadCredentialEncrypted=`.
