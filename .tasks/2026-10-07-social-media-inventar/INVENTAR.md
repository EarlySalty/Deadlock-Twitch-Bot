# Social-Media-Dashboard: Bestand, Klassen und Clip-Agent-Bausteine

Stand: 2026-10-07. Codebasis: `e0b0dbaf662d7680c4ceaa210bf15f1443693cd8`. Produktiver Datenbanksnapshot: **2026-10-07 06:13:23 UTC**. Bericht-Branch: `docs/social-media-inventar`.

## Kurzfassung

**Das Social Studio ist integriert und ausgeliefert. Es ist kein aufgegebener Prototyp.** Clipbestand, manuelle Texte, Layouts, Facecam, Vorschau, Freigaben, Planung und Upload haben bestehende Rust-Pfade. Die produktive Datenbank enthält **206 Social-Clips, 58 Uploadaufträge, 17 Reports und 82 VOD-Archivdatensätze**. Bei der zusätzlichen Statusprobe gab es einen abgeschlossenen YouTube-Upload, 54 wartende Aufträge und drei fehlgeschlagene TikTok-Aufträge. Das belegt eine vorhandene Pipeline, keine umfassende Plattformabnahme.

### Zahlen je Klasse

Die folgenden Zahlen zählen die nummerierten Softwarebausteine R, H, F, A, N, P und T. Tabellen, Branches und wiederholte Einträge in der Bausteinkarte werden nicht erneut gezählt. Historische, bereits entfernte Python-Bausteine sind mitgezählt und ausdrücklich gekennzeichnet.

| Klasse | Bausteine |
| --- | ---: |
| lebt | 56 |
| gebaut, unverdrahtet | 14 |
| abgeschaltet | 13 |
| Schrott | 2 |
| doppelt | 8 |
| Legacy-Python | 11 |
| Gesamt | 104 |

### Größte Altlasten und Doppelungen

1. **Entfernter Python-Hauptteil:** `bot/social_media/` wurde bereits am 2026-07-21 entfernt: 44 Dateien, 13.404 Zeilen. Noch einmal portieren wäre Doppelarbeit. Übrig ist dagegen ein zusammenhängender Python-Highlight-Detektor mit etwa 1.356 Quellzeilen und 172 Testzeilen. Dieser ist kein einzelnes Mini-Skript; kurz laufende Prüf-/Kalibrierungsaufrufe dürfen dennoch bleiben. Vor Verwendung als dauerhafter Botworker muss dessen Runtime Rust sein.
2. **Zwei VOD-Archive:** `tb-vod-archive` arbeitet im Bot mit PostgreSQL; `~/vod-archive` hat einen eigenen Rust-Downloader und SQLite-Katalog. Der Botpfad schreibt aktuell. Der eigenständige Dienst war bei der Erhebung inaktiv. Für den Bot bleibt `tb-vod-archive`; einzigartige Funktionen des Nebenrepos vor Bereinigung abgleichen.
3. **Zwei Clip-Downloadkerne:** Prep, Preview und Batch verwenden atomaren Download. Der Uploadworker besitzt daneben einen direkten Download ins gemeinsame Ziel. Zusammenführen, nicht einen dritten Weg bauen.
4. **Echte kleine Schrottstelle, große aufbewahrte Akten:** Der unaufgerufene alte Social-Redirectstub ist durch die echte SPA ersetzt. Das Referenz-Studio ist eine zweite, mockbasierte Oberfläche, nicht die produktive App. Die größten Aufgabenakten enthalten Screenshots und Nachweise, keine laufenden Parallelservices: Clip-Format etwa 16,4 MB, Highlight-Erkennung 3,3 MB, Shell-Größe 1,7 MB, Studio-Handoff 1,6 MB. Größe allein ist kein Schrottbeleg.
5. **Wiederverwendungslücken statt fehlender Grundlagen:** Highlight-Ergebnisse erreichen die Social-Queue nicht; der lokale Rust-STT-Dienst war inaktiv; die Kontextdaten umfassen erst einen Lauf mit 180 Sekunden. Für einen Clip-Agenten sind viele Bausteine vorhanden, aber die Qualität und Übergaben sind nicht durchgehend belegt.

**Wichtiger Reparaturbefund:** Das getrennte Adminpanel sendet für eine Social-Freigabe `streamer_login`; Rust verlangt `twitch_user_id`. Der Social-Studio-Client hat bereits den richtigen Vertrag. Der Adminweg ist verdrahtet, aber im untersuchten Snapshot nicht korrekt beschreibbar. Kein Schrottbeweis und in diesem Auftrag nicht repariert.

## 1. Lesart, Beweise und Grenzen

BESTAND[BS-1]: ja | Fundort: rust/crates/tb-social-media/src/clip_context.rs:116 | Anknüpfung: vorhandene Kontextsignale, Schnittvorschläge, Layout-, Preview-, Approval- und Uploadpfade

- **lebt:** produktiv eingebundener Pfad, mit Aufrufer und laufendem Elternsystem beziehungsweise Datenaktivität belegt. Ein einzelner Instagram-Upload oder die Nutzung jeder UI-Aktion wird damit nicht behauptet. Entsprechende Grenzen stehen in den Zeilen.
- **gebaut, unverdrahtet:** kein aktueller Produktclient oder automatischer Anschluss gefunden. Bei alten HTTP-Routen ist die Registrierung ausdrücklich erhalten; externe Nutzer dieser Routen wurden nicht gemessen. CLI- und Testbausteine brauchen nicht zwangsläufig einen Botloop.
- **abgeschaltet:** dokumentierter Abschaltvertrag, fehlender bewusster Workerstart, deaktivierter Highlight-Zweig oder nicht verfügbare Messfunktion. Die deaktivierte Funktion wird vom weiterhin lebenden Speicher-/Handbearbeitungspfad getrennt.
- **Schrott / doppelt / Legacy-Python:** Ersatz, zweiter Weg oder Rust-Port wird konkret benannt. Leere Tabellen und fehlende Journalmeldungen reichen dafür nicht.

Fundstellen ohne absoluten Präfix beziehen sich auf das Repo bei `e0b0dbaf`, nicht auf den veränderten gemeinsamen Checkout. Verkürzte Nachbarpfade in derselben Tabellenzelle beziehen sich auf den zuerst genannten `src/`- beziehungsweise Frontend-Fachordner; in den historischen Pythonzeilen gilt der Gitstand für die ganze Zelle. `main.rs` in Workerbelegen bedeutet `rust/bin/tb-bot/src/main.rs`. Historische Python-Fundstellen mit `@cbcfeca2^` sind Git-Objekte, keine vorhandenen Dateien. Nebenrepos wurden separat auf ihren gelesenen Ständen untersucht: Uplink `cb8e584`, eigenständiges VOD-Archiv `5342074`. `/etc/caddy/Caddyfile` ist Hostbestand, nicht Teil des Git-Snapshots.

**Laufzeitbeweis:** Bot und Dashboard waren als Systemdienste aktiv. `current` zeigte auf den untersuchten SHA. Das ausgelieferte Hauptbundle liegt unter `/opt/deadlock/twitch/current/bot/analytics/dashboard_v2/dist`, nicht unter dem Quellpfad `bot/dashboard_v2`; es enthält Social-, API-, Preview- und Titelrouten. Die API liefert diese SPA über `handlers/spa.rs:359` aus. Journalmeldungen belegen Fetch, Approval und VOD-Archiv. Die genaue Methode und Messgrenzen stehen in `EVIDENCE.md`.

**Default ist nicht Ist-Zustand:** `clip_fetcher_enabled` und `highlight_clipper_enabled` haben beide Default `false` (`rust/crates/tb-config/src/operations.rs:102`). Das aktuelle Journal belegt dennoch den Start des periodischen Clip-Fetchers um 05:38:46.034 UTC. Der Highlight-Clipper wurde um 05:38:46.033 UTC dagegen ausdrücklich als deaktiviert gemeldet. Die periodische Clipbeschaffung ist deshalb nicht pauschal als abgeschaltet klassifiziert.

## 2. Rust: Social-Media-Bausteine

Geprüft wurden die 59 Rust-Quelldateien von `tb-social-media` samt Integrationstest. Gemeinsam genutzte Typen und unmittelbar zusammengehörige Service-/Workerdateien sind in einer Zeile zusammengefasst. Die Empfehlungen sind nicht ausgeführt.

### 2.1 Eingang, Freigaben und Planung

| ID | Name und Ort | Zweck | Klasse | Beleg | Empfehlung |
| --- | --- | --- | --- | --- | --- |
| R01 | Modulvertrag und Cliptypen, `rust/crates/tb-social-media/src/lib.rs:51`, `clip/mod.rs:1`, `clip/model.rs:3` | Gemeinsame Daten- und Moduloberfläche. | lebt | Helix, Repository, Service und Composition-Root verwenden die Typen. | behalten |
| R02 | Twitch-Clipquelle, `rust/crates/tb-social-media/src/clip/helix.rs:77` | Einzelclips, Broadcaster-Pagination und Top-Game-Clips holen. | lebt | Fetch-Service, Chatadapter und `korpus_ernte` rufen den vorhandenen Client. | für Clip-Agent wiederverwenden |
| R03 | Cliprepository, `rust/crates/tb-social-media/src/clip/repository.rs:79` | Clips, Kategorien, VOD-Zeit, Layout und Fetch-Historie speichern. | lebt | Service und manueller Upload; 206 Clips, aktuelle Downloadaktivität. | für Clip-Agent wiederverwenden |
| R04 | Clip-Fetch-Service, `rust/crates/tb-social-media/src/clip/service.rs:73` | Beschaffung und Persistenz koordinieren. | lebt | API `social_media.rs:1506`; Journal bis 05:41:18 UTC. | behalten |
| R05 | Periodischer Fetch, `rust/crates/tb-social-media/src/clip/task.rs:33` | Aktive Partner alle sechs Stunden abfragen. | lebt | `main.rs:1920`; Default aus, tatsächlicher Taskstart im Journal um 05:38:46.034 UTC. | behalten |
| R06 | Clipmanagement, `rust/crates/tb-social-media/src/clip_manager.rs:30` | Dateiimport, Liste, Batch-Queue und Uploadmarkierung bedienen. | lebt | Registrierte Handler; aktueller Multipart-Client benutzt Dateiimport. | behalten; Freigabevertrag von Alt-Batchpfaden abgleichen |
| R07 | Texttemplates und Hashtags, `rust/crates/tb-social-media/src/clip_templates.rs:178` | Globale und persönliche Textbausteine anbieten. | lebt | Handler `social_media.rs:2661` im manuellen Metadatenpfad; fünf globale Vorlagen vorhanden. | für Clip-Agent wiederverwenden |
| R08 | Externe Clipformulare, `rust/crates/tb-social-media/src/forms.rs:216` | Reservierung und Ergebnis externer Formulare speichern. | gebaut, unverdrahtet | Alter Handler `social_media.rs:1414`; keine aktuelle UI-Anbindung, null Übermittlungen; zusätzlicher `forms_submit_enabled`-Guard. | behalten, bis externe Aufrufer geklärt sind |
| R09 | Approval und Approvalworker, `rust/crates/tb-social-media/src/approval.rs:230`, `approval_worker.rs:40` | Freigaben, Auto-Freigabe, Nachreihen und Stornierung ausführen. | lebt | `main.rs:1698`; 206 Zustände, Journal und Entscheidung bis 05:49:12 UTC; keine Enrichment-Voraussetzung mehr. | für Clip-Agent wiederverwenden |
| R10 | Uploadqueue, `rust/crates/tb-social-media/src/clip_queue.rs:71` | Termine, Plattformzustände, Retry und Kontingent verwalten. | lebt | Approval und Uploadworker; 58 Aufträge. Ein Queueeintrag ersetzt keine Freigabe. | behalten |
| R11 | Postingplan und Scheduler, `rust/crates/tb-social-media/src/posting_plan.rs:454`, `scheduler.rs:44` | Zeitzone, Kadenz, Kategorien und Vorrat bestimmen. | lebt | Approval und PostingPlanDraft; sechs Plattformpläne und vier Kategorieeinstellungen. | für Clip-Agent wiederverwenden |
| R12 | Settings und Partnerzugang, `rust/crates/tb-social-media/src/settings.rs:46`, `partner_access.rs:22` | Einstellungen, externe Datenfreigabe und ID-gebundenen Zugang prüfen. | lebt | Aktuelle Handler und Guards; zwei Partnerfreigaben. | behalten |

### 2.2 Vorbereitung, Hochkant, Facecam und Vorschau

| ID | Name und Ort | Zweck | Klasse | Beleg | Empfehlung |
| --- | --- | --- | --- | --- | --- |
| R13 | Clip-Prep, `rust/crates/tb-social-media/src/clip_prep_worker.rs:60` | Atomar herunterladen und Downloadfehler begrenzen. | lebt | `main.rs:1713`, Preview und Batch; 114 Clipzeilen mit Downloadzeit bei Zusatzprobe. Der Kategorie-Schalter heißt weiterhin `enrichment_enabled`, ohne aktive KI. | für Clip-Agent wiederverwenden |
| R14 | Previewworker, `rust/crates/tb-social-media/src/preview.rs:129` | Vorschauen auf Anforderung rendern und hängende Claims wiederaufnehmen. | lebt | `main.rs:1722`, API `social_media.rs:4009`, ClipCard; eine gespeicherte Previewzeit. | für Clip-Agent wiederverwenden |
| R15 | Offline-Batchrenderer, `rust/crates/tb-social-media/src/batch.rs:55`, `bin/render_clips.rs:58` | Ausgewählte Clips ohne Upload rendern. | gebaut, unverdrahtet | Eigenständiger CLI-Einstieg; kein automatischer Produktaufruf oder aktueller Lauf nachgewiesen. | für Clip-Agent wiederverwenden |
| R16 | Layout und Facecam-Geometrie, `rust/crates/tb-social-media/src/layout.rs:202` | Gameplay-/Kameraboxen, PiP, Stacked, Blur und Overrides validieren. | lebt | LayoutEditor, Repository, Render; ein gespeichertes Streamerlayout. | für Clip-Agent wiederverwenden |
| R17 | Gemeinsamer Medienrenderer, `rust/crates/tb-social-media/src/render.rs:39`, `video_processor.rs:160` | FFmpeg-Compositing, Hochkant, Blur, Branding und atomare Ausgabe erzeugen. | lebt | Preview, Batch und Upload rufen denselben Rendervertrag; Logo und Font werden eingebettet. | für Clip-Agent wiederverwenden |
| R18 | Ältere Compose-/Trim-/Subtitle-Wrapper, `rust/crates/tb-social-media/src/video_processor.rs:436`, `:514`, `:588`, `:623` | Zusätzliche Einzeloperationen für Videos bereitstellen. | gebaut, unverdrahtet | Keine produktiven äußeren Aufrufer gefunden; aktueller Renderpfad ist R17. | zusammenführen, wenn Schnittbedarf entsteht |
| R19 | ASS-Branding, `rust/crates/tb-social-media/src/subtitles.rs:189` | Kanalmarke und manuellen Hook einblenden. | lebt | `render.rs:67`; Sprachsegmente werden dabei als leere Liste übergeben. | für Clip-Agent wiederverwenden |
| R20 | Sprachcues und Transkriptleser, `rust/crates/tb-social-media/src/subtitles.rs:60`, `render.rs:11`, `:29`, `:68` | Gesprochene Untertitel aus Segmenten bilden. | abgeschaltet | Aktiver Render übergibt keine Sprachsegmente; die Transkript-/Schalterleser sind nicht aufgerufen. | für Clip-Agent wiederverwenden, Abschaltung nicht nebenbei aufheben |

### 2.3 Konten, Veröffentlichung und Auswertung

| ID | Name und Ort | Zweck | Klasse | Beleg | Empfehlung |
| --- | --- | --- | --- | --- | --- |
| R21 | Credentials, OAuth und Refresh, `rust/crates/tb-social-media/src/credentials.rs:80`, `oauth.rs:128`, `refresh_worker.rs:55` | ID-gebundene verschlüsselte Konten verbinden und erneuern. | lebt | Registrierte Callbacks; `main.rs:1752`; drei Kontoverbindungen, Refresh am 2026-10-07. | behalten; laufenden Ablauf-/DM-Auftrag respektieren |
| R22 | Uploadworker und TikTok-Recovery, `rust/crates/tb-social-media/src/upload_worker.rs:141`, `tiktok_recovery.rs:62` | Freigabe erneut prüfen, rendern, veröffentlichen und unsichere Uploads verfolgen. | lebt | `main.rs:1738`; persistente Queue und abgeschlossener YouTube-Auftrag. | behalten; laufenden Uploadtext-/Direct-Post-Auftrag respektieren |
| R23 | Uploadervertrag, `rust/crates/tb-social-media/src/uploaders/mod.rs:98` | Validierung, Fehler, Checkpoints und Metriktypen vereinheitlichen. | lebt | Upload- und Insightsworker. Der Checkpoint-Default verweigert statt Erfolg zu erfinden. | behalten |
| R24 | YouTube-Uploader, `rust/crates/tb-social-media/src/uploaders/youtube.rs:605` | Resumable Shorts, Refresh und Basisstatistik bedienen. | lebt | Workeraufruf; ein Uploadzeitpunkt am 2026-09-24. | für Clip-Agent wiederverwenden |
| R25 | Instagram-Uploader, `rust/crates/tb-social-media/src/uploaders/instagram.rs:474` | Uploadcontainer, Veröffentlichung und Insights bedienen. | gebaut, unverdrahtet | Workerzweig existiert; bei Zusatzprobe kein Instagram-Konto und kein Upload. Keine aktuelle Plattformnutzung belegt. | behalten, bei Verbindung funktional abnehmen |
| R26 | TikTok-Uploader, `rust/crates/tb-social-media/src/uploaders/tiktok.rs:178` | Chunks, Checkpoints, Status und Inbox-Fallback bedienen. | lebt | Verbundenes Konto; 27 pending und drei failed bei Zusatzprobe, kein fertiger Post. Direct Post ist separater laufender Auftrag. | behalten |
| R27 | TikTok-Analytics, `rust/crates/tb-social-media/src/uploaders/tiktok.rs:503` | Plattformzahlen abrufen, wenn Scope und echte Video-ID vorhanden sind. | abgeschaltet | Funktion liefert `NotImplemented`; keine produktive Messung. Das ist nicht der Uploadstub. | behalten als erkennbare Lücke, keine Nullmesswerte ableiten |
| R28 | Analytics und Insights, `rust/crates/tb-social-media/src/analytics.rs:82`, `insights_worker.rs:208`, `clip_analytics.rs:13` | Plattformwerte speichern, pollen und fürs Dashboard aggregieren. | lebt | `main.rs:1764`; drei gespeicherte Messzeilen bis 2026-10-03. Bucketproblem siehe Abschnitt 6. | behalten, Messsemantik vor Agent-Feedback korrigieren |
| R29 | Reportwriter und Dispatcher, `rust/crates/tb-social-media/src/report_writer.rs:490`, `report_dispatcher.rs:49` | Rankings und Wochenberichte mit deterministischem oder LLM-Text erstellen. | lebt | `main.rs:1703`; 17 Reports, letzter 2026-10-05. Report-LLM bleibt erreichbar, obwohl Clip-Enrichment aus ist. | behalten |
| R30 | Retention und Worker, `rust/crates/tb-social-media/src/retention.rs:82`, `retention_worker.rs:36` | Verwerfen, Ablauf und Dateibereinigung koordinieren. | lebt | `main.rs:1693`; Clipmanager und Queue nutzen Retentionsstatus. Abgeleitete Uploadrenderdateien fehlen im Cleanup. | behalten, Dateiumfang vervollständigen |
| R31 | Archiv-Einstellungen, `rust/crates/tb-social-media/src/vod_archive.rs:50` | Aktive VOD-Archivbesitzer und Einstellungen verwalten. | lebt | Aktuelle Archivkarte und `tb-vod-archive`; zwei Einstellungen. Kein weiterer Downloadworker in dieser Datei. | behalten |

### 2.4 Erhaltene Analyse, Kontext und Hilfsmodule

| ID | Name und Ort | Zweck | Klasse | Beleg | Empfehlung |
| --- | --- | --- | --- | --- | --- |
| R32 | Manuelle Enrichment-Persistenz, `rust/crates/tb-social-media/src/enrichment.rs:121` | Vorhandene Plattformtexte und manuelle Änderungen lesen und speichern. | lebt | EnrichmentPanel, API und Approval-Edit. Tabellenname bedeutet keine aktive Automatik. | für Clip-Agent wiederverwenden |
| R33 | Enrichmentworker und Pipeline, `rust/crates/tb-social-media/src/enrichment_worker.rs:33`, `enrich_pipeline.rs:183` | Transkript, Korrektur und LLM-Texte verarbeiten. | abgeschaltet | Kein Konstruktor in `main.rs:1732`; Start-API gibt 503; Abschaltvertrag seit e0b0dbaf. | behalten, latenten Persistenzfehler vor späterer Nutzung beachten |
| R34 | Social-STT und Korrektur, `rust/crates/tb-social-media/src/transcription.rs:14`, `correction.rs:201` | Lokale Segmente und Deadlock-Begriffskorrektur liefern. | abgeschaltet | Ausschließlich im abgeschalteten Analyse-/Sprachzweig erreichbar; kein aktiver Social-Transcriber-Konstruktor. | für Clip-Agent wiederverwenden, Abschaltgrenze beibehalten |
| R35 | Vokabular und Seed, `rust/crates/tb-social-media/src/vocab.rs:134`, `seed_vocab.rs:229` | Fachbegriffe und Slang zentral verwalten. | gebaut, unverdrahtet | Rust-CRUD-Routen registriert, aber keine aktuelle Vokabularoberfläche; Analyseverbraucher aus. | für Clip-Agent wiederverwenden |
| R36 | Clip-LLM-Prompt und Parser, `rust/crates/tb-social-media/src/llm.rs:87`, `:288` | Strukturierte Plattformtexte aus Clipkontext erzeugen. | abgeschaltet | Clip-Generate-Zweig liegt hinter R33; geteilte Typen bleiben für Reports relevant. | behalten, nicht als aktive Titelgenerierung ausweisen |
| R37 | Social-LLM-Dispatcher, `rust/crates/tb-social-media/src/llm_dispatch.rs:66` | Zentrale LLM-Auswahl und externe Datenfreigabe anwenden. | lebt | Reportwriter `:716` verwendet den Connector zu `tb-llm`. | für Clip-Agent wiederverwenden |
| R38 | Titelheuristik, `rust/crates/tb-social-media/src/title_gate.rs:39` | Generik, Wortzahl und Vokabular als Titelsignal bewerten. | gebaut, unverdrahtet | Kein produktiver Aufrufer gefunden. | für Clip-Agent wiederverwenden als unkalibriertes Signal |
| R39 | Clipkontext und Lernen, `rust/crates/tb-social-media/src/clip_context.rs:65`, `:116`, `:263` | Momente auflösen, Schnitte empfehlen und Gewichtstemplates aus Bestand lernen. | lebt | Momentauflösung in `chat_wiring.rs:183`; `clip_context_learn` wertet bestehenden Korpus aus. Neue Ernte davon getrennt. | für Clip-Agent wiederverwenden |
| R40 | Kontext-Ernte, `rust/crates/tb-social-media/src/clip_context_harvest.rs:585` | Audio, OCR, Chat und STT pro Sekunde erheben. | abgeschaltet | Keine produktive neue Ernte; `rust/bin/tb-dashboard/src/bin/clip_context_learn.rs:141` verweigert sie ausdrücklich. | für Clip-Agent wiederverwenden nach späterer ausdrücklicher Entscheidung |
| R41 | `korpus_ernte`, `rust/crates/tb-social-media/src/bin/korpus_ernte.rs:10` | Top-Game-Clipmetadaten als JSON auf stdout liefern. | gebaut, unverdrahtet | Eigenständige CLI, Defaultlimit 800; kein Download, STT, Lernen oder DB-Schreiben. | für Clip-Agent wiederverwenden als Metadatenquelle |
| R42 | Legacy-Schema-Helfer, `rust/crates/tb-social-media/src/schema.rs:148` | Tabellen per `ensure_schema` anlegen. | doppelt | Kein produktiver Aufrufer; Baseline und Folgemigrationen bilden den aktiven Weg. | entfernen, Migrationen behalten |
| R43 | Rechtstext-Rendering und HTTP-Schutz, `rust/crates/tb-social-media/src/rendering.rs:29`, `http_security.rs:6` | Rechtstexte rendern und Endpunkte/Redirects absichern. | lebt | Registrierte Rechtstext-, OAuth-, Instagram- und Vokabularpfade. Nicht mit Video-`render.rs` verwechseln. | behalten |
| R44 | Testsupport und Kontext-Integrationstest, `rust/crates/tb-social-media/src/test_support.rs:1`, `tests/clip_context_learning.rs:9` | Datenbanktests und Bestandsschutz ermöglichen. | gebaut, unverdrahtet | Erwarteter Testpfad, kein produktiver Worker. Historische Tests wurden nicht erneut ausgeführt. | behalten als Tests |

### 2.5 Highlight-Crate

Die 13 Dateien in `tb-highlight` sind nicht komplett tot: Der Highlightloop ist aus, der separate Offline-VOD-Export hat einen anderen aktiven Anschluss.

| ID | Name und Ort | Zweck | Klasse | Beleg | Empfehlung |
| --- | --- | --- | --- | --- | --- |
| H01 | Highlight-Komposition, Worker, Partner und State, `rust/crates/tb-highlight/src/lib.rs:16`, `config.rs:11`, `worker.rs:148`, `partners.rs:109`, `state.rs:26` | Partner und Steamzuordnung abfragen und verarbeitete Matches merken. | abgeschaltet | `main.rs:1654`; Default false und Journal bestätigt deaktiviert. State ist lokales JSON. | für Clip-Agent wiederverwenden; keine blinde Reaktivierung |
| H02 | Matchclient und Demodownload, `rust/crates/tb-highlight/src/deadlock_client.rs:58`, `demo_downloader.rs:24` | Match-History, Metadaten, Demo und Cache liefern. | abgeschaltet | Aufrufer im deaktivierten Worker `:179`, `:226`, `:247`. | für Clip-Agent wiederverwenden |
| H03 | Demo-/Eventanalyse, `rust/crates/tb-highlight/src/boon.rs:61`, `demo_analyzer.rs:54`, `event_detector.rs:57` | Kills, Combos, HP, Clutch, Teamfights und Deduplizierung bewerten. | abgeschaltet | Worker `:254`, `:269`, `:275`; echte heuristische Implementierung, keine laufende Social-Analyse. | für Clip-Agent wiederverwenden |
| H04 | Highlight-Sender, `rust/crates/tb-highlight/src/highlight_sender.rs:25` | VOD-Ausschnitte an lokalen Relay übergeben. | abgeschaltet | Kein Empfänger im geprüften Rust-Bestand, keine Social-Registrierung; Worker markiert nach Best-effort-Senden trotzdem processed. | zusammenführen mit vorhandenem Übergabepfad, erst nach bestätigtem Erfolg abschließen |
| H05 | Twitch-VOD-Port und Offline-Export, `rust/crates/tb-highlight/src/twitch_vod.rs:21`, `vod_export.rs:30` | Matchzeit einem VOD zuordnen und Offline-VOD exportieren. | lebt | `main.rs:876`, `eventsub_hooks.rs:1318`; eigener Exportzweck trotz ausgeschaltetem Highlightloop. Kein neuer Lauf durch diese Inventur. | für Clip-Agent wiederverwenden; Exportziel nicht als Social-Queue missverstehen |

## 3. Dashboard, Editor, API und Routing

### 3.1 Produktfrontend

Das Social-Frontend umfasst 16 Fachdateien mit etwa 6.883 Zeilen, ohne gemeinsame Shell, Übersetzungen und Tests. Die größten Dateien sind `SocialMedia.tsx` mit 2.538, `LayoutEditor.tsx` mit 837 und der API-Client mit 588 Zeilen. Das ist Wartungsumfang, nicht automatisch Schrott.

| ID | Name und Ort | Zweck | Klasse | Beleg | Empfehlung |
| --- | --- | --- | --- | --- | --- |
| F01 | Social-Zugang und Kanalwahl, `bot/dashboard_v2/src/App.tsx:433`, `pages/SocialMediaAdmin.tsx:28` | Admins und freigeschaltete Partner zum Studio führen. | lebt | Tatsächliche Mounts bei `SocialMediaAdmin.tsx:177`, `:183`; ausgeliefertes Bundle. Der Kommentar „Admin-only“ ist veraltet. | behalten |
| F02 | Studio, Clipkarten und Dateiimport, `bot/dashboard_v2/src/pages/SocialMedia.tsx:167`, `:1173`, `:2163` | Pool, Status, Upload und Clipaktionen anbieten. | lebt | Kanalgebundene Queries und Multipart-Mutation; vier reale Views. | für Clip-Agent wiederverwenden |
| F03 | Freigabe-, Zeitplan-, Kategorie- und Vorratskarten, `bot/dashboard_v2/src/pages/SocialMedia.tsx:1278`, `:1393`, `:1710`, `:1773` | Posting und Nachschub konfigurieren. | lebt | Echte Draft-Callbacks und Fetch-Mutation bei `:338`. | behalten |
| F04 | PostingPlanDraft, `bot/dashboard_v2/src/components/socialmedia/PostingPlanDraft.tsx:53` | Änderungen lokal sammeln und kontrolliert speichern. | lebt | Mount bei `SocialMedia.tsx:492`; schreibt drei bestehende API-Gruppen, nicht atomar. | behalten |
| F05 | LayoutEditor, `bot/dashboard_v2/src/components/socialmedia/LayoutEditor.tsx:535` | Ausschnitte, Facecam und Hochkantmodi einstellen. | lebt | Standardlayout und Clipoverride bei `SocialMedia.tsx:945`, `:1098`. | für Clip-Agent wiederverwenden |
| F06 | Clipeditor und manuelle Texte, `bot/dashboard_v2/src/pages/SocialMedia.tsx:1071`, `components/socialmedia/EnrichmentPanel.tsx:86` | Layout, Titel, Beschreibung und Hashtags bearbeiten. | lebt | GET/PUT bei EnrichmentPanel `:94`, `:104`; kein Regenerierungsaufruf. | für Clip-Agent wiederverwenden |
| F07 | AnalyticsTab, `bot/dashboard_v2/src/components/socialmedia/AnalyticsTab.tsx:83` | Veröffentlichungswerte und Reports anzeigen. | lebt | Dialogmount `SocialMedia.tsx:996`; Reports im Adminpfad. Auswahl lädt höchstens 100 Clips je Veröffentlichungsstatus. | behalten, Mess- und Umfangsgrenze anzeigen |
| F08 | WorkspaceDialog, Queue- und Fehlerzustände, `bot/dashboard_v2/src/components/socialmedia/WorkspaceDialog.tsx:5`, `queuePresentation.ts:22`, `kartenZustand.ts:35`, `LadeFehlerHinweis.tsx:10` | Aktionen fokussieren und bei unbekanntem Serverstand sperren. | lebt | Mehrere Produktmounts; vollständiger Poolabruf mit `loadQueueSnapshot`. | behalten |
| F09 | Fachlabels, CSS, Typen und Geometriehelfer, `bot/dashboard_v2/src/components/socialmedia/labels.ts:34`, `studio.css:1`, `utils/socialMediaChannel.ts:7`, `utils/socialMediaLayout.ts:49`, `types/socialMedia.ts:56` | Einheitliche Texte, Identität und Layoutvertrag liefern. | lebt | Direkte Imports in Studio, Kanalwahl und Editor. | für Clip-Agent wiederverwenden |
| F10 | Sprachwahl und Wörterbuch, `bot/dashboard_v2/src/context/LanguageContext.tsx:43`, `i18n/dictionary.ts:651` | DE/EN im Dashboard anbieten. | lebt | Provider über App-Routen; LanguageCard bei `SocialMedia.tsx:2109`; fehlende Übersetzung fällt auf Deutsch zurück. | behalten |
| F11 | Konten und VOD-Archivkarte, `bot/dashboard_v2/src/pages/SocialMedia.tsx:1859`, `:2014` | Plattformverbindungen und Archiv einstellen. | lebt | Echte Status-, Disconnect- und Archivmutationen; zwei Archivkonfigurationen. | behalten |
| F12 | Zweite Social-Freigabe im Adminpanel, `bot/admin_dashboard/src/pages/streamers/StreamerDetail.tsx:653`, `api/client.ts:1358`, `hooks/useAdmin.ts:595`, `utils/partnerAccess.ts:10`, `api/types.ts:594` | Partnerzugang in der vorhandenen Verwaltung schalten. | doppelt | Zweite Oberfläche für dieselben Handler; PUT bei Client `:1376` sendet fälschlich Login statt Twitch-ID. Feste deutsche Texte, kein belegter eigener DE/EN-Pfad. | behalten, Identitätsvertrag korrigieren; nicht durch dritte Oberfläche ersetzen |

### 3.2 API- und Hostbestand

Die Social-API einschließlich Contestbrücke umfasst etwa 7.884 Zeilen, davon ungefähr 4.295 vor den Haupt-Testbereichen.

| ID | Name und Ort | Zweck | Klasse | Beleg | Empfehlung |
| --- | --- | --- | --- | --- | --- |
| A01 | Social-SPA und Ingress, `rust/crates/tb-dashboard-api/src/lib.rs:1781`, `handlers/spa.rs:359`, `/etc/caddy/Caddyfile:684`, `:692` | Social und Dashboard an Rust auf Port 8769 ausliefern. | lebt | Routerintegration bei `lib.rs:2144`, App-Mount und installiertes Bundle; CSP bei Caddy `:237`, `:241`. | behalten |
| A02 | Öffentliche Rechtstexte und OAuth, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:106`, `:111`, `:1875`, `:3714`, `:3766`, `:3833` | Rechtstexte, Verbindungen, Callback und Trennung bedienen. | lebt | Kontenkarte und registrierte öffentliche Routen. | behalten |
| A03 | ID-Guards und Adminalias, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:570`, `:608`, `:3882`, `:3948`, `lib.rs:1092` | Zugriff und Eigentum prüfen und auf Adminhost verfügbar machen. | doppelt | Adminalias ruft dieselben Handler. Caddy `:1293`, `:1302` blockiert Social auf Adminhost bewusst mit 404. | behalten, notwendiger Hostvertrag; keine zweite Implementierung |
| A04 | Paginierter Pool und Verwerfen, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:2277`, `:2385` | Studio-Clipbestand und Archivierung anbieten. | lebt | `fetchClips` und `discardClip` im Produktfrontend. | behalten |
| A05 | Dateiimport und Twitch-Fetch, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:1008`, `:1468` | Clips lokal aufnehmen oder aus Twitch holen. | lebt | Aktuelle UI; der Fetchhandler ignoriert derzeit `result.error`. | behalten, Ergebnisvertrag korrigieren |
| A06 | Streamerlayout und Clipoverride, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:1082`, `:1123`, `:1170` | Standard- und Einzellayout speichern. | lebt | Beide LayoutEditor-Pfade. | für Clip-Agent wiederverwenden |
| A07 | Manuelle Metadaten, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:2562`, `:2689` | Plattformtexte lesen und verändern. | lebt | EnrichmentPanel GET/PUT. | für Clip-Agent wiederverwenden |
| A08 | Alter Enrichment-Start, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:2715` | Aufrufern die Abschaltung der Automatik melden. | abgeschaltet | Nach Besitzprüfung 503 `clip_enrichment_disabled` bei `:2738`; unbenutzter Startwrapper bleibt. | behalten als klare Abschaltantwort |
| A09 | Approval und Veto, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:2962`, `:3068` | Freigaben bearbeiten und geplante Posts stoppen. | lebt | Aktuelle React-Mutationen und Approval-Service. | für Clip-Agent wiederverwenden |
| A10 | Postingplanhandler, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:3235`, `:3252`, `:3352`, `:3466` | Modus, Zeitzone, Plattformkadenz und Kategorien bedienen. | lebt | PostingPlanDraft benutzt die Endpunkte. | behalten |
| A11 | VOD-Archivhandler, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:3581`, `:3602` | Kanalgebundene Archivkonfiguration lesen und schreiben. | lebt | VodArchiveCard und bestehender Botworker. | behalten |
| A12 | Previewauftrag, Status und MP4, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:4000`, `:4021`, `:4047` | On-demand-Vorschau mit Range-Auslieferung anbieten. | lebt | ClipCard, Previewworker und installierte Route. | für Clip-Agent wiederverwenden |
| A13 | Einzelclipanalytics und Reports, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:2749`, `:2785`, `:2846` | Plattformmetriken lesen und Reports erzeugen. | lebt | AnalyticsTab, Analytics-/Reportdienste. | behalten |
| A14 | Detail- und Vokabularwrapper, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:1696`, `:2362`, `:2939`, `bot/dashboard_v2/src/api/socialMedia.ts:181`, `:300`, `:479` | Einzelantworten und Vokabularverwaltung anbieten. | gebaut, unverdrahtet | HTTP implementiert und registriert, aktuelle React-Komponenten rufen die Wrapper nicht. | behalten bei künftigem Fachbedarf, sonst zusammenführen |
| A15 | Alte Clip-/Hashtagantworten, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:267`, `:285` | `/api/clips` und `/last-hashtags` bedienen. | gebaut, unverdrahtet | Vertragstest `bot/dashboard_v2/tests/socialMediaContract.test.ts:254` hält sie aus dem aktuellen UI heraus; Pool benutzt A04. | externe Aufrufer prüfen, dann zusammenführen |
| A16 | Alte Upload-/Batch-/Markierungspfade, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:1281`, `:1531`, `:1604` | Queue-/Batchwirkungen und manuelle Uploadmarkierung anbieten. | gebaut, unverdrahtet | Echte implementierte HTTP-Wirkung, kein aktueller UI-Aufrufer; keine Stubs. | externe Aufrufer prüfen, Freigabevertrag vereinheitlichen |
| A17 | Alte Texttemplate-APIs, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:400`, `:414`, `:441`, `:485` | Globale und persönliche Textvorlagen anwenden. | gebaut, unverdrahtet | Aktuelle „Templates & Layouts“-Seite benutzt drei lokale Layoutpresets, nicht diese APIs. | für Clip-Agent wiederverwenden, keinen zweiten Template-Speicher bauen |
| A18 | Statistik-/Analytics-Aliasse, `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:254` | Zwei alte URLs auf denselben Summary-Handler führen. | doppelt | `/social-media/api/stats` und `/analytics`; aktuelle UI benutzt Einzelclipanalytics. | Nutzung prüfen, dann zusammenführen |
| A19 | Obsoleter Social-Redirectstub, `rust/crates/tb-dashboard-api/src/handlers/obsolete_routes.rs:48` | Zurückgestelltes Feature behaupten und nach Home leiten. | Schrott | Kein Aufrufer; durch A01 und App-Mount konkret ersetzt. | entfernen |
| A20 | Contest-Brücke, `rust/crates/tb-dashboard-api/src/handlers/social_media_clip_contest.rs:117` | Clips an den vorhandenen Contest übergeben. | lebt | Re-export, registrierte Route und ClipCard-Aktion. | behalten, kein zweiter Contestpfad |
| A21 | Titel-Studio und Vorschlaghandler, `bot/dashboard_v2/src/pages/TitleGenerator.tsx:79`, `rust/crates/tb-dashboard-api/src/handlers/title.rs:462` | Personalisierte Streamtitel vorschlagen. | lebt | App `:452`, Client `src/api/title.ts:73`, POST `/twitch/api/v2/title/suggest`. `/twitch/titel` selbst ist die SPA. | für Clip-Agent wiederverwenden nach Nebenwirkungsprüfung |

**Altpfadumfang:** Zehn alte API-Pfadfamilien sind bewusst weiter registriert, aber aus dem aktuellen UI genommen; einzelne Familien haben mehrere HTTP-Methoden. Kein Treffer im Vertragstest ist ein Benutzungsnachweis. Für externe Integrationen wurden keine Zugriffszahlen erhoben.

## 4. Nebenrepos, Python und Referenzmaterial

### 4.1 Archive, LLM, STT und Signalzwillinge

| ID | Name und Ort | Zweck | Klasse | Beleg | Empfehlung |
| --- | --- | --- | --- | --- | --- |
| N01 | Bot-VOD-Archiv, `rust/crates/tb-vod-archive/src/worker.rs:623`, `store.rs:111`, `config.rs:55` | VODs herunterladen, teilen und fortsetzbar hochladen. | lebt | `main.rs:1791`, `:1798`; 82 VODs und 80 Teile, Journal/DB bis 06:00:47 UTC. | für Clip-Agent wiederverwenden; dieser Botpfad bleibt |
| N02 | Eigenständiges Rust-Archiv, `/home/nathanael/vod-archive/src/main.rs:61`, `:84`, `:213`, `src/config.rs:47` | VODs mit eigenem Downloadroot und SQLite-Katalog verarbeiten. | doppelt | Acht Rust-Dateien, 1.824 Zeilen; `state/vods.db` vorhanden. Systemdienst installiert, bei Erhebung inactive/dead. Kein gemeinsamer Dateifund oder Katalog belegt. | zusammenführen; Botkatalog bleibt, besondere Standalone-Funktionen zuerst abgleichen |
| N03 | Uplink-Hochkant-Primitiven, `/home/nathanael/repos/uplink/crates/uplink-media/src/portrait.rs:42`, `graph.rs:858` | Crop, Stacked und PiP im Live-Mediengraph validieren und umsetzen. | doppelt | Echter Uplink-Serviceaufrufer `uplink-service/src/media_output.rs:200`; zweite Geometrie-/FFmpeg-Implementierung zu R16/R17. Uplink-Livebetrieb hier nicht abgenommen. | zusammenführen bei Geometrieprüfungen; Social-Offlinerender und Uplink-Livegraph als verschiedene Zwecke behalten |
| N04 | Uplink-VOD-Aufnahme, `/home/nathanael/repos/uplink/crates/uplink-vod/src/recording.rs:1` | Komprimierte Aufnahme und VOD-Export vorbereiten. | gebaut, unverdrahtet | Keine äußeren Dienstaufrufer gefunden; README `:61` benennt Anschluss als offen. | für Clip-Agent wiederverwenden, nicht als laufendes Archiv zählen |
| N05 | Zentraler LLM-Hub, `rust/crates/tb-llm/src/hub.rs:74`, `selection.rs:27` | Text-LLM mit Fristen, JSON-Modus, Antwortprüfung und Ledger aufrufen. | lebt | Dashboard-Assistent `:591`, Social-Reportconnector; Fireworks-Modellpolicy im Bestand. Kein direkter Video-/Bildvertrag belegt. | für Clip-Agent wiederverwenden |
| N06 | Lokaler Rust-STT-Dienst, `rust/bin/tb-stt-server/src/main.rs:213`, `ops/stt-server/deadlock-stt-server.service:8` | Whisper-/VAD-Inferenz über lokale HTTP-Route liefern. | abgeschaltet | Installierter Userdienst `deadlock-stt-server.service` inactive/dead. Segmentzeiten vorhanden, keine Wortzeiten. Defaultport 8791 ist aktuell von `deadlock-patchn` belegt und antwortet 401, kein STT-Beweis. | für Clip-Agent wiederverwenden; realen Listener und Dienststatus vor Verwendung klären |
| N07 | Rust-STT-Client, `rust/crates/tb-engagement/src/transcribe.rs:93`, `rust/bin/tb-stream-audit/src/main.rs:2741` | Audio per FFmpeg extrahieren und lokale Segmente parsen. | abgeschaltet | Produktaufrufer existieren; lokaler Dienst war inaktiv, Social-Sprachpfad bewusst aus. `from_local_config()` schützt Loopback, Redirects und Proxy. | für Clip-Agent wiederverwenden; keine funktionierende aktuelle Transkription behaupten |
| N08 | Python-Highlight-Detektor, `ops/highlight-detector/highlight_detector/cli.py:73`, `signals.py:100`, `detector.py:19` | Clips/VODs analysieren, Score lernen, Kandidaten messen, schneiden und speichern. | doppelt | Eigene CLI und DB-Persistenz; etwa 1.356 Quellzeilen. OCR/Audio/Scoring überschneiden sich mit Rust-Kontext-/Highlightpfaden, der End-to-end-Detektor ist nicht vollständig ersetzt. Kein laufender Prozess oder Bot-/Dienstanschluss gefunden. | zusammenführen; vor Einsatz als Botworker nach Rust portieren, dann Nicht-Rust-Runtime entfernen; kurz laufende Prüf-/Evalskripte dürfen bleiben |

**Doppelung präzise:** `korpus_ernte` ist ein Metadatenexport, der Python-Detektor ein Signal-/Messprogramm, `clip_context_harvest` eine deaktivierte Rust-Sekundenernte und `tb-highlight` ein deaktivierter Demo-/Match-Kandidatenpfad. Die Namen stehen nicht für vier gleichwertige Agenten. Sie teilen Teilaufgaben, haben aber unterschiedliche Ein-/Ausgänge und Kataloge.

### 4.2 Historisch entfernte Python-Runtime

Alle P01 bis P10 stammen aus `cbcfeca2^`. Sie fehlen im aktuellen Berichtsworktree und auch im älteren gemeinsamen Checkout. Rust-Ersatz ist vorhanden; die Löschung ist bereits erfolgt. Die Klasse beschreibt Herkunft, nicht eine noch vorhandene Baustelle.

| ID | Name und historischer Ort | Zweck | Klasse | Ersatzbeleg | Empfehlung |
| --- | --- | --- | --- | --- | --- |
| P01 | Dashboard, Storage, Settings, `bot/social_media/dashboard.py@cbcfeca2^:88`, `storage.py:21`, `settings.py:45` | Oberfläche, Tabellen und Einstellungen betreiben. | Legacy-Python | Rust-Handler, Settings und Migrationen. | entfernen, bereits erfolgt |
| P02 | Clip-Fetch und Management, `bot/social_media/clip_fetcher.py@cbcfeca2^:20`, `clip_manager.py:28` | Clips beschaffen und verwalten. | Legacy-Python | Rust `clip/service.rs:13`, `clip_manager.rs:30`. | entfernen, bereits erfolgt |
| P03 | Enrichment und Worker, `bot/social_media/enrichment.py@cbcfeca2^:451`, `enrichment_worker.py:16` | Automatische Plattformtexte erzeugen. | Legacy-Python | Rust `enrich_pipeline.rs:132`, `enrichment_worker.rs:23`, derzeit aus. | entfernen, bereits erfolgt |
| P04 | Approval und Worker, `bot/social_media/approval/approval_service.py@cbcfeca2^:464`, `approval_worker.py:18` | Freigaben und Nachreihen ausführen. | Legacy-Python | Rust Approval und Approvalworker. | entfernen, bereits erfolgt |
| P05 | LLM-Dispatcher und Promptpaket, `bot/social_media/llm/dispatcher.py@cbcfeca2^:39`, `prompts.py:77` | Textgenerierung koordinieren. | Legacy-Python | Rust `llm_dispatch.rs:150`, zentraler Hub; frühere Providerwahl nicht als erhaltene Funktion zählen. | entfernen, bereits erfolgt |
| P06 | Transkription, Korrektur, Vokabular und Seed, `bot/social_media/transcription/whisper.py@cbcfeca2^:232`, `correction.py:182`, `vocab.py:144`, `seed_vocab.py:204` | Sprache und Fachbegriffe bearbeiten. | Legacy-Python | Rust-Module und Rust-STT-Service. | entfernen, bereits erfolgt |
| P07 | Layout und Video, `bot/social_media/layout/storage.py@cbcfeca2^:10`, `rendering.py:24`, `uploaders/video_processor.py:106` | Layout speichern und Clips rendern. | Legacy-Python | Rust Layout, Rechtstext-Rendering und VideoProcessor. | entfernen, bereits erfolgt |
| P08 | Uploadworker und Uploader, `bot/social_media/upload_worker.py@cbcfeca2^:28`, `uploaders/base.py:13` | Plattformuploads ausführen. | Legacy-Python | Rust Uploadworker und Uploadervertrag. | entfernen, bereits erfolgt |
| P09 | OAuth, Credentials und Refresh, `bot/social_media/oauth_manager.py@cbcfeca2^:97`, `credential_manager.py:36`, `token_refresh_worker.py:47` | Plattformzugänge betreiben. | Legacy-Python | Rust OAuth/Credentials/Refreshworker. | entfernen, bereits erfolgt |
| P10 | Analytics, Reports und Retention, `bot/social_media/analytics/insights_worker.py@cbcfeca2^:37`, `report_writer.py:60`, `retention_worker.py:21` | Messen, berichten und bereinigen. | Legacy-Python | Rust Insights, Reportwriter und Retention. | entfernen, bereits erfolgt |
| P11 | Standalone-Archiv und alter Kontozugang, `/home/nathanael/vod-archive/legacy-python/vod_archive.py:2`, `yt_auth.py:2` | VODs archivieren und separaten Kontozugang verwalten. | Legacy-Python | Standalone-Rust `src/main.rs`, `src/bin/yt_auth.rs`; 578 plus 136 Python-Zeilen. | entfernen, Rust-Ersatz vorhanden |
| P12 | Bilderhilfen und Referenztest, `bot/dashboard_v2/tools/uplink-bilder/bauen.py:9`, `_stil.py:16`, `.tasks/2026-09-21-social-studio-handoff/reference/tests/browser_check.py:1` | Offline-Assets und Browserreferenz prüfen. | gebaut, unverdrahtet | Einzelne Hilfs-/Testskripte, kein produktiver Social-Botloop; Bilderhilfen etwa 181 Zeilen. | Hilfsskript, darf bleiben |

Im aktuellen Hauptrepo sind 44 Python-Dateien getrackt. Das ist **nicht** gleichbedeutend mit 44 produktiven Botmodulen: darunter sind Tests, Verwaltungswerkzeuge, Bilderhilfen und der oben benannte Detektor. Eine allgemeine vollständige Inventur sämtlicher Watchdog-, Chatter- und Roadmap-Werkzeuge liegt außerhalb dieses Social-Auftrags. Laufende Servicefunktion darf aber nicht allein wegen eines `ops/`- oder `tools/`-Pfads zum erlaubten Hilfsskript erklärt werden.

### 4.3 Referenzmaterial und irreführende Beschreibungen

| ID | Name und Ort | Zweck | Klasse | Beleg | Empfehlung |
| --- | --- | --- | --- | --- | --- |
| T01 | Mock-Studio-Referenz, `.tasks/2026-09-21-social-studio-handoff/reference/src/react/Example.jsx:7`, `DashboardShell.jsx:11`, `ClipQueueCard.jsx:9`, `AutoPilotSchedule.jsx:18`, `WorkspaceDialog.jsx:5` | Übergabeentwurf mit lokalen Beispielzuständen zeigen. | doppelt | `DEMO_CLIPS`, lokale Approve-/Archive-Änderungen; keine Produktimports. Spätere echte Integration in F01 bis F06 vorhanden. Paket 51 Dateien, etwa 1,56 MB. | entfernen aus künftigem aktiven Entwicklungsbestand, kompakte Design-/Prüfnachweise erhalten |
| T02 | Überholte Python-/Startup-Anweisungen, `docs/architecture/social-media.md:3`, `docs/architecture/engagement-reaktions-lernmodus.md:132`, `rust/crates/tb-social-media/src/lib.rs:42` | Alte Runtime und frühere Workerstarts beschreiben. | Schrott | Beschriebene Python-Dateien entfernt; Enrichmentstart fehlt bewusst; Rust-STT ersetzt Python. Gemeint sind diese Aussagen, nicht pauschal die ganzen Dokumente. | veraltete Anweisungen entfernen und aktuellen Rust-Stand dokumentieren |

## 5. Produktive Daten und Migrationen

### Messgrenze für „letzter Schreibzeitpunkt“

`track_commit_timestamp=off`. Exakte letzte PostgreSQL-Commits pro Tabelle sind damit aus diesem Zustand nicht rekonstruierbar. Die nächste Tabelle zeigt **COUNT(*) und letzte gespeicherte Anwendungsaktivität**, nicht eine erfundene Commitzeit. Vorhandenes `updated_at` wurde bevorzugt, andernfalls das passende Erstell-/Entscheidungs-/Synchronisationsfeld. Zukunftstermine, Retentionsgrenzen, Reportperioden und ursprüngliche Twitch-Erstellzeiten wurden nicht als Schreibzeit benutzt. Spätere Änderungen ohne Aktualisierung eines solchen Feldes bleiben eine Messlücke.

Zeitangaben UTC, Sekunden gerundet. Migrationspfade beginnen mit `rust/migrations/`. `B` steht für `20260601000000_baseline_schema.sql`, `S` für `20260815120000_social_media_scheduling.sql`, `C` für `20260929120000_clip_context_learning.sql`, `V` für `20260814100000_twitch_vod_archive.sql`. Die konkrete Datei mit Zeile bleibt durch diese Auflösung eindeutig.

| Tabelle | Zeilen | Letzte gespeicherte Aktivität UTC | Herkunft / Zustand / Empfehlung |
| --- | ---: | --- | --- |
| `clip_fetch_history` | 58.193 | 2026-10-07 05:41:18 | B:73, `fetched_at`; lebt, behalten. Historie enthält mehr als den aktuell gehaltenen Clipbestand. |
| `clip_last_hashtags` | 0 | keine | B:93; gebaut, keine heutige Nutzung belegt, kein Löschbeweis. |
| `clip_templates_global` | 5 | 2026-02-17 21:10:55 | B:99, `created_at`; vorhandene Defaults, R07 wiederverwenden. |
| `clip_templates_streamer` | 0 | keine | B:120; UI-Anschluss alter Templates fehlt, zusammenführen bei Bedarf. |
| `social_media_category` | 2 | 2026-08-15 03:09:42 | S:13, `created_at`; Konfiguration behalten. |
| `social_media_category_settings` | 4 | 2026-09-05 11:58:58 | S:104, `updated_at`; behalten. |
| `social_media_clip_approval` | 206 | 2026-10-07 05:49:13 | B:246, `decided_at`/DM-/Nachreihzeiten; lebt, behalten. |
| `social_media_clip_enrichment` | 60 | 2026-10-07 05:37:21 | B:258, `updated_at`; Altanalyse und manuelle Texte erhalten. Automatik seit Release aus. |
| `social_media_partner_access` | 2 | 2026-09-05 11:57:20 | `20260806120000_social_media_partner_access.sql:5`, `granted_at`; ID-Migration später angewandt, diese Zeit ist kein Beweis für deren Zeitpunkt. |
| `social_media_partner_access_unresolved` | 0 | keine | `20261001090000_social_media_partner_ids_required.sql:14`; Schutz-/Berichtstabelle behalten. |
| `social_media_platform_auth` | 3 | 2026-10-07 06:09:47 | B:286; Textzeitfelder nach `timestamptz` konvertiert, `last_refreshed_at`; zwei YouTube-, eine TikTok-Verbindung. Keine Credential-Spalten gelesen. |
| `social_media_platform_schedule` | 6 | 2026-09-23 21:41:46 | S:82, `updated_at`; behalten. |
| `social_media_reauth_notifications` | 0 | keine | B:317; laufender Ablauf-/DM-Auftrag, kein Schrottbefund. |
| `social_media_reports` | 17 | 2026-10-05 02:14:03 | B:324, `created_at`; lebt, behalten. |
| `social_media_settings` | 1 | 2026-10-05 02:14:03 | B:345, `updated_at`; letzte Reportperioden-Markierung, nicht die ganze externe Botkonfiguration. |
| `social_media_streamer_layout` | 1 | 2026-09-04 17:47:05 | B:352, `updated_at`; Layoutbestand behalten. |
| `social_media_streamer_settings` | 2 | 2026-09-05 11:58:58 | S:68, `updated_at`; behalten. |
| `social_media_vod_archive` | 2 | 2026-09-05 11:57:20 | `20260814150000_vod_archive_pro_streamer.sql:15`, `updated_at`; aktive Besitzerkonfiguration behalten. |
| `twitch_clip_context_runs` | 1 | 2026-09-30 23:17:47 | C:14, `analyzed_at`; bestehender Korpus, keine neue Ernte. |
| `twitch_clip_context_seconds` | 180 | nicht messbar, kein Zeitfeld | C:31; gehört zum einen Kontextlauf, Daten für Agent/Evaluation wiederverwenden. |
| `twitch_clip_cut_templates` | 1 | 2026-09-30 23:17:47 | C:50, `updated_at`; `sample_count=1`, nicht belastbar trainiert. |
| `twitch_clip_form_submissions` | 0 | keine | `20260716120000_clip_form_submissions.sql:1`; R08, keine produktive Nutzung belegt. |
| `twitch_clip_merkmale` | 1.415 | 2026-09-06 14:08:43 | `20260906130001_twitch_clip_merkmale.sql:1`, `created_at`; historischer Signalbestand wiederverwenden. |
| `twitch_clips_social_analytics` | 3 | 2026-10-03 20:08:30 | B:596, `synced_at`; Bucketsemantik vor Feedback korrigieren. |
| `twitch_clips_social_media` | 206 | 2026-10-07 05:43:31 | B:631; Download-/Upload-/Preview-/Verwerfzeiten, nicht `created_at` von Twitch; lebt, behalten. |
| `twitch_clips_upload_queue` | 58 | 2026-10-07 05:49:13 | B:679; `created_at`/`last_attempt_at`/`completed_at`, nicht future `scheduled_at`; lebt, behalten. |
| `twitch_vod_archive_vods` | 82 | 2026-10-07 06:00:47 | V:18, `updated_at`; lebt, behalten. |
| `twitch_vod_archive_parts` | 80 | 2026-10-07 06:00:47 | V:46, `updated_at`; lebt, behalten. |
| `twitch_vod_highlights` | 22 | 2026-09-06 14:14:14 | `20260906130002_twitch_vod_highlights.sql:1`, `created_at`; historischer Detektorbestand, kein aktueller Social-Uploadanschluss. |

**Ergänzende Statusprobe, 06:07 bis 06:10 UTC:** 30 approved und 176 awaiting_approval; Enrichment 49 failed und elf transcribing. Diese Zustände sind Altbestand, kein Beweis laufender Inferenz. Bei 206 Clips waren 114 Downloadzeitpunkte, ein YouTube-Upload und eine Previewzeit gesetzt. Das sind DB-Metadaten, keine Prüfung der aktuellen Dateiexistenz. VOD-Status: 78 archived, drei uploaded, einer unavailable. Highlights: 22 `neu`, Scorebereich 0,5 bis 0,896.

**Signalbestand:** 1.415 Merkmalszeilen für 205 verschiedene Clips aus 60 Streamer-IDs. Ausgegeben werden keine IDs oder Inhalte. Verteilung: `sprache_pegel` 663/173 Clips, `ocr_soul_sprung` 276/65, `bild_death_screen` 300/43, `ocr_objective` 87/29, `ocr_kill` 80/14, `ocr_multikill` 9/4. „sprache_pegel“ bezeichnet ein Pegelsignal, keine bewiesene Spracherkennung. Der Kontextlauf hat `stt_status=failed:timeout`, `visual_status=sampled`.

Die zugehörigen Social-/VOD-/Clipmigrationen einschließlich `20261001090000` waren in `_sqlx_migrations` erfolgreich eingetragen. Migrationen bleiben Schemawahrheit; die ID- und Preview-Nachträge dürfen nicht zugunsten des unbenutzten `ensure_schema` zurückgebaut werden. Tabellen aus separaten Testschemata wurden ausgeschlossen. Contesttabellen und allgemeine Chat-/Commandtabellen existieren ebenfalls, sind aber eigenständige Systeme; sie werden nicht als Social-Korpus umetikettiert.

## 6. Wirkungslücken und echte Dopplungen

Diese Liste beschreibt den untersuchten Snapshot. Es wurden keine Fixes ausgeführt. Laufende Aufträge im nächsten Abschnitt dürfen Befunde inzwischen bereits verändert haben.

| Befund | Konkreter Beleg | Konsequenz für Bestand und Wiederverwendung |
| --- | --- | --- |
| Admin-Freigabe sendet falsche Identität | `bot/admin_dashboard/src/api/client.ts:1376` gegenüber `handlers/social_media.rs:3882`; Studio-Client `:113` ist korrekt. | Verdrahtet, aber Admin-PUT erfüllt den Vertrag nicht; vorhandene Verwaltung reparieren. |
| Einzel-/Batch-Queue ist nicht gleich Freigabe | `handlers/social_media.rs:1393`, `clip_manager.rs:291`, `approval.rs:376`, `upload_worker.rs:144`. | Veröffentlichung nicht aus einer Queuebestätigung ableiten. |
| Fetch kann Fehler als Erfolg melden | `handlers/social_media.rs:1505` bis `:1515`, `clip/service.rs:85`. | `result.error` wird nicht weitergegeben; Nutzbarkeit des Diensts nicht mit korrekter Ergebnisanzeige verwechseln. |
| Zweiter nicht atomarer Download | `upload_worker.rs:429` bis `:446` gegenüber `clip_prep_worker.rs:147`. | Prep-/Preview-/Batchkern bleibt; Uploaddownload zusammenführen. |
| Abgeleitete Renderdateien entgehen Retention | `upload_worker.rs:119`, `retention_worker.rs:50` bis `:95`. | Original-/Preview-Cleanup deckt plattformspezifische `branded_v2`-Ausgaben nicht ab. |
| Highlight-Vorlauf und Abschlussvertrag fehlen | `tb-highlight/src/worker.rs:82`, `:294`, `:337`, `:347`, `highlight_sender.rs:25`. | Berechnetes `pre_roll_s` wird durch sechs Sekunden ersetzt; Sendefehler oder noch fehlendes VOD können trotzdem processed werden. Kein belastbarer Social-Eingang. |
| Latenter Enrichment-Persistenzfehler | `enrich_pipeline.rs:333` bis `:375`. | Speicherfehler können vor Approval/Done nur geloggt werden; Abschaltung beibehalten, vor späterer Nutzung korrigieren. |
| Analytics summiert kumulative Buckets | `insights_worker.rs:140`, `clip_analytics.rs:55` bis `:62`. | 24h/7d/30d sind nicht belegt echte Zeitfenster; Dashboard-Summe kann denselben Clip mehrfach zählen. Keine zuverlässige Lernzielgröße. |
| Auswertungsfehler verlieren Ursache | `insights_worker.rs:239`, `:251`, `report_writer.rs:733`. | Teilweise Details/Persistenzfehler ignoriert, Reportfallback ohne Fehlerlog. Fallbacktext ist kein Messbeweis. |
| Drei fehlende Analyseübergänge | `chat_wiring.rs:214`, `clip_context_learn.rs:141`, `highlight_sender.rs:25`. | `!clip` schreibt Commandevents, nicht automatisch Social-Clips; neue Kontext-Ernte ist gesperrt; Highlightdateien erreichen keine Social-Queue. |
| STT-Betrieb nicht verfügbar belegt | Userdienst inactive/dead; Default `tb-config/src/stt.rs:38` auf 8791, dort anderer Prozess und HTTP 401. | Vorhandenen Rust-Dienst und Client nutzen, aber keinen erfolgreichen heutigen Transkriptionspfad behaupten. Konfigurierten abweichenden Port nicht gelesen. |

WIRKUNGSPRUEFUNG[WP-1]: 10 Befunde | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 12/12 geprüft

Diese Pflichtzeile bezeichnet die zehn statisch geprüften Backendbefunde des Recherche-Workers und zwölf Schnittstellengruppen. Die zusätzlichen Admin-/STT-Beobachtungen oben sind ergänzend. Es gab keine neue Fremd-API-Funktionsprüfung, keinen Upload und keinen separaten Review-Thread.

## 7. Branches, Worktrees und Aufgabenakten

Die Abstandszahlen wurden gegen `e0b0dbaf` beziehungsweise das zu diesem Zeitpunkt identische `origin/main` erhoben. `hinter/voraus` zählt Commits, nicht fehlende Produktfunktionen. Der Remote-Main wurde während der Inventur durch die laufenden Fixaufträge weiterbewegt; das ändert die Codebasis dieses Berichts nicht.

| Bestand | Erhobener Stand | Klasse / Empfehlung |
| --- | --- | --- |
| `feat/highlight-erkennung-vod` | Lokaler Branch und gleichnamiger Remote-Ref nicht mehr gefunden; Merge `a05a61e1` ist Vorfahr, Exit 0. `~/.worktrees/tb-highlight-erkennung` fehlt. | Bereits integriert und aufgeräumt; kein liegengelassener Worktree. |
| `feat/clip-aufbereitung-social` | Ref nicht mehr gefunden; Merge `0608d088` ist Vorfahr, Exit 0. | Bereits integriert; aktuelle Teile R13 bis R20 prüfen statt alten Branch wiederbauen. |
| `feat/clip-social-format-20260929` | `1922db82`, 398/8; 39 geänderte Dateien im historischen Three-dot-Diff. `git cherry`: sieben von acht Nicht-Mergecommits patchgleich übernommen. | doppelt als alte Integrationsträger; übrigen Patch abgleichen, nicht blind mergen oder löschen. |
| `feature/social-media-pipeline-teststand` und `sicherung/tb-social-pipeline` | Beide `5176332d`, 1.717/1, 21 Dateien, +2.797/-825 im damaligen Diff; kein eigener aktueller Worktree gefunden. | doppelte Refs derselben WIP-Sicherung; einzigartige Änderungen vor Entfernung sichern/klassifizieren. Nicht als vollständig Schrott bewiesen. |
| `feat/titel-studio-costream` | `db7d73e9`, 663/10; mehrere Patches bereits übernommen, zwei nicht patchgleich. | Alter teilweise übernommener Stand; vorhandenes Titel-Studio bleibt. Verbleibende Review-Fixes abgleichen, kein pauschaler Neumergeschluss. |
| `feat/uplink-cast-studio-20260912` und Codex-Nachfolger | Ursprungsref `1269ad60`, 1.176/1; Nachfolger `ed2e994f`, 141/6. Nachfolgerworktree vorhanden, Gate-Fixer laut Akte nicht verfügbar. | gebaut, unverdrahtet beziehungsweise blockierter Nebenauftrag; kein bestätigter Social-Agent-Ersatz. Fremden Worktree erhalten. |
| `docs/social-golive-check` | `923b0024`, 2/1; ein Bericht mit 241 Zeilen. | Vorhandener Fachbericht, behalten; kein Produktcode. |
| `fix/social-token-ablauf` | Eigenständiger Worktree `~/.worktrees/tb-social-token-ablauf`; während Erhebung von `4305407d` auf `31044228` fortgeschrieben. | **Laufender Auftrag laut Briefing**, kein Schrottkandidat, nichts verändern. |
| `fix/social-golive-upload-texte` | Worktree `~/.worktrees/tb-social-golive-upload`; von `ac08dba4` auf `07f511a3` fortgeschrieben. Letzter beobachteter `origin/main` zeigte anschließend auf `07f511a3`. | Laufender beziehungsweise während Inventur integrierter Auftrag; keine Cleanup-Empfehlung. Bericht bleibt e0b0dbaf-gebunden. |
| `feat/social-tiktok-direct-post` | `f637d138`, bei Erhebung 0/2; Worktree `~/.worktrees/tb-social-tiktok-direct`. | **Laufender Auftrag**, nicht als fehlgeschlagener Altpfad werten. |

Keiner dieser fremden Branches oder Worktrees wurde verändert. Der gemeinsame Checkout hatte beim Kontrollblick 305 Statuszeilen und wurde ebenfalls erhalten. Branchlöschung verlangt später eigene Ancestry-/Patchprüfung und eine gesicherte Kopie einzigartiger Arbeit.

### Aufgabenakten: aktueller Stand gegenüber alten Meldungen

| Akte | Beleg und Einordnung |
| --- | --- |
| `.tasks/2026-09-06-clip-aufbereitung-social/` und `2026-09-06-highlight-erkennung-vod/` | Code später integriert; Highlight-Loop dennoch aktuell aus. Kalibrierung/Korpus sind historische Nachweise, keine laufende Agentenabnahme. |
| `.tasks/2026-09-21-social-studio-handoff/` | 51 Dateien, 1.563.910 Bytes, 17 JS-/TS-/Python-Quell-/Testdateien. README `:65` dokumentiert damalige fehlende Integration; heutiger App-Mount widerlegt, dass das ganze Studio noch unverdrahtet sei. |
| `.tasks/2026-09-21-social-studio/` | 12 Dateien, 708.601 Bytes. EVIDENCE enthält frühere BLOCK-/Wartezustände. Commits `83756fb5` und `2c4f4c46` inzwischen Vorfahren von e0b0dbaf, Exit 0. |
| `.tasks/2026-09-22-social-dashboard-shell-size/` | 17 Dateien, 1.691.239 Bytes. Commits `29dd9c2d` und `f3ec187d` inzwischen Vorfahren, Exit 0; Sondernavigation später zurückgebaut. Nachweise behalten, alten „noch nicht integriert“-Text nicht als Ist-Zustand übernehmen. |
| `.tasks/2026-09-29-clip-social-format/` und `2026-10-01-social-media-ids-kopfzeile/` | Formatakte 14 Dateien, 16.375.020 Bytes, keine zusätzliche Quelldatei; ID-Akte 13 Dateien, 649.422 Bytes. ID-Migration produktiv erfolgreich, heutige UI ID-gebunden; alter Koordinationshold ist kein aktueller Abschaltbeleg. |

Ein Verzeichnis mit dem abweichenden Präfix `20260922-*` und Social-Bezug wurde im fixierten Aufgabenbestand nicht gefunden. Historische Register werden nicht nachträglich als aktuelle Betriebsregister ausgegeben.

## 8. Bausteinkarte für einen Clip-Analyse-Agenten

Bestand und Lücken, keine neue Agentenarchitektur. Reifegrad meint hier: **eingebundene Produktfunktion**, **funktionaler, aber abgeschalteter Bestand**, **heuristischer Prototyp** oder **unverdrahteter vorbereiteter Code**. Messwerte sind mit ihrer Aussagegrenze aufgeführt.

| Bedarf | Vorhandener Baustein / Reifegrad | Messwerte und Grenzen | Fehlender Bestand oder Nachweis |
| --- | --- | --- | --- |
| Clips und VOD-Momente finden | Helix, Repository, `clip_context.rs:65`, `tb-highlight/twitch_vod.rs:65`; Produktanschlüsse vorhanden. | 206 Social-Clips, 114 gesetzte Downloadzeiten; kein aktueller Dateiexistenz-/Abspieltest. `!clip` erzeugt Commandevents, keine automatische Social-Registrierung. | Einheitlicher Eingang von Command-/Highlight-Ergebnissen in vorhandenen Bestand nicht belegt. |
| VOD-Material nutzen | `tb-vod-archive` aktiv; separates Archiv funktionaler Zwilling. | 82 VODzeilen, 80 Teile, aktuelle Aktivität. Doppelte Downloads derselben VOD-ID sind möglich, nicht als tatsächlich passiert gemessen. | Gemeinsamer Katalog/Dateifund zwischen den beiden Archiven fehlt. |
| Killfeed, Souls, Objectives | Rust `clip_context_harvest.rs:339`; Python `ocr.py:35`, `:44`, `:54`, `signals.py:117`; funktionale Heuristiken, Ernte aus. | OCR-Crops/Tesseract, Fuzzy-Spielerabgleich; kein strukturierter Ereignisbesitzer. Soulsparser entfernt Nichtziffern, Zahlensprünge können OCR-Artefakte sein. | Belastbare Genauigkeit nach HUD, Auflösung, Sprache und Streamer fehlt. |
| Pegel, Sprache, Lachen, Ausruf | Rust `clip_context.rs:4`, Harvest; Python `audio.py:13`, `:41`; Messlogik vorhanden. | LUFS, Peak-dBFS und RMS; RMS umfasst Gameplay und Stimme. Python-Lachen ist Textwortsuche, kein akustischer Klassifikator. | Sprecher-/Gameplaytrennung und nachgewiesene Emotionserkennung fehlen. |
| Death-Screen und Szenenwechsel | Rust-Kontext plus Python `signals.py:87`; schwache heuristische Signale. | Death-Screen aus Sättigungsabfall unter 55 % des Medians; erkennt allgemeine Entsättigung, nicht sicher eigenen Tod. OCR nur in Stichproben alle drei/neun Sekunden. | Menschlich validierte Death-Screen-/Szenenwechsel-Messung fehlt. |
| Strukturierte Match-/Demoereignisse | `boon.rs:61`, `demo_analyzer.rs:39`, `event_detector.rs:37`; funktionaler, abgeschalteter Bestand. | Killzahl, HP-Anteil, Comboabilities, High-Impact, Clutch, Excitement, Eventzeit und Vorlauf vorhanden. | Spielerzuordnung und VOD-Zeitausrichtung abnehmen; Relay-/Retry-Übergabe nicht abgeschlossen. |
| Highlightqualität messen | Python `messen.py:41`, historische Kalibrierung und `korpus-report.md:3`; heuristischer Eval-Bestand. | Historisch 241 Clips verarbeitet: RMS 72 %, Souls-Sprung 27 %, Death-Screen 18 %, Objective 12 %, eigener Kill 6 %. Signalprävalenz, keine Präzision. Bei Schwelle 0,5 wurden vier von vier vorhandenen Zuschauerclips getroffen, daneben 18 Kandidaten ohne Clip; bei 0,55 drei von vier und zehn ohne Clip. | Winzige Referenzstichprobe, kein unabhängiger Voll-VOD-Test. „Ohne Zuschauerclip“ ist nicht menschlich bestätigt falsch-positiv. |
| Schnittbeginn/-ende bestimmen | `clip_context.rs:116`, `:263`, ältere Trimwrapper und Batchrenderer. | Gewichteter Peak, Reaktionsnachlauf, gelernter Vor-/Nachlauf; ein Kontextlauf mit 180 Sekunden und ein Template mit `sample_count=1`. | Keine kalibrierte Confidence; Highlightworker ignoriert berechneten Vorlauf und nimmt sechs Sekunden. |
| Hochkant und Facecam schneiden | Social-Layout/Render/VideoProcessor produktiv eingebunden; Uplink `portrait.rs:42`, `graph.rs:858` mit Dienstaufrufer. | PiP, Stacked, Crop, Blur-Pad; Uplink prüft Grenzen, Überläufe und gerade YUV420-Geometrie. | Keine automatische Gesichtserkennung, Motivverfolgung oder intelligente Layoutwahl belegt. Uplink-Primitiven sind kein fertiger Offlineeditor. |
| Untertitel und Effekte | ASS-Cues/Branding sowie FFmpeg-Compositing vorhanden; Branding aktiv, Sprachcues aus. | Cues maximal fünf Wörter, zwei Zeilen, 28 Zeichen, 0,8 bis 2,5 Sekunden. | Wortzeitstempel fehlen. Bestand zeigt Branding, Blur und Layout, keinen allgemein belegten semantischen Effekt-/Zoom-/B-Roll-Planer. |
| Vorschau, Freigabe, Veröffentlichung | Preview, manuelle Editoren, Approval, Scheduler und Uploadworker eingebunden. | On-demand Claims mit pending/rendering/ready/error; erneute Approvalprüfung vor Upload; persistente TikTok-Unsicherheit. | Keine freigegebene vollständige Plattformabnahme aus dieser Inventur; Highlightübergabe fehlt, Alt-Batchvertrag uneinheitlich. |
| Titel und LLM-Zugang | Titel-Studio `title.rs:462`, Clip-Prompt/Parser deaktiviert, `tb-llm/hub.rs:74` aktiv für Text. | `/twitch/titel` ist die Seite; Vorschlag-POST nimmt `keywords`, `include_live`, `streamer` mit Twitch-ID. Liefert Primär-/Alternativtitel. | Keine Clip-ID-/Transkript- oder Video/Bildschnittstelle. `title.rs:423` kann bei gespeicherter Einstellung den Twitch-Kanaltitel setzen; keine nebenwirkungsfreie Clip-Titel-API. |
| Lokales STT | Rustserver mit whisper.cpp/whisper-rs und Silero-VAD, lokaler Rustclient; derzeit nicht laufend belegt. | Ein Inferenzlauf gleichzeitig; Segmentzeiten, No-Speech-/Logprobfilter. Serverconfig bestimmt Modell/Sprache, Requestfelder wechseln sie nicht. Vorhandener Kontextlauf hatte STT-Timeout. | Aktiven Listener und Modellbereitschaft nachweisen; Defaultport aktuell anderweitig belegt, keine Wortzeiten. Keine Nutzeraudios zur Prüfung versandt. |
| Leistung als Feedback | Vorhandene Analytics, Insights und Reports. | Drei Messzeilen, YouTube-Basisstatistik; TikTok-Analytics nicht implementiert. Reportscore benutzt Views, Likes, Kommentare, Shares und Engagement. | Bucket-Doppelzählung und Fehlerbehandlung korrigieren; keine belastbare Retention-/Watchtime-Lernbasis über mehrere Plattformen. |

**Weitere gefundene Quellen, kein unmittelbarer Social-Ersatz:** `ai-coach/packages/detectors/.../fight_outcome.py:164` wertet strukturierte Matchdaten aus, kein Video-OCR. Wenn dessen Botfunktion übernommen wird: nach Rust portieren, dann Python entfernen. `Deadlock-Bots/rust/crates/dl-voice/src/scrim_record/audio.rs:323` hat Downmix/WAV-Aufnahme für Discord, mit anderem Berechtigungs- und Quellenkontext. `dachlock-media-lab/analyze_match_timeline.py:26` und `check_stt.py:2` sind einzelne Diagnose-/Auswertungsskripte: **Hilfsskript, darf bleiben**. Diese Quellen sind nicht zusätzlich in der Softwareklassenzählung enthalten.

### Datenlücken für den späteren Agenten

1. **Keine belastbare Ground Truth:** Zuschauerclips sind selektive Referenzen. Es fehlen menschlich bewertete gute/schlechte Momente samt richtiger Schnittgrenzen und unabhängiger Voll-VOD-Messung.
2. **Kein durchgängiger aktueller Analysepfad:** Social-STT, neue Kontext-Ernte und Highlightloop sind aus beziehungsweise inaktiv. Elf transcribing-Zustände bedeuten keine laufenden Jobs.
3. **Keine semantische Video- und Wortzeitbasis:** Zentraler Text-LLM ist vorhanden; direkte Video-/Bildanalyse, Worttimestamps, sichere Gesichtserkennung und Effektentscheidungen wurden nicht als fertiger Bestand gefunden.
4. **Messwerte noch keine Lernziele:** Ein Kontextlauf und drei Plattformzeilen reichen nicht als belastbare Qualitätsmessung. Kumulative Buckets, fehlende TikTok-Messung und unsichere Postzustände sind vorher zu unterscheiden.

## 9. Aufräumvorschlag

Reihenfolge nach Risiko und Wiederverwendung, nicht nach dem Wunsch nach möglichst vielen Löschungen. Größe bezeichnet betroffene Fläche, keine bereits freigegebene Umsetzung. **S:** wenige Dateien; **M:** ein Fachmodul/Crate; **L:** mehrere Pfade oder Port mit Daten-/Evalvertrag.

### Jetzt

| Reihenfolge | Vorschlag | Größe | Voraussetzung |
| --- | --- | --- | --- |
| 1 | Admin-Freigabevertrag korrigieren und überholte Runtime-/Startup-Aussagen entfernen. | S, etwa drei Adminvertragsdateien plus wenige Dokustellen. | Aktuellen Main berücksichtigen; bestehendes Studio und Adminalias erhalten. |
| 2 | Unaufgerufenen Redirectstub und doppelten `ensure_schema` entfernen. | S, ein Stub plus eine Schemakomponente. | Aufruferprüfung gegen dann aktuellen Stand; Migrationen bleiben. |
| 3 | Clipdownload und abgeleitete Dateiretention vereinheitlichen. | M, Upload-, Prep- und Retentionmodule. | Laufenden Uploadfix nicht duplizieren; Dateien nicht allein nach Alter löschen. |
| 4 | Alt-API-Familien und mockbasierte Referenzoberfläche auf konkrete Aufrufer prüfen; nicht benötigte Wrapper/zweite Oberfläche entfernen. | M, zehn API-Pfadfamilien, Clientwrapper, Referenzpaket 51 Dateien/1,56 MB. | Externe Aufrufer nicht gemessen; Quellen-/Design-/Prüfnachweise kompakt erhalten. |
| 5 | VOD-Archive abgleichen; Botkatalog als verbleibenden Botpfad setzen und doppelte Sicherungsrefs nach Patchprüfung bereinigen. | M bis L, Botarchiv plus 1.824-Zeilen-Nebenrepo; mehrere alte Refs. | Einzigartige Standalone-/WIP-Funktionen sichern. Keine fremden Worktrees ungefragt entfernen. |

### Später, vor Clip-Agent-Nutzung

| Reihenfolge | Vorschlag | Größe | Voraussetzung |
| --- | --- | --- | --- |
| 6 | Highlight-Übergabe, Vorlauf und bestätigten Abschluss sowie Einzel-/Batchfreigabeverträge vervollständigen. | M, vorhandene Highlight-/Approval-/Queuepfade. | Keine Reaktivierung des deaktivierten Workers als Nebenwirkung. |
| 7 | Analytics-Buckets und Fehler-/Fallbacknachweise korrigieren. | M, Insights, Analytics und Reports. | Erst danach Plattformwerte als Qualitätsfeedback verwenden. |
| 8 | STT-Dienstbereitschaft und Portbelegung klären; latenten Enrichment-Persistenzfehler vor späterer bewusster Verwendung korrigieren. | S bis M, Listenerkonfiguration und bestehende Pipeline. | Keine automatische Aufhebung des Abschaltvertrags. |
| 9 | Historische Evaldaten sichern und Messung mit belastbaren Referenzen nachziehen. | L, Daten-/Bewertungsarbeit statt neuer Parallelservice. | Vorhandene 205 Merkmalsclips, historische 241-Clip-Auswertung und 22 Kandidaten nicht als echte Negativ-/Positivlabels missverstehen. |

### Nicht-Rust im Dauerbetrieb raus

Die letzte Präzisierung des Auftraggebers ist maßgeblich: **Bots, Worker und dauerhaft laufende produktive Dienste müssen Rust sein.** Das gilt ebenso für Node-Services und Shell-Daemons. Kurz laufende Tests, Prüfungen, Einmalskripte und kleine Timer-Skripte mit wenig Arbeit dürfen bleiben, auch in Python. Ein Shellstarter mit abschließendem `exec` auf ein Rustbinary ist kein zweiter Shell-Daemon. Statisch ausgelieferte React-/TypeScript-Bundles sind kein Node-Serverprozess.

#### Erhobene Prozesse und Units

| Bestand / Prozess oder Unit | Erhobener Zustand / Fundstelle | Entscheidung |
| --- | --- | --- |
| Bot und Social-Loops, `deadlock-twitch-bot-rust.service`, Prozess `tb-bot` | Systemdienst active/running; `rust/scripts/run_tb_bot_service.sh:159` ersetzt den Launcher per `exec`. | Bereits Rust; behalten. Keine Social-Python-/Node-Runtime im laufenden Botpfad gefunden. |
| Dashboard, `deadlock-twitch-dashboard-rust.service`, Prozess `tb-dashboard` | Systemdienst active/running; Rust-API und statisches Haupt-/Adminbundle. | Bereits Rust; Frontendquellen nicht wegen TypeScript als Serverdienst zum Port erklären. |
| Stream-Coaching und Collector, `deadlock-twitch-stream-coaching-watch.service` / `tb-stream-audit`, `tb-category-collector.service` / `tb-category-col` | Beide active/running; benachbarter Botbetrieb. | Bereits Rust; kein Nicht-Rust-Portauftrag. |
| Uplink/Relay, Userunit `rs-relay.service`, Prozess `uplink-service` | active/running. Uplink-Hochkant hat echten Dienstaufrufer. | Bereits Rust; behalten, Clip-/Livezwecke getrennt ausweisen. |
| Lokales STT, Userunit `deadlock-stt-server.service` | Installiert, inactive/dead; `ops/stt-server/deadlock-stt-server.service:8` startet Rust. | Bereits Rust; Betriebsbereitschaft offen, kein Python-Port nötig. |
| Eigenständiges Archiv, Systemunit `vod-archive.service` | Installiert, inactive/dead; Nebenrepo ist Rust. | Rust-Zwilling N02 zusammenführen; kein Nicht-Rust-Dauerprozess. |
| Gold-Preview, Userunit `tb-gold-glanz-preview.service`, Prozess `node` | active/running; Vite-Preview auf Port 4187 aus einem Previewworktree, nicht die produktive Social-Auslieferung. | Liegengebliebene temporäre Prüfoberfläche, kein produktiver Botdienst. Bei freigegebener Bereinigung beenden statt Vite nach Rust neu zu bauen. Nicht gestoppt. |
| Python-Highlight-Detektor, `ops/highlight-detector/highlight-detector:3` | Eigene Analyse-/Mess-CLI; kein passender aktiver Prozess oder Worker-/Timeranschluss gefunden. | Kein aktueller Dauerbetrieb belegt. Kurz laufende Kalibrierungs-/Prüfaufrufe dürfen bleiben; vor Übernahme als Botworker Funktion nach Rust portieren, dann Nicht-Rust-Runtime entfernen. |
| Watchdog und kleine Betriebsjobs | `deadlock-twitch-bot-watchdog.service`, `deadlock-twitch-llm-usage-recover.service`, `deadlock-twitch-invite-sync.service` waren inactive/dead, Type oneshot; Timer sind periodisch. Watchdog-Quelle `ops/systemd/deadlock-twitch-bot-watchdog.service:11` startet bereits Rust. | Kleine kurz laufende Timer-/Verwaltungsaufrufe dürfen bleiben, auch Python oder Shell. Kein pauschales Portieren wegen Timerbetrieb. |
| FFmpeg, yt-dlp und Shellstarter im Medienpfad | Pro Medienauftrag gestartete Werkzeuge beziehungsweise `exec`-Launcher; kein zusätzlicher Social-Daemon daraus nachgewiesen. | Jobwerkzeuge von eigenständigem dauerhaftem Botworker unterscheiden. Keine Rust-Neuimplementierung externer Medienwerkzeuge aus dieser Inventur ableiten. |

Die Prozessnamen wurden ohne Kommandoargumente oder Prozessumgebungen erhoben. Es wurden keine Secrets für diesen Abgleich gelesen. Der belegte aktive Nicht-Rust-Prozess im untersuchten Umfeld ist die temporäre Node-Preview, nicht ein produktiver Social-Botservice. Für andere Hostprojekte wurde keine umfassende Dauerprozessinventur behauptet.

#### Reihenfolge und Größe

| Reihenfolge | Bestand und Entscheidung | Größe |
| --- | --- | --- |
| NR-1 | **Bereits erledigt:** Social-Python nicht neu portieren. `cbcfeca2` entfernte Runtime, `dfaed810` HTML-Kopien. Alte Start-/Fallbackanweisungen entfernen, keine Python-Social-Runtime reaktivieren. | 44 Runtime-Dateien/13.404 Zeilen und drei HTML-Dateien/1.167 Zeilen historisch entfernt; Dokumentkorrektur S. |
| NR-2 | **Durch Rust ersetzte Nebenrepo-Runtime:** `legacy-python/vod_archive.py` und `yt_auth.py` entfernen; alte Kontodateipfade nicht reaktivieren. Temporäre Node-Preview bei späterer erlaubter Bereinigung beenden. | S, zwei Dateien/714 Zeilen und eine Previewunit. Keine Aktion in dieser Inventur. |
| NR-3 | **Vor Einsatz als dauerhafter Botworker:** nicht ersetzte Teile des Python-Detektors in vorhandene Rust-Analyse-/Score-/Persistenzbausteine portieren und dann die Nicht-Rust-Runtime entfernen. Kurz laufenden Eval-/Testbestand getrennt behalten. | L, maximal etwa 1.356 Quellzeilen in 14 nichtleeren Modulen plus 172 Testzeilen; tatsächliche Runtime-/Evalgrenze vor einem Port festlegen. |
| NR-4 | **Darf bleiben:** kleine Python-Timer-Skripte, einzelne Bilder-, Diagnose-, Verwaltungs-, Prüf- und Testskripte. Der Referenz-Browsertest darf Test bleiben. Keine dauerhaft laufende Botfunktion unter dem Namen „Hilfsskript“ verstecken. | Bilderhilfen etwa 181 Zeilen, Referenztest 136 Zeilen; weitere Miniwerkzeuge außerhalb des Social-Scope separat prüfen. |

Keine der genannten Bereinigungen, Portierungen, Tabellenänderungen oder Dienstaktionen wurde durchgeführt. Das Ergebnis ist ein gesicherter Bericht auf einem Dokumentationsbranch, kein Main-Merge.

## Nachweiszeilen

TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 6 belegt | Senke: .tasks/2026-10-07-social-media-inventar/INVENTAR.md

MERGEPROTOKOLL[MS-1]: 7 Git-Schritte einzeln | Anläufe: 1 | Gate: kein Main-Merge beauftragt; vorhandene Commit-/Push-Hooks bleiben wirksam

Die sieben schreibenden Git-Einzelschritte für den Berichtabschluss sind Fetch, detached Worktree, Berichtbranch, gezieltes Add, Dokumentationscommit, Berichtbranch-Push und Entfernung des eigenen Worktrees. Diese Zeile beschreibt den vorgesehenen Abschluss; Commit, Push, Remote-Sicherung und Cleanup werden anschließend anhand der Werkzeugantworten in der Übergabe bestätigt. Lesende Git-Abfragen sind nicht mitgezählt.
