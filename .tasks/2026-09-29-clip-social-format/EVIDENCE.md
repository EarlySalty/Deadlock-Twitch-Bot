status: erledigt · 2026-09-29

# Paket A: Nachweise

## Sichtprüfung

Die PNGs stammen aus einem echten FFmpeg-Render mit Rust-`render_clips`, nicht aus einer Layout-Skizze. Quellen sind Kopien der vorhandenen Clips; die Testdatenbank `tb_social_test.t_clip_social_format` enthält die isoliert angewandte Migration. Clip 124535 ist eine Kopie der Quelle 124534 ohne gespeichertes Layout.

| Clip | 1 Sekunde | 9 Sekunden | Befund |
| --- | --- | --- | --- |
| 124534 | [PNG](EVIDENCE-124534-1s.png) | [PNG](EVIDENCE-124534-9s.png) | Kamerastreifen, Goldkante, DDC-Logo, Titel, Kanalname und vollständiges HUD |
| 124453 | [PNG](EVIDENCE-124453-1s.png) | [PNG](EVIDENCE-124453-9s.png) | Gleiche Gestaltung bei heller Kameraszene und anderem Spielausschnitt |
| 124549 | [PNG](EVIDENCE-124549-1s.png) | [PNG](EVIDENCE-124549-9s.png) | Gesprochener Text als eingebrannte Untertitel |
| 124535 | [PNG](EVIDENCE-no-layout-1s.png) | [PNG](EVIDENCE-no-layout-9s.png) | Ohne Layout: vollständiges Querformat vor verschwommenem Hintergrund, keine künstliche Kamerakachel |

`ffprobe` bestätigte für alle vier MP4s 1080 × 1920, H.264 High, `yuv420p`, 60 fps und AAC mit 48 kHz bei rund 192 kbit/s. Die Videobitraten lagen bei 9,25 bis 10,27 Mbit/s. Der Renderer nutzt einen FFmpeg-Aufruf pro Clip für Komposition, Logo, Titel, Untertitel und Audio.

## TikTok und Vorschau

Der voreingestellte Upload nutzt `video.upload` und `/v2/post/publish/inbox/video/init/`. `SEND_TO_USER_INBOX` bleibt als Postfachzustand sichtbar und setzt `uploaded_tiktok` nicht; erst `PUBLISH_COMPLETE` bestätigt einen veröffentlichten Beitrag. Tests prüfen den HTTP-Endpunkt, den Audit-Fehler-Fallback, den Datenbankzustand und die Sperre gegen einen zweiten Warteschlangeneintrag. Es gab keinen Upload auf ein echtes Konto. Offizielle Referenzen: [Upload](https://developers.tiktok.com/docs/en/content-posting-api-reference-upload-video) und [Status](https://developers.tiktok.com/docs/en/content-posting-api-reference-get-video-status).

Vorschauen speichern `data/clips/<id>_preview.mp4` statt eines absoluten Release-Pfads. Beide produktiven Dienste binden dieses Verzeichnis per systemd an `/var/lib/deadlock-twitch-media/clips`. Ein Test prüft die Auflösung alter Release-Pfade. Neue Upload-Konvertierungen verwenden `_branded_v1.mp4`, damit alte `_vertical.mp4`-Dateien aus wartenden Aufträgen nicht als fertiger Markenrender durchgehen. Die Produktionsmigration wurde nicht angewandt.

Die Migration schaltet den Modus-Default auf `stacked`. Gespeicherte PiP-Werte wandelt sie nur um, wenn die vollständige Geometrie einem der beiden am 29. September 2026 in der Produktionsdatenbank gelesenen Altstände entspricht (20 EarlySalty-Overrides mit derselben Geometrie und 14 Standard-Overrides). Andere Layouts bleiben erhalten. Ein Transaktionstest in `tb_social_test.t_clip_social_format` prüfte beide Altstände sowie eine abweichende Kameraposition: zwei Umwandlungen, der angepasste Clip unverändert, danach `ROLLBACK`.

TikToks bestehender OAuth-Weg fordert `video.upload` bereits in `rust/crates/tb-social-media/src/oauth.rs` an. Die [TikTok-Statusdokumentation](https://developers.tiktok.com/docs/en/content-posting-api-reference-get-video-status) bestätigt, dass `SEND_TO_USER_INBOX` nach einer Veröffentlichung durch den Streamer zu `PUBLISH_COMPLETE` werden kann und keine feste Abschlussfrist garantiert ist. Deshalb bleibt die Statusabfrage für offene Postfach-Clips aktiv; sie ist im Worker auf zwei Einträge pro Durchlauf mit mindestens zehn Minuten Abstand je Eintrag begrenzt. Einträge ohne Vorgangsnummer werden als fehlerhaft markiert. Fehlen Zugangsdaten, wird der nächste Prüfzeitpunkt verschoben, damit ältere Einträge spätere Clips nicht dauerhaft blockieren. Ein Datenbanktest prüft diese Reihenfolge.

## Prüfungen

- Rust: `PATH=~/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin:$PATH SQLX_OFFLINE=1 TB_TEST_DATABASE_URL='postgres:///tb_bb_test?host=/var/run/postgresql' cargo test -p tb-social-media -- --include-ignored`: 254 bestanden, 0 fehlgeschlagen, 0 ignoriert, 0 gefiltert. Die drei weiteren Test-Binaries und die Doc-Tests enthielten keine Tests.
- Rust: `SQLX_OFFLINE=1 cargo clippy -p tb-social-media --all-targets -- -D warnings`: erfolgreich ohne Warnungen.
- Dashboard: `npm run build` erfolgreich; gezielte Social-Media-Vertragstests 24 bestanden.
- Gesamte Dashboard-Suite: 405 von 409 bestanden, vier Fehler. Vergleich in einem sauberen, losgelösten Worktree auf demselben HEAD: ebenfalls 405 von 409 bestanden, dieselben vier Fehler in Farbpaletten- und OBS-Hilfetests.
- `cargo fmt --check --package tb-social-media` bleibt rot: 96 Diff-Blöcke gegenüber 102 Diff-Blöcken auf dem sauberen HEAD. `git diff --check` ist sauber.
