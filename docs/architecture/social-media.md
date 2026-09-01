# Social-Media-Architektur

> Produktiver Pfad: `rust/crates/tb-social-media/`,
> `rust/crates/tb-dashboard-api/src/handlers/social_media.rs` und
> `bot/dashboard_v2/`. Die alte Python-Implementierung ist nur noch
> Legacy-Referenz.

## Zweck und Grenze

Das Subsystem übernimmt Clips aus Twitch oder einem manuellen Upload, baut eine
prüfbare Hochkant-Version, verwaltet Metadaten, Freigabe und Kadenz und stellt
die Provideradapter für TikTok, Instagram und YouTube bereit. Die Erkennung von
Momenten im VOD gehört zum Highlight-Clipper und ist nicht Teil dieser Pipeline.

Aufbereitung und Veröffentlichung sind technisch getrennt. Das gespeicherte
`release_mode = prepare_only` ist der Standard und verhindert Provider-Aufrufe,
ohne Download, Render, Vorschau oder Planung abzuschalten. Daneben hält
`social_media_platform_auth.provider_calls_enabled = false` jeden Providerweg
einzeln geschlossen; eine OAuth-Verbindung öffnet diese Schranke nicht.

## Komponenten

| Bereich | Produktive Dateien | Aufgabe |
|---|---|---|
| Eingang | `clip/`, `clip_manager.rs` | Twitch-Clips holen, manuelle Uploads registrieren |
| Vorbereitung | `preparation.rs`, `video_processor.rs`, `layout.rs` | Quelle materialisieren, Layout anwenden, 9:16-MP4 bauen |
| Metadaten | `enrich_pipeline.rs`, `enrichment*.rs`, `llm_dispatch.rs` | optionale Textvorschläge und manuelle Bearbeitung |
| Freigabe | `approval.rs`, `approval_worker.rs` | Clip-/Plattformentscheidung und Queue-Aufbau |
| Planung | `posting_plan.rs`, `scheduler.rs` | Zeitzone, Kadenz, Kategorien, Vorratsprognose |
| Veröffentlichung | `clip_queue.rs`, `upload_worker.rs`, `uploaders/` | atomarer Claim, Provider-Upload, Wiederholungen |
| Plattformzugang | `oauth.rs`, `credentials.rs`, `refresh_worker.rs` | OAuth-Zustand, verschlüsselte Tokens, Refresh |
| Nachlauf | `insights_worker.rs`, `report_writer.rs`, `retention*.rs` | unterstützte Metriken, Reports, kontrolliertes Aufräumen |
| API | `tb-dashboard-api/.../social_media.rs` | Scope-/Partner-Gates und JSON-/Medienrouten |
| UI | `bot/dashboard_v2/src/pages/SocialMedia.tsx` | Vorschau, Prüfung, Zeitplan, Konten und Analytics |

## Zustandsfluss

```text
registriert
   |
   v
pending -> materializing -> source_ready -> rendering -> preview_ready
   |              |                              |
   +--------------+---------- failed <-----------+
                                                  |
                                                  v
                                      awaiting_approval
                                                  |
                                      approved / skipped / editing
                                                  |
                                                  v
                                    Queue: pending -> processing
                                                  |
                                     completed / failed
                                                  |
                             reconciliation_required
```

Vorbereitungszustände liegen in `social_media_clip_preparation`, fachliche
Freigaben in `social_media_clip_approval` und Providerjobs in
`twitch_clips_upload_queue`. Dadurch überleben alle Stufen einen Prozessneustart.

## Wichtige Invarianten

- Fehlendes oder unbekanntes `release_mode` bedeutet immer `prepare_only`.
- Der Upload-Worker prüft das Release-Gate vor der Arbeit und direkt vor dem
  Provider-Aufruf.
- Ein Providerweg muss zusätzlich auf der exakt verwendeten Streamer- oder
  globalen Auth-Zeile freigeschaltet sein. OAuth setzt diese Freigabe nie.
- Eine Freigabe gilt nur für den gespeicherten Fingerprint des tatsächlich
  geprüften MP4. Layout- oder Renderänderungen machen sie ungültig.
- Queue-Jobs werden in einer Transaktion mit `SKIP LOCKED` übernommen.
- Ein verworfener Clip kann nicht neu geclaimt werden; bereits laufende
  Provideraufrufe werden ehrlich separat gemeldet.
- Nach Beginn eines Provideraufrufs wird ein unklarer Ausgang nie automatisch
  wiederholt. Er bleibt als `reconciliation_required` mit externer Provider-ID
  sichtbar, bis er auf der Zielplattform geprüft wurde.
- Ein Clip ohne aktive Plattform gilt nicht als veröffentlicht und wird deshalb
  nicht allein wegen abgelaufener Retention aus dem Vorbereitungsvorrat gelöscht.
- Medienrouten akzeptieren keinen Clientpfad. Sie lesen nur den gespeicherten
  Renderpfad eines Clips innerhalb des vorgesehenen Render-Verzeichnisses.
- Externe Textanreicherung benötigt gespeicherten Consent und läuft über den
  zentralen `tb-llm`-Pfad.

## Aktueller KI-/Medienstand

Die Textanreicherung nutzt im produktiven Bot den zentralen freigegebenen
Deepseek-V4-Flash-Pfad. Eine Transkription ist im Rust-Worker nicht verdrahtet;
es gibt daher noch keine automatische Untertitel-Erzeugung. Das Video-Layout
arbeitet mit Spielausschnitt und optionaler Cam als PiP oder gestapeltem
Bereich.

## Providergrenzen

- TikTok Direct Post ist technisch angebunden, bleibt aber vollständig aus der
  Automatik und aus Provideraufrufen gesperrt. Vorher fehlen die vorgeschriebene
  dynamische Auswahl von Sichtbarkeit und Interaktionen sowie eine ausdrückliche
  Zustimmung pro Clip; außerdem steht der App-Audit aus. Analytics fehlen.
- Instagram nutzt Instagram Login, resumable Upload und wartet auf den fertigen
  Mediencontainer, bevor veröffentlicht wird.
- YouTube nutzt resumable Upload. Nicht angefragte Analytics-Scopes werden auch
  nicht durch erfundene Nullwerte ersetzt. Neue Testuploads sind standardmäßig
  privat.

Ein Adapter im Code ist kein Beleg für einen freigegebenen Produktionsweg. Der
jeweilige Pfad gilt erst nach Plattform-Audit und kontrolliertem privaten oder
isolierten End-to-End-Test als bestätigt.

## Betrieb

Die Rust-Migrationen müssen vor dem neuen Code angewandt sein. Das Release
bündelt die exakt geprüfte `yt-dlp`-Version für einen socketaktivierten,
secretfreien Downloader-Helfer. Der Helfer schreibt nur in einen vom Bot
geöffneten Dateideskriptor; Bubblewrap, ein eigener Systemnutzer und eine eigene
nftables-Ausgangskette trennen Parser, Dateisystem und interne Netze vom Bot.
Kein Dienst sucht das Programm in `PATH` oder einem Benutzerverzeichnis. Nach
Änderungen werden Bot und Dashboard neu gestartet und die Vorbereitung sowie
das Release-Gate live geprüft. Während dieser Prüfung bleibt der Kanal auf
`prepare_only`.

Ausführlicher Ablauf, Tabellen und Testreihenfolge:
[Social-Media-Pipeline](../internal/social-media-pipeline.md). Nutzeransicht:
[Social-Media-Clips und Uploads](../funktionsweise/social-media-uploads.md).
