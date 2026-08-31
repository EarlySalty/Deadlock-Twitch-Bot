# Gehärtete Twitch-Laufzeit

Die produktiven Prozesse laufen als getrennte Systemnutzer:

- `twitchbot`
- `twitchdash`
- `twitchaudit`

Alle drei sind weder in `sudo` noch in `docker`. Nur das Medienverzeichnis
`/var/lib/deadlock-twitch-media/clips` ist über die nicht privilegierte Gruppe
`twitchmedia` geteilt. Sonstiger Zustand und Logs sind getrennt. Der Code liegt
als root-eigenes, nicht beschreibbares Release unter `/opt/deadlock/twitch`.
`UMask=0007` lässt Dateien im setgid-Medienordner für beide Dienste lesbar;
die privaten Zustandswurzeln gehören weiterhin jeweils nur einem Dienstkonto.
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

## Installation und Deploy

1. Frontends und die drei Rust-Release-Binaries in einem eigenständigen Clone
   mit der separaten Build-Identität `twitchbuild` bauen und testen. Kein
   Laufzeitdienst gehört dieser Identität an. Den fertigen Baum danach
   root-eigen und nicht mehr beschreibbar unter
   `/opt/deadlock/twitch/builds/<git-sha>` einfrieren. Arbeitsbaum und HEAD
   werden noch als `twitchbuild` geprüft; root führt auf dem Checkout bewusst
   kein `git status` aus.
2. Den geprüften Installer zuerst root-eigen nach
   `/usr/local/sbin/install-twitch-release` installieren und ausschließlich
   diese Kopie als root mit `<checkout> <git-sha>` ausführen. Niemals das
   Skript direkt aus dem Build-Checkout als root starten. Der
   Installer prüft SHA, Eigentümer, Dateitypen und Artefakte,
   übernimmt ausführbare Quellen direkt aus Git, schreibt ein Prüfsummenmanifest
   und kopiert root-eigen nach
   `/opt/deadlock/twitch/releases/<sha>` und wechselt `current` atomar.
3. Die Regeln aus `pg_hba-twitch.conf` vor den allgemeinen Local- und
   Host-Regeln einfügen und PostgreSQL neu laden. Positiv gegen
   `twitch_analytics` sowie negativ gegen `postgres` und eine weitere lokale
   Datenbank prüfen. Dadurch können weder die Peer-Rollen noch `twitchlegacy`
   eine andere lokale Datenbank öffnen.
4. Alle drei alten User-Dienste stoppen und deaktivieren. Verifizieren, dass keine
   alten `tb-bot`-/`tb-dashboard`-/`tb-stream-audit`-Prozesse und keine zugehörigen
   PostgreSQL-Backends mit der Superuser-Rolle mehr leben.
5. Erst offline `deadlock-twitch-migrate.service` starten. Nach erfolgreichen Migrationen
   wendet sein `ExecStartPost` die Runtime-Rollen und Grants erneut an; dadurch
   sind neue Tabellen nutzbar, Migrations- und Sicherungstabellen aber gesperrt.
6. Das gemeinsame `TWITCH_ANALYTICS_DSN`-Secret auf `twitchlegacy` umstellen,
   alle verbleibenden Verbraucher neu starten und über `pg_stat_activity`
   verifizieren, dass kein TCP-Backend mehr als `postgres` verbunden ist. Erst
   danach das TCP-Passwort der Rolle `postgres` entfernen.
7. Das verschlüsselte rclone-Credential anlegen und als `twitchaudit` mit einer
   reinen Leseprobe prüfen. Nur bei Erfolg die neuen Systemdienste starten und
   Bot, Dashboard, MCP, Audit, STT und Datenbankrechte live prüfen. Ein Rollback
   wechselt auf das vorige root-eigene Release, führt aber niemals die alten
   Dienste oder PostgreSQL-Superuser-Zugänge wieder ein.

Alte Klartextkopien des Infisical-Bootstrap-Tokens und der rclone-Konfiguration
dürfen erst nach einer erfolgreichen Live-Prüfung entfernt werden. Danach die
alten Bootstrap- und Google/rclone-Tokens serverseitig widerrufen, neue
Credentials hostgebunden verschlüsseln und die Dienste erneut prüfen. Die
Systemdienste verwenden ausschließlich `LoadCredentialEncrypted=`.
