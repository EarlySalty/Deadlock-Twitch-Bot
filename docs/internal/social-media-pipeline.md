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
- `veto_window`: Clip wird eingeplant und geht raus, wenn bis zum Termin niemand
  widerspricht. Der Widerspruch laeuft ueber
  `POST /social-media/api/approval/:clip_db_id/cancel`: die Route raeumt die noch
  nicht angefassten Queue-Zeilen ab und setzt den Clip zurueck auf
  `awaiting_approval`. Zeilen, die schon in `processing` oder `completed` stehen,
  bleiben unangetastet und werden in der Antwort als `already_running` gemeldet,
  damit die Oberflaeche ehrlich sagen kann, dass eine Plattform schon durch war.
  Der geplante Termin je Plattform steht am Clip in `scheduled_at`, der letzte
  Fehlgrund in `upload_errors`.
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

### Plattform-Anbindungen: was gilt, wo es klemmt

**TikTok.** Direct Post ueber die Content Posting API v2. Ablauf: Creator-Info
abfragen (Pflicht, liefert die erlaubten Sichtbarkeiten und die Kommentar-,
Duett- und Stitch-Sperren des Kanals), dann `/post/publish/video/init/` mit
`post_info` und `source_info`, dann die Chunks per `PUT` gegen die von init
gelieferte `upload_url`. Einen eigenen Publish-Aufruf gibt es nicht, TikTok
startet nach dem letzten Chunk selbst.

Solange die TikTok-App nicht auditiert ist, laesst TikTok nur `SELF_ONLY` zu und
blockt jeden oeffentlichen Post schon beim init mit
`unaudited_client_can_only_post_to_private_accounts`. Der Default im Uploader ist
deshalb `SELF_ONLY`. Nach dem Audit umstellen, nicht vorher.

Der Upload liefert nur eine `publish_id`, also "zur Verarbeitung angenommen".
Der Upload-Worker fragt danach bis zu drei Minuten lang
`/post/publish/status/fetch/` ab: `PUBLISH_COMPLETE` liefert die echte Post-ID,
`FAILED` den Grund. Bleibt die Antwort im Zeitfenster unentschieden, bleibt es
bei der `publish_id` und es wird nichts erneut hochgeladen; ein doppelter Post
waere schlimmer als eine fehlende Bestaetigung.

TikTok-Analytics sind nicht angebunden. `/v2/video/query/` gehoert zur Display
API, braucht den Scope `video.list` und eine echte Video-ID. Der Uploader meldet
deshalb `NotImplemented`, statt Nullen als Messung in die Datenbank zu schreiben.

**Instagram.** Weg ist Instagram Login gegen `graph.instagram.com`, nicht
Facebook Login gegen `graph.facebook.com`. Damit braucht der Streamer keine
Facebook-Seite. Die Scopes heissen entsprechend `instagram_business_basic` und
`instagram_business_content_publish`; die Facebook-Namen `instagram_basic` und
`instagram_content_publish` werden am Instagram-Authorize-Endpunkt mit "Invalid
scope" abgewiesen.

Das Video geht per resumable Upload direkt an `rupload.facebook.com`, es braucht
also keinen oeffentlichen Media-Host. Reels werden dreistufig veroeffentlicht:
Container anlegen, auf `status_code = FINISHED` warten, erst dann
`media_publish`. Wer den Wartepunkt ueberspringt, bekommt bei laengeren Clips
verlaesslich einen 400er.

Der Code-Tausch liefert ein Token mit EINER Stunde Laufzeit. Es wird sofort per
`ig_exchange_token` gegen ein 60-Tage-Token getauscht. Instagram hat keinen
Refresh-Token: das Langzeit-Token verlaengert sich per `ig_refresh_token` selbst,
solange es noch gueltig ist. Der Refresh-Worker holt Instagram deshalb sieben
Tage vor Ablauf, nicht wie die Stundentoken der anderen Plattformen eine Stunde
vorher.

**YouTube.** Uploads laufen ueber das resumable Protokoll mit Wiederaufnahme
nach Abbruch. `snippet.tags` ist ein Zeichenlimit von 500, keine Anzahl. Der
Scope `yt-analytics.readonly` wird bewusst nicht angefragt, damit das
Google-Audit nicht ueber einen Bereich stolpert, der im Demo-Video nicht in
Benutzung zu sehen ist. Folge: `watch_time_seconds` und `ctr_percent` bleiben fuer
YouTube leer, und `shares` gibt es in der Data API gar nicht.

Die Google-Zugangsdaten liest der Code in der Reihenfolge `GOOGLE_OAUTH_ID`,
`GOOGLE_CLIENT_ID`, `YOUTUBE_CLIENT_ID`. Unter dem letzten Namen liegt teils noch
ein alter Client, der sonst den aktuellen ueberstimmt.

### Fehler sind nicht gleich Fehler

Ein fehlgeschlagener Upload landete frueher immer auf `status = failed`, und
`failed` holt die Warteschlange nie wieder ab. Ein einzelnes 502 oder ein volles
Tageskontingent hat den Clip damit endgueltig verbrannt.
`upload_worker::verzoegerung_fuer` trennt das jetzt:

| Fehler | Verhalten |
|---|---|
| `QuotaExceeded` | neuer Termin in 24 Stunden, zaehlt nicht gegen die Versuchsgrenze |
| `Request` (Netz, 5xx) | neuer Termin in 15 Minuten |
| `NotAuthenticated` | neuer Termin in 30 Minuten, der Refresh-Worker laeuft alle 5 Minuten |
| `Validation`, `Api`, `NotImplemented`, `Io` | `failed`, das bleibt beim naechsten Mal kaputt |

Nach fuenf Anlaeufen gilt ein Job als kaputt. Ein erschoepftes Kontingent zaehlt
dabei nicht mit, denn der Clip ist in Ordnung.

### Tote Zugaenge werden sichtbar

Scheitert ein Token-Refresh mit `invalid_grant` (Zugriff entzogen, Token zu lange
ungenutzt, Passwortwechsel), setzt der Refresh-Worker `token_expires_at` auf
jetzt. Das Dashboard zeigt den Zugang dadurch als abgelaufen an, statt weiter
gruen zu melden, waehrend jeder Upload ins Leere laeuft. Der Eintrag bleibt
`enabled = 1`, damit ein erneutes Verbinden dieselbe Zeile aktualisiert.

Der Insights-Worker faellt nicht mehr auf die Sammelverbindung zurueck. Ohne
eigene Plattform-Verbindung wird ein Kanal uebersprungen, statt sein privates
oder ungelistetes Video mit dem Betreiber-Token abzufragen und die leere
Trefferliste als "0 Views" zu speichern. Aus demselben Grund schreibt ein
gescheiterter Abruf nur noch den naechsten Termin, nicht mehr Nullen ueber
bestehende Messwerte.

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
