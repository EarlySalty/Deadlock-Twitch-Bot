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
- Social-Media-Bibliothek nach Integration seriell: 309 bestanden, 0 fehlgeschlagen, 0 ignoriert (`--include-ignored --test-threads=1`). Im vorherigen Parallellauf bestand die Suite 308 Tests, ein bestehender TikTok-Timeouttest scheiterte vor seinem Checkpoint; der vollständige serielle Nachlauf bestätigt auch diesen Test. Er wurde nicht verändert oder übersprungen. Vor Integration: 308 bestanden, 0 fehlgeschlagen, 0 ignoriert. API-Handler vor Integration: 48 bestanden, 0 fehlgeschlagen, 0 ignoriert. Fresh-Schema-Vertrag: 1 bestanden, 0 fehlgeschlagen, 0 ignoriert. Der eigene Zugang ohne historischen Kanalnamen und die Wiederaufnahme aus dem Wartezustand sind enthalten.
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

WIRKUNGSPRUEFUNG[WP-1]: 0 Befunde | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 4/4 geprüft

Geprüfte Pfade: Google-Widerruf, TikTok-Widerruf, YouTube-Uploadantwort und YouTube-Statusabfrage. Queue-Eingang und Worker wurden beide geprüft, ebenso beide YouTube-Erfolgspfade. Kein offener Befund; Fehlermeldungen unterscheiden bestätigten Erfolg und ausstehende Bestätigung.

## Noch offen

Gate, Push, Release, Migration, Neustart, Live-Beweis und Cleanup. Keine Fertigmeldung und kein Deploy-Nachweis liegt vor.
