# Social-Media-Pipeline

## Zielbild

Die Social-Media-Strecke ist keine einzelne Route, sondern eine Worker-Kette:

1. Clip-Fetch
2. Enrichment
3. Approval
4. Upload
5. Retention
6. Analytics/Reports

Das Ganze lebt unter `bot/social_media/` und wird ueber mehrere Cogs im Hintergrund gefahren.

## 1. Clip-Fetch

`ClipFetcher` laeuft standardmaessig alle `6 Stunden`. Er liest alle aktiven Partner-Streamer und zieht pro Streamer bis zu `20` Clips aus den letzten `7` Tagen.

Wichtige Tabellen:

- `twitch_clips_social_media`
- `clip_fetch_history`

Neue Clips werden mit `status='pending'` registriert. Zusaetzlich wird bereits ein Default-Layout angewendet und ein `retention_until` gesetzt.

## 2. Enrichment

`SocialMediaEnrichmentWorker` laeuft alle `90 Sekunden` mit Batch-Groesse `3`. Die Pipeline arbeitet clipweise und hat eine eigene Status-Maschine in `social_media_clip_enrichment`:

- `pending`
- `transcribing`
- `correcting`
- `llm`
- `done`
- `failed`
- `skipped_no_key`

Gespeichert werden unter anderem:

- Rohtranskript
- korrigiertes Transkript
- Segmentdaten
- erkannte Deadlock-Terme
- Titel/Beschreibung/Hashtags je Plattform
- LLM-Provider, Modell und Kostenschaetzung

Wichtige Tabellen:

- `social_media_clip_enrichment`
- `deadlock_vocab`
- `social_media_settings`

Sobald ein Clip fertig angereichert ist, wird er fuer Approval vorbereitet.

## 3. Approval

`SocialMediaApprovalWorker` laeuft jede `60 Sekunden`. Er macht zwei Dinge:

- DMs fuer Clips versenden, die Freigabe brauchen
- bereits freigegebene Clips in die Upload-Queue ueberfuehren

Approval-State lebt in `social_media_clip_approval`. Gueltige States:

- `awaiting_approval`
- `approved`
- `skipped`
- `editing`

Gespeichert werden auch:

- `approved_platforms`
- `approver_user_id`
- `decided_at`
- `dm_message_id`
- `dm_channel_id`
- `last_sent_at`

Die Plattformfreigabe ist explizit pro Clip und pro Zielnetzwerk. Ein Clip kann also fuer YouTube freigegeben sein, aber fuer TikTok nicht.

Auto-Approve-Flags liegen als Key/Value-Settings in `social_media_settings`:

- `auto_approve_youtube`
- `auto_approve_tiktok`
- `auto_approve_instagram`

## 4. Upload

`UploadWorker` laeuft ebenfalls alle `60 Sekunden`, mit `max_parallel=2`. Quelle ist `twitch_clips_upload_queue`.

Ein Queue-Item repraesentiert effektiv `clip x platform`. Der Worker:

1. prueft Approval
2. markiert die Queue-Zeile als `processing`
3. laedt den Clip herunter, falls lokal nichts vorliegt
4. rendert/konvertiert das Video vertikal
5. laedt auf TikTok, YouTube oder Instagram hoch
6. schreibt Queue- und Clip-Status zurueck

Wichtige Tabellen:

- `twitch_clips_upload_queue`
- `social_media_platform_auth`
- `social_media_streamer_layout`
- `twitch_clips_social_media`

Auf Clip-Ebene tauchen spaeter im UI typischerweise diese Status auf:

- `pending`
- `awaiting_approval`
- `approved`
- `publishing`
- `published_partial`
- `published_all`
- `discarded`
- `failed`

## 5. Retention

`SocialMediaRetentionWorker` laeuft alle `30 Minuten`. Die Retention ist absichtlich einfach:

- Standardfrist: `14 Tage ab created_at`
- geloescht wird nur, wenn der Clip entweder auf allen aktiven Plattformen veroeffentlicht oder bewusst verworfen wurde

Der Worker entfernt zuerst lokale Dateien und danach den DB-Eintrag. Relevante Felder in `twitch_clips_social_media`:

- `retention_until`
- `discarded_at`
- `upload_local_path`
- `local_file_path`
- `uploaded_tiktok`
- `uploaded_youtube`
- `uploaded_instagram`

## 6. Analytics und Reports

Nach erfolgreichem Upload uebernimmt `SocialMediaInsightsWorker`. Er pollt Plattformmetriken in den Buckets `24h`, `7d` und `30d`.

Tabelle:

- `twitch_clips_social_analytics`

Reports werden separat in `social_media_reports` geschrieben. Report-Arten:

- `streamer`
- `cross`
- `admin`

## Wichtige operative Konsequenzen

- Die Pipeline ist asynchron; "Clip registriert" heisst nicht "Clip schon gepostet".
- Approval ist der haerteste Gatekeeper vor dem Upload.
- Analytics bauen auf erfolgreich veroeffentlichten Plattform-IDs auf; ohne Video-ID kein Polling.
- Retention ist keine Archivierungsfunktion, sondern Cleanup fuer Produktionsmaterial.
