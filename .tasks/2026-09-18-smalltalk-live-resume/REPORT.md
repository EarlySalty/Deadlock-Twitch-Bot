# Smalltalk-Live: fortgesetzter Deploy und Live-Nachweis

Stand der DB-Prüfung: 18.09.2026, 08:50:39 Uhr Europe/Berlin.

## Ergebnis

Die Kandidatenänderung war bereits als Commit
`69d1549d93c7ad6d1267ff5d2fe8568f8889b79a` auf `origin/main` vorhanden.
Der vorbereitete Release `44cc2aa90e1d1b59269533b5d50b643e56bfbba8`
war dagegen noch nicht fertig gebaut oder aktiviert. Diese Fortsetzung hat
den Build abgeschlossen, die Tests wiederholt, den Release installiert und
den tatsächlichen Datenbankpfad repariert.

Der Live-Prozess findet jetzt einen Kandidaten außerhalb der bisherigen
Roster-Auswahl und führt den offiziellen Helix-Refresh selbst aus. Zum
Prüfzeitpunkt gab es aber keinen zulässigen Mini-Streamer. Es wurde keine
neue Live-Testsession geöffnet. Ein echter Smalltalk-Chat und dessen
Gesprächsqualität sind damit noch nicht live nachgewiesen.

## Unveränderte Sicherheitsgrenzen

Live-State und stabile Twitch-ID dienen der Auswahl. Das System erzeugt
keine künstlichen Outreach-Einträge. Erforderlich bleiben eine offene
Live-Testsession, identische ID/Login, Nicht-Partner, keine Blacklist und
kein Cooldown, `smalltalk_live`, aktivierte Settings sowie 0 bis 49 Follower
aus einem maximal zwölf Stunden alten Helix-Nachweis. Die Live-Bestätigung
verfällt bereits nach 90 Sekunden. Fehlende, fehlerhafte oder zukünftige
Nachweise geben keinen Send frei.

Die finale Prüfung erfolgt nach einem möglichen Token-Refresh direkt vor
Chat-HTTP. Der Prozess-Latch verhindert eine zweite Session. Werbe-/Linkfilter,
lokale Transkription und Sieben-Sekunden-Modellfrist bleiben erhalten. Kein
OpenAI-Testpfad, kein Providerwechsel und kein serieller Fallback wurden
hinzugefügt.

## Wiederholte Prüfungen

Alle Befehle liefen gegen den gepinnten Release-Clone, mit `--locked` und
höchstens zwei Cargo-Jobs je Lauf. Die relevanten neuen DB-Sicherheitsfälle
nutzen temporäre lokale PostgreSQL-Cluster, nicht die Produktionsdatenbank.

| Prüfung | Ergebnis |
| --- | --- |
| `cargo test -p tb-engagement` | 255 bestanden: 235 Unit-Tests und 20 Integrationstests |
| Darin `smalltalk_loop_store` | 15 bestanden, einschließlich leerem Outreach-Roster, Followergates und One-Shot |
| `cargo test -p tb-bot smalltalk_loop_wiring` | 12 bestanden |
| `cargo test -p tb-transport-twitch` | 109 bestanden |
| `cargo check -p tb-bot` | Exit 0 |
| Clippy für Engagement, Bot und Twitch-Transport, alle Targets, `--no-deps` | Exit 0; Warnungen bleiben, unter anderem unnötige Clones in Tests |
| `git diff --check` für den Implementierungscommit | Exit 0 |
| Release-Build aller drei vorgeschriebenen Binaries | Exit 0 |

Prüfprotokolle auf dem Host:
`/home/nathanael/build/smalltalk-resume-verify-20260918.log` und
`/home/nathanael/build/smalltalk-resume-release-20260918.log`.
Die zugehörigen `.result`-Dateien enthalten jeweils `SUCCESS`.

## Deployment

Eigenständiger Clone mit internem `.git`, sauberer Git-Status. Die drei
Frontends waren bereits gebaut und wurden im vollständigen Release-Clone
wiederverwendet. Alle drei Rust-Binaries wurden fertig gebaut und ihre
ELF-Sektion `.twitch_build` zeigt exakt auf
`44cc2aa90e1d1b59269533b5d50b643e56bfbba8`.

Aufruf:

```text
/usr/local/bin/deploy-twitch-release 44cc2aa90e1d1b59269533b5d50b643e56bfbba8 /home/nathanael/repos/twitch-release-44cc2aa90e1d1b59269533b5d50b643e56bfbba8
```

Der Wrapper meldete Exit 0 und startete Bot, Dashboard und Coaching-Watch
neu. `/opt/deadlock/twitch/current` zeigt auf den neuen Release. Die
vollständige `SHA256SUMS`-Prüfung des installierten Releases liefert Exit 0.

Bot-PID vorher: `1480028`. Nachher: `2228240`, gestartet am 18.09.2026 um
08:43:16 Uhr Europe/Berlin. Dashboard-PID: `2228188`. Prozessargumente des
Bots zeigen auf `/opt/deadlock/twitch/current/rust/target/release/tb-bot`.
Coaching-Watch verwendet weiterhin seinen separat gepinnten Audit-Release;
dieser unabhängige Pfad wurde nicht umkonfiguriert.

MCP `http://127.0.0.1:8892/healthz`: `ok: true`.

## Entdeckte Deploy-Lücke und Behebung

Der installierte Wrapper wechselt den Release und startet Dienste neu,
führt aber entgegen der bisherigen Smalltalk-Dokumentation weder
Migrationen noch Runtime-Grants aus. Der Bot startet mit deaktivierten
Startmigrationen. Trotz grünem Healthcheck fehlte zunächst
`twitch_smalltalk_candidate_state` in der tatsächlich vom Bot verwendeten
Datenbank `twitch_analytics`.

Der reguläre installierte SQLx-Migrator bestätigte genau eine ausstehende
Migration: `20260918024500_smalltalk_candidate_state`. Sie wurde mit dem
bestehenden berechtigten lokalen DB-Zugang aus dem unveränderlichen
Release-Verzeichnis angewendet. Anschließend wurde die vollständige
versionierte `ops/systemd/twitch-runtime-roles.sql` in einer Transaktion
angewendet. Keine selbst gebauten Tabellen, keine manuell gesetzten
Migrationsmarker, keine erfundenen Followerwerte und kein generisches
`sudo`/`systemctl` wurden verwendet.

Der Host-Wrapper selbst wurde nicht verändert. Die Betriebsdokumentation
weist jetzt ausdrücklich auf den erforderlichen separaten Migrations- und
Rechteschritt vor zukünftigen Releasewechseln hin.

Verifiziert: Migration erfolgreich; `twitchbot` hat INSERT-Recht auf der
neuen Tabelle; `twitchdash` und `twitchlegacy` haben dieses Schreibrecht nicht.

## Tatsächlicher Live-Nachweis

Automatisch vom laufenden Bot geschriebener Eintrag:

| Feld | Gemessener Wert |
| --- | --- |
| Twitch-ID | `36167006` |
| Login | `1337cammy` |
| Quelle | `helix` |
| Follower | **1.655**, damit ausgeschlossen |
| Follower- und Live-Messzeit | 18.09.2026, 08:47:32 Uhr Europe/Berlin |
| Live Deadlock zum Messzeitpunkt | `true` |
| Letzter API-Fehler | keiner |
| Nächster vorgesehener Refresh | 18.09.2026, 09:02:32 Uhr Europe/Berlin |
| Neue Smalltalk-Sessions seit Beginn dieser Fortsetzung | keine |

Die kleine Viewerzahl wurde nicht als Followerzahl verwendet. Der
Follower-Refresh funktioniert tatsächlich mit der vorhandenen offiziellen
Anbindung. Ein zulässiger Kanal unter 50 Followern ist zum dokumentierten
Prüfzeitpunkt jedoch nicht vorhanden. Der One-Shot-Slot ist nicht durch
eine künstliche Session verbraucht worden.

## Grenzen des Nachweises

Die direkte Auflösung von `/proc/<pid>/exe` war mit dem verfügbaren
Betriebssystemkonto nicht erlaubt. Deshalb wird `Binary nicht (deleted)`
nicht als direkt geprüft ausgegeben. Neuer PID, Prozessargumente,
Release-Symlink, ELF-Herkunft und vollständiges Release-Manifest wurden
separat geprüft.

Auch das Systemjournal des unter einem anderen Benutzer laufenden Bots
war nicht einsehbar. `journalctl` zeigte einen Berechtigungshinweis; die
Ausgabe `No entries` ist kein Beleg für Fehlerfreiheit. Funktionsnachweis
hier sind Datenbankzustand und Healthcheck, nicht ein angeblich sauberes
Fehlerjournal.
