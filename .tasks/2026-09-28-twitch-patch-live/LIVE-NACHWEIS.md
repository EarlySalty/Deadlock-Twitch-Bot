status: aktiv (2026-09-29)

# Twitch-Patch-Meldung: Live-Stand

## Release nach dem echten Spielpatch

Valve veröffentlichte am 29.09.2026 um 20:25:11 UTC die offizielle Deadlock-Steam-News „City Never Sleeps“ (App 1422450, GID 1844751498235383, Feed `steam_community_announcements`). Die vollständige Steam-News hat 3332 Zeichen; der offizielle Text kündigt eine neue Karte, sechs Helden und weitere Änderungen an. Der öffentliche deutsche Artikelindex stand unmittelbar vor dem Deploy weiterhin bei 46 Einträgen mit maximaler ID 285.

Unabhängiger Schlussreview: `REVIEW-INTEGRATION-ERGEBNIS.md`, 774 bestandene Tests und derselbe einzelne fehlgeschlagene Bot-Test auf Feature und unverändertem Main. Das SHA-gebundene Scheduler-Review wurde clean/released abgeschlossen; `aae90de9d508907b7394608c34692248ea45b610` wurde durch den geschützten Push nach `origin/main` gemergt. Vor dem Deploy war genau dieser SHA aktuelles Remote-Main. Ein sauberer eigenständiger Clone enthielt die vier Binaries und drei Frontend-Builds; alle vier ELF-Build-Revisionen trugen denselben SHA. Der Release-Build dauerte 27 Minuten 44 Sekunden. Keine synthetische Twitch-Nachricht.

`/usr/local/bin/deploy-twitch-release aae90de9d508907b7394608c34692248ea45b610 <eigenständiger Release-Clone>` beendete sich erfolgreich. Der Wrapper aktivierte den Release, führte `deadlock-twitch-migrate.service` aus und startete Dashboard, Bot, Coaching-Watch sowie Category-Collector neu. Migrationsdienst: `Result=success`, `ExecMainStatus=0`. `_sqlx_migrations` enthält `20260928120000` mit `success=true`. `twitchbot` darf die neuen Beobachtungen lesen und anlegen, `twitchdash` nur lesen, `twitchlegacy` erhält keinen Lesezugriff auf Ankündigungen. Der Bot pollte den Index nach dem Start; `twitch_patch_feed_state.bootstrapped_at` ist gesetzt und 46 Beobachtungen mit IDs 1 bis 285 stehen auf `historical`, keine neuere Beobachtung. Der Poll-Rückstand betrug bei der Prüfung 17 Sekunden. Der Bot-PID wechselte von 1091752 auf 3203510, der Dashboard-PID von 920507 auf 3203395. Beide `/proc/<pid>/exe` zeigten ohne `(deleted)` auf die Release-Binaries des SHA. Die privilegierten Error-Journale beider Systemdienste waren seit dem Deploy leer. Im laufenden Bot-Binary wurde `twitch_patch_feed_observations` gefunden.

LIVEBEWEIS[DV-1]: PID 1091752->3203510 | exe ohne (deleted) | journal -p err leer | Anker "twitch_patch_feed_observations" in Binary | Funktion: 46 alte Artikel historisch, Feed-Poll aktiv, neue Chat-Zustellung noch ausstehend | Ort: https://deutsche-deadlock-community.de/patchnotes/index.json, Patchnotes-Index

## Offener Ende-zu-Ende-Blocker

Der produktive Patchnotes-Bot erkannte die offizielle Steam-News nicht. In `changelog_latest_fetcher.py` verlangt `_steam_patch_score` mindestens fünf Punkte; „City Never Sleeps“ enthält keines der Titelwörter `update`, `patch`, `hotfix`, `balance`, keine `[p]`-Aufzählungen und keine gezählten Abschnittsmarker, also Score 0. `_check_latest_steam` und `check_latest_steam_signal` verwerfen den Eintrag vor dem Trigger. Das Journal zeigte zudem einen Steam-News-Timeout um 22:59 CEST. Brain kennt weiterhin nur den 16.09.; der deutsche Website-Index endet weiterhin bei ID 285. Die Ursache wurde am 29.09. an den bestehenden Rust-Port-Thread `17337791-d5fe-49c8-ad55-2e31a2e016d2` übergeben. Python bleibt lesbare Legacy-Referenz und wird nicht produktiv geändert.

Wenn ein neuer deutscher Artikel erscheint, sind dessen Metadaten und kanonische URL gegen den Twitch-Vertrag zu prüfen. Erst gespeicherte Zustellversuche und Ergebnisse pro berechtigtem Kanal sowie der echte Chat-Verlauf können den vollständigen Nutzerwunsch belegen. Kein manueller Replay alter Artikel und kein synthetischer Send an echte Partner-Chats.
