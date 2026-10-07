# Auftrag D: Nachweise

## Umsetzung

1. Der gemeinsame Queue-Eingang prüft Plattformfähigkeit und verwendbare Kontoverbindung. Fehlende Verbindungen erzeugen `waiting_connection` mit einer verständlichen Meldung. Der Worker prüft erneut und nimmt fällige Warteaufträge nach einer Verbindung wieder auf. Die Warteprüfung verbraucht weder Uploadversuche noch Kontingentvertagungen und erhält den geplanten Termin. Abbrechen entfernt auch Warteaufträge. Die Terminbelegung berücksichtigt sie.
2. Der Status-Endpunkt liefert Upload- und Statistikfähigkeiten. TikTok-Statistiken werden nicht angefordert. Instagram ist ohne ausdrücklich bestätigte App-Freigabe und konfigurierte App-Zugänge gesperrt. Verbindung, Freigabeauswahl, Zeitplan und Statistikansicht berücksichtigen die Fähigkeiten.
3. Trennen löscht die eigene gespeicherte Verbindung. Google und TikTok werden über die dokumentierten Widerruf-Endpunkte angesprochen. Fehlende Bestätigung führt zu einer ehrlichen Aufforderung, die App im Plattformkonto zu entfernen. Für Instagram ist kein unterstützter Widerruf-Endpunkt belegt; die gespeicherten Zugangsdaten werden gelöscht und die zusätzliche Entfernung beim Anbieter wird angezeigt. Partner können die Sammelverbindung nicht löschen. Die Einordnung eigener Verbindungen nutzt die stabile Twitch-ID statt eines historischen Kanalnamens.
4. Die tatsächliche YouTube-Sichtbarkeit wird aus der Uploadantwort oder einer anschließenden Statusabfrage übernommen. Ohne Bestätigung wird nicht öffentlich behauptet. Die neue Migration speichert den bestätigten Wert, die API und Clipkarte zeigen ihn.
5. Löschfristtexte erklären die bedingte Bereinigung nach 14 Tagen. TikTok-Passagen und Anreicherung bleiben gemäß geänderter Entscheidung unberührt. Das Startlog zählt tatsächlich gestartete Worker mit und ohne Verschlüsselungskonfiguration.

## Bisher abgeschlossene Prüfungen

- Offline-Compilerprüfung: `cargo check -p tb-social-media -p tb-dashboard-api --all-targets -j 2`, Exit 0.
- Isolierte PostgreSQL-16-Instanz auf 127.0.0.1:55437. Datenbank `tb_social_upload_test`, UTF8, lokale TimescaleDB 2.29.1. Der ursprünglich vorgesehene Dockerlauf mit TimescaleDB 2.17.2 konnte wegen eines devpts-Mountfehlers nicht starten. Kein Test lief gegen die Produktionsdatenbank.
- Migrationen auf der isolierten Datenbank angewendet. Das SQLx-Metadatenartefakt des geänderten Insights-Queries stammt aus dieser echten Datenbank. Andere Metadaten wurden nicht entfernt.
- Social-Media-Bibliothek: 307 bestanden, 0 fehlgeschlagen, 0 ignoriert. API-Handler Social Media: 48 bestanden, 0 fehlgeschlagen, 0 ignoriert. Fresh-Schema-Vertrag: 1 bestanden, 0 fehlgeschlagen, 0 ignoriert. Diese Läufe enthalten noch nicht den zuletzt ergänzten Test zur eigenen Verbindung ohne historischen Kanalnamen; ein abschließender Lauf steht aus.
- Dashboard-Verträge: 45 bestanden, 0 fehlgeschlagen. Chromium mit Produktionsbundle und isoliertem API-Vertrag: 25 bestanden, 0 fehlgeschlagen. Änderungen seit diesem Browserlauf: Abbruchmöglichkeit für einen freigegebenen Warteauftrag ohne Termin. Ein abschließender Build und Browserlauf steht aus.
- Sichtprüfung: `/home/nathanael/.claude/sichtpruefung/social-golive-upload/`. Viewportaufnahmen für 1440 und 390 Pixel zeigen Wartezustand, Verbindungsweg, private YouTube-Sichtbarkeit, eigenes Konto statt Trennen bei Sammelverbindung und nicht angebotene Instagram-Verbindung. Kein horizontaler Dokumentüberlauf im Browserlauf.
- Die vollständige API-Bibliothek wurde zusätzlich ausgeführt: 1309 bestanden, 22 fehlgeschlagen, 0 ignoriert. Kein Social-Media-Handler gehört zur Fehlermenge. Eine Baseline-Messung zur Zuordnung steht noch aus; diese Suite wird nicht als grün gemeldet.
- Clippy mit Abhängigkeiten wurde ausgeführt und scheitert bei `tb-raid/src/signup_denylist.rs:71` an `clippy::result_unit_err`. Der geänderte Code dort gehört nicht zu Auftrag D. Die Prüfung der betroffenen Pakete ohne Abhängigkeitslint läuft separat; es wurde kein Lint unterdrückt.

## Wirkungsprüfung

- Gemeinsamer Eintritt: `clip_queue::queue_upload`. Wiederprüfung: `UploadWorker::run_once`. Wiederverwendung, fällige Warteprüfung, Terminbelegung, Abbrechen und erfolgreiche Wiederaufnahme sind berücksichtigt.
- Fremddienstpfade: Google-Widerruf bestätigt nur HTTP 200; TikTok-Widerruf verlangt zusätzlich den dokumentierten leeren JSON-Erfolg. HTTP-Fehler, Transportfehler und nicht bestätigte Antworten ergeben keine Erfolgsmeldung. Providerantworten und Zugangsdaten werden nicht geloggt. YouTube-Uploadantworten und Statusabfrage liefern nur bestätigte Sichtbarkeitswerte.
- Live-Rechte vor Deploy lesend geprüft: `twitchdash` besitzt DELETE auf `social_media_platform_auth` und INSERT auf `twitch_clips_upload_queue`. Keine Rechte wurden erweitert.
- Produktionsschema vor Deploy: `youtube_visibility` und Migration 20261007213000 fehlen. Die Migration muss vor Aktivierung des neuen Codes angewendet werden.
- Echte Streamer-Zugänge wurden nicht getrennt. Providerproben benutzen synthetische Zugangsdaten und lokale HTTP-Server.

## Noch offen

Abschließende Prüfungen, Baseline, frisches origin/main, Gate, Merge, Push, Release, Migration, Neustart, Live-Beweis und Cleanup. Keine Fertigmeldung und kein Deploy-Nachweis liegt vor.
