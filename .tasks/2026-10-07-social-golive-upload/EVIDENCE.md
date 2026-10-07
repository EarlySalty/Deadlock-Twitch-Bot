# Auftrag D: Nachweise

## Umsetzung

1. Der gemeinsame Queue-Eingang prüft Plattformfähigkeit und verwendbare Kontoverbindung. Fehlende Verbindungen erzeugen `waiting_connection` mit einer verständlichen Meldung. Der Worker prüft erneut und nimmt fällige Warteaufträge nach einer Verbindung wieder auf. Die Warteprüfung verbraucht weder Uploadversuche noch Kontingentvertagungen und erhält den geplanten Termin. Abbrechen entfernt auch Warteaufträge. Die Terminbelegung berücksichtigt sie.
2. Der Status-Endpunkt liefert Upload- und Statistikfähigkeiten. TikTok-Statistiken werden nicht angefordert. Instagram ist ohne ausdrücklich bestätigte App-Freigabe und konfigurierte App-Zugänge gesperrt. Verbindung, Freigabeauswahl, Zeitplan und Statistikansicht berücksichtigen die Fähigkeiten.
3. Trennen löscht die eigene gespeicherte Verbindung. Google und TikTok werden über die dokumentierten Widerruf-Endpunkte angesprochen. Fehlende Bestätigung führt zu einer ehrlichen Aufforderung, die App im Plattformkonto zu entfernen. Für Instagram ist kein unterstützter Widerruf-Endpunkt belegt; die gespeicherten Zugangsdaten werden gelöscht und die zusätzliche Entfernung beim Anbieter wird angezeigt. Partner können die Sammelverbindung nicht löschen. Die Einordnung eigener Verbindungen nutzt die stabile Twitch-ID statt eines historischen Kanalnamens.
4. Die tatsächliche YouTube-Sichtbarkeit wird aus der Uploadantwort oder einer anschließenden Statusabfrage übernommen. Ohne Bestätigung wird nicht öffentlich behauptet. Die neue Migration speichert den bestätigten Wert, die API und Clipkarte zeigen ihn.
5. Löschfristtexte erklären die bedingte Bereinigung nach 14 Tagen. TikTok-Passagen und Anreicherung bleiben gemäß geänderter Entscheidung unberührt. Das Startlog zählt tatsächlich gestartete Worker mit und ohne Verschlüsselungskonfiguration.

## Bisher abgeschlossene Prüfungen

- Offline-Compilerprüfung nach Integration: `cargo check -p tb-social-media -p tb-dashboard-api -p tb-bot --all-targets -j2`, `PATH=/home/nathanael/.cargo/bin:$PATH SQLX_OFFLINE=true`, Exit 0, Dauer 3 Minuten 38 Sekunden. Nur bestehende Deprecation-Warnungen.
- Isolierte PostgreSQL-16-Instanz auf 127.0.0.1:55437. Datenbank `tb_social_upload_test`, UTF8, lokale TimescaleDB 2.29.1. Der ursprünglich vorgesehene Dockerlauf mit TimescaleDB 2.17.2 konnte wegen eines devpts-Mountfehlers nicht starten. Kein Test lief gegen die Produktionsdatenbank.
- Migrationen auf der isolierten Datenbank angewendet. Das SQLx-Metadatenartefakt des geänderten Insights-Queries stammt aus dieser echten Datenbank. Andere Metadaten wurden nicht entfernt.
- Social-Media-Bibliothek nach Integration seriell: 309 bestanden, 0 fehlgeschlagen, 0 ignoriert (`--include-ignored --test-threads=1`). Im vorherigen Parallellauf bestand die Suite 308 Tests, ein bestehender TikTok-Timeouttest scheiterte vor seinem Checkpoint; der vollständige serielle Nachlauf bestätigt auch diesen Test. Er wurde nicht verändert oder übersprungen. Vor Integration: 308 bestanden, 0 fehlgeschlagen, 0 ignoriert. API-Handler nach Integration: 48 bestanden, 0 fehlgeschlagen, 0 ignoriert. Fresh-Schema-Vertrag: 1 bestanden, 0 fehlgeschlagen, 0 ignoriert. Der eigene Zugang ohne historischen Kanalnamen und die Wiederaufnahme aus dem Wartezustand sind enthalten.
- Dashboard nach Integration: Produktionsbuild Exit 0, Verträge 45 bestanden, 0 fehlgeschlagen, Chromium 25 bestanden, 0 fehlgeschlagen. Der echte Recovery-Button öffnet die Konteneinstellungen. Ein abgelaufenes eigenes Konto bietet Neu verbinden und Trennen, die Sammelverbindung nur das eigene Konto. Sichtprüfung mit 1440 und 390 Pixel bestätigt die Aktionen.
- Sichtprüfung: `/home/nathanael/.claude/sichtpruefung/social-golive-upload/`. Viewportaufnahmen für 1440 und 390 Pixel zeigen Wartezustand, Verbindungsweg, private YouTube-Sichtbarkeit, eigenes Konto statt Trennen bei Sammelverbindung und nicht angebotene Instagram-Verbindung. Kein horizontaler Dokumentüberlauf im Browserlauf.
- Die vollständige API-Bibliothek: Änderungsstand 1309 bestanden, 22 fehlgeschlagen, 0 ignoriert. Unveränderte Basis ebenfalls 1309 bestanden, 22 fehlgeschlagen, 0 ignoriert. Die sortierten Namen aller 22 Fehler sind identisch. Kein Social-Media-Handler gehört zur Fehlermenge. Diese Suite wird nicht als grün gemeldet.
- Clippy mit Abhängigkeiten scheitert bei `tb-raid/src/signup_denylist.rs:71` an `clippy::result_unit_err`. Ohne Abhängigkeitslint scheitern Basis und Änderungsstand an denselben zwei Befunden: `analytics.rs:321` (`too_many_arguments`) und `vocab.rs:164` (`needless_borrows_for_generic_args`). Kein Lint wurde unterdrückt.
- Formatierung: workspaceweite Basis 43 Dateien mit Abweichungen, Änderungsstand 42; keine neue fehlschlagende Datei. Dateiweises `rustfmt --edition 2021 --config skip_children=true --check` für alle eigenen Rust-Dateien endet mit Exit 0. Ein versehentlicher Prüflauf mit Edition 2024 wurde nicht mutierend ausgeführt und durch die korrekte Workspace-Edition ersetzt.
- Frisches `origin/main` e0b0dbaf integriert. Auftrag F hat die Anreicherung abgeschaltet; die tatsächliche Startzählung ist entsprechend fünf unabhängige Worker plus vier bei verfügbarer Verschlüsselung. TikTok Direct Post ist auf diesem Basisstand noch nicht integriert; das API-Merkmal bleibt deshalb `inbox`.

## Wirkungsprüfung

- Gemeinsamer Eintritt: `clip_queue::queue_upload`. Wiederprüfung: `UploadWorker::run_once`. Wiederverwendung, fällige Warteprüfung, Terminbelegung, Abbrechen und erfolgreiche Wiederaufnahme sind berücksichtigt.
- Fremddienstpfade: Google-Widerruf bestätigt nur HTTP 200; TikTok-Widerruf verlangt zusätzlich den dokumentierten leeren JSON-Erfolg. HTTP-Fehler, Transportfehler und nicht bestätigte Antworten ergeben keine Erfolgsmeldung. Providerantworten und Zugangsdaten werden nicht geloggt. YouTube-Uploadantworten und Statusabfrage liefern nur bestätigte Sichtbarkeitswerte.
- Live-Rechte vor Deploy lesend geprüft: `twitchdash` besitzt DELETE auf `social_media_platform_auth` und INSERT auf `twitch_clips_upload_queue`. Keine Rechte wurden erweitert.
- Produktionsschema vor Deploy: `youtube_visibility` und Migration 20261007213000 fehlen. Die Migration muss vor Aktivierung des neuen Codes angewendet werden.
- Echte Streamer-Zugänge wurden nicht getrennt. Providerproben benutzen synthetische Zugangsdaten und lokale HTTP-Server.

## Prüfprotokoll

TESTNACHWEIS[TW-1]: 309 passed, 0 ignored | Baseline: 22 rot

Die Pflichtzeile zählt die integrierte Social-Media-Bibliothek. Die 22 Baselinefehler gehören zur vollständigen API-Suite und sind oben mit Zahl und identischer Fehlermenge belegt. Die API-Teilprüfung, das Schema und das Dashboard sind separat gezählt.

TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 1 belegt | Senke: Produktionsbundle und Social-Media-Rechtstexte

Neue Produkttexte enthalten echte Umlaute. Die einzige neu geprüfte absolute Formulierung erklärt, dass ein Anbieterwiderruf nicht sämtliche lokalen Daten löscht; der Löschpfad entfernt ausschließlich die Kontoverbindung und nicht Clips oder Statistiken. Das gebaute Dashboard und die Viewportaufnahmen belegen die neuen Hinweise. Der Patch enthält keine neuen Code-Kommentare und keine Gedankenstriche in neuem Produkttext.

WIRKUNGSPRUEFUNG[WP-1]: 3 Befunde | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 4/4 geprüft

Geprüfte Fremddienstpfade: Google-Widerruf, TikTok-Widerruf, YouTube-Uploadantwort und YouTube-Statusabfrage. Queue-Eingang und Worker wurden beide geprüft, ebenso beide YouTube-Erfolgspfade. Der anschließende Merge-Gate fand einen bestätigten Blocker zur Fairness der gemeinsamen Queue-Auswahl sowie zwei Nits. Die Mängelliste und beide betroffenen Queue-Stellen stehen in REVIEW.md. Fehlermeldungen der Provider unterscheiden bestätigten Erfolg und ausstehende Bestätigung.

## Gate-Protokoll

MERGEPROTOKOLL[MS-1]: 33 Git-Schritte einzeln | Anläufe: 1 | Gate: BLOCK gpt-6.1-sol

Die Git-Zählung ist der Transcriptstand bis zur Gate-Entscheidung, vor dem anschließenden reinen Dokumentationscommit für die Übergabe. Kein Gate wurde umgangen und kein Urteil bei einem anderen Modell neu angefordert.

## Historischer Übergabestand

Gate-Runde 1: BLOCK durch `gpt-6.1-sol`, Kandidat f2b64b39706417ca63071e5fcf8cc857ab9dff01. Der ursprüngliche Worker übergab an einen frischen Fixer. Sein Release-Build wurde gestoppt. Keine Produktionsmigration, kein Merge oder Push auf main, kein Deploy und kein Neustart in dieser Runde.

## Fixer und Gate-Folgerunde

Der unveränderte Übergabecommit ac08dba4ceb5406c62ad512341bc9ca6eadafb58 wurde zuerst mit normalem Push auf `origin/fix/social-golive-upload-texte` gesichert. Kein Force-Push. `origin/main` wurde erneut geholt; es steht weiterhin auf e0b0dbaf662d7680c4ceaa210bf15f1443693cd8.

Die Uploadauswahl und die Verbindungsprüfung sind getrennt. `pending` wählt keine Warteaufträge aus. Ein unabhängiger Scan prüft fällige Warteaufträge nach `last_attempt_at ASC NULLS FIRST`, danach Erstellungszeit und ID. Wiederaufnahme und bestehendes Scanlimit bleiben erhalten. Der echte Worker erreicht im isolierten PostgreSQL-Test bei 100 älteren unverbundenen Aufträgen den neueren verbundenen Auftrag im ersten Durchlauf und den wartenden verbundenen Auftrag im sechsten Durchlauf. Die übrigen Warteaufträge behalten null Versuche und null Kontingentvertagungen.

Fehlende und unvollständige Verbindungen liefern strukturierte Gründe. Das Dashboard übersetzt die gemeinsamen Vorlagen mit Plattformnamen. Gespeicherte deutsche Altgründe erhalten einen übersetzten allgemeinen Verbindungshinweis. Kein neuer Test hängt an unabhängig dupliziertem Produktwortlaut.

### Tatsächlich abgeschlossene Fixerprüfungen

Gemeinsame Rust-Umgebung: `PATH=/home/nathanael/.cargo/bin:/usr/local/bin:/usr/bin:/bin SQLX_OFFLINE=true`. Testdatenbank: `TB_TEST_DATABASE_URL=postgresql://nathanael@127.0.0.1:55437/tb_social_upload_test`. Manifest: `/home/nathanael/.worktrees/tb-social-golive-upload/rust/Cargo.toml`.

- `cargo test --manifest-path <Manifest> -p tb-social-media --lib -- --include-ignored --test-threads=1`: 311 bestanden, 0 fehlgeschlagen, 0 ignoriert, 0 gefiltert. Echte Datenbankprüfung, keine Provideruploads. Log `/tmp/tb-upload-fixer-social-tests.log`, Exit 0.
- `cargo test --manifest-path <Manifest> -p tb-dashboard-api --lib handlers::social_media:: -- --include-ignored --test-threads=1`: 53 bestanden, 0 fehlgeschlagen, 0 ignoriert, 1278 gefiltert. Log `/tmp/tb-upload-fixer-api-tests.log`, Exit 0.
- `cargo check --manifest-path <Manifest> -p tb-social-media -p tb-dashboard-api -p tb-bot --all-targets -j 2`: Exit 0, 5 Minuten 47 Sekunden. Log `/tmp/tb-upload-fixer-check.log`. Dateiweises `rustfmt --edition 2021 --config skip_children=true --check` für die drei geänderten Rust-Dateien: Exit 0.
- Im Dashboard: `node --import tsx --test tests/socialMediaContract.test.ts tests/i18n.test.ts tests/socialMediaLayout.test.ts tests/socialStudioRedesign.test.ts`: 46 bestanden, 0 fehlgeschlagen, 0 ignoriert. `npm run build`: Exit 0. Logs `/tmp/tb-upload-fixer-dashboard-tests.log` und `/tmp/tb-upload-fixer-dashboard-build.log`.
- `STUDIO_ARTIFACT_DIR=/home/nathanael/.worktrees/tb-social-golive-upload/.tasks/2026-10-07-social-golive-upload/browser-fixer node --test /home/nathanael/.worktrees/tb-social-golive-upload/bot/dashboard_v2/tests/socialStudio.browser.test.mjs`: 26 bestanden, 0 fehlgeschlagen, 0 übersprungen, Exit 0. Log `/tmp/tb-upload-fixer-browser-tests.log`. `browser-state.json` enthält keine Browserfehler.

Die historischen roten Gesamt-API- und Clippy-Baselines oben wurden in der Fixerprüfung nicht neu ausgeführt. Sie werden nicht als neue grüne Läufe ausgegeben.

### Zugänglicher Sichtnachweis

Zehn tatsächliche Chromium-Viewportaufnahmen, `browser-state.json` und `waiting-i18n-dom.json` liegen versionierbar in `browser-fixer/`. Deutsche Pipeline und Kontoverbindungen sowie englische fehlende, unvollständige und alte Verbindungsgründe wurden bei 1440 und 390 Pixel aufgenommen. In den englischen DOM-Messungen entspricht die Dokumentbreite jeweils der Viewportbreite. Der Recovery-Knopf öffnet die Konteneinstellungen. Desktop- und Mobilaufnahmen wurden betrachtet. `SICHTPRUEFUNG.txt` ist der explizite Eingabeblock für `GATE_SICHTPRUEFUNG_BLOCK` beim manuellen Gate.

TESTNACHWEIS[TW-1]: 311 passed, 0 ignored | Baseline: 22 rot

Die Pflichtzeile zählt den neuen Social-Media-Lauf. Die 22 roten Fehler bezeichnen ausdrücklich die historische, zahlenmäßig verglichene vollständige API-Baseline. Der neue API-Teillauf mit 53 Tests ist grün.

TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: übersetzte Wartegründe im Produktionsbundle

Die Regeländerung des Nutzers vom 7. Oktober 2026 gilt für die Folgerunden: Bei BLOCK startet diese Session einen frischen nativen Fixer-Subagenten mit den offenen Funden, Verträgen und dem Worktreestand. Der Gate behält `gpt-6.1-sol`. Keine neuen T3-Threads für die Fixschleife. Eskalation bei tatsächlichem Blocker oder spätestens nach fünf erfolglosen Runden. Merge, Release, Migration, Deploy und Cleanup sind vor dem ausstehenden ALLOW noch nicht erfolgt.
