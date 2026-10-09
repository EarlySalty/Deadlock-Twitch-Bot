# Nachweise

## TikTok und Speicherung

Offizielle Richtlinie: https://developers.tiktok.com/doc/content-sharing-guidelines, mit Moli am 9. Oktober 2026 gelesen, Seitenstand 4. August 2026. Abschnitt 2b verlangt eine manuell gewählte Sichtbarkeit ohne Standardwert. Abschnitt 2c verlangt manuell eingeschaltete Interaktionen ohne Anfangshaken. Abschnitt 3a verlangt ausgeschaltete Werbekennzeichnung als Anfangszustand. Das ist kein Nachweis einer TikTok-App-Prüfung.

Der Dialog startet deshalb ohne Sichtbarkeit und mit ausgeschalteten Interaktionen und Werbung. Gespeicherte Standardwerte und Entwürfe werden durch ausdrückliche Knöpfe übernommen. Nicht angebotene Sichtbarkeiten und gesperrte Interaktionen werden dabei verworfen. Standardwerte enthalten keine Werbeauswahl. Ein ausdrücklich geladener Clip-Entwurf kann dessen Werbeangaben wiederherstellen. Die Zustimmung zur Musiknutzung wird beim Übernehmen zurückgesetzt und ist kein Feld des serverseitigen Entwurftyps.

Bestehender Speicher: `social_media_settings`, Schlüssel `tiktok_defaults:<Twitch-ID>` und `tiktok_draft:<Twitch-ID>:<Clip-ID>`. Die Twitch-ID wird aus dem Clip gelesen, nicht vom Client angenommen. GET und PUT prüfen Anmeldung, Clip-Zuständigkeit und Partnerfreigabe. Keine Migration und keine neuen SQLx-Makroabfragen. Entwurfsspeicherung verändert weder `tiktok_post_options` noch die Veröffentlichungswarteschlange.

## Facecam

Echter Clip 124789 von earlysalty, Twitch-ID 1186925760:
https://www.twitch.tv/earlysalty/clip/SillyIronicGerbilCorgiDerp-zIbtkrQlXjv6fEEC

Quelle: HEVC, 1920×1080, 60 Bilder/s, 30 Sekunden, Video 7.447.408 Bit/s, AAC 129.420 Bit/s. Layout und Clip-Override: Cam x41/y237, 303×336; Zielkachel 1080×600, gestapeltes Layout. FFmpeg rundet den YUV420-Ausschnitt tatsächlich auf 302×336. Cover-Skalierung etwa 3,576-fach, anschließend vertikaler Beschnitt. Der sichtbare Quellbereich entspricht ungefähr 302×168 Pixeln. Diese Quelldetails begrenzen die Schärfe.

Aktueller Vorschaupfad: ein FFmpeg-Filtergraph einschließlich Layout, Logo und ASS, ein H.264-Encode. Kein zweiter Encode in diesem geprüften Pfad. Änderung: Cam-Skalierung mit Lanczos, geringe Luma-Schärfung `unsharp=5:5:0.35:5:5:0`, CRF 18 statt 20. Preset bleibt veryfast, keine neue Quelle.

Vergleich aus denselben Quellsekunden 5 bis 7, jeweils ein Encode, ohne Logo und ASS zur isolierten Messung: vorher 11.170.060 Bit/s, nachher 14.188.168 Bit/s, ungefähr 27 Prozent mehr. Verbesserung moderat, keine Rekonstruktion fehlender Details. Vorhandene fertige Vorschauen werden nicht massenweise gelöscht und profitieren erst beim erneuten Rendern.

Standbilder bei ungefähr Quellsekunde 6:
- `/home/nathanael/.claude/sichtpruefung/tiktok-dialog-tiefe/cam-vorher.png`
- `/home/nathanael/.claude/sichtpruefung/tiktok-dialog-tiefe/cam-nachher.png`

Quelle und Vergleichsvideos: `/tmp/tb-tiktok-cam-source-20261009/`. Crop-Nachweis: `/tmp/tb-tiktok-dialog-crop-20261009.log`.

## Dialogprüfung mit Moli

Reproduzierbare isolierte Prüffixture: `bot/dashboard_v2/tiktok-evidence.html` und `tests/tiktok-evidence.tsx`. Sie mountet den echten Dialog, ersetzt seine API-Antworten im Arbeitsspeicher und verweigert tatsächliches Veröffentlichen. Die Fixture belegt keine serverseitige Persistenz.

`browser-check.mjs` verbindet Playwright ausschließlich mit dem eigenen Moli-Prozess auf 9237. Zehn Zustandsprüfungen bestanden: leere Sichtbarkeit, ausgeschaltete Interaktionen, ausdrückliche Übernahme, aktuelle Kontosperren, Speichern und erneutes Laden, Zustimmung bleibt aus. Keine externen Uploads. Geometrie Desktop 1440/1440 und Mobil 390/390, keine horizontale Überbreite.

Moli meldet sichtbare Elemente teilweise als unsichtbar. Deshalb wurden DOM-Klicks statt vollständiger Mausbedienung geprüft. Berechnete Checkbox-Stile sind golden mit `appearance: none`; Moli malt sie im Screenshot trotzdem nativ weiß. Diese Browsergrenze ist offen, kein Nachweis vollständiger Checkbox-Paint- oder Tastaturprüfung.

Ablage `/home/nathanael/.claude/sichtpruefung/tiktok-dialog-tiefe/`: `vorher.png`, `nachher.png`, `dialog-1440.png`, `dialog-390.png`, `moli-dom.json`. Erfolgslog: `/tmp/tb-tiktok-dialog-moli-final-20261009.log`.

## Frontend

`npm run build`: erfolgreich. Gezielte Vertrags-, Layout-, Studio-, Dictionary- und Sprachtests: 50 bestanden, 0 fehlgeschlagen, 0 übersprungen.

Vollständige Suite: VOD 9/9, Kalender 9/9, Hauptlauf 436 bestanden und 5 fehlgeschlagen. Vollständiger archivierter Basisstand c6e1e6301: exakt dieselben Zahlen, insgesamt jeweils 454 bestanden und 5 fehlgeschlagen. Bestehende Fehler betreffen drei Palette-Prüfungen, Analytics-Gesamtbreite und OBS-Hilfe/Uploadbudget. Lint auf Änderung und Basis: jeweils 8 Fehler und 7 Warnungen, keine neue Differenz.

Logs: `/tmp/tb-tiktok-dialog-build-final-20261009.log`, `/tmp/tb-tiktok-dialog-tests-final-20261009.log`, `/tmp/tb-tiktok-dialog-targeted-tests-20261009.log`, `/tmp/tb-tiktok-lint.log`, `/tmp/tb-tiktok-baseline-20261009/lint.log`, `/tmp/tb-tiktok-baseline-20261009/tests-complete.log`.

## Rust, Gate und Betrieb

Rustc 1.97.1, SQLX_OFFLINE=true. Abschließender gezielter Editorlauf: 3 bestanden, 0 fehlgeschlagen, 0 ignoriert. Dazu gehört der echte isolierte PostgreSQL-Roundtrip. Erster vollständiger Dashboard-Lauf: 1356 bestanden, 7 fehlgeschlagen, 0 ignoriert; ein gültiger Basisvergleich läuft separat. Ein vorheriger archivierter Rust-Basisversuch war wegen fehlender Git-Herkunft kein gültiger Testlauf.

`cargo fmt --check` auf Änderung und archivierter Basis liefert nach Pfadnormalisierung exakt dieselben 265 Abweichungsblöcke in unberührten Dateien. Keine neue Formatabweichung. Lesende Produktionsprüfung: `twitchdash` und `twitchbot` besitzen SELECT sowie INSERT/UPDATE auf dem vorhandenen Einstellungsspeicher.

Lokales Gate für `9083826d8`: `[gpt-6.1-sol] ALLOW: No merge-blocking defect found in the supplied diff.` Nicht blockierender Browserhinweis steht in `REVIEW.md`. Noch kein Merge oder Deploy belegt. Abschließende Suite-, Integrations- und Betriebsnachweise werden im Abschlussprotokoll außerhalb des zu löschenden Worktrees gesichert.
