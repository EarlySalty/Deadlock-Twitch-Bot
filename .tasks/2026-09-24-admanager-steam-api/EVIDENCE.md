# Abnahme: Twitch-Werbemanager

## Nachprüfung nach Rebase auf main

Der PR wurde auf den aktuellen Hauptzweig nachgezogen. Dessen CI lädt das
kanonische Brain-Schema bereits aus `scripts/ci/brain-schema/`; die ältere,
PR-eigene Kopie unter `rust/schema-gate/` wurde entfernt. Das aktuelle
Dashboard-Layout blieb erhalten, die korrigierten Status- und Hilfetexte sind
integriert. Eine vorhandene moderne Steam-Verknüpfung ohne gewähltes
Primärkonto sperrt nun die Rückkehr zu älteren Engagement- oder
Discord-Konten. Der Wegwerf-Datenbanktest deckt diesen Fall ab.

Stand: 26. September 2026. Ersatz-PR #985 auf Branch `fix/admanager-steam-api-20260926`. Noch nicht produktiv ausgeliefert.

## Ursache und Korrektur

Der alte Statusleser suchte Steam-Presence über einen JOIN in der Twitch-Datenbank. Der Steam-Bot besitzt diese Daten jedoch in einer getrennten Datenbank. Quellenfehler wurden zur fehlenden Presence und anschließend zum Chat-Ruhe-Fallback. Zusätzlich wird eine direkte Twitch/Steam-Kontoverknüpfung im alten Engagement-Lesepfad nicht berücksichtigt.

Der gemeinsame Statusleser für Worker und Dashboard verwendet jetzt die stabile Twitch-ID. Er berücksichtigt direkte Kontoverknüpfung und Trennung oder das bestehende explizite Engagement-Feld. Direkte Konten haben Vorrang. Die öffentliche Discord-Abfrage wurde entfernt: Bei unvollständiger Steam-Presence kann sie fälschlich `live:false` liefern. Eine Discord-Verknüpfung ohne SteamID64 bestätigt daher kein Werbefenster. Nach dem HTTP-Abruf wird die Zuordnung erneut geprüft. Steam-Presence kommt aus der strikten internen Steam-Bot-API; der Twitch-Prozess erhält keinen zentralen Datenbankzugang. Bot und Dashboard bekommen den bestehenden internen Token über ihre geladene Runtime-Konfiguration.

Unbekannte, veraltete, unvollständige und widersprüchliche Statusdaten bestätigen kein Werbefenster. Eigene automatische Werbestarts bleiben dann gesperrt, auch bei ruhigem Chat und überfälligem Budget. Bei bevorstehender Twitch-Werbung werden verfügbare Pausen genutzt, auch bei dichtem Werbeplan. Der effektive Vorlauf beträgt mindestens 60 Sekunden für den bestehenden 25-Sekunden-Worker. Frisches Match sperrt ab dem ersten Tick; bestätigte Queue und Menü bleiben nutzbar.

Dashboard-Texte unterscheiden fehlende Verbindung, veraltete Daten und eine nicht verfügbare Quelle. Das frühere Versprechen eines anschließend werbefreien Matches wurde entfernt. Die Anzahl der Twitch-Pausen ist begrenzt; geplante Werbung kann ohne Pausen weiterhin in ein Match fallen.

## Durchgeführte Prüfungen

Rust/Cargo 1.98.0, `--locked -j 2`, eigener Worktree.

- `python3 .tasks/2026-09-24-admanager-steam-api/run-tests.py`: eigener Wegwerf-Timescale-Container, keine zentrale Presence-Tabelle. 14 ausgewählte Bibliothekstests, 32 Entscheider-Tests, 4 zusätzliche Sicherheitsregressionen und 2 Store-Tests bestanden. 52 Tests, 0 Fehler, 0 ignoriert.
- `cargo test -p tb-bot -p tb-dashboard-api ad_manager`: 7 Worker-Tests und 7 Dashboard-API-Tests bestanden. 14 Tests, 0 Fehler, 0 ignoriert. Andere Tests wurden durch den bewusst gewählten Filter nicht ausgeführt.
- Frontend: `npm ci --ignore-scripts --no-audit --no-fund`, danach `node --import tsx --test tests/adManager.test.ts tests/verwaltungTabs.test.ts`: 18 bestanden, 0 Fehler, 0 übersprungen.
- `tsc -b`: erfolgreich.
- ESLint auf den drei geänderten Frontend-Quelldateien: erfolgreich.
- Rustfmt für die neuen Rust-Module und `git diff --check`: erfolgreich.

Nach dem Token- und Discord-Fix wurden 66 gezielte Rust-Tests erneut bestanden; die Frontend-Prüfungen stammen aus der vorherigen #985-Abnahme. Dies ist keine vollständige Workspace-Abnahme. Ein vorbestehender veralteter Store-Testaufbau wurde um die bereits vorhandenen Budget- und Hinweis-Migrationen ergänzt; keine neue Produktionsmigration. Eine vorbestehende MemFdCreateFlag-Deprecation im Uplink-Testcode bleibt außerhalb dieses Fixes.

## Auslieferungsabhängigkeit und offene Prüfungen

Steam-Producer: EarlySalty/Deadlock-Steam-Bot PR #69, Branch `fix/player-live-by-steamid-20260924`. Sein `/internal/player-live`-Vertrag muss vor Auslieferung dieses Verbrauchers vorhanden sein. Der Verbraucher bleibt bis dahin konservativ gesperrt statt Daten zu erfinden.

PR #958 wurde durch #985 ersetzt und geschlossen. Die frühere CI-Nachprüfung des alten PR steht in CI-NACHPRUEFUNG.md; für #985 gelten die aktuellen Checks und der unabhängige Re-Review. Keine echten Ads und noch kein Live-Deploy.
