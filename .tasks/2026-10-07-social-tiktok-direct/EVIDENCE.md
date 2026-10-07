# Nachweise vor Integration

## Bestand und Vertrag

BESTAND[BS-1]: Graphify zuerst verwendet. Creator-Abfrage, Direct-Post-Initialisierung, Chunktransfer, Checkpoint, Nachprüfung und Vorschau-Verarbeitung wiederverwendet. Neue Uploads besitzen keine Postfach-Umleitung. Frühere Postfach-Vorgänge behalten ihre Nachprüfung.

TEXTNACHWEIS[DR-1]: TikTok-spezifische Absätze in terms.html und privacy.html angepasst. Keine neuen Postfach-Uploads angekündigt. Beschreibung, Sichtbarkeit, Interaktionen, Werbung, Musikbestätigung, Verarbeitung und Status sind im Dialog erklärt. Keine Änderungen an den übrigen Rechtstexten aus Auftrag D.

## Rust und Datenbank

Arbeitsverzeichnis: `/home/nathanael/.worktrees/tb-social-tiktok-direct/rust`. Cargo aus `/home/nathanael/.cargo/bin`, `SQLX_OFFLINE=true`. Eigener PostgreSQL-16-Cluster mit TimescaleDB, ausschließlich Loopback, Port 59471. Keine Tests gegen die Produktionsdatenbank.

- `TB_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:59471/postgres cargo test -j 2 -p tb-social-media --offline -- --include-ignored --test-threads=1`: 303 Bibliothekstests und 1 Integrationstest bestanden, 0 fehlgeschlagen, 0 ignoriert. Reservierung, Snapshot-Trigger und Statuspersistenz liefen gegen PostgreSQL.
- `TB_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:59471/postgres cargo test -j 2 -p tb-dashboard-api --offline social_media -- --include-ignored --test-threads=1`: 53 bestanden, 0 fehlgeschlagen, 0 ignoriert, 1278 Bibliothekstests ausgefiltert. Zwei bisherige Erwartungen an TikTok-Uploads ohne individuelle Freigabe wurden ausdrücklich korrigiert; generische erfolgreiche Abläufe bleiben über YouTube geprüft.
- `TB_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:59471/postgres cargo test -j 2 -p tb-db --test fresh_migrations_schema --offline -- --include-ignored --test-threads=1`: 1 bestanden, 0 fehlgeschlagen, 0 ignoriert. Erster Versuch scheiterte am fehlenden Datenbanknamen twitch_analytics im isolierten Cluster. Nach dessen Anlage liefen alle Migrationen und der Snapshot-Vergleich erfolgreich.
- `cargo clippy -j 2 -p tb-social-media -p tb-dashboard-api --all-targets --offline`: Exit 0, Warnungen vorhanden. Keine Behauptung eines warnungsfreien Laufs.
- `rustfmt --check --edition 2021 --config skip_children=true` für Uploader, Uploader-Trait, Recovery, Worker, beide betroffenen Handler und Diagnosebeispiel: Exit 0. Paketweites `cargo fmt --check` bleibt wegen weiterer Formatabweichungen rot; diese wurden nicht pauschal korrigiert.

Neue Datenbankabfragen verwenden dynamisches SQL. Die neuen Felder werden bewusst über `to_jsonb` gelesen, damit vorhandene Offline-SQLx-Abfragen und ihre Spaltenverträge unverändert bleiben. `.sqlx` wird nicht um fremde Cacheänderungen erweitert. Offline-Kompilierung und der vollständige frische Schema-Vergleich prüfen diesen Stand.

Ein zusätzlich gestarteter breiter Dashboard-API-Lauf ergab vor der Korrektur der zwei betroffenen Erwartungen 1307 bestanden und 24 fehlgeschlagen. Er ist kein grüner Nachweis. Für die anderen Fehler wurde keine Rust-Baseline gemessen; sie werden nicht als nachgewiesen vorbestehend bezeichnet.

## Dashboard und Sichtprüfung

Arbeitsverzeichnis: `/home/nathanael/.worktrees/tb-social-tiktok-direct/bot/dashboard_v2`.

- `npm run build`: Exit 0, Produktionsbundle gebaut.
- Betroffene Vertragsdateien: 36 bestanden, 0 fehlgeschlagen, 0 übersprungen.
- `STUDIO_ARTIFACT_DIR=/home/nathanael/.claude/sichtpruefung/tb-social-tiktok-direct STUDIO_TIKTOK_VIDEO=/home/nathanael/.claude/sichtpruefung/tb-social-tiktok-direct/fixture-video.mp4 node --test tests/socialStudio.browser.test.mjs`: 24 bestanden, 0 fehlgeschlagen, 0 übersprungen. Produktionsbundle, isolierter HTTP-Vertrag, wirklicher Chromium-Browser. Keine voreingestellte Sichtbarkeit, deaktivierte Zustimmung, kontogesperrte Kommentare, Werbeeinschränkung, bearbeitete Beschreibung und vollständige übermittelte Freigabe geprüft.
- Desktop 1440px und Mobil 390px angesehen. Keine horizontale Überbreite. Screenshotserie nach tatsächlich decodiertem Videoframe: `tiktok-desktop.png`, `tiktok-mobile-top.png`, `tiktok-mobile-bottom.png` im genannten Sichtprüfungsordner. Das stumme Farbvideo ist ausschließlich eine lokale Testfixture, kein TikTok-Live-Beweis.

Der vollständige Frontendlauf ist nicht grün: Kalender 9 bestanden; Hauptlauf 421 bestanden, 5 fehlgeschlagen. Dieselben drei betroffenen Testdateien gegen unveränderten Ausgangsquelltext: 18 bestanden, dieselben 5 fehlgeschlagen, 23 insgesamt. Nur diese fünf Fehler besitzen eine gemessene Baseline.

TESTNACHWEIS[TW-1]: 418 passed, 0 ignored | Baseline: 5 rot

Die 418 beziehen sich auf 304 Social-Media-Rusttests, 53 betroffene API-Tests, 1 frischen Schema-Test, 36 betroffene Frontend-Verträge und 24 Browsertests. Breite rote Läufe werden oben separat ausgewiesen.

## Nur lesende Live-Abfrage

Am 7. Oktober 2026 wurde mit dem Rust-Beispiel `tiktok_creator_probe` unter der Dienstidentität twitchbot die gespeicherte earlysalty-Verbindung gelesen und ausschließlich `creator_info/query` aufgerufen. Datenbankverbindungen waren standardmäßig schreibgeschützt. Keine Zugangsdaten ausgegeben, kein Post initialisiert.

Tatsächliche Antwort: Konto `earlysalty`, Anzeigename `EarlySalty`; Sichtbarkeit `PUBLIC_TO_EVERYONE`, `MUTUAL_FOLLOW_FRIENDS`, `SELF_ONLY`; Kommentare, Duett und Stitch nicht kontoseitig gesperrt; Höchstdauer 3600 Sekunden. Diese Kontoeinstellungen ersetzen keine Freigabe für öffentliche Posts. Erlaubt bleibt ausschließlich der gesondert bestätigte eine SELF_ONLY-Test nach ALLOW, Merge und Deploy.

## Noch offen

Integration auf frisches origin/main, Gate, Merge und Push, produktive Migration, Release und Neustarts, Auswahl und Ansicht eines eigenen vorbereiteten Deadlock-Clips, genau ein genehmigter privater Test, finaler Live-Beweis und Aufräumen. Bis zu diesem Nachweis kein Fertigstatus.
