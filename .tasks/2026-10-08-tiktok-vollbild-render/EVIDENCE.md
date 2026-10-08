# Prüfbelege

## Verhalten und Oberfläche

Der gemeinsame `VideoFullscreenButton` wird im TikTok-Dialog direkt unter dem Video und auf der Clipkarte neben der Vorschau verwendet. Er ruft Fullscreen am jeweiligen Video auf. Bei fehlender oder abgelehnter Standard-API wird die iOS-Video-API versucht. Fehler werden sichtbar angezeigt; ein erneuter Klick setzt die Meldung zurück. Der Knopf hat `type="button"` und verändert keine Zustimmung.

Moli prüfte den echten Dialog mit einem isolierten API-Vertrag. Gemessene Breiten: Desktop 1920/1920, Mobil 390/390, kein horizontaler Überlauf. Sichtbelege:

- `/home/nathanael/.claude/sichtpruefung/tiktok-vollbild/dialog-desktop.png`
- `/home/nathanael/.claude/sichtpruefung/tiktok-vollbild/dialog-mobile.png`
- `/tmp/tb-vollbild-measure-20261008/fullscreen-check-final.json`

Standard-Aufruf, iOS-Fallback, Fallback nach Standard-Ablehnung, beide APIs abgelehnt und anschließender Retry wurden durch Klicks auf die echte React-Komponente geprüft. Die Browser-APIs wurden dafür ersetzt. Moli bietet keine native Video-Vollbildansicht; tatsächliche Safari- und Desktop-Vollbildwiedergabe sind damit nicht belegt. Kein anderer Browser wurde verwendet.

Der erste lokale Vite-Prüfserver wurde nach seinem 30-Minuten-Limit automatisch beendet. Für die mobile Abnahme wurde er erneut gestartet. Eigene Prüfserver und Moli wurden danach beendet, die temporären Fixture-Dateien entfernt.

## Compiler, Linter und Tests

Rust 1.97.1, FFmpeg aus dem Hostbestand. Die Wegwerf-Testdatenbank lief auf dem eigens angelegten Container `tb-tiktok-vollbild-test3-20261008`, Port 33097. `TB_TEST_REQUIRE_DB=1` verhindert stumme DB-Skips. Die ersten beiden isolierten Containerstarts scheiterten an automatisch auf 12 GB gesetztem Shared Buffer; der dritte Start begrenzt RAM und CPU und benutzt 128 MB Shared Buffer.

```sh
TB_TEST_DATABASE_URL='postgres://postgres:tbtest@127.0.0.1:33097/postgres' TB_TEST_REQUIRE_DB=1 cargo +1.97.1 test -p tb-social-media -j 2 -- --include-ignored --test-threads=1
cargo +1.97.1 clippy -p tb-social-media --all-targets -j 2
cargo +1.97.1 fmt -p tb-social-media -- --check
```

- Rust: 346 Unit-Tests und ein Doc-Test bestanden, null fehlgeschlagen, null ignoriert. Ein vorheriger paralleler Lauf hatte 345 bestandene Tests und einen Timeout-Erwartungsfehler im TikTok-Wiremock-Test. Der vollständige serielle Lauf ist grün.
- Clippy ohne `-D warnings`: Exit 0. Der strengere Lauf meldet vier Befunde in analytics.rs, credentials.rs, upload_worker.rs und vocab.rs. Keiner liegt im geänderten Rendercode. Der unveränderte Baseline-Lauf wird separat protokolliert.
- Formatter: Die geänderte Rust-Datei wurde mit Rustfmt 1.97.1 formatiert. Crateweiter Cargo-Fmt-Check und vollständige unveränderte Baseline melden dieselbe Fundstelle in `llm_dispatch.rs:218`. Keine fremde Datei umformatiert.
- Frontend: vollständiger TypeScript-/Vite-Build und ESLint der beiden Dialog-/Knopfdateien jeweils Exit 0. Gesamte npm-Suite: 439 bestanden, fünf fehlgeschlagen. Vollständiger Baseline-Checkout f04c0ef0: ebenfalls 439 bestanden und dieselben fünf Fehler bei Markenfarben, Analytics-Breite und OBS-Hilfetext. Keine neue Regression.

Logs: `/tmp/tb-vollbild-rust-test-serial.log`, `/tmp/tb-vollbild-clippy-default.log`, `/tmp/tb-vollbild-clippy.log`, `/tmp/tb-vollbild-clippy-baseline.log`, `/tmp/tb-vollbild-cargo-fmt.log`, `/tmp/tb-vollbild-cargo-fmt-baseline.log`, `/tmp/tb-vollbild-frontend-test.log`, `/tmp/tb-vollbild-frontend-baseline-test-complete.log`, `/tmp/tb-vollbild-frontend-final-build.log`, `/tmp/tb-vollbild-eslint.log`.

## Echter Render und Live-Prüfung

Die Messung in `MESSUNG.md` verwendet den bestehenden Rust-Produktivpfad und einen echten Bot-Clip. Full-HD-Hochformat, 60 fps, Logo, Titel und Audio bleiben erhalten. Neue erfolgreiche Render schreiben `social_media_render_complete` mit Preset, CRF und Millisekunden in das Dienstprotokoll.

Der bestehende Dienstzugang wurde über den vorhandenen Infisical-Launcher ausschließlich im lokalen Prozessspeicher verwendet. Der begrenzte Vorschau-GET für eigenen Clip 124788 liefert HTTP 200, application/json, status null, ready false. Keine Geheimnisse ausgegeben, keine Sessionwerte gelesen, kein eigener Auth-Bypass.

Der abschließende Release-/PID-/Journal-/Artefakt-/Vorschau-Beleg wird nach Gate, Merge und Deploy in `/home/nathanael/.claude/sichtpruefung/tiktok-vollbild/` abgelegt. Vor einem erfolgreichen Deploy besteht hier kein Live-Abschlussanspruch.

TESTNACHWEIS[TW-1]: Rust 347 passed, 0 ignored; npm 439 passed | Baseline: npm 5 rot, Formatter 1 Fundstelle
TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 2 belegt | Senke: Dialog, Vollbild-Knopf und Auftragsakte
