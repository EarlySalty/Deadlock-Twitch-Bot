---
status: aktiv
datum: 2026-09-06
thema: Was kann das Social-Media-Modul heute bei der Clip-Aufbereitung, was fehlt
methode: graphify query -> sed/grep Nachlese -> SELECT-only DB (twitch_analytics)
---

# Clip-Aufbereitung "social media ready" - Bestandsaufnahme tb-social-media

TLDR: Der Zuschnitt ins Hochformat 9:16 existiert und ist verdrahtet, aber der echte
Upload-Pfad nutzt nur einen dummen Center-Crop. Facecam-Compositing, Untertitel/STT und
Blur sind NICHT im Live-Pfad (compose_vertical ohne Aufrufer, Transkription per
Entscheidung deaktiviert). Titel/Hashtags laufen korrekt ueber tb-llm/Fireworks, haben
aber in der DB null Ergebnisse produziert. Modul ist verdrahtet, aber praktisch kalt.

Beobachtung = mit `pfad:zeile` belegt. Vermutung = ausdruecklich als solche markiert.

---

## 1. Crate `rust/crates/tb-social-media` - Datei fuer Datei

### layout.rs (864 Z.) - Layout-Modell, KEIN ffmpeg
- Beobachtung: Zielformat fest 9:16, `TARGET_WIDTH=1080`, `TARGET_HEIGHT=1920`
  (`rust/crates/tb-social-media/src/layout.rs:36-38`).
- Beobachtung: Layout beschreibt zwei Ausschnitte aus dem 1920x1080-Twitch-Bild:
  `game_crop` und `cam_crop`, plus `cam_position` als Zielrechteck im Hochformat-Frame
  (`layout.rs:205-206`, Modul-Doku `layout.rs:1-30`). PiP-Default-Kachel `DEFAULT_PIP_TILE`
  (`layout.rs:47`).
- Beobachtung: reine Datenstruktur mit Validierung/Persistenz, kein Subprocess
  (`layout.rs`, keine `Command`-Nutzung im ganzen File). Zustand: fertig.

### video_processor.rs (422 Z.) - der einzige ffmpeg-Renderer
- Beobachtung: dünner ffmpeg/ffprobe-Subprocess-Wrapper, Modul-Doku nennt zwei Pfade:
  `convert_and_trim` (Center-Crop, vom Upload-Worker) und `compose_vertical`
  (Game+Cam als PiP/Stacked, "vom Dashboard-Compose genutzt")
  (`video_processor.rs:1-8`).
- Beobachtung: `build_compose_filter` baut den `-filter_complex`-Graph fuer Game-Crop +
  optional Cam-Kachel als PiP oder Stacked, mit `overlay` (`video_processor.rs:47-93`).
  Encode `libx264 -preset medium -crf 23` (`video_processor.rs:188-192`).
- Beobachtung: `build_crop_filter` macht Center/Top/Bottom-Crop + Scale auf Zielgroesse
  (`video_processor.rs:96-131`). `compose_vertical` bei `:178`, `convert_to_vertical`
  bei `:205`, `convert_and_trim` bei `:258`; `convert_and_trim` ruft
  `convert_to_vertical(..., "center")` mit hart verdrahtetem "center"
  (`video_processor.rs:272`).
- Beobachtung: KEIN `boxblur`/`drawtext`/`subtitles`/`-vf subtitles` irgendwo im Crate
  (crateweiter Grep auf `blur|drawtext|subtitle|.srt|untertitel` liefert nur
  Kommentar-Treffer und Instagram/TikTok-Caption-Strings, kein Filter). Zustand: fertig
  fuer Crop und Compositing, aber ohne Untertitel und ohne Blur-Hintergrund.

### KRITISCH: Facecam-Compositing hat keinen Aufrufer im Live-Pfad
- Beobachtung: `compose_vertical` (Game+Facecam PiP/Stacked) wird nirgends aufgerufen.
  Grep ueber `tb-social-media/src` und `tb-dashboard-api/src` findet als einzige
  Aufrufer von `convert_to_vertical`/`convert_and_trim` nur den Upload-Worker; fuer
  `compose_vertical` KEIN Aufrufer (`upload_worker.rs:306`, `upload_worker.rs:563`,
  `upload_worker.rs:573`; Definition `video_processor.rs:178` ohne Call-Site).
- Beobachtung: Der echte Upload nutzt `convert_to_vertical(&local_path, &item.platform)`
  -> `convert_and_trim(...)` -> Center-Crop (`upload_worker.rs:563-582`,
  `video_processor.rs:272`). Es geht KEIN StreamerLayout in den Upload ein, nur die
  Plattform (fuer die Laenge).
- Folgerung (Vermutung): Der Layout-Editor im Dashboard speichert ein StreamerLayout, das
  der produktive Upload-Pfad ignoriert. Facecam-PiP/Stacked ist gebaut und getestet, aber
  im Live-Weg toter Code. Das ist der groesste Bruch zwischen "was das Modul kann" und
  "was beim Upload passiert".

### enrich_pipeline.rs (544 Z.) - Orchestrator transcribe->correct->LLM->save
- Beobachtung: verkettet Whisper-Transkription -> Vokabel-Korrektur -> LLM-Anreicherung
  -> Speichern (`enrich_pipeline.rs:1-6`).
- Beobachtung: `Transcriber` ist ein injizierbares Trait, aber die Transkription ist
  "per Grillme-Entscheidung (Block 15) bewusst deaktiviert - es wird KEIN Transcriber
  injiziert (`transcriber = None`), die Stage wird uebersprungen. Die frühere OpenAI-Impl
  ist entfernt (kein OpenAI)" (`enrich_pipeline.rs` Trait-Doku am `Transcriber`).
- Zustand: LLM-Stage aktiv, Transkriptions-Stage strukturell vorhanden aber ausgeschaltet.

### enrichment.rs (615 Z.) / correction.rs (291 Z.) / vocab.rs (422) / seed_vocab.rs
- Beobachtung: `enrichment.rs` haelt die DB-Zeilen (`ensure_enrichment_row`,
  `save_transcript`, `save_corrected`, `save_llm_output`, Status-Konstanten inkl.
  `STATUS_SKIPPED_NO_KEY`) (referenziert in `enrich_pipeline.rs` use-Block).
- Beobachtung: `correction.rs` ist Fuzzy-Korrektur eines Whisper-Transkripts gegen das
  Deadlock-Vokabular (`correction.rs:1`). Ohne Transkript hat die Korrektur nichts zu tun.
- Zustand: fertig, aber der Eingang (Transkript) fehlt strukturell (siehe oben).

### llm.rs (398) / llm_dispatch.rs (221) - LLM-Weg
- Beobachtung: `llm_dispatch.rs` ist "Zentraler Fireworks-Dispatcher... Frühere Ollama-,
  MiniMax- und Claude-Bahnen sind entfernt. Auch dieser Bereich nutzt ausschliesslich
  `tb_llm::complete`" (`llm_dispatch.rs:1-14`). `USE_CASE = "social_media"`, Timeout 60 s,
  `endpoint_for(USE_CASE)`, `ledger_purpose("social-media-fireworks")`
  (`llm_dispatch.rs:15-16`, FireworksProvider `from_connector`/`generate_text`).
- Urteil: LLM-Aufrufe laufen konform ueber den zentralen tb-llm-Hub (Fireworks/Deepseek),
  nicht ueber ein eigenes KI-Modul. Erzeugt pro Plattform Titel, Beschreibung, Hashtags.
- Zustand: fertig und regelkonform.

### title_gate.rs (162) - Qualitaets-Weiche fuer Titel
- Beobachtung: `decide(...)` entscheidet zwischen `UseExisting`,
  `GenerateFromMetadata`, `TranscribeThenGenerate` je nach generischem Titel,
  Vokabeltreffer und ob ein Transkript vorliegt (`title_gate.rs:15-60`). Generische Titel
  wie "clip", "highlight", "live" werden verworfen (`title_gate.rs:1-13`).
- Folgerung (Vermutung): Weil kein Transcriber laeuft, faellt der Pfad
  `TranscribeThenGenerate` praktisch aus; es bleibt Titel aus Metadaten (Clip-Titel,
  Spielname), nicht aus dem gesprochenen Inhalt.

### forms.rs (764) - Google-Forms-Fallback fuer manuelle Freigabe
- Beobachtung: baut Formular-Payload aus Clip-Titel/URL (`forms.rs:221-305`,
  `entry.19302401...`-Felder). Reiner Metadaten-Weg, kein Rendering.

### posting_plan.rs (704) / scheduler.rs (360) - Planung
- Beobachtung: als Nodes/Handler vorhanden (`posting_plan.rs:1`, `scheduler.rs:1`),
  Handler `posting_plan_put_handler`, `posting_plan_category_put_handler`
  (`handlers/social_media.rs:3110`, `:3319`). Steuert Kadenz/Reihenfolge je Plattform.
  Nicht tief gelesen; kein ffmpeg, reine Zeit-/Reihenfolgen-Logik (Vermutung nach Namen
  und Handlernamen).

### upload_worker.rs (1364) - Download + Convert + Upload
- Beobachtung: Ablauf `process`: Approval-Check (`is_clip_approved_for`) ->
  `do_upload` -> Uploader (`upload_worker.rs:142-217`). `do_upload` laedt per yt-dlp
  `-f best` von `clip_url` (`download_clip`, `upload_worker.rs:~335-360`), dann
  `convert_to_vertical` (Center-Crop), dann Upload (`upload_worker.rs:300-322`).
- Beobachtung: `download_clip` schreibt `local_file_path` und `downloaded_at` in
  `twitch_clips_social_media`; Datei liegt unter `clips_dir` als `<clip_db_id>.mp4`
  (`upload_worker.rs` download_clip-Block, `clips_dir = data/clips`, Kommentar
  `main.rs` "clips_dir = Python-Default data/clips").
- Zustand: fertig, aber ohne Layout-Compositing (siehe KRITISCH oben).

### uploaders/youtube.rs (1678), tiktok.rs (787), instagram.rs (1040), mod.rs (255)
- Beobachtung: echte Plattform-Uploader mit `authenticate/validate_video/upload_video/
  get_video_status`, resumable Uploads, Caption-Bau je Plattform (`build_caption` in
  `tiktok.rs:103`, `instagram.rs:125`; YouTube `#Shorts`-Beschreibung `youtube.rs:804`).
  TikTok wartet auf `PUBLISH_COMPLETE` gegen Doppel-Post (`upload_worker.rs` warte_auf_tiktok).
- Zustand: fertig implementiert (Upload selbst), unabhaengig von der Aufbereitung.

### approval.rs (1711) / approval_worker.rs (273) - Freigabe
- Beobachtung: Freigabe-Logik und Nachreih-Worker; Clip muss freigegeben sein, bevor
  der Upload-Worker ihn nimmt (`upload_worker.rs:142-158` -> `approval_required`).
- Zustand: fertig.

### Weitere: retention(_worker), insights_worker, refresh_worker, report_writer/_dispatcher,
### credentials, oauth, clip_manager, clip_queue, clip_analytics, analytics, settings, vod_archive
- Beobachtung: Retention setzt `retention_until = NOW()+14 Tage` bereits im Tabellen-Default
  (`clip_manager.rs:342`), Cleanup ueber `retention_worker`. clip_manager legt manuelle
  Uploads an (`register_manual_upload`, Quelle `manual_upload`) und liefert Dashboard-Listen.
  Kein Rendering in diesen Modulen.

### STT / ops/stt-server Port 8791
- Beobachtung: Das Crate nutzt den STT-Server (Port 8791) NICHT. Grep ueber das ganze
  Crate auf `8791|stt-server|Whisper|transcribe|faster-whisper` findet nur das
  deaktivierte `Transcriber`-Trait und Doku-Kommentare, keinen HTTP-Call nach 8791.
  Der einzige echte Transcriber im Repo ist `OpenAiTranscriber` im Outreach-Shadow-Pfad
  (`rust/bin/tb-bot/src/outreach_shadow_wiring.rs:222`), ein anderes Feature.
- Urteil: Untertitel-Erzeugung aus Sprache ist derzeit nirgends angebunden.

---

## 2. Doku-Abgleich - falsche Versprechen

`docs/funktionsweise/social-media-uploads.md`:
- Falsch: "Transkribieren: Er hoert den Ton des Clips ab und wandelt ihn in Text um...
  damit Untertitel und Beschreibungen stimmen" (`social-media-uploads.md:10`). Im Code
  ist die Transkription deaktiviert (`transcriber = None`, `enrich_pipeline.rs`
  Transcriber-Doku). Es wird kein Ton abgehoert.
- Falsch: "Plattformgerecht rendern: ... und kann Untertitel einblenden"
  (`social-media-uploads.md:13`). Kein `drawtext`/`subtitles`-Filter im Crate; Untertitel
  werden nie eingebrannt.
- Teilrichtig: "bringt das Video ins Hochkant-Format" (`social-media-uploads.md:5`) - ja,
  aber nur Center-Crop, ohne das im selben Dokument implizierte Layout.

`docs/architecture/social-media.md`:
- Falsch/veraltet: "Anreichern: `enrichment` baut ueber den `llm/dispatcher`
  (waehlt Claude-Haiku/MiniMax/Ollama)..." (`social-media.md:~52`, Punkt 3). Diese Bahnen
  sind entfernt; es laeuft ausschliesslich tb_llm/Fireworks (`llm_dispatch.rs:1-14`).
- Falsch/veraltet: "Transkribieren: `transcription/whisper` erzeugt Text..."
  (`social-media.md:~51`, Punkt 2) und "Extern: ... Whisper" (`social-media.md`
  Abschnitt 6). Transkription ist aus.
- Optimistisch: "Rendern: ... ggf. Layout/Untertitel aus `layout/`" (`social-media.md`
  Punkt 5). Layout-Compositing existiert, wird im Upload aber nicht aufgerufen; Untertitel
  gar nicht.

---

## 3. Dashboard - Routen, UI, Admin vs. Streamer

### Routen (`rust/crates/tb-dashboard-api/src/lib.rs`)
- Oeffentlich: `/social-media/terms`, `/social-media/privacy` (`lib.rs:142-143`).
- App: `/social-media` index (`lib.rs:190`), `GET /social-media/api/stats`,
  `.../api/clips`, `.../api/last-hashtags`, `.../api/access` (+PUT),
  `.../api/access/me`, `.../api/analytics`, `.../api/upload`, `.../api/fetch-clips`,
  `.../api/clips/upload`, `.../api/templates/{global,streamer,apply}`
  (`lib.rs:192-247`).
- Admin-Prefix `/social-media/api/admin/...`: `streamer-layout` (GET/PUT),
  `clips/:id/layout` (PUT), `vocab` (+seed, +:term), `clips`, `clips/:id`,
  `clips/:id/discard`, `approval/:id`, Enrichment/Analytics/Reports
  (`lib.rs:252-293`; Frontend `api/socialMedia.ts:23` `ADMIN_PREFIX`).

### Handler-Auth (`rust/crates/tb-dashboard-api/src/handlers/social_media.rs`)
- Beobachtung: `require_sm_access` scoped auf `?streamer=`; Partner sehen nur den eigenen
  Kanal, `__global__` erreicht nur Admin (`social_media.rs:191-206, 244-290`).
  `check_partner_access_guard` prueft je Clip die Freigabe des Streamers
  (`social_media.rs:337-353`). Serverseitige Admin-/Scope-Pruefung, kein reines
  Frontend-Flag.

### Frontend
- Beobachtung: `SocialMediaAdmin.tsx` ist "Admin-only", gemountet unter
  `/social-media-admin`, umschliesst die eigentliche `SocialMedia`-Seite und verwaltet
  die Partner-Freigabeliste (`SocialMediaAdmin.tsx:20-56`, isAdminView aus
  `authStatus.isAdmin || isLocalhost`).
- Beobachtung: `SocialMedia.tsx` ist die Arbeitsflaeche: Clip-Liste mit Statusfilter
  (Default `pending`), Layout-Editor (`LayoutEditor`, Vorschauclips per Thumbnail),
  Enrichment-Panel, Freigabe (`decideClipApproval`), manueller Upload (`uploadClip`),
  Streamer-Layout speichern (`SocialMedia.tsx:34-93, 166-222`).
- Beobachtung: Die "Vorschau" im Layout-Editor arbeitet mit Clip-Thumbnails
  (`clip.thumbnail_url`), nicht mit einem gerenderten Video (`SocialMedia.tsx:212-222`).
- Folgerung (Vermutung): Der Nutzer kann das Layout am Standbild einstellen, sieht aber
  kein fertig gerendertes Hochformat-Video, und das eingestellte Layout wirkt beim Upload
  ohnehin nicht (Center-Crop, siehe 1).

---

## 4. Clip-Herkunft, Dateien, Aufraeumen

- Beobachtung: Clips kommen aus Twitch. `build_clip_fetch_task` wird in tb-bot gespawnt
  (`rust/bin/tb-bot/src/main.rs:1819`), Tabelle `twitch_clips_social_media` mit
  `source_kind DEFAULT 'twitch'` (`clip_manager.rs:342`). Manuelle Uploads gehen ueber
  `register_manual_upload` (Quelle `manual_upload`, `clip_manager.rs:25-56`).
- Beobachtung: Kein eigener Render aus dem VOD. Der Clip wird als fertige Twitch-Clip-Datei
  per yt-dlp `-f best` heruntergeladen (`upload_worker.rs` download_clip). VOD-Archiv
  (`tb-vod-archive`) ist eine getrennte Strecke (`main.rs:~1690`).
- Beobachtung: Dateien liegen unter `clips_dir` = `data/clips/<clip_db_id>.mp4`, konvertiert
  als `..._<platform>_vertical.mp4` (`upload_worker.rs:120-121`, download_clip). Retention
  `NOW()+14 Tage` per Default, `retention_worker` raeumt auf (`clip_manager.rs:342`).

---

## 5. DB-Zustand (twitch_analytics, SELECT only, 2026-09-06)

Hinweis: Die Tabellen liegen in DB `twitch_analytics`, NICHT in der Default-DB `deadlock`
des DEADLOCK_CENTRAL_DSN (dort `does not exist`). DSN-DB auf `twitch_analytics` umgesetzt.

- `twitch_clips_social_media`: 126 Zeilen, alle `source_kind = twitch`.
  Status: `awaiting_approval` 125, `approved` 1.
  Je Streamer (Top): albiiionlu 35, earlysalty 25, whysolowkey 13, timosius 13,
  deusasta 9, denoshock 9, friduzockt 7, wardendl 5, dehackxas 2, suelze_ 2.
  `discarded_at IS NOT NULL`: 0. `local_file_path IS NOT NULL`: 0.
  uploaded youtube/tiktok/instagram: 0 / 0 / 0.
- `social_media_clip_enrichment`: 0 Zeilen. Transkript vorhanden: 0 / 0.
- `twitch_clips_upload_queue`: 1 Zeile, Status `pending`.
- `social_media_platform_auth`: 2 Konten, beide `youtube`, kein tiktok/instagram.
- `social_media_streamer_layout`: 1 Zeile.

Urteil: Clips werden eingesammelt und bleiben in `awaiting_approval` liegen. Es wurde noch
NIE ein Clip heruntergeladen (0 local_file), NIE angereichert (0 enrichment-Zeilen), NIE
hochgeladen (0 uploads). Die Anreicherung hat trotz gespawntem Worker null Ergebnis.
Vermutung fuer die 0 Enrichment-Zeilen: `external_llm_consent` je Streamer aus (Doku
`social-media-uploads.md:39` beschreibt den Zustimmungsschalter, Default aus), deshalb
ueberspringt die Pipeline und schreibt nichts. Zu verifizieren an `social_media_settings`.

---

## 6. Verdrahtung - Modul tot oder scharf?

- Beobachtung: Alle Worker werden in `rust/bin/tb-bot/src/main.rs` bedingungslos gespawnt
  (nicht hinter `if services.api`): `social_retention_worker`, `social_approval_worker`,
  `social_report_dispatcher`, `social_enrichment_worker` (cipher-frei), und cipher-gated
  `social_upload_worker`, `social_token_refresh_worker`, `social_insights_worker`,
  `vod_archive_worker` (`main.rs:1590-1700`). An/Aus datengetrieben ueber
  `social_media_settings` (Consent + Auto-Approve).
- Beobachtung: Upload/Refresh/Insights/VOD brauchen den Field-Cipher (DB_MASTER_KEY_V1);
  fehlt der, laufen nur die cipher-freien Worker (`main.rs:~1635` "kein Field-Cipher...
  Worker aus").
- Beobachtung: `enrichment_worker.rs:80-81` traegt noch den veralteten Kommentar
  "Noch nicht in tb-bot gespawnt (Wiring = Cutover-Slice)", obwohl `main.rs:1625` ihn
  spawnt. Stale Kommentar.
- Urteil: Der Code ist verdrahtet, aber der Betrieb ist kalt (0 Downloads/Enrichment/
  Uploads bei 126 Clips). Praktisch produziert die Aufbereitung heute nichts; das ist
  ein Verdrahtungs-/Gating-Befund, kein reiner Code-Mangel.

---

## Wiederverwendbar

- Hochformat-9:16-Crop end-to-end: `video_processor.rs` (`convert_and_trim`,
  `build_crop_filter`), im Upload-Worker verdrahtet.
- Facecam-Compositing PiP/Stacked ist FERTIG gebaut und getestet
  (`video_processor.rs:47-93,178`, `layout.rs`) - fehlt nur der Aufruf im Upload-Pfad.
- Titel/Beschreibung/Hashtags je Plattform ueber den zentralen tb-llm-Hub, mit Ledger und
  Qualitaets-Weiche (`llm_dispatch.rs`, `title_gate.rs`) - regelkonform.
- Vollstaendige Plattform-Uploader (YouTube/TikTok/Instagram) inkl. resumable Upload und
  Status-Warten (`uploaders/*`).
- Freigabe-, Retention-, Insights-, OAuth-/Token-Refresh-Strecke inkl. Dashboard-UI und
  serverseitiger Scope-/Admin-Auth (`approval.rs`, `retention.rs`, `oauth.rs`,
  `handlers/social_media.rs`).
- Layout-Editor mit Vorschau (Standbild) und Streamer-Layout-Persistenz
  (`SocialMedia.tsx`, `social_media_streamer_layout`).

## Fehlt

- Facecam-Layout wirkt nicht: `compose_vertical` hat keinen Aufrufer, der Upload nutzt
  hart Center-Crop (`upload_worker.rs:563-582`, `video_processor.rs:272`). Der im
  Dashboard eingestellte Zuschnitt/Facecam-PiP landet nie im hochgeladenen Video.
- Untertitel: keine STT-Anbindung (Port 8791 ungenutzt), kein `subtitles`/`drawtext`-
  Einbrennen. Doku verspricht Untertitel (`social-media-uploads.md:10,13`), Code liefert
  keine.
- Transkription/Sprachinhalt: deaktiviert (`enrich_pipeline.rs` Transcriber = None).
  Titel/Hashtags entstehen nur aus Metadaten, nicht aus dem Gesagten; die
  Vokabel-Korrektur (`correction.rs`) laeuft dadurch leer.
- Blur-Hintergrund / andere Layout-Varianten (z. B. verschwommener Rand statt Crop):
  nicht vorhanden (kein `boxblur`).
- Kein echter Video-Vorschau-Render im Dashboard; Vorschau nur am Thumbnail
  (`SocialMedia.tsx:212-222`).
- Betrieb kalt: 0 Downloads, 0 Enrichment-Zeilen, 0 Uploads bei 126 Clips. Ursache
  vermutlich Consent-/Cipher-Gating (`social_media_settings`, DB_MASTER_KEY_V1) - zu
  verifizieren. Nur youtube-Konten verbunden, kein tiktok/instagram.
- Doku ist an mehreren Stellen falsch (Whisper/Untertitel/Claude-MiniMax-Ollama), gehoert
  gegen den Code korrigiert (`social-media-uploads.md:10,13`; `social-media.md` Punkte 2,3).
- Stale Kommentar `enrichment_worker.rs:80-81` (behauptet "noch nicht gespawnt").
