# Analytics Internal

## Scope

Diese Datei meint die Social-Media-Analytics-Cogs und die daran haengende Report-Logik, nicht das grosse Stream-Analytics-Dashboard unter `bot/analytics/`.

Der Kern besteht aus:

- `SocialMediaInsightsWorker`
- Storage-Helfern in `bot/social_media/analytics/__init__.py`
- `SocialMediaReportWriter`

## Welche Metriken werden gespeichert?

Pro `clip x platform x bucket` speichert das Backend:

- `views`
- `likes`
- `comments`
- `shares`
- `watch_time_seconds`
- `ctr_percent`
- `engagement_rate`
- `provider`
- `synced_at`
- `next_pull_at`

Die Buckets sind fest:

- `24h`
- `7d`
- `30d`

Damit kann derselbe Clip mehrfach ueber die Zeit nachgezogen werden, statt nur einen einmaligen Snapshot zu besitzen.

## Speicherort

Die Rohsnapshots liegen in `twitch_clips_social_analytics`. Der Upsert ist schluesselartig auf:

- `clip_id`
- `platform`
- `bucket`

optimiert. Eine vorhandene Zeile wird ueberschrieben, ansonsten neu angelegt.

`next_pull_at` steuert, wann derselbe Clip erneut abgefragt werden darf. Erfolgreiche Pulls schieben den naechsten Poll je nach Bucket weiter nach hinten; Fehler setzen nur einen kuerzeren Retry.

## Polling-Logik

Der `SocialMediaInsightsWorker` laeuft alle `30 Minuten` und sammelt Targets aus bereits hochgeladenen Clips. Voraussetzung ist:

- der Clip ist nicht verworfen
- die Plattform wurde erfolgreich markiert (`uploaded_* = 1`)
- eine Plattform-Video-ID ist vorhanden

Danach wird je Plattform-Client die passende Analytics-API abgefragt. Fehlende Credentials oder API-Fehler fuehren nicht zum Abbruch des gesamten Laufs, sondern zu einem Retry-Zeitpunkt pro Target.

## Reports

Auf den Snapshots bauen Reports in `social_media_reports` auf. Gespeichert werden:

- `kind`
- `streamer_login`
- `period_start`
- `period_end`
- `content_md`
- `model`
- `created_at`

Aktuelle Report-Typen:

- `streamer`: Wochenreport pro Streamer
- `cross`: plattformuebergreifender Monatsreport
- `admin`: operativer Wochenreport fuer Admins

Der Writer nutzt LLM-Rendering, faellt aber auf datengetriebene Fallback-Texte zurueck, wenn kein Modell verfuegbar ist oder keine Daten vorliegen.

## Operative Interpretation

- Ohne erfolgreiches Upload-Mapping gibt es keine Analytics.
- Buckets sind absichtlich zeitversetzt; "keine 24h-Daten" direkt nach Upload ist normal.
- Reports sind abgeleitete Produkte, nicht die Quelle der Wahrheit. Bei Unstimmigkeiten immer zuerst `twitch_clips_social_analytics` pruefen.
