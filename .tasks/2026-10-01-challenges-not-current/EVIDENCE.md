# Belege

Ausgangszustand: produktive Quelle category_collection und engine ungesund seit 06:16 UTC, übrige fünf Quellen gesund. Collector nach Neustart aktiv, historische Abdeckung weiterhin unvollständig. Current zeigt f4034597517f2921ad250738b964dd4c712f9448; dieser Stand ist Vorfahr der Branch-Basis origin/main 14bc1f47. Keine Liveconfig geändert.

Frontend: npm run build erfolgreich. Neun bestehende Challenges-Tests erfolgreich. Browserregression einschließlich neuem Teilzustand erfolgreich, Desktop und Mobil ohne horizontales Überlaufen. Screenshots unter /home/nathanael/.claude/sichtpruefung/challenges-not-current/challenges-partial-desktop.png und challenges-partial-mobile.png visuell geprüft.

Rust-Prüfungen laufen mit dem vorhandenen Compiler 1.97.1. Der systemweite /usr/bin/cargo ist zu alt für Lockfile v4. Strict-Clippy meldet bestehende Warnungen in tb-chat und unveränderten Dashboard-Dateien. Reguläres Clippy und betroffene Tests werden protokolliert.

Weitere Units: Bot und Dashboard verwenden Restart=always/5s, StartLimitIntervalSec=10 und Burst=5; diese Kombination erreicht mit fünfsekündigem Abstand das Limit nicht. Coaching-Watch verwendet on-failure/30s und dasselbe Zehnsekundenfenster. Der bisherige OnFailure-Notify dagegen hat 900s/Burst=1 und Credential-/Mount-Namespace-Abhängigkeit. Er wird aus dem Collector-OnFailure-Pfad genommen; der vorhandene Watchdog-Timer übernimmt mit Rust, Postgres und dem unveränderten lokalen Broker. Das Watchdog-Sandboxing verzichtet gezielt auf Namespace-/LoadCredential-Setup. Nur CAP_SETUID und CAP_SETGID sind zum endgültigen Rechteabgeben erlaubt: bestehender Infrastruktur-Schlüssel im Speicher lesen, Gruppen entfernen, Identität dauerhaft auf twitchcollector setzen. Bei fehlendem Schlüssel wird trotzdem der Vorfall erfasst. Der unveränderte Peer-Zugang ist als twitchcollector lesend erfolgreich geprüft (current_user twitchcollector, 6120 Collector-Läufe).

Live-Abschluss noch offen: koordinierte Release-Installation, Migration als postgres, Unit-Installation und Live-API aus bestehender Sitzung. Hier ist kein Browser verbunden, die Hauptsession wurde um den authentifizierten GET-Beweis gebeten. Build und Tests laufen weiter.

Reguläres Clippy über alle Targets der vier betroffenen Pakete ist erfolgreich (Exit 0). Bestehende Warnungen stammen aus unveränderten Dashboard-Dateien. Die zusätzliche vollständige Dashboard-Lib-Suite: 1303 Tests bestanden, acht fehlgeschlagen, drei ignoriert. Fehler betreffen master_session-Statuscode, drei Affiliate-Claims, das Engagement-Fixture ohne live_test-Spalte, zwei Social-Media-Fixtures und den Twitch-Admin-Login. Auth, Affiliate, Engagement, Social Media und Streamers sind bytegleich mit origin/main (git diff --quiet: Exit 0). Die beiden Challenges- und sieben Ranglisten-Tests sind darin erfolgreich. Die isolierten betroffenen Tests werden zusätzlich auf dem abschließenden Stand ausgeführt.

Abschließender Stand: isolierte Challenges-Tests 2/2, Ranglisten-Tests 7/7 erfolgreich. Collector 8/8, Watchdog 2/2, Effort-Unit 19/19, Effort-Postgres 16/16 und Source-Readiness 1/1 erfolgreich. Der vorhandene Single-Connection-Test prüft nun das beauftragte neue Verhalten: Tick und Anzeige funktionieren trotz pausierter Sammlung, vollständige Kategorie-Abdeckung und striktes Gate bleiben falsch. Raid-Unit 377/377 erfolgreich; weitere Raid-Integrationsprüfungen im Laufprotokoll. Clippy erneut Exit 0. rustfmt --check mit skip_children für sämtliche geänderten Rust-Dateien erfolgreich. Paketweites fmt zeigt bestehende Abweichungen in unveränderten Raid-Dateien, die nicht zum Paket gehören. Shell-Syntax und git diff --check erfolgreich. Dashboard, Admin und Website gebaut; Dashboard nach letzter Vertragsänderung erneut gebaut.

Koordination: additive Brain-Shadow-Konfiguration inzwischen produktiv, SHA256 bot.toml 301b802b668b2c12d4ca9895677b46db1d9626a620dd2c77dc961946f9d4b7e0. Neustarts 22:44:56/57 CEST abgeschlossen; Deploy-Sperre bei lesender Prüfung frei. Dieses Paket verändert diese Konfiguration nicht. /root/integration hat den Twitch-Deployslot exklusiv für dieses Paket einschließlich TokenDB von origin/main 14bc1f47 freigegeben; Brain be2 und Serve bleiben unverändert. Authentifizierter Live-GET bleibt vor Abschluss notwendig; vorhandene Hauptsession ist darum gebeten.

Sichtprüfung des echten Teilzustands:

![Desktop](/home/nathanael/.claude/sichtpruefung/challenges-not-current/challenges-partial-desktop.png)

![Mobil](/home/nathanael/.claude/sichtpruefung/challenges-not-current/challenges-partial-mobile.png)
