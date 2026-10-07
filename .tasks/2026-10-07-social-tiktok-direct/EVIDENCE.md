# Nachweise und Übergabe

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

TESTNACHWEIS[TW-1]: 419 passed, 0 ignored | Baseline: 5 rot

Der abschließende Nachlauf nach Rebase auf origin/main e0b0dbaf bestand mit 304 Bibliothekstests und 1 Integrationstest in tb-social-media, 53 betroffenen API-Tests, 36 Frontend-Verträgen und 24 Browsertests. Dazu kommt der oben dokumentierte frische Schema-Test. Insgesamt 419 bestanden, 0 fehlgeschlagen, 0 ignoriert. Der API-Nachlauf benötigte 5m 37s Kompilierung und 26.87s Laufzeit. `npm run build` nach der Integration ebenfalls Exit 0. TypeScript-Verträge liefen mit dem etablierten `node --import tsx --test`; ein vorheriger Versuch ohne tsx scheiterte an der Modulauflösung und wird nicht als Produktfehler oder erfolgreicher Test gezählt. Breite rote Läufe werden oben separat ausgewiesen.

## Nur lesende Live-Abfrage

Am 7. Oktober 2026 wurde mit dem Rust-Beispiel `tiktok_creator_probe` unter der Dienstidentität twitchbot die gespeicherte earlysalty-Verbindung gelesen und ausschließlich `creator_info/query` aufgerufen. Datenbankverbindungen waren standardmäßig schreibgeschützt. Keine Zugangsdaten ausgegeben, kein Post initialisiert.

Tatsächliche Antwort: Konto `earlysalty`, Anzeigename `EarlySalty`; Sichtbarkeit `PUBLIC_TO_EVERYONE`, `MUTUAL_FOLLOW_FRIENDS`, `SELF_ONLY`; Kommentare, Duett und Stitch nicht kontoseitig gesperrt; Höchstdauer 3600 Sekunden. Diese Kontoeinstellungen ersetzen keine Freigabe für öffentliche Posts. Erlaubt bleibt ausschließlich der gesondert bestätigte eine SELF_ONLY-Test nach ALLOW, Merge und Deploy.

## Gate und verbleibender Abschluss

Integration auf origin/main e0b0dbaf ist abgeschlossen; Commit 601142ae. Die Übersetzungskollision wurde durch Erhaltung beider Pakete und genau eines Reload-Schlüssels gelöst. Die erneuten betroffenen Prüfungen sind oben dokumentiert.

Gate Runde 1: gpt-6.1-sol BLOCK, drei blockierende Befunde zu abgelehnter Zeitplanung, früheren NULL-Freigaben und erneuter Freigabe nach bestätigtem FAILED. Vollständige Liste und Übergabe in REVIEW.md und BRIEFING-FIXER.md. Keine Eigenkorrektur des Implementierers. Keine weiteren Threads gestartet.

Merge und Push nach main, produktive Migration, Release und Neustarts, Auswahl und Ansicht eines eigenen vorbereiteten Deadlock-Clips, genau ein genehmigter privater Test, finaler Live-Beweis und Branch-/Worktree-Cleanup bleiben für den neuen Fixer offen. Der einzig gefundene ready-Eintrag für Clip 124589 verweist auf eine fehlende Datei, auch der current-Fallback fehlt. Er wurde nicht als geeigneter Test bestätigt. Kein Post ausgeführt.

LIVEBEWEIS[DV-1]: PID nicht neu gestartet | exe ungeprüft | journal -p err ungeprüft | Anker nicht geprüft | Funktion: kein Deploy und kein Post | Ort: noch kein produktiver Funktionsbeweis

Worktree und Branch bleiben ausdrücklich zur Fixer-Übernahme erhalten. Der eigene isolierte Testcluster wird vor der Übergabe geordnet gestoppt. Seine Daten unter /tmp/tb-tiktok-db-666eaa47 können für Folgeprüfungen wiederverwendet werden.

## Fortschreibung durch den frischen Fixer

Die Angaben unter „Gate und verbleibender Abschluss“ beschreiben die ursprüngliche Übergabe. Der Fixer korrigierte die drei Befunde, integrierte D2 auf aktuellem origin/main und erhielt ALLOW in Runde 2 und Runde 3, beide mit gpt-6.1-sol. 9315b3cf wurde anschließend per Fast-Forward und normalem HEAD:main-Push integriert. Die vier integrierten Prüfläufe bestanden mit 434 Tests. Einzelbelege stehen in FIXER-EVIDENCE.md und REVIEW.md.

Die frühere Dateifehlmeldung zu Clip 124589 war falsch: Quelle und Vorschau liegen im tatsächlichen Dienst-Bind-Mount. Die autorisierte lokale Administrationsabfrage bestätigt earlysalty und eine fertige Vorschau. Das Video wurde nicht gepostet; eine Hörprüfung wird nicht durch Metadaten ersetzt. Der eigene Testcluster wurde vom Fixer geordnet gestoppt.

Release 0452e03cb7eab42d9e08ee5d39bde380514f1cd3 wurde nach frischem Origin-Abgleich über den Deploy-Wrapper ausgeliefert. Journalbeleg der neuen TikTok-Migration vor den Neustarts, neue Prozessnummern, tatsächliche Programmdateien, eingebettete Revisionen und Anker sowie die fehlerfreien Journale stehen in FIXER-EVIDENCE.md. Die alte Vorschau wurde mit dem vorhandenen gespeicherten Ausschnitt in das aktuelle Format neu gerendert. Produktive API bestätigt ready=true, Konto earlysalty und zur gesicherten Datei passende TikTok-Videobindung. Der genau eine private Test bleibt wegen fehlender Hörprüfung und Dashboard-Browsersitzung offen. Keine TikTok-Freigabe und keine neue Vorgangsnummer gespeichert.
