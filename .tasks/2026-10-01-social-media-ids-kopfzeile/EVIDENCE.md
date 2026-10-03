# Nachweise

Graphify wurde vor der Quellprüfung verwendet; Suchbegriffe und Treffer stehen in GRAPHIFY.md.

## Frontend

- npm run build: erfolgreich.
- Bestehende Tests: 419 bestanden, 5 fehlgeschlagen. Die fünf Fehler (Brandpalette, Analytics-Maximalbreite, OBS-Hilfe) wurden auf einem separaten Export von origin/main identisch reproduziert.
- Social-Media-Browserprüfung: 24 bestanden, keine Fehler. Prüft Kanalwahl/ID-Vertrag, Freigabeschalter und gemeinsame Kopfzeile ohne studio-brand/Partner-Chip; Screenshots für Desktop und Mobilgeräte. Nach der Gate-Korrektur bestätigt der direkte Vergleich mit /analyse 212px Kopfzeilenhöhe bei 320, 390, 768, 1024 und 1440px Fensterbreite. Beide Seiten bestehen die Prüfung auf horizontalen Überlauf.
- Alte Adminmode-Browserprüfung benötigt einen hier fehlenden geckodriver. Die Social-Media-Prüfung verwendet vorhandenes Playwright-Chromium.
- Geprüfte Browserbilder mit Testdaten liegen zusätzlich unter screenshots/: analyse-1440.png, analyse-320.png, analyse-390.png, studio-1440.png und studio-390.png. Sie sind keine Produktivbeweise.
- Browser-Artefakte: /home/nathanael/.claude/sichtpruefung/social-media-ids-kopfzeile/.

## Datenbank und Rust

- Vollständige Migrationen auf leerer isolierter PostgreSQL-/Timescale-Datenbank: bestanden; Schema stimmt mit dem aktualisierten Snapshot überein.
- Backfill-Integration: bestanden. Beide bekannten Altfreigaben erhalten die echte Twitch-ID; unauflösbare Daten bleiben vollständig erhalten; fehlende, ungültige und doppelte IDs werden abgewiesen; Wiedervergabe eines Logins überträgt keine Freigabe.
- SQLx-Onlineprüfung `tb-social-media --all-targets`: bestanden; neue Offline-Metadaten erzeugt.
- `cargo fmt` ausgeführt; alle geänderten Rust-Dateien bestehen `rustfmt --check`. `cargo clippy` der vier berührten Crates wurde erfolgreich mit bestehenden Warnungen ausgeführt; abschließender Wiederholungslauf einschließlich der VOD-ID-Erweiterung ebenfalls Exit 0.
- VOD-Tests: alle 42 bestanden, einschließlich TokenDB-Migration auf eigener Wegwerf-Datenbank.
- tb-db-Lib: alle 8 bestanden.
- Vollständiger Dashboard-API-Lauf: 1253 bestanden, 58 fehlgeschlagen, 3 ignoriert. 37 Social-Media-Fehler kamen vom nachzurüstenden Test-Schema; dieses wurde korrigiert. Die übrigen 21 Fehler betreffen Auth-Level und andere Handler; diese wurden durch dieses Paket nicht geändert und bleiben als offene Prüfung dokumentiert. Der abschließende gezielte Social-Media-Lauf besteht mit allen 48 Tests.
- Dashboard-API Social-Media-Handler: alle 48 bestanden, einschließlich Zugriff per ID, fremde/recycelte Namen und globale Verbindung ohne Übergriff auf ID-gebundene Zeilen.
- Globaler Report-Scope: bestanden; fehlender Anzeigename überträgt ID-gebundene Reports nicht in den globalen Scope.
- Der TikTok-Chunk-Timeout-Test traf im parallelen Gesamtlauf schon vor seinem Checkpoint ein 20ms-Timeout; isolierter Wiederholungslauf bestanden. Der anschließende serielle Gesamtlauf besteht: 299 Social-Media-, 8 tb-db- und 42 VOD-Tests, keine Fehler.

Logs: /home/nathanael/buildlogs/tb-sm-ids-kopfzeile/.

## Produktiv-Vorprüfung

Bestandsfreigaben haben NULL-IDs, lassen sich aber eindeutig über twitch_streamers zuordnen: dach_lock=1367527782; earlysalty=1186925760. Unauflösbare Produktivfreigaben wurden nicht gefunden.

Der tatsächlich installierte Deploy-Wrapper startet inzwischen deadlock-twitch-migrate.service. Der Auftrag nennt einen früheren Stand ohne diese Migration. Die eigene Migration wird trotzdem wie beauftragt explizit als postgres in einer Transaktion mit Version und sha384-Checksumme eingetragen; der Wrapper prüft anschließend den Release-Stand.

bot.toml unter /var/lib/deadlock-twitch/config/bot.toml ist unverändert; sha256=301b802b668b2c12d4ca9895677b46db1d9626a620dd2c77dc961946f9d4b7e0.

Produktiv-Migration, Aktivierung und Live-Beweis stehen unter der verbindlichen Challenges-/Root-Deploy-Koordination und wurden bisher nicht ausgeführt.

## Abschließende Codeprüfung vor dem Gate

Mit SQLX_OFFLINE=true, verpflichtender isolierter Testdatenbank und CARGO_BUILD_JOBS=2:

- `cargo test -p tb-social-media -p tb-db -p tb-vod-archive --lib -- --test-threads=1`: Exit 0; 299/8/42 bestanden.
- `cargo test -p tb-vod-archive --lib`: letzter Source-Stand erneut geprüft, Exit 0; alle 42 bestanden.
- `cargo test -p tb-db --test social_media_partner_id_migration`: Exit 0; Backfill, Mehrdeutigkeit, archivierte Altfreigaben, ID-Constraints und Kategorie-FK geprüft.
- `cargo test -p tb-db --test fresh_migrations_schema`: Exit 0; alle Migrationen auf erneut leerer Datenbank, Snapshot einschließlich VOD-Besitz-ID stimmt überein.
- `cargo clippy -p tb-social-media -p tb-dashboard-api -p tb-db -p tb-vod-archive --all-targets`: Exit 0 mit bestehenden Warnungen.
- Alle geänderten Rust-Dateien bestehen `rustfmt --edition 2021 --check`; `git diff --check` besteht.

VOD-Warteschlange und Upload-Nachprüfung verwenden IDs. Die Tests belegen, dass zwei Besitzer mit demselben historischen Namen getrennt bleiben, ein Rename die Warteschlange erhält und NULL-ID-Altdaten nicht übernommen werden. Aktive Archive beziehen den aktuellen Anzeigenamen aus dem über ID verknüpften Streamer.

Log-Hashes (SHA256):

- handlers-last.log: 00de73f2fe0e2243b5702dade0adde380cfedcfffabc417b85ae4c99e00071f7
- crates-vod-id-final.log: 49cef7244f5491864aa93dfcb838f884fb22c4e7aa7d45a375ec4d141e185644
- vod-final-source.log: ec5ef326e5840f14f876d0012de443ca6de8aaeec70112064850219b63bbcaaa
- migration-vod-id-final.log: d1db8bb42875a6fda36bbc2f26d48df19d00da3e20c8c1b409fa72daac3e60e2
- fresh-schema-vod-id-final.log: 942c9f33038ba7df738ec975dee5f73a698d493879730c7d0239543c704441dd
- clippy-vod-id-final.log: 42f46234e7c75ade77648a44eb01849a13f7661bd146ce5eeb09c2c043a28b66
- frontend-tests-final.log: d891615304a3cabedf2d76b22d05e5311c8d15155c95f0c849a9bf61b4993015
- frontend-baseline.log: 9eac8952e3cd46dd40b1b9cc65228c8faedabdb28ba589fb861dee4e6be1635e
- browser-final.log: b6dce179bc8b2d3f8568624a75bc8ad4156a0d40ce336995adc17e97021e6e6b
- frontend-build-header-final.log: 0bf883fe1de0c2543a10d51ca2e723ec5ac2b38cdbc89bd1538ca824367b5a18

Migration 20261001090000 hatte vor der historischen Backfill-Ergänzung SHA384 c107a3f24efc7d92cd76ba72eae4db1e23cd61dadd54cd34f00e70d67b0b43e35c2f54d56dbdd12ea73854c896ecf0d0. Die aktuelle SHA384 steht unter Historische Backfill-Ergänzung. Noch keine Produktiv-DDL.

## Gate-Korrektur

Der reguläre Lauf für 7e44af96 ergab BLOCK wegen des Login-Schlüssels im Insights-Cache. Hinzu kamen Hinweise zur VOD-Erkennung und zur fehlenden mobilen Sichtprüfung. Die Änderungen und ihre Prüfung stehen in REVIEW.md.

Frontend-Build nach der Cache-/VOD-Korrektur: Exit 0. Bestehende Frontend-Tests: erneut 419 bestanden und dieselben fünf Baselinefehler. Browserprüfung: Exit 0, alle 24 bestanden; Log browser-review-final.log. Rustfmt der geänderten Dateien und `git diff --check` bestehen. In diesem Schritt änderte sich die Migration noch nicht; die spätere Backfill-Ergänzung ist unten belegt.

Der bestätigte serielle Rust-Lauf besteht mit 300 Social-Media- und 46 VOD-Tests, Exit 0. Die neuen Prüfungen decken die Trennung des Insights-Caches nach ID, VOD-Pagination nach ID, fehlende oder fremde Besitzer-IDs, API-Fehler, wiederholte Cursor und ungültige Dauerangaben ab.

Nach Absicherung des bisherigen v-Präfixes besteht der vollständige VOD-Lauf erneut mit 46 Tests, Exit 0. Der abschließende Clippy-Lauf für Social Media, Dashboard-API, Datenbank, VOD-Archiv und Bot einschließlich aller Targets besteht mit Exit 0 und vorhandenen Warnungen.

Die abschließende gezielte Insights-Prüfung besteht mit beiden Tests, Exit 0. Die private TokenDB-Testkonfiguration wurde anschließend aus dem Worktree entfernt.

Neue Log-Hashes (SHA256):

- review-fix-crates-confirmed.log: 8245f98ee2660cb8f4182f91a9616192fd81b07f2af08d43a831141f1cf7ec35
- browser-review-final.log: fdc2922b2fa7da53324b8e79c299182e4150fc19a7236fff0dcb1d52ad36a772
- frontend-review-build.log: b1a202a4156992c9f96555b2f2745b3ba83815ea0129027ee508775659466655
- frontend-review-tests.log: 525161db2020f42bd2567d61e1208400c0e3339d243b9c3d1c525355b088ae3a
- vod-review-final.log: 5225c6b4e5deaf34c68e9ac84436d43c4f1960dc6dd1ace28290adfb53fca68c
- clippy-review-final.log: 4d33c63f92f84428dfc8b69e82812058190d418fb20d6d8d99b039262aaa6dd7

## Historische Backfill-Ergänzung

Reguläres Gate für a0c34c308f10b9ddb13476f6ed0f26d32afc5a09: ALLOW, Exit 0. Log gate-a0c34c30.log, SHA256 6c24c18eb77a8bc9acd90652cd931806f671a491dfb23e72b3b8e51527edce06. Der Hinweis zum historischen Backfill führte zu folgender lesender Produktivprüfung:

- Vorlagen und Hashtags: jeweils keine Bestandszeilen.
- Layouts: eine Zeile ohne ID, earlysalty, eindeutig 1186925760 zugeordnet.
- Plattformverbindungen: drei Zeilen ohne ID, TikTok und YouTube für earlysalty sowie YouTube für dach_lock. Die Prüfung las nur Plattform, Anzeigename und ID, keine Tokenbytes.
- Frühere ID-Migration 20260802140000 ist erfolgreich eingetragen. Sie deckt spätere Legacy-Schreibvorgänge nicht ab.
- Alle 148 Clips haben IDs; alle 16 kanalgebundenen Reports haben IDs; keine Reauth-Benachrichtigungen. In der Fetch-Historie bleiben 22853 von 56004 Zeilen ohne ID erhalten. Die Laufzeit liest Historie über IDs und übernimmt diese Altzeilen nicht in einen fremden Scope.

Migration 900 füllt deshalb NULL-IDs in Vorlagen, Hashtags, Layouts und Plattformverbindungen nach. Die Integrationsprüfung mit befüllten Tabellen besteht, Exit 0. Sie prüft eindeutige Auflösung, Erhalt bereits gebundener IDs, unveränderte Tokenbytes und Anzeigenamen, globale Verbindungen ohne Kanalbindung, mehrdeutige und unauflösbare Namen sowie den Erhalt aller Zeilen. Die erneute Vollmigration auf einer vorher geleerten eigenen Testdatenbank besteht, Exit 0; der Schema-Snapshot bleibt unverändert. Clippy für alle tb-db-Targets besteht, Exit 0.

Aktuelle SHA384 für Migration 20261001090000: 31cda589137ab6760c186eda16c65f4afe67fb96aedb91d6aadfbebe99430674f499f4052c704c062c148f115c25d9a8. Sie wurde nicht produktiv ausgeführt.

Log-Hashes (SHA256):

- migration-legacy-assets-final.log: 5f68730ac909add02a0c0714f1b74b422123533e940faa82d1370f74a2ce8df8
- fresh-schema-legacy-assets-final.log: cc250287f8522997faeae4122cef22fdd3396fa6cc8259887e78e7f91aa61c8b
- clippy-legacy-assets-final.log: f4531dcb23934886ba9930c2b83669b99a4d9c5de151d1bb2f52f069a46cd498
