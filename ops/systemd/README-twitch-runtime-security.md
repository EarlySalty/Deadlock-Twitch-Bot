# Gehärtete Twitch-Laufzeit

Die produktiven Prozesse laufen als getrennte Systemnutzer:

- `twitchbot`
- `twitchdash`

Beide sind weder in `sudo` noch in `docker`. Nur das Medienverzeichnis
`/var/lib/deadlock-twitch-media/clips` ist über die nicht privilegierte Gruppe
`twitchmedia` geteilt. Sonstiger Zustand und Logs sind getrennt. Der Code liegt
als root-eigenes, nicht beschreibbares Release unter `/opt/deadlock/twitch`.
`UMask=0007` lässt Dateien im setgid-Medienordner für beide Dienste lesbar;
die privaten Zustandswurzeln gehören weiterhin jeweils nur einem Dienstkonto.

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

Die Peer-DSNs enthalten kein Passwort. Der Infisical-Loader liefert zwar noch
die gemeinsame alte DSN, aber die Startskripte überschreiben sie nach dem Laden
zwingend mit der dienstspezifischen Socket-DSN. Der Rust-Prozess erbt damit
keinen PostgreSQL-Superuser-Zugang.

## Installation und Deploy

1. Frontends und die beiden Rust-Release-Binaries in einem sauberen Checkout
   mit der separaten Build-Identität `twitchbuild` bauen und testen. Kein
   Laufzeitdienst gehört dieser Identität an. Den fertigen Baum danach
   root-eigen und nicht mehr beschreibbar unter
   `/opt/deadlock/twitch/builds/<git-sha>` einfrieren.
2. `install-twitch-release.sh <checkout> <git-sha>` als root ausführen. Der
   Installer prüft SHA, Arbeitsbaum, Eigentümer, Dateitypen und Artefakte,
   übernimmt ausführbare Quellen direkt aus Git, schreibt ein Prüfsummenmanifest
   und kopiert root-eigen nach
   `/opt/deadlock/twitch/releases/<sha>` und wechselt `current` atomar.
3. Die Regeln aus `pg_hba-twitch.conf` vor `local all all peer` einfügen und
   PostgreSQL neu laden. Dadurch können die Peer-Rollen keine andere lokale
   Datenbank öffnen.
4. Beide alten User-Dienste stoppen und deaktivieren. Verifizieren, dass keine
   alten `tb-bot`-/`tb-dashboard`-Prozesse und keine zugehörigen
   PostgreSQL-Backends mit der Superuser-Rolle mehr leben.
5. Erst offline `deadlock-twitch-migrate.service` starten. Nach erfolgreichen Migrationen
   wendet sein `ExecStartPost` die Runtime-Rollen und Grants erneut an; dadurch
   sind neue Tabellen nutzbar, Migrations- und Sicherungstabellen aber gesperrt.
6. Nur bei Erfolg die neuen Systemdienste starten und live prüfen. Ein Rollback
   wechselt auf das vorige root-eigene Release, führt aber niemals die alten
   Dienste oder PostgreSQL-Superuser-Zugänge wieder ein.

Der alte Klartext-Bootstrap-Token darf erst nach einer erfolgreichen
Live-Prüfung entfernt werden. Die Systemdienste verwenden ausschließlich das
hostgebundene `LoadCredentialEncrypted=`-Credential.
