# social_media/ — Architektur & Funktionsreferenz

> Pfad: `bot/social_media/` · Stand: 2026-06-08 · 43 Dateien, ~13.180 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [highlight-clipper.md](highlight-clipper.md) (liefert Clips), [api.md](api.md) (Twitch-Clips), [core.md](core.md) (LLM), [internal/social-media-pipeline.md](../internal/social-media-pipeline.md).

## 1. Zweck & Abgrenzung

`social_media/` ist die **Clip-zu-Social-Media-Pipeline**: Twitch-Clips holen → transkribieren → per LLM anreichern (Titel/Hashtags/Beschreibung) → zur **Freigabe** vorlegen → plattformgerecht rendern → auf **TikTok/Instagram/YouTube** hochladen → Analytics einsammeln → Reports erzeugen. Dazu OAuth-/Credential-Verwaltung je Plattform und eine Admin-Dashboard-Oberfläche.

Abgrenzung: Die *Erkennung* einzelner Highlight-Momente macht [highlight-clipper.md](highlight-clipper.md); `social_media/` ist die Verteil-/Veröffentlichungs-Maschine drumherum (Worker-basiert).

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | `TwitchStreamCog` (Worker-Start), Admin-Dashboard (Social-Media-Sektion). |
| **Nutzt** | `api/` (Twitch-Clips), LLM (`llm/` → Claude-Haiku/MiniMax/Ollama), Whisper (Transkription), Plattform-APIs (TikTok/Instagram/YouTube), `ffmpeg` (Video-Processing), `storage/`. |
| **DB-Tabellen** | Social-Media-Plattform-Auth, Clip-/Upload-/Approval-/Analytics-Tabellen (`social_media_*`, siehe Migrations-Phasen). |
| **Externe Dienste** | Twitch, TikTok-Content-Posting-API, Instagram-Graph-API, YouTube-Data-API, LLM-Provider, Whisper. |
| **Secret-Namen** | Plattform-OAuth-Credentials (pro Plattform), LLM-Keys. |

## 3. Dateien im Überblick (nach Pipeline-Stufe)

| Stufe | Dateien (Zeilen) | Rolle |
|-------|------------------|-------|
| **Orchestrierung** | `clip_manager.py` (1349), `clip_fetcher.py` (282) | Clips holen + Lebenszyklus steuern. |
| **Transkription** | `transcription/whisper.py` (253), `vocab.py` (308), `seed_vocab.py` (277), `correction.py` (235) | Audio→Text + Deadlock-Vokabular-Korrektur. |
| **Anreicherung** | `enrichment.py` (673), `enrichment_worker.py` (74), `llm/*` (dispatcher 157, claude_haiku 116, minimax 124, ollama 178, base 87, prompts 70, _parsing 173) | Titel/Hashtags/Beschreibung via LLM. |
| **Freigabe** | `approval/approval_service.py` (739), `approval_worker.py` (92) | Review-Workflow vor Upload. |
| **Rendern** | `uploaders/video_processor.py` (447), `rendering.py` (44), `layout/*` (197+142) | Plattformgerechtes Video + Layout. |
| **Upload** | `upload_worker.py` (406), `uploaders/base.py` (228), `tiktok.py` (339), `instagram.py` (311), `youtube.py` (274) | Hochladen je Plattform. |
| **Auth** | `oauth_manager.py` (646), `credential_manager.py` (224), `token_refresh_worker.py` (446) | Plattform-OAuth + Token-Refresh. |
| **Analytics** | `analytics/report_writer.py` (570), `report_dispatcher.py` (162), `insights_worker.py` (261) | Plattform-Analytics + Reports. |
| **UI/Infra** | `dashboard.py` (1916), `storage.py` (546), `settings.py` (120), `retention.py` (132), `retention_worker.py` (78) | Admin-Dashboard, Persistenz, Retention. |

## 4. Datenfluss / Lebenszyklus

1. **Holen:** `clip_fetcher` zieht neue Twitch-Clips (oder übernimmt Clips aus dem Highlight-Clipper); `clip_manager` legt sie an und treibt den Zustandsautomaten.
2. **Transkribieren:** `transcription/whisper` erzeugt Text; `vocab`/`seed_vocab` halten ein **Deadlock-Vokabular**, `correction` korrigiert Fehlhörungen (Heldennamen, Items) damit Untertitel/Beschreibungen stimmen.
3. **Anreichern:** `enrichment` baut über den `llm/dispatcher` (wählt Claude-Haiku/MiniMax/Ollama) Titel, Hashtags und Beschreibung — gated über `external_llm_consent` (kein externer LLM-Versand ohne Zustimmung).
4. **Freigabe:** `approval_service` legt den Clip zur Review vor; erst nach Freigabe geht es weiter (`approval_worker`).
5. **Rendern:** `uploaders/video_processor` bringt das Video ins Plattformformat (Seitenverhältnis, Länge, ggf. Layout/Untertitel aus `layout/`).
6. **Hochladen:** `upload_worker` ruft den passenden `PlatformUploader` (TikTok/Instagram/YouTube) — `authenticate` → `validate_video` → `upload_video` → `get_video_status`.
7. **Analytics:** `insights_worker` holt periodisch `fetch_video_analytics`; `report_writer`/`report_dispatcher` erzeugen + verschicken Reports.
8. **Retention:** `retention_worker` räumt alte Clips/Daten nach Policy auf.

Auth läuft quer dazu: `oauth_manager` führt den Plattform-OAuth-Flow, `credential_manager` speichert die Tokens, `token_refresh_worker` hält sie gültig.

## 5. Funktionsreferenz pro Bereich

### Orchestrierung
- `clip_manager.py` — `ClipManager`: Worker-Loop + Lebenszyklus (`_worker_loop`, `_load_clip_context`, `cog_unload`), Übergänge zwischen den Stufen. `setup()` registriert es.
- `clip_fetcher.py` — holt Twitch-Clips als Pipeline-Input.

### uploaders/
- `base.py` — abstrakte `PlatformUploader`: `authenticate`, `validate_video`, `upload_video`, `get_video_status`, `format_hashtags`, `download_clip`, `get_video_duration`, `get_video_resolution`.
- `tiktok.py` — `TikTokUploader` (Content-Posting-API): `authenticate`, `validate_video`, `upload_video` (`_init_upload` → `_upload_chunks` → `_publish_post`), `get_video_status`, `fetch_video_analytics`.
- `instagram.py` — `InstagramUploader` (Graph-API), `youtube.py` — `YouTubeUploader` (Data-API), je mit demselben Vertrag.
- `video_processor.py` — Re-Encoding/Zuschnitt ins Plattformformat (ffmpeg).

### Auth
- `oauth_manager.py` — `authenticate`/OAuth-Flows je Plattform.
- `credential_manager.py` — Token-/Credential-Persistenz (verschlüsselt).
- `token_refresh_worker.py` — Hintergrund-Worker, der Plattform-Tokens vor Ablauf refresht (`_worker_loop`).

### Anreicherung & LLM
- `enrichment.py` / `enrichment_worker.py` — Titel/Hashtags/Beschreibung erzeugen.
- `llm/dispatcher.py` — wählt den Provider; `generate_text(...)`. Provider: `claude_haiku.py`, `minimax.py`, `ollama.py` (lokal), Basis `base.py`, Prompts `prompts.py`, Antwort-Parsing `_parsing.py`. `external_llm_consent` gated den Versand an externe Anbieter.

### Transkription
- `whisper.py` — Whisper-Transkription. `vocab.py`/`seed_vocab.py` — Deadlock-Vokabular (Helden/Items). `correction.py` — korrigiert Transkripte gegen das Vokabular.

### Freigabe
- `approval/approval_service.py` — Review-Status/Workflow (vorlegen, freigeben, ablehnen). `approval_worker.py` — verarbeitet freigegebene Clips weiter.

### Analytics
- `analytics/insights_worker.py` — `SocialMediaInsightsWorker` (`_worker_loop`, `_process_due_targets`, `_collect_due_targets`, `_resolve_client`, `_schedule_retry`) holt Plattform-Analytics. `report_writer.py` baut Reports, `report_dispatcher.py` verschickt sie.

### UI/Infra
- `dashboard.py` — Admin-Dashboard-Sektion (Clip-Liste, Freigabe, Status, Konto-Verknüpfung). `storage.py` — Persistenz aller Pipeline-Entitäten. `settings.py` — Konfiguration. `retention.py`/`retention_worker.py` — Aufräum-Policy. `layout/` — Video-Layout/Templates. `rendering.py` — Render-Helfer.

## 6. Datenbank & externe Schnittstellen

- **DB:** `social_media_platform_auth` + die Pipeline-Tabellen aus den Migrations-Phasen (Layout/Uploads, Enrichment, Analytics, Approval).
- **Extern:** Twitch (Clips), TikTok/Instagram/YouTube (Upload + Analytics), LLM-Provider, Whisper.

## 7. Stolperfallen / Besonderheiten

- **Worker-Pipeline, kein Request/Response:** Fast alles läuft in Hintergrund-Workern (`*_worker.py`) mit Zustandsautomat — ein „hängender“ Clip steckt meist in einer Stufe fest, nicht in einem fehlgeschlagenen Request.
- **External-LLM-Consent:** Vor dem Versand an externe LLMs greift `external_llm_consent`; ohne Zustimmung bleibt nur der lokale Provider (Ollama). Beim Debuggen prüfen, welcher Provider der `dispatcher` wählt.
- **Transkript-Korrektur ist domänenspezifisch:** Ohne `vocab`/`correction` verschreibt Whisper Deadlock-Begriffe — die Vokabular-Pflege ist Teil der Qualitätskette.
- **Token-Refresh ist eigener Worker:** Plattform-Tokens laufen ab; `token_refresh_worker` muss laufen, sonst scheitern Uploads mit Auth-Fehlern trotz „verbundenem“ Konto.
- **Plattform-Limits im `video_processor`:** Jede Plattform hat eigene Format-/Längen-Regeln — `validate_video` lehnt ungeeignete Clips ab, bevor der Upload startet.
- **Freigabe vor Upload:** Clips werden erst nach `approval_service`-Freigabe veröffentlicht — automatischer Upload ohne Review ist nicht der Default.
