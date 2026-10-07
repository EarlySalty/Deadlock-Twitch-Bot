# Offener Abschluss

## Stand am 7. Oktober 2026

Ablauf-Fix und SQL-Statusfehler-Fix sind umgesetzt. Der geschützte Main-Push wurde nach der ersten Freigabe wegen verändertem `origin/main` abgewiesen. Die neuen Main-Stände sind regulär in `merge/social-token-ablauf-r2` integriert, zuletzt `9315b3cf` in `8cc0efda`. ID-gebundene Kontozuordnung, Upload-Funktionen und TikTok-Freigaben bleiben erhalten. `8cc0efda` erhielt mit `gpt-6.1-sol` ALLOW. Die an die erweiterte gemeinsame Fixture angepasste API-Regression hat danach 56 passed, 0 failed, 0 ignored. Vor dem Main-Push folgen der laufende Bot-Adaptertest und ein neues Gate für die Fixture-Korrektur. Am aktuellen Dashboard sind fünf Ablauf- und Fehlerzustände geprüft, darunter drei unbekannte Plattformzustände ohne Verbindungsaktionen nach HTTP 500.

Implementierung: `fix/social-token-ablauf`, Integration: `merge/social-token-ablauf-r2`, bisherige Sicherung: `origin/fix/social-token-ablauf-r2`, Worktree `/home/nathanael/.worktrees/tb-social-token-ablauf`. Der alte Remote-Feature-Branch wurde nicht überschrieben, seine Historie wurde regulär integriert. Der fremd veränderte Haupt-Checkout bleibt unangetastet. Migration und Produktionshilfe bleiben unverändert.

## Offene Schritte

1. Nach frischem Fetch regulär nach `main` integrieren und mit `git push origin HEAD:main` pushen. Git-Schritte einzeln mit literalem Worktree-Pfad. Die historische Remote-Feature-Historie vor ihrer späteren Löschung regulär integrieren und prüfen. Weitere inhaltliche BLOCK-Runden nach neuer Nutzerregel autonom mit je einem frischen nativen Fixer und demselben Gate-Modell fahren; nach spätestens fünf erfolglosen Runden einen echten Blocker melden.
2. Die freigegebene Produktionsmigration als `postgres` mit `PROD-MIGRATION.sql` anwenden. Danach acht erforderliche Rust-Binaries im eigenen sauberen Worktree mit `-j 2` bauen, `.twitch_build` gegen den frisch gefetchten `origin/main`-SHA prüfen und mit drei gebauten Frontends in einen eigenständigen Release-Clone übernehmen. `deploy-twitch-release <sha> <clone>` bleibt der Deploy-Weg.
3. Bot, Dashboard und weitere betroffene Dienste am laufenden Prozess prüfen. TikTok soll beim echten Backfill-Refresh `refresh_expires_at` schreiben. Das ausgelieferte Dashboard-Artefakt muss die neue Anzeige enthalten. Der Discord-Pfad ist gegen die Wegwerf-DB und den lokalen Broker belegt; kein echter Zugang wird als Probe entwertet.
4. Nach Live-Beweis Nachweise und ignorierte Logs sichern. Eigene Test-DB und den fehlgeschlagenen eigenen Testcontainer prüfen und aufräumen. Beide Remote-Feature-Branches und lokale Arbeitsbranches nach erfolgreichem Ancestor-Check löschen, Worktree entfernen, Hauptthread informieren und erst nach belegtem Abschluss settlen.

## Erhaltene Betriebsmittel

- Eigene Test-DB `tb_social_token_ablauf` im Wegwerf-Cluster auf Port 33045 bleibt bis zum Abschluss erhalten.
- Der zusätzliche Datenbankname `twitch_analytics` im Wegwerf-Cluster ist keine Produktionsdatenbank und wird nicht ungeprüft gelöscht.
- Eigener fehlgeschlagener Container: `tb-social-token-ablauf-db`. Kein fremder Container wird gestartet oder gestoppt.
- Test- und Buildlogs sind im Task-Ordner ignoriert und müssen vor Worktree-Löschung gesichert werden.
- Die gebaute Dashboard-Probe ist belegt; Produktionsanzeige, Deploy und Cleanup bleiben offen.
