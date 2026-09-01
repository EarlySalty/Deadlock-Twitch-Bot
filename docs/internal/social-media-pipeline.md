# Social-Media-Pipeline

## Ziel und Sicherheitsgrenze

Die produktive Pipeline lebt in Rust unter `rust/crates/tb-social-media/` und
wird vom `tb-bot` gestartet. Sie trennt zwei Abschnitte bewusst voneinander:

1. **Aufbereiten:** Clips einsammeln, lokal materialisieren, layouten, als
   9:16-MP4 rendern, im Dashboard prüfen und Texte vorbereiten.
2. **Veröffentlichen:** nach Freigabe und Zeitplan über ein verbundenes
   Plattformkonto hochladen.

`social_media_streamer_settings.release_mode` ist die kanalweite harte Grenze:

- `prepare_only` ist der ausfallsichere Standard. Die komplette Vorbereitung
  läuft, aber kein Provider-Aufruf darf stattfinden.
- `live` erlaubt dem Upload-Worker den Provider-Aufruf, sofern außerdem
  Clip-Freigabe, Zielplattform, Termin und Zugang gültig sind.
- Unbekannte oder fehlende Werte werden wie `prepare_only` behandelt.

Der Upload-Worker prüft die Grenze beim Claim und erneut unmittelbar vor dem
Provider-Aufruf. Die Migration schaltet bestehende Kanäle nicht automatisch
live.

Unabhängig davon hat jede verwendete Auth-Zeile die zweite Schranke
`provider_calls_enabled`. Sie ist standardmäßig `false`, wird durch OAuth weder
beim ersten Verbinden noch beim erneuten Verbinden gesetzt und wird beim Claim
und unmittelbar vor dem Netzaufruf erneut geprüft. Erst beide Schranken zusammen
können einen Provideraufruf ermöglichen.

## Datenfluss

```text
Twitch Helix / manueller Upload
            |
            v
    Clip registrieren
            |
            +--------> Quelle materialisieren
            |                   |
            |                   v
            |          Layout + 60-s-Render
            |                   |
            |                   v
            |          echte Vorschau/Download
            |
            +--------> optionale Textanreicherung
                                |
                                v
                    Freigabe + Plattformwahl
                                |
                                v
                       Zeitplan / Queue
                                |
                       release_mode = live?
                         |              |
                       nein             ja
                         |              |
                    bleibt liegen   Providerweg freigegeben?
                                      |              |
                                    nein             ja
                                      |              |
                                 bleibt liegen   Provider-Upload
                                                        |
                                                        v
                                               Analytics / Retention
```

## 1. Eingang

`ClipFetchTask` startet mit verfügbarem Helix-Client nach 60 Sekunden und läuft
danach alle sechs Stunden. Pro aktivem Partner werden höchstens 20 Twitch-Clips
aus einem 14-Tage-Fenster gelesen. Bereits bekannte Twitch-Clip-IDs werden nicht
doppelt angelegt.

Manuelle Uploads nutzen denselben Clip-Datensatz, aber `source_kind =
'manual_upload'` und einen kontrolliert gespeicherten lokalen Quellpfad.

Wichtige Tabellen:

- `twitch_clips_social_media`
- `clip_fetch_history`
- `social_media_clip_preparation`

Ein Trigger legt für jeden neuen Clip einen Vorbereitungsauftrag an. Bei der
Migration werden vorhandene Clips ebenfalls als offene Aufträge erfasst.

## 2. Plattformfreie Aufbereitung

`ClipPreparationWorker` verarbeitet alle 30 Sekunden bis zu zwei Clips. Der
Schreibpfad ist unabhängig von TikTok, Instagram und YouTube:

1. vorhandene manuelle oder lokale Quelle wiederverwenden;
2. Twitch-Clips andernfalls über den isolierten Downloader-Helfer mit dem
   release-lokal gebündelten `yt-dlp` herunterladen;
3. Quelldatei mit SHA-256 fingerprinten;
4. wirksames Streamer-/Clip-Layout laden;
5. höchstens 60 Sekunden als 1080×1920 MP4 rendern;
6. Render-Fingerprint, Pfad und Zustand speichern.

Erlaubte Zustände:

- `pending`
- `materializing`
- `source_ready`
- `rendering`
- `preview_ready`
- `failed`

Quelle, Layout und Renderer-Version gehen in den Fingerprint ein. Ein
unverändertes fertiges Artefakt wird wiederverwendet; nach einer Layoutänderung
entsteht ein neues Artefakt. Erst nach atomarem Umbenennen wird es als bereit
markiert. Der spätere Upload verwendet genau dieses geprüfte MP4.

Für Twitch-Downloads sind ausschließlich HTTPS-Adressen auf den Twitch-Hosts
zugelassen. `yt-dlp` wird als exakt versioniertes Release-Artefakt installiert;
weder der Bot noch der Helfer raten über `PATH` oder Benutzerverzeichnisse einen
Binary-Pfad. Der Bot öffnet die neue Quelldatei selbst und übergibt dem
socketaktivierten Helfer nur diesen gehaltenen Dateideskriptor. Der eigene
Helfer-Nutzer hat keinen Zugriff auf Bot-Secrets, Datenbank oder Medienpfade.
Bubblewrap begrenzt Prozess und Dateisicht, nftables erlaubt diesem Nutzer nur
festes DNS sowie öffentliche HTTP-/HTTPS-Ziele und sperrt Loopback, private,
link-lokale und reservierte Netze. Anschließend prüft der Bot denselben offenen
Inode mit einem netzlosen, begrenzten `ffprobe`, bevor die Quelle verwendbar ist.

Dashboard-API:

- `GET /social-media/api/admin/clips/:id/preparation`
- `POST /social-media/api/admin/clips/:id/preparation`
- die in der Statusantwort gelieferten Vorschau-/Download-Adressen

Der Medienpfad liefert ausschließlich den in der Vorbereitungszeile
referenzierten Render aus dem erlaubten Render-Verzeichnis und nutzt dieselbe
sessiongebundene Kanalberechtigung wie die übrigen Social-Media-Routen.

## 3. Textanreicherung

`EnrichmentWorker` läuft alle 90 Sekunden mit Batchgröße 3. Er ist nur für
Kategorien aktiv, bei denen `enrichment_enabled` gesetzt ist. Die externe
Texterzeugung ist durch den gespeicherten Consent gegatet und läuft zentral über
`tb-llm` mit dem freigegebenen Deepseek-V4-Flash-Pfad.

Der aktuelle produktive Rust-Pfad hat **keine Transkription und keine
Untertitel-Erzeugung**. Der Transcriber ist nicht verdrahtet. Titel,
Beschreibungen und Hashtags können deshalb aus vorhandenen Clip-Metadaten
entstehen oder im Dashboard manuell gepflegt werden; sie sind nicht als
vollständige Inhaltsanalyse zu bewerben.

Wichtige Tabellen:

- `social_media_clip_enrichment`
- `deadlock_vocab`
- `social_media_settings`

## 4. Prüfung, Freigabe und Zeitplan

Die Prüfung findet im Social-Media-Dashboard statt. Es gibt derzeit keinen
produktiven Discord-DM-Freigabepfad.

Eine manuelle Freigabe ist nur möglich, wenn:

- die echte Render-Vorschau bereit ist,
- mindestens eine Zielplattform gewählt wurde,
- der Veröffentlichungsmodus erfolgreich geladen wurde.

Die Freigabe speichert Plattformen clipweise in
`social_media_clip_approval`. Der `ApprovalWorker` reiht freigegebene Clips alle
60 Sekunden in `twitch_clips_upload_queue` ein. Ein Queue-Eintrag entspricht
`Clip × Plattform`.

Zusätzlich speichert die Freigabe den Fingerprint genau der geprüften
Render-Vorschau. Layoutänderung, neue Aufbereitung oder eine Bearbeitung setzen
die Freigabe zurück; ein alter Queue-Eintrag darf danach nicht zum Provider.
Bei `veto_window` und `full_auto` versucht der Worker die Freigabe erneut,
sobald sowohl Aufbereitung als auch Metadaten fertig sind. Damit ist die
Reihenfolge der beiden Worker unerheblich und es entsteht trotzdem nur eine
Queue-Zeile je Clip und Plattform.

TikTok ist davon ausdrücklich ausgenommen: Die allgemeinen Automatikmodi dürfen
keine TikTok-Freigabe und keinen TikTok-Queue-Claim erzeugen. Direct Post braucht
vorher persistierte, explizite Einstellungen und Zustimmung für genau diesen
Clip; diese Oberfläche ist noch nicht gebaut.

Der Termin wird aus der Streamer-Zeitzone und der Plattformkadenz berechnet.
`posts_per_week`, `max_posts_per_day` und `post_times` begrenzen den Plan. Eine
Plattform mit null Kadenz wird nicht eingeplant.

`approval_mode` steuert, wie die fachliche Clip-Freigabe entsteht (`manual`,
`veto_window`, `full_auto`). Das hebt `release_mode` nicht auf: Selbst ein
vollautomatisch freigegebener Clip bleibt im Testbetrieb vor dem Provider
stehen.

## 5. Upload

`UploadWorker` läuft jede Minute mit höchstens zwei parallelen Jobs. Pending-Jobs
werden atomar per `FOR UPDATE ... SKIP LOCKED` geclaimt, sodass parallele
Worker-Durchläufe denselben Eintrag nicht doppelt posten.

Vor einem Provider-Aufruf müssen alle Gates grün sein:

1. Clip existiert und wurde nicht verworfen;
2. Kanal steht auf `release_mode = live`;
3. Clip ist für diese Plattform freigegeben;
4. Termin ist erreicht;
5. Plattformzugang ist vorhanden;
6. der konkrete Providerweg ist über `provider_calls_enabled` freigeschaltet;
7. das vorbereitete Render ist bereit;
8. bei TikTok liegt die vorgeschriebene Zustimmung samt Einstellungen für genau
   diesen Clip vor.

Fehlende Zugänge, ein noch laufendes Render und andere Fehler vor dem
Provideraufruf werden mit stabilem Fehlercode und neuem Termin zurückgestellt.
Lokale Videoprüfung und Vorbereitung passieren vor der Provider-Lease.

Unmittelbar vor dem externen Aufruf wird unter Streamer-Lock erneut geprüft,
dass Release-Modus, Zielplattform, Freigabe und der freigegebene
Render-Fingerprint zusammenpassen und der Providerweg noch freigeschaltet ist.
Danach wird eine eindeutige Provider-Lease
gespeichert und die Datenbanktransaktion vor dem Netzaufruf beendet. Erfolg und
Fehler dürfen nur mit genau diesem Lease-Token abgeschlossen werden.

Ist der Provideraufruf bereits gestartet und sein Ausgang wegen Timeout,
Prozessabbruch oder unvollständiger Providerantwort nicht sicher, wird der Job
`reconciliation_required`. Er wird nicht automatisch erneut gesendet. Eine
vom Provider erhaltene externe ID wird sofort gespeichert. Dashboard und API
zeigen den unklaren Zustand, bis jemand das Ergebnis auf der Zielplattform
prüft und die Verwaltung den Queue-Eintrag ausdrücklich als abgeglichen
bestätigt. Diese Aktion akzeptiert nur bereits gestartete Zeilen im Zustand
`reconciliation_required`; normale wartende oder laufende Jobs kann sie nicht
als Erfolg abkürzen.

Provider:

- **TikTok:** Direct Post API technisch angebunden, aktuell aber hart gesperrt.
  Vor App-Audit und einer vorgeschriebenen Auswahl-/Zustimmungsoberfläche pro
  Clip gibt es keinen Provideraufruf. TikTok-Analytics sind noch nicht
  angebunden.
- **Instagram:** Instagram Login, resumable Upload, Container-Statusprüfung vor
  `media_publish`, Verlängerung des Langzeittokens.
- **YouTube:** resumable Upload mit Wiederaufnahme und privater
  Standardsichtbarkeit. Analytics sind auf die freigegebenen Data-API-Metriken
  begrenzt.

Die technische Adapterstrecke ist vorhanden. Ein echter Plattform-Release gilt
erst nach Freischaltung/Audit des jeweiligen Kontos und einem bewusst
freigegebenen Sandbox-/Testpost als validiert.

## 6. Verwerfen und Retention

Beim Verwerfen werden Clip, Freigabe und noch nicht gestartete Queue-Zeilen
gemeinsam stillgelegt. Bereits laufende Provider-Aufrufe werden in der Antwort
ehrlich als `already_running` gemeldet; die Oberfläche darf in diesem Fall
nicht behaupten, dass garantiert nichts mehr veröffentlicht wird.

Beim Wechsel von `prepare_only` zu `live` werden überfällige, noch nicht
gestartete Queue-Zeilen neu auf zukünftige Kadenzplätze verteilt. Ein alter
Vorbereitungsvorrat darf deshalb nicht als Upload-Spitze sofort anlaufen. Beim
Zurückschalten wird `prepare_only` zuerst dauerhaft gespeichert; bereits
begonnene oder unklare Providerjobs werden anschließend als Warnung gemeldet.

`RetentionWorker` läuft alle 30 Minuten. Nach der 14-Tage-Frist löscht er nur,
wenn ein Clip bewusst verworfen oder auf allen tatsächlich aktiven Plattformen
veröffentlicht wurde. Ohne aktive Plattform bleibt ein vorbereiteter Vorrat
erhalten. Abgeleitete Render werden ausschließlich im erlaubten
`data/clips/rendered`-Verzeichnis entfernt.

## 7. Analytics und Reports

`SocialMediaInsightsWorker` zieht nach erfolgreichen Uploads unterstützte
Provider-Metriken in 24h-, 7d- und 30d-Buckets nach. Fehlende oder nicht
freigeschaltete API-Funktionen bleiben leer; sie werden nicht als gemessene Null
ausgegeben. Reports liegen in `social_media_reports`.

## Testreihenfolge vor dem ersten echten Release

1. Pipeline im `prepare_only`-Modus mit lokalen Fixtures und einem Twitch-Clip
   durchlaufen lassen.
2. Quelle, 9:16-Render, Layoutänderung, erneutes Rendern, Vorschau und Download
   prüfen.
3. Manuelle Texte, Freigabe, Plattformwahl und Zeitplan prüfen; Queue muss vor
   dem Provider-Gate liegen bleiben.
4. Fehlerfälle prüfen: ungültige Quelle, defektes Video, paralleles Render,
   fehlender Zugang, Verwerfen und Neustart.
5. Den jeweiligen Providerweg erst für den kontrollierten Test separat
   freischalten: YouTube privat, Instagram auf einem isolierten Testkonto;
   TikTok erst nach App-Audit und eigener Auswahl-/Zustimmungsoberfläche.
6. Erst nach erfolgreicher Prüfung `release_mode` für den gewünschten Kanal
   bewusst auf `live` setzen und die doppelte Sperre unmittelbar vor dem
   Provideraufruf erneut nachweisen.

## Roadmap

- echte Transkription und Untertitel mit einem freigegebenen, zentralen Pfad;
- automatische Erkennung guter Fails und lustiger Momente im Highlight-Clipper;
- Plattform-Audits/Freischaltungen und je ein dokumentierter Sandbox-End-to-End-Test;
- TikTok-Auswahl und ausdrückliche Zustimmung pro Clip gemäß Direct-Post-Vorgaben;
- TikTok-Analytics und weitere nur tatsächlich verfügbare Plattformmetriken;
- ausgebauter Redaktionskalender, Filter/Pagination und belastbare Dashboard-KPIs;
- beobachtbare Qualitätsstufen für Schnitt, Untertitel, Hook und Textvarianten;
- kontrollierte A/B-Auswertung, bevor eine Aufbereitungsvariante automatisch
  bevorzugt wird.
