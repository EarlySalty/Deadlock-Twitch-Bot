# Social Media: Architektur und Betrieb

Stand: 2026-10-07. Code: `rust/crates/tb-social-media/`, Workerstart: `rust/bin/tb-bot/src/main.rs`.

## Laufzeit

Die Social-Media-Pipeline läuft im Rust-Bot `tb-bot`. Die früheren Dateien unter `bot/social_media/` sind entfernt. Das Schema wird durch die versionierten SQLx-Migrationen unter `rust/migrations/` verwaltet, nicht durch einen zweiten Laufzeit-Bootstrap.

Der periodische Clip-Fetcher wird durch `bot.clip_fetcher_enabled` gesteuert. Der Default ist `false`; der tatsächliche Start hängt von der gespeicherten Betriebskonfiguration und einem verfügbaren Helix-Client ab. Beim Live-Abgleich am 2026-10-07 war der Fetcher gestartet. Der Highlight-Clipper hat einen eigenen Schalter und war bei diesem Abgleich deaktiviert.

`tb-bot` startet Retention, Approval-Queue, Report-Dispatcher, Clip-Prep und Vorschau. Upload, Kontenerneuerung und Insights benötigen zusätzlich den Schlüssel für die verschlüsselten Plattformzugänge. Der automatische Enrichment-Worker wird nicht gestartet. Vorhandene Analyse- und Bearbeitungsbausteine bleiben erhalten; ihre Existenz bedeutet keinen laufenden Transkriptions- oder LLM-Auftrag. Der Report-Dispatcher kann unabhängig davon Modellaufrufe ausführen.

## Bausteine

Die folgenden Pfade liegen unter `rust/crates/tb-social-media/src/`:

| Bereich | Rust-Module | Aufgabe |
| --- | --- | --- |
| Eingang | `clip/`, `clip_manager.rs` | Twitch-Clips abrufen, manuelle Uploads aufnehmen, Clipzustände speichern |
| Vorbereitung | `clip_prep_worker.rs`, `preview.rs`, `render.rs`, `video_processor.rs` | Atomarer Download, Vorschau, Hochkantformat und Untertitel |
| Freigabe | `partner_access.rs`, `approval.rs`, `approval_worker.rs` | Kanalzugriff prüfen, Clipentscheidung speichern, freigegebene Clips einreihen |
| Veröffentlichung | `clip_queue.rs`, `upload_worker.rs`, `uploaders/`, `tiktok_recovery.rs` | Übertragung, Wartestatus und plattformspezifische Bestätigung |
| Konten | `credentials.rs`, `oauth.rs`, `refresh_worker.rs` | Konten über die Twitch-ID verbinden und Zugänge erneuern |
| Auswertung | `analytics.rs`, `insights_worker.rs`, `report_writer.rs`, `report_dispatcher.rs` | Verfügbare Plattformdaten und Berichte speichern |
| Aufbewahrung | `retention.rs`, `retention_worker.rs` | Abgelaufene, verworfene oder vollständig veröffentlichte Clips aufräumen |
| Erhaltene Anreicherung | `enrichment.rs`, `enrich_pipeline.rs`, `transcription.rs`, `llm_dispatch.rs`, `vocab.rs`, `correction.rs` | Analysezustände und manuelle Bearbeitung; kein automatischer Enrichmentstart |

## Bedienung und Verträge

Der Social-Media-Manager liegt unter `/twitch/social-media`; die Oberfläche stammt aus `bot/dashboard_v2/src/pages/SocialMediaManager.tsx`. Unterpfade öffnen dieselbe SPA. `/social-media` und `/social-media-admin` leiten samt Unterpfad und Query mit HTTP 308 unmittelbar auf den neuen Pfad weiter. Die API verwendet `/twitch/social-media/api/*`; der alte API-Namensraum bleibt für offene Tabs erhalten. `/social-media/terms`, `/social-media/privacy`, `/privacy` und die registrierten alten OAuth-Callbacks behalten direkte Handler. Die neuen Rechts- und Callbackadressen unter `/twitch/social-media` sind zusätzliche Aliase. Details und Plattform-Schritte stehen in [Twitch-Pfadmigration](../TWITCH_PATH_MIGRATION.md). Die Rust-API liegt in `rust/crates/tb-dashboard-api/src/handlers/social_media.rs`. Die Kanal-Freigabe kann auch im Admin-Dashboard unter der Streamer-Detailseite gesetzt werden. Der Adminalias `/twitch/api/admin/partner-access` verwendet denselben Handler wie `/twitch/social-media/api/access`. Beide Schreibwege erwarten `twitch_user_id`, nicht den veränderlichen Login.

Ein fehlgeschlagener manueller Clip-Fetch liefert HTTP 502 mit `success: false` und dem Fehler des Fetchdiensts. Eine leere, erfolgreiche Suche bleibt ein Erfolg mit null Clips.

Prep, Vorschau und Upload verwenden `download_atomic`: Download in eine eindeutige temporäre Datei, danach Umbenennen zum Ziel. `register_local_file` speichert den Pfad und löscht einen früheren Downloadfehler.

Die Aufräumfrist wird aus `retention_until` gelesen. Erst nach Ablauf und nach Verwerfen oder Veröffentlichung auf den aktiven Plattformen werden die gespeicherten Originalpfade, die Vorschau und die daraus abgeleiteten plattformspezifischen `branded_v2`-Dateien entfernt. Ein Dateilöschfehler lässt den Datensatz für einen erneuten Versuch bestehen. Das Dateialter ist kein eigenständiger Löschgrund.

## Grenzen

Eine Queuebestätigung ist kein Beleg für einen veröffentlichten Post. TikTok unterscheidet den bestätigten Post, die Übergabe an den Posteingang und eine noch offene Übertragung. TikTok-Analytics sind nicht implementiert; daraus dürfen keine gemessenen Nullwerte entstehen. Instagram benötigt ein verbundenes Konto, bevor seine Uploadfunktion im Betrieb geprüft werden kann.

Verwandt: [Highlight-Clipper](highlight-clipper.md), [Social-Media-Pipeline](../internal/social-media-pipeline.md).
