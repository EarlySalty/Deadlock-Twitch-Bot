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

### Freigabe-Modi (pro Streamer)

Der Modus steht in `social_media_streamer_settings.approval_mode`:

- `manual`: jeder Clip braucht eine ausdrueckliche Freigabe (Default)
- `veto_window`: Clip wird eingeplant und geht raus, wenn bis zum Termin niemand widerspricht
- `full_auto`: Clip wird ohne Sichtung eingeplant

Unbekannte Werte fallen auf `manual` zurueck.

Die frueheren Key/Value-Settings `auto_approve_youtube` / `_tiktok` / `_instagram` in
`social_media_settings` sind entfallen (Migration `20260815120000`). Sie galten global
fuer die ganze Instanz, liessen sich von jedem freigegebenen Partner umschalten und
loesten ausserdem nie eine Freigabe aus: sie mischten sich nur additiv in eine manuelle
Entscheidung und ueberschrieben damit still die Auswahl des Nutzers.

Automatisch eingeplant wird nur, wenn beides zusammenkommt: der Modus laesst es zu
**und** die Kategorie des Clips ist eingeschaltet
(`social_media_category_settings.auto_post`). Einstiegspunkt ist
`approval::auto_approve_if_allowed`, aufgerufen am Ende der Enrichment-Pipeline und
im `ApprovalWorker`.

## 3b. Zeitplan

Freigegeben heisst nicht mehr „sofort raus". `approval::ensure_queued_uploads` setzt
beim Einreihen `twitch_clips_upload_queue.scheduled_at` auf den naechsten freien
Termin aus `posting_plan::plan_next_slot`. Der Upload-Worker zieht ohnehin nur
Zeilen, deren `scheduled_at` NULL oder erreicht ist.

Kadenz je Streamer und Plattform in `social_media_platform_schedule`:

| Spalte | Default | Bedeutung |
|---|---|---|
| `auto_post` | `false` | Plattform postet automatisch |
| `posts_per_week` | `4` | Obergrenze im rollierenden Sieben-Tage-Fenster |
| `max_posts_per_day` | `1` | Obergrenze pro lokalem Kalendertag |
| `post_times` | `["18:00"]` | Tageszeiten in der Zeitzone des Kanals |

Die Defaults kommen aus der Kadenz-Recherche: hoechstens ein Post pro Tag und
Plattform, rund drei bis fuenf pro Woche.

Gerechnet wird in `scheduler::next_cadence_slot`, rein funktional ohne IO und ohne
Systemzeit. Steht eine Kadenz auf null, gibt es keinen Termin und die Plattform
zaehlt nicht als aktiv. Der Suchhorizont betraegt 180 Tage.

## 3c. Kategorien

Clips tragen `game_id` (aus Helix) und `category_key`. Der Katalog steht in
`social_media_category`:

- `deadlock`: `enrichment_enabled = true`
- `other`: Fallback, ohne Anreicherung

Zugeordnet wird beim Registrieren des Clips ueber `posting_plan::resolve_category`:
die `twitch_game_id` schlaegt den Abgleich ueber `match_game_names`, ohne Treffer
landet der Clip in `other`.

Das Kategorie-Gate haengt an zwei Stellen:

- `enrichment::iter_pending_enrichments` nimmt nur Kategorien mit `enrichment_enabled`.
- `approval::iter_clips_ohne_enrichment` holt die uebrigen ab und schleust sie in den
  Approval-Workflow. Ohne diesen zweiten Pfad wuerden Clips anderer Spiele nie
  auftauchen, denn `awaiting_approval` setzt sonst erst das Ende der
  Enrichment-Pipeline.

## 3d. Vorratswarnung

`posting_plan::pool_forecast` rechnet aus Pool-Bestand und Kadenz aus, fuer wie viele
Posts der Vorrat reicht. Gezaehlt werden Clips des Kanals, die nicht verworfen und
nicht schon ueberall veroeffentlicht sind und in einer eingeschalteten Kategorie
liegen. Ein Clip ergibt einen Post je aktiver Plattform. Traegt der Vorrat keine
volle Woche mehr, setzt die Rechnung `warnung`, und das Dashboard zeigt eine eigene
betonte Zeile ueber Clip-Pool und Zeitplan.

Clip-Nachschub ist Sache der Streamer; die Plattform warnt nur.

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
- Freigegeben heisst "eingeplant", nicht "gepostet": zwischen Freigabe und Upload
  liegt der Termin aus der Kadenz.
- Zeitplan, Freigabe-Modus und Kategorie-Schalter haengen am Kanal, nicht an der
  Instanz. Das Partner-Scoping aus `social_media_partner_access` gilt unveraendert:
  ein Partner sieht und setzt nur den eigenen Kanal.
- Analytics bauen auf erfolgreich veroeffentlichten Plattform-IDs auf; ohne Video-ID kein Polling.
- Retention ist keine Archivierungsfunktion, sondern Cleanup fuer Produktionsmaterial.
