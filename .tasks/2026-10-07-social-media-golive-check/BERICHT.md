# Social-Media-Dashboard: Go-Live-Prüfung

Prüftag: 7. Oktober 2026. DB-Snapshots unter anderem um 05:35 und 05:43 Uhr CEST. Auftrag des Haupt-Orchestrators `d3a1741e-82bc-4a48-865b-2845c663dca7`.

## Ergebnis

**Für die vollständige Partner-Freischaltung noch nicht bereit.** Das Dashboard hat einen umfangreichen Rust-Unterbau. Die wichtigste Unterbrechung liegt in der Clip-Anreicherung: 49 fehlgeschlagene Einträge, 11 seit September festhängende Einträge und kein fertiges Enrichment. Dazu kommen zwei konkrete Fehler bei „Neu generieren“ und eine fehlende Rückmeldung für Uploads ohne nutzbare Verbindung.

TikToks App-Zulassung wird entsprechend dem Auftrag als gegeben behandelt. Sie schaltet den vorhandenen Upload-Code aber nicht auf Direct Post um: Die produktive Fabrik verwendet weiterhin das TikTok-Postfach. TikTok-Insights sind ausdrücklich nicht implementiert. YouTube hat historische Upload-Erfolge; die öffentliche Sichtbarkeit neuer Shorts und die Projektfreigabe sind damit nicht belegt. Für Instagram fehlt der produktive Verbindungs- und Upload-Nachweis.

Eine eingegrenzte Beta ist nach Behebung der technischen Blocker vertretbar: TikTok ausdrücklich als Postfach-Übergabe, YouTube mit nachgewiesener Sichtbarkeit, Instagram und TikTok-Insights bis zur Freigabe ohne Funktionsversprechen.

### Prüfgrundlage und Grenzen

- Codebasis: detached Worktree von `origin/main`, SHA `67786ba2c0b740f964a337dd0d75c19e7958ef66`. Bericht auf `docs/social-golive-check`, keine Änderung am Anwendungscode.
- Laufzeit: `/proc/1184203/exe` und `/proc/1184089/exe` zeigen auf `tb-bot` und `tb-dashboard` im Release desselben SHA. Beide Systemdienste sind aktiv, gestartet am 6. Oktober gegen 20:13 Uhr CEST.
- Datenbank: `twitch_analytics`, Schema `public`, lesende Abfragen mit `default_transaction_read_only=on`. Keine verschlüsselten Zugangsfelder selektiert oder entschlüsselt. Ergebnisse im Bericht sind aggregiert.
- HTTP: öffentliche Rechtsseiten und unauthentifizierte Zugriffsprüfungen über Caddy und direkt auf Port 8769. Kein OAuth-Start, kein Callback mit echtem Code, kein Trennen, keine Freigabe, kein Upload, kein erneutes Enrichment ausgelöst. Manche authentifizierten GET-Handler legen Datensätze an, etwa `social_media.rs:2649` und `posting_plan.rs:184`; sie wurden deshalb nicht aufgerufen.
- Sichtprüfung: öffentliche Nutzungsbedingungen mit lokalem Headless-Browser geprüft. Screenshot: `/home/nathanael/.claude/sichtpruefung/tb-social-golive/terms.png`. Die T3-Browser-Automation meldete „No preview automation host“. Eine angemeldete Partneransicht stand nicht zur Verfügung. Ein bestehendes Vorschauvideo wurde im Dienst-Dateisystem mit `stat` und `ffprobe` geprüft, nicht neu gerendert. Die zusätzliche Einzelbild-Aufnahme wurde vom Worktree-Hook blockiert und nicht umgangen.

**Urteile:** „funktioniert“ bezeichnet einen belegten Teilpfad. „teilweise“ bedeutet vorhandener Backendpfad mit fehlendem Ende-zu-Ende-Nachweis, Einschränkung oder Defekt. „kaputt“ bezeichnet einen konkret belegten Fehler. „nur UI ohne Backend“ bezeichnet eine angebotene Funktion ohne ausführende Implementierung. Ein historischer Upload ist kein vollständiger Nachweis für die heutige Partnerbedienung.

**Fixgrößen:** S: bis zu einem halben Arbeitstag; M: ein bis zwei Arbeitstage; L: drei bis fünf Arbeitstage. Externe Plattformfreigaben sind nicht in diesen Größen enthalten.

BESTAND[BS-1]: teilweise | Fundort: rust/crates/tb-dashboard-api/src/handlers/social_media.rs:16 | Anknüpfung: vorhandene Rust-Handler, tb-social-media-Worker und Dashboard-SPA; kein Neubau erforderlich

WIRKUNGSPRUEFUNG[WP-1]: 8 Befunde | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 7/7 geprüft

Die sieben geprüften Pfadgruppen sind Plattform-OAuth/Refresh, TikTok-Upload/Status, YouTube-Upload/Status, Instagram-Upload/Status, Plattform-Insights, lokales STT/optionale externe Anreicherung sowie VOD-Archiv. „Geprüft“ bedeutet Code- und verfügbare Laufzeitprüfung, nicht sieben neue externe Transaktionen. Zusatzpfade für Formulare und Clip-Contest sind unten abgegrenzt.

## 1. Verbindungen

| Funktion | Urteil | Beleg | Blocker für Go-Live | Fixvorschlag mit Größe |
| --- | --- | --- | --- | --- |
| TikTok verbinden, speichern | teilweise | `rust/crates/tb-social-media/src/oauth.rs:951`: PKCE/S256, `user.info.basic,video.upload,video.publish`; `oauth.rs:170`: gespeicherter Zustand; `oauth.rs:182`: Callback. D1: ein aktiver eigener Zugang, Refresh vorhanden, nicht abgelaufen. | Ja, aktueller Bediennachweis fehlt. Die App-Zulassung selbst wird nicht infrage gestellt. | Mit freigegebenem Testkonto Callback und anschließende Postfach-Übergabe belegen, M. Keine realen Konten für diesen Audit verändern. |
| YouTube verbinden, speichern | teilweise | `oauth.rs:969`: PKCE, Offline-Zugriff, Consent, Upload und Readonly. D1: zwei aktive eigene Zugänge. | Ja für die allgemeine Freischaltung, Projektfreigabe siehe Abschnitt 6. | OAuth mit freigegebenem Testkonto dokumentieren; tatsächliche Rückgabe-Sichtbarkeit prüfen, S plus externe Freigabe. |
| Instagram verbinden, speichern | teilweise | `oauth.rs:992`: Instagram Login mit `instagram_business_basic`, `instagram_business_content_publish`, `instagram_business_manage_insights`. D1: kein Instagram-Zugang. | Ja, falls Instagram angeboten wird. | App-Review, passende professionelle Konten und den gesamten Callback belegen; bis dahin kennzeichnen oder aus der Freischaltung nehmen, M plus externe Freigabe. |
| Erneuerung TikTok und YouTube | funktioniert | `rust/crates/tb-social-media/src/refresh_worker.rs:55`, `oauth.rs:454` für die plattformspezifische Trennung. D1: TikTok zuletzt 6. Oktober 19:27 UTC; YouTube während des Audits erneut aktualisiert, zuletzt 7. Oktober 03:35 UTC im ersten Snapshot. Beide Plattformen ohne abgelaufenen aktiven Zugang. | Nein für den belegten Bestand. | Fehlermeldung und Neu-Verbinden-DM aus dem parallelen Auftrag abnehmen, nicht doppelt fixen. |
| Erneuerung Instagram | teilweise | `refresh_worker.rs:63`: Auswahl ohne Refresh-Token; `oauth.rs:454`: `ig_refresh_token` für Langzeit-Zugang. Kein produktiver Instagram-Eintrag. | Ja für Instagram-Freischaltung. | Erneuerung im freigegebenen Instagram-Testkonto nach dessen Mindestalter prüfen, S plus Wartezeit der Plattform. |
| Trennen je Plattform | teilweise | `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:3798`: ID-gebundenes `enabled=0`, globaler Zugang gesondert geschützt. Keine Provider-Revoke-Anfrage und keine Entfernung der gespeicherten Zugangsfelder. Im Audit nicht ausgelöst. | Nein für die lokale Abschaltung; Ja, falls der Text vollständigen Widerruf verspricht. | Lokale Abschaltung und Widerruf im Plattformkonto auseinanderhalten; gegebenenfalls Revoke/Datentilgung ergänzen, M. |
| Eigene Verbindung versus Sammelverbindung | teilweise | `credentials.rs:88` erlaubt Fallback; `credentials.rs:97` verlangt für Insights eigenen Zugang. `social_media.rs:3501` schützt `__global__` mit Admin-Prüfung. D1: derzeit keine Sammelverbindung. UI-Defekt B8. | Nein für den aktuellen Bestand, Ja vor Einführung einer Sammelverbindung. | Eigene Verbindung auch bei aktivem Fallback anbieten; wirkungslose kanalbezogene Trennaktion nicht als Erfolg zeigen, S. |
| Ablaufanzeige und Neu-Verbinden-DM | teilweise | `credentials.rs:206`, `bot/dashboard_v2/src/pages/SocialMedia.tsx:1945`: Ablauf und Neu-Verbinden-Zustand vorhanden. Auftrag `2026-10-07-social-media-token-ablauf` läuft parallel. | Separates Abnahmekriterium. | Ergebnis dieses Auftrags vor Freischaltung übernehmen; hier keine Änderung. |
| Weitere Plattformen | teilweise | `rust/crates/tb-social-media/src/credentials.rs:19`: Social-Media-Plattformen sind TikTok, YouTube und Instagram. `handlers/plattform_oauth.rs` behandelt zusätzlich Kick/YouTube für andere Produktpfade, nicht diesen Clip-Uploader. | Nein. | Keine Kick-, X- oder Facebook-Clip-Veröffentlichung für dieses Dashboard versprechen. Kein neuer Baustein nötig. |

## 2. Clip-Pipeline

| Funktion | Urteil | Beleg | Blocker für Go-Live | Fixvorschlag mit Größe |
| --- | --- | --- | --- | --- |
| Automatische Twitch-Clip-Erfassung | funktioniert | `rust/crates/tb-social-media/src/clip/task.rs:1`; Journal L1: Lauf am 7. Oktober 00:15 bis 00:18 UTC, 68 Partner versucht, 65 erfolgreich, 118 Clips, davon 7 neu, 3 Fehler. D2: 198 Clipdatensätze. | Nein, die drei Fehler sind separat zu überwachen. | Fehler je betroffenem Kanal diagnostisch sichtbar halten, S. |
| Manuell Clips holen | teilweise | `social_media.rs:1470`: Sessionziel und Freigabe vor Helix-Aufruf; Mengenbegrenzung. `social_media.rs:1465`: `days` wird nicht verwendet. Kein schreibender Live-Aufruf im Audit. | Nein, sofern kein frei wählbares Zeitfenster versprochen wird. | UI-Vertrag und tatsächliches Erfassungsfenster abstimmen; eigener Testkanal für Bediennachweis, S. |
| Manuelle Videodatei hochladen | teilweise | `social_media.rs:1010`: Multipart-Pfad mit Sessionziel; `bot/dashboard_v2/src/api/socialMedia.ts:529`: Behandlung unter anderem von 415. Im Audit keine Datei in Prod eingestellt. | Bediennachweis offen. | Erlaubten Dateityp, Größen-/Dauergrenzen, Duplikat und Fehler mit synthetischer Datei auf Testumgebung prüfen, S. |
| Download und gemeinsame Dateiablage | funktioniert | `clip_prep_worker.rs:60`: atomarer Download; `clip_prep_worker.rs:147`: Auswahl und sechs Stunden Abstand nach Downloadfehler. D2/F1: 109 lokale Referenzen, 109 Dateien im Dateisystem des Bot-Dienstes vorhanden. | Nein. | Bestehende gemeinsame Medienablage weiterverwenden. Host-Checkout-Dateien nicht mit Dienst-Bind-Mounts verwechseln. |
| Facecam, Hochformat und Layout | teilweise | `render.rs:41` lädt gespeichertes Layout; `render.rs:99` rendert es. F1: vorhandene Vorschau mit H.264, AAC, 1080×1920 und 30 Sekunden. Layouteditor hat Bildausschnitte, kein eigener Twitch-Iframe. | Keine belegte technische Sperre; vollständige Bedien-Sichtprüfung offen. | Mit Partner-Testsession Layout speichern, Vorschau und resultierenden Upload zusammen abnehmen, S. |
| Untertitel, Sprachkorrektur und automatisch erzeugte Metadaten | kaputt | D3: 49 `failed`, 11 alte `transcribing`, kein `done`. B1 bis B3. `transcription.rs:14` nutzt den alten Env-Client; `render.rs:58` baut Untertitel aus vorhandenen Segmenten. | Ja. | Lokalen Rust-STT-Pfad korrekt verdrahten, bestehende Einwilligungsgrenze erhalten, abgebrochene Arbeit wiederaufnehmen und Retry fair machen, L. |
| „Neu generieren“ | kaputt | B4: Handler `social_media.rs:2691` übergibt keinen Transcriber, Pipeline `enrich_pipeline.rs:240` überschreibt das Transkript trotzdem. B5: API liefert Wrapper, Client erwartet nacktes Enrichment und setzt diesen Wrapper in den Editor-Cache. | Ja. | Vorhandenes Transkript beim Metadaten-Neulauf erhalten; kompatiblen Antwortvertrag und Fehlerbehandlung herstellen, M. |
| Metadaten manuell bearbeiten und speichern | teilweise | `social_media.rs:2544`, `social_media.rs:2623`: Speicherung und nackte Enrichment-Antwort passen zu `api/socialMedia.ts:281`. Sessionbesitz wird geprüft. Im Audit nicht verändert. | Nein unabhängig von B4/B5, Bediennachweis offen. | Eigene Testdaten speichern und erneut laden, S. |
| Hochformat-Vorschau und MP4-Auslieferung | funktioniert | `preview.rs:196`: Renderziel; `preview.rs:64`: Remap alter Releasepfade. F1: die vorhandene Vorschau ist über diesen Remap erreichbar. `social_media.rs:4012` prüft Besitzer, `social_media.rs:4074` unterstützt Range. | Nein für den nachgewiesenen Dateipfad; Partner-HTTP-Bediennachweis offen. | Vorhandenen Remap beibehalten; neu angeforderte Vorschau in Testsession prüfen, S. |
| Freigabe, Ablehnen und geplante Posts stoppen | teilweise | `approval.rs:461`: Entscheidung; `approval.rs:762`: Sperre und Behandlung laufender Uploads. D4: drei freigegebene und 100 wartende Datensätze. Deadlock-Zufluss hängt an erfolgreicher Anreicherung, `approval_worker.rs:40`. | Ja für die automatische Deadlock-Pipeline wegen B1 bis B3. | Nach Pipeline-Fix manuelle und automatische Entscheidung sowie Stoppen vor/nach Uploadbeginn belegen, M. |
| Zeitplan, Zeitzone, Kadenz, Kategorien und Reihenfolge | teilweise | `social_media.rs:3200`, `social_media.rs:3317`; `scheduler.rs:44`: Kadenz; `approval.rs:312`: faire Rotation beim Nachreihen; `clip_queue.rs:221`: fällige Jobs, anschließend Priorität und Erstellungszeit. D5: aktive TikTok-Kadenz 4/Woche und YouTube 7/Woche. D4: kein wartender Upload. | Ja für das Versprechen eines laufenden Autopiloten. Planungscode ist vorhanden, heutige Wirkung durch Pipelineblockade nicht belegt. | Termin und tatsächliche Übertragung mit Testclip verbinden; TikTok-Termin als Postfach-Übergabe bezeichnen. Keine freie Drag-and-drop-Reihenfolge versprechen, M. |
| TikTok-Upload | teilweise | `upload_worker.rs:63` baut `TikTokUploader::new`; `uploaders/tiktok.rs:124` startet im Inbox-Modus. `tiktok.rs:258` verwendet Inbox-Init. D4: drei historische fehlgeschlagene Direct Posts, kein produktiver Inbox-Erfolg. | Ja bis zum aktuellen Postfach-Nachweis, nicht wegen der alten App-Zulassung. | Ein freigegebenes Testvideo ins Postfach übertragen und dort kontrollieren. Caption-Einschränkung beachten: Inbox-Body in `tiktok.rs:278` enthält keine Titel-/Beschreibungsmetadaten, M. |
| YouTube-Shorts-Upload | teilweise | `uploaders/youtube.rs:605`: resumierbarer Upload; `youtube.rs:768`: `public` und `selfDeclaredMadeForKids=false`. D4: ein historisch abgeschlossener Clipupload. | Ja für zugesagte öffentliche Veröffentlichung, Audit/Sichtbarkeit nicht nachgewiesen. | Freigabe des API-Projekts und Sichtbarkeit des resultierenden Testvideos nachweisen; tatsächliche Sichtbarkeit zurückmelden, M. |
| Instagram-Reels-Upload | teilweise | `uploaders/instagram.rs:474`: Dateiweg mit Resumable-Container, Transfer, Status und Veröffentlichung; API `graph.instagram.com/v23.0`. D1/D4: kein Zugang und kein Upload. | Ja, falls Instagram angeboten wird. | Professionelles Testkonto, App-Review, Container bis `FINISHED` und resultierendes Reel belegen, M plus Plattformfreigabe. |
| TikTok-Inbox-/Publish-Status und Duplikatschutz | teilweise | `upload_worker.rs:478` wartet auf Status; `upload_worker.rs:754` prüft Inbox-Einträge weiter; `clip_queue.rs:89` vermeidet zweite Inbox-Übertragung. `SocialMedia.tsx:2384` unterscheidet Postfach und unbestätigte Übertragung. Kein solcher Prod-Eintrag vorhanden. | Ja bis zum TikTok-Bediennachweis. | `SEND_TO_USER_INBOX`, `PUBLISH_COMPLETE`, Fehler und unklaren Zustand kontrolliert prüfen, M. |
| Upload-Retry und Backoff | teilweise | `upload_worker.rs:534`: fünf Versuche, gesondert 30 Kontingent-Vertagungen; `upload_worker.rs:901`: 24 Stunden bei Quota, 15 Minuten Transport, 30 Minuten fehlender Anmeldung. TikTok-Checkpoint schützt unklare Transfers. Aktuelle Prod-Queue hat keine laufenden Retries. | Nein für den vorhandenen Mechanismus; B6 bleibt eigenständiger Blocker. | Fehlerfälle auf Testumgebung nachweisen; keine pauschale Wiederholung unklarer TikTok-Übertragungen, M. |
| Queue ohne nutzbare Verbindung | kaputt | B6: `upload_worker.rs:856` nimmt einen Job bei fehlendem Uploader nicht in den Batch auf und aktualisiert dessen Zustand nicht. `social_media.rs:3405` lässt Auto-Posting ohne Verbindungsprüfung speichern. | Ja. | Sichtbaren Wartezustand mit konkretem nächsten Schritt setzen; Verfügbarkeit serverseitig prüfen, M. |

## 3. Analytics, Rechte und Zusatzfunktionen

| Funktion | Urteil | Beleg | Blocker für Go-Live | Fixvorschlag mit Größe |
| --- | --- | --- | --- | --- |
| TikTok-Insights | nur UI ohne Backend | `uploaders/tiktok.rs:503` gibt ausdrücklich `NotImplemented` zurück. `video.list` fehlt im OAuth-Scope. Echte Video-ID muss erst aus dem Publish-Status gewonnen werden. B7. | Ja, falls als funktionierende TikTok-Analyse angekündigt. | Für Beta ausdrücklich „noch nicht verfügbar“; Display-API-Freigabe, OAuth-Scope und echte ID ergänzen, L plus Plattformfreigabe. |
| YouTube-Insights | teilweise | `uploaders/youtube.rs:985` liest Data-API-Statistiken. D6: drei Snapshots für 24h/7d/30d, jeweils 301 Views, zuletzt 3. Oktober. OAuth fragt keine YouTube Analytics API an. | Nein für Views/Likes/Kommentare; Ja für zugesagte Watchtime, CTR, Reichweite oder Zeitfenster-Analysen. | Unterstützte Kennzahlen klar begrenzen. Die Buckets sind Ablesezeitpunkte, keine belegten Zeitraum-Differenzen. Erweiterung erst mit passendem Scope, M/L. |
| Instagram-Insights | teilweise | `uploaders/instagram.rs:539` liest Media-/Reels-Insights; OAuth enthält Insights-Bereich. Keine Prod-Messung. | Ja für Instagram-Analytics-Versprechen. | Nach erfolgreichem Reel mit freigegebenem Testkonto belegen; fehlende Messung nicht als Erfolg/Nullleistung ausgeben, M. |
| Berichte | teilweise | `report_writer.rs:713` fällt bei fehlender externer Anreicherung auf Textvorlage zurück. D6: 17 Admin-Berichte, sämtlich `model=NULL`. `social_media.rs:2755` und `social_media.rs:2816` prüfen Admin. `AnalyticsTab.tsx:146` verbirgt Partner-Berichte. | Nein für Admin-Vorlagenberichte, Ja für behauptete KI-Berichte oder Partnerzugriff darauf. | Fallback sichtbar benennen; externen Datenversand nicht zur Herstellung eines grünen Audits einschalten, S. |
| Partner-Datenisolation | teilweise | `social_media.rs:579`: Freigabe und Twitch-ID aus Session; `social_media.rs:610`: Clipbesitz; `social_media.rs:2289`: Liste an Session-ID. Frontend `SocialMediaAdmin.tsx:183` reicht die eigene ID weiter. D2: keine Clip-/Auth-Besitzer-ID fehlt. | Authentifizierter Zwei-Konten-Nachweis offen, keine belegte IDOR-Lücke in diesen Pfaden. | Mit zwei freigegebenen Testsessions fremde IDs für Lesen, Layout, Vorschau, Freigabe und Trennen prüfen, M. |
| Admin-Pfade und Freigabeverwaltung | funktioniert | `social_media.rs:548` prüft echten Admin; `social_media.rs:3859` und `social_media.rs:3883` nutzen diesen Guard. Namenspräfix `/admin/clips` bedeutet dagegen kanalgebundene Partnerbedienung mit Besitzprüfung. H1: ohne Session API 401. | Nein für die belegten Guards; Partner/Administrator-Matrix live offen. | Matrixabnahme zusammen mit Partner-Datenisolation, S. |
| Caddy-Allowlist und CSP | funktioniert | `/etc/caddy/Caddyfile:237`, `:241`, `:684`: Social-Routen werden an Port 8769 weitergereicht und bekommen Dashboard-CSP. H1: APIs 401, Rechtsseiten 200. D2: Thumbnail-Host `static-cdn.jtvnw.net` ist freigegeben. `SocialMedia.tsx:2296` nutzt ein gleichursprüngliches Video, keinen fremden Iframe. | Nein für die geprüften Ressourcen. | CSP nach einer angemeldeten vollständigen Bedienrunde nochmals gegen Netzwerkfehler prüfen, S. |
| Leere Liste, Ladefehler, OAuth-Fehler | teilweise | `SocialMedia.tsx:795`, `SocialMedia.tsx:1934`, `SocialMedia.tsx:1961` unterscheiden Fehler und unbekannten Verbindungsstand. `AnalyticsTab.tsx:211` zeigt Fehler statt Diagramm. B5/B6 umgehen diese saubere Rückmeldung. | Ja wegen B5/B6, kein genereller Leerzustandsdefekt. | Antwortvertrag und tatsächliche Warte-/Fehlerzustände korrigieren; Rohfehler in verständliche Produkttexte übersetzen, M. |
| VOD-Archiv und YouTube-Spiegelung | funktioniert | `tb-bot/src/main.rs:1799`: eigener Worker; `tb-social-media/src/vod_archive.rs:96`: ID-gebundene Einstellung. D7: zwei aktivierte private Archive, ein VOD `uploaded`, 75 Teile `done` mit YouTube-ID. L2: aktueller Archivlauf und Upload-Nachprüfung. | Nein für den belegten privaten Betrieb. | Nicht mit Shorts-Sichtbarkeit gleichsetzen; öffentliche Sichtbarkeit braucht den separaten Audit-Nachweis. |
| Templates und Hashtags | teilweise | `social_media.rs:443`, `social_media.rs:487`: eigene Templates und Clipbesitz geprüft. Keine Speicherung im Audit. TikTok-Inbox übernimmt diese Caption-Felder nicht. | Nein für YouTube/Instagram-Metadaten; Einschränkung für TikTok offenlegen. | Speicherung und Anwendung im eigenen Testkanal prüfen; TikTok-Postfachtext separat erklären, S. |
| Deadlock-Wörterbuch | teilweise | `social_media.rs:1698`, `:1736`, `:1789`, `:1822`: Adminpfade. `render.rs:66` verwendet Vokabular für Untertitel. Abhängigkeit vom kaputten Transkriptpfad. | Nein als Admin-Zusatzfunktion, Untertitelblocker bleibt. | Nach STT-Fix an echten lokalen Segmenten prüfen, S. |
| Clip-Contest/Formular-Zusatzpfade | teilweise | `social_media_clip_contest.rs:123` und `:130`: Freigabe/Besitz vor Producer-Aufruf. `social_media.rs:1412`: Formularanfrage mit protokolliertem Ergebnis. Im Audit nicht abgesendet, da das Inhalte weitergeben würde. | Nein für Social-Posting; eigene Freigabe nötig, falls mitbeworben. | Akzeptiert, bereits vorhanden, abgewiesen und nicht erreichbar mit Testdaten abnehmen, M. |

Die DB-Rollen wurden nicht aus älteren Arbeitsnotizen abgeleitet: Die tatsächlichen Grants geben `twitchdash` SELECT/INSERT/UPDATE auf `social_media_platform_auth` und INSERT auf Einstellungen/Upload-Queue. In `pg_stat_activity` sind die Dashboard-Pools als `twitchdash|tb-dashboard` sichtbar. Der vermutete allgemeine Schreibrechte-Blocker besteht hier nicht.

## 4. Acht technische Befunde

### Pipeline und Regeneration

**B1: Lokale Transkription ist produktiv gestört.** D3 enthält aktuelle `http_status(401)`- und Transportfehler, daneben ältere Timeouts. Der Social-Pfad benutzt `SttTranscriber::from_default()` (`transcription.rs:14`) und damit `OpenAiTranscriber::from_env()` (`tb-engagement/src/transcribe.rs:140`). Dessen Default ist `http://127.0.0.1:8791/v1/audio/transcriptions` (`transcribe.rs:27`). Auf Port 8791 lauscht beim Audit `deadlock-patchn`; `GET /health` liefert HTTP 401 und „API-Key fehlt oder ist falsch“. Damit ist der Default aktuell kein erreichbarer STT-Gesundheitspfad. Ob zusätzlich ein Env-Override aktiv ist, wurde ohne Lesen von Prozess-Secrets nicht festgestellt. Fix: den wirksamen Endpunkt ohne Geheimnisse diagnostisch sichtbar machen und den Social-Worker an den bestehenden lokalen Rust-STT-Vertrag anbinden. Größe L zusammen mit B2/B3.

**B2: Die nächste Anreicherungsstufe ist ebenfalls gesperrt.** `external_llm_consent` fehlt in `social_media_settings`; `settings.rs:110` verlangt ausdrückliche Zustimmung. D3 hat 13 entsprechende Fehler. STT-Reparatur allein stellt die volle Pipeline daher nicht her. Die Grenze ist beabsichtigt und kein Anlass, die Einwilligung selbst zu setzen. Fix: freigegebenen lokalen/fallbackfähigen Produktweg herstellen oder externe Anreicherung aus der Beta herausnehmen. Bei echter externer Verarbeitung braucht es eine eigenständige Freigabe. Größe M/L je Produktweg.

**B3: Festhängende und wiederholt scheiternde Enrichments werden nicht fair weiterbearbeitet.** `enrichment.rs:335` wählt `pending`/`failed`, nicht alte `transcribing`; `:337` sortiert absteigend nach Clipdatum, Batchgröße ist drei (`enrichment_worker.rs:20`). Elf Einträge stehen seit September in `transcribing`, zuletzt aktualisiert spätestens am 30. September. Wiederholte Fehler haben keinen Zeitabstand und können die neuesten drei Plätze erneut besetzen. `enrich_pipeline.rs:233` liefert den Fehlschlag als normales Outcome; `enrichment_worker.rs:60` loggt dagegen Pipeline-`Err`. Ein ruhiges Journal ist deshalb kein Gesundheitsnachweis. Fix: Lease-/Abbrucherkennung, fairer Auswahlzeitpunkt, Fehlerklassifikation und Retry-Abstand. Größe M.

**B4: „Neu generieren“ kann vorhandene Untertitelgrundlagen löschen.** Der manuelle Handler ruft `pipeline.run(..., None, ...)` auf (`social_media.rs:2691`). Ohne Transcriber bleibt das neue Transkript leer (`enrich_pipeline.rs:220`, `:238`), wird aber gespeichert (`:240`). `enrichment.rs:242` ersetzt Rohtranskript, Segmente und Sprache durch NULL. Das geschieht auch vor einem anschließenden LLM-Fehlschlag. Ein erfolgreicher fertiger Eintrag ist bei `force=true` ebenfalls betroffen. Der Hintergrundzwilling injiziert dagegen den Transcriber (`tb-bot/src/main.rs:1732`, `enrichment_worker.rs:62`). Fix: vorhandenes Transkript bei reinem Metadaten-Neulauf erhalten; Daten erst nach erfolgreicher neuer Transkription ersetzen. Größe M.

### Antwortvertrag, Upload-Wartezustand und Plattformgrenzen

**B5: Antwortvertrag von „Neu generieren“ passt nicht zum Frontend.** `social_media.rs:2700` liefert `{clip_db_id,outcome,enrichment}`. `api/socialMedia.ts:292` deklariert und verwendet die Rückgabe als `ClipEnrichment`. `EnrichmentPanel.tsx:143` schreibt den Wrapper als Datenobjekt in den Cache; `:145` baut daraus leere Editfelder und `:158` fällt beim fehlenden obersten `status` auf pending zurück. Es gibt kein aktives Statuspolling für diesen Wrapper. Der Speicherzwilling ist korrekt: `social_media.rs:2623` liefert das nackte Enrichment passend zum Client. Fix bevorzugt im Rust-Vertrag, ohne produktiven Python-Pfad: kompatible Rückgabe und zusätzlich klarer Fehlschlagstatus. Größe S.

**B6: Ohne entschlüsselbaren, vollständigen Zugang bleibt ein Job unbemerkt stehen.** `upload_worker.rs:724` liefert keinen Uploader, etwa bei fehlendem Zugang oder erforderlicher Client-ID. `:856` überspringt den Job, ohne Wartezustand/Fehler/Termin zu speichern. Der Schedule-Handler prüft Felder und Identität, nicht die Plattformverfügbarkeit (`social_media.rs:3405`); die gemeinsame Queue prüft den Plattformnamen (`clip_queue.rs:85`). Mit deaktivierter oder fehlender Verbindung kann ein freigegebener Job deshalb als pending stehen bleiben. Der Insights-Zwilling setzt für fehlenden Client wenigstens `error:<platform>:missing_client` und einen neuen Zeitpunkt (`insights_worker.rs:226`). Fix: nutzerverständlichen Wartezustand und Backendprüfung am gemeinsamen Einreih-/Verarbeitungspfad. Größe M.

**B7: TikTok-Insights sind absichtlich unimplementiert.** `tiktok.rs:503` gibt `NotImplemented` zurück. Der Posting-Scope ersetzt `video.list` nicht; eine `publish_id` ersetzt keine echte Video-ID. Die gemeinsame Insights-Schleife behandelt das als API-Retry (`insights_worker.rs:238`), nicht als klar sichtbare fehlende Fähigkeit. Fix: verfügbare Plattformfähigkeiten in der API explizit kennzeichnen, TikTok-Zahlen in der Beta nicht versprechen. Eine echte Integration braucht Display-API-Freigabe, Scope und ID-Auflösung. Größe S für ehrliche Anzeige, L für Funktion.

**B8: Bei Sammelverbindung fehlen der Wechsel zum eigenen Konto und eine wirksame Trennaktion.** `SocialMedia.tsx:1992` zeigt bei verbundenem Fallback „Trennen“ statt „Verbinden“. Der kanalbezogene Disconnect deaktiviert die eigene Twitch-ID-Zeile (`social_media.rs:3826`), die Sammelzeile aber richtigerweise nicht. Existiert keine eigene Zeile, ist die Aktion ein erfolgreich gemeldetes No-op und nach dem Reload weiter verbunden. Die eigene Verbindung ist in diesem Zustand über die dargestellte Aktion nicht erreichbar. D1 enthält derzeit keine globale Zeile; dies ist ein konkret belegter bedingter Codefehler, kein beobachteter heutiger Streamer-Schaden. Fix: „Eigenes Konto verbinden“ anbieten und Fallback/Trennung korrekt erklären. Größe S.

Zwillingssuche: Die genannten Handler wurden gegen Frontend-API und Editor, Hintergrund-Worker, gemeinsame Queue, Plattformfabrik und Insights-Worker gelesen. Der ältere Python-Unterbau wurde nicht verändert. Die laufenden Executables sind Rust im geprüften Release. Es wurden keine eigenen Review- oder Fix-Threads gestartet.

## 5. Laufende Worker und Laufzeitbelege

Der Social-Block enthält zehn Starts: Retention (`main.rs:1684`), Approval (`:1689`), Report (`:1694`), Clip-Prep (`:1704`), Preview (`:1719`), Enrichment (`:1737`), Upload (`:1758`), Refresh (`:1767`), Insights (`:1780`) und VOD-Archiv (`:1806`). Die letzten vier benötigen den Field-Cipher. Der Logtext „8 Loops inkl. VOD-Archiv“ (`:1814`) ist veraltet und zählt die heutigen Starts nicht korrekt.

| Beleg | Beobachtung | Bewertung |
| --- | --- | --- |
| L1, Bot-Journal seit aktuellem Start | 6. Oktober 18:13:16 UTC: „Social-Media-Pipeline-Worker gestartet“; 7. Oktober 00:18:09 UTC: Clip-Fetch mit sieben neuen Clips und drei Fehlern beendet. | Verdrahtung und echter Clipzufluss vorhanden. |
| D1, Credential-Zeitstempel | YouTube-Refresh während des Audits; aktive TikTok-Verbindung mit aktuellem Refresh. | Cipher-/Refresh-Pfad wirkt im Bestand. |
| D3, Enrichment-Zeitstempel | Fehler werden während des Audits erneut aktualisiert; elf Transkriptionszustände bleiben alt. | Workeraktivität ist belegt, Gesundheit nicht. |
| L2, VOD-Journal | 6. Oktober 18:18:18 UTC: Upload-Nachprüfungen; 18:18:19 UTC: Archivlauf mit zwei Kanälen beendet. | VOD-Worker läuft. |
| D4/D6 | Keine pending/processing/Inbox-Queue und keine neue TikTok-/Instagram-Messung. | Kein Beleg für Upload- oder Insights-Gesundheit durch bloße Stille. |

Journal wurde lesend mit `sudo -n journalctl -u deadlock-twitch-bot-rust` beziehungsweise Dashboard-Dienst ausgewertet. Binäre MESSAGE-Felder wurden als UTF-8 dekodiert. Keine Neustarts, Builds, Migrationen oder Test-Suites durchgeführt.

**F1, Dateiüberprüfung:** Beide Dienste binden `/var/lib/deadlock-twitch-media/clips` in `current/data/clips` ein. Erst ein Check über `/proc/<PID>/root` prüft die echte Dienstansicht. In ihr existieren 109 von 109 referenzierten Quelldateien. Der absolute gespeicherte Vorschaubestand zeigt auf einen alten Release, wird aber vom vorhandenen `resolve_preview_path` auf `data/clips/<id>_preview.mp4` umgesetzt. Die effektive Datei existiert: 26.670.801 Bytes, 1080×1920, H.264/AAC, 30 Sekunden. Kein Befund „Dateien fehlen“ verbleibt nach dieser Gegenprüfung.

**H1, HTTP:** Auf öffentlichem Host und Port 8769 liefern `/social-media` und `/social-media-admin` ohne Session 303 zum Login. `/social-media/api/access/me`, `/social-media/api/platforms/status`, `/social-media/api/stats` liefern 401. Terms und Privacy liefern 200. Öffentlich wird die CSP aus `/etc/caddy/Caddyfile:241` mitgeliefert. Keine Aussage über eine erfolgreiche angemeldete Bedienung wird daraus abgeleitet.

## 6. Plattformauflagen vor Veröffentlichung

### C1: TikTok-Modus und Pflichtoberfläche

Die Zulassung der App ist laut Auftrag erfolgt. Der heutige Produktpfad bleibt trotzdem Inbox. Für diesen Weg müssen Nutzer wissen, dass sie die Benachrichtigung im TikTok-Postfach öffnen, den Entwurf bearbeiten und dort veröffentlichen. Der vorhandene Hinweis erscheint nach dem Transfer (`SocialMedia.tsx:2386`); die Darstellung des Zeitplans und der Metadaten muss das schon vor der Übergabe erklären. Der Inbox-Request übernimmt die im Dashboard erzeugte Caption nicht.

Direct Post ist eine vorhandene Bibliotheksoption über `with_privacy_level`, nicht die verdrahtete Partnerfunktion. Für eine spätere Aktivierung reichen Backend-Creator-Info und ein festes `SELF_ONLY` nicht: Laut TikTok muss die Postingseite aktuelle Creator-Info samt Nickname, erlaubte Dauer, manuell gewählte Privatsphäre ohne Default, freigegebene Interaktionsschalter, Werbeoffenlegung, die vorgeschriebenen Erklärungen und ausdrückliche Zustimmung abbilden. Diese UI ist im Dashboard nicht vorhanden. Keine vorhandene App-Freigabe wird hier als Ersatz für diese Implementierung bewertet.

Der Renderpfad ergänzt einen Kanalnamen (`subtitles.rs:211`), kein nachgewiesenes Plattform-/Community-Werbelogo. Die Nutzungsbedingungen bezeichnen das Produkt jedoch als „für interne Zwecke“; TikToks Direct-Post-Richtlinie schließt reine interne Team-Upload-Werkzeuge aus. Vor späterem Direct Post tatsächliches Produkt, Freigabeumfang und diese Dokumentation abstimmen. Dies ist keine Feststellung, dass die bereits erteilte App-Zulassung ungültig sei.

### C2: YouTube-Freigabe, Sichtbarkeit und Quota

OAuth-Zustimmung, Verifizierung des Consent-Screens und Compliance-Audit des Data-API-Projekts sind getrennte Voraussetzungen. Die offizielle `videos.insert`-Dokumentation beschränkt Uploads nicht auditierter neuerer Projekte auf private Sichtbarkeit. Der Shorts-Uploader fordert dennoch fest `public` an (`youtube.rs:768`) und meldet beim Uploadabschluss keine tatsächliche Privatsphäre an die UI zurück. Die VOD-Oberfläche berücksichtigt dagegen `YOUTUBE_AUDIT_PASSED`, Default nicht bestanden (`social_media.rs:3537`). Der wirksame Prod-Wert und die Console-Freigabe wurden ohne Zugang zur Console nicht festgestellt.

Die am Prüftag abgerufene Google-Dokumentation nennt **100 `videos.insert`-Aufrufe pro Tag** und **eine Einheit im Video-Uploads-Quota-Bucket pro Aufruf**. Daher keine ältere pauschale Rechnung mit 1.600 Einheiten pro Upload übernehmen. Das konkrete Projektkontingent und sein Verbrauch wurden nicht aus der Console nachgewiesen. Backend erkennt Kontingentfehler und vertagt; das ist kein Kapazitätsnachweis für zusätzliche Partner.

### C3: Instagram-Freigabe

Der Code verwendet Instagram Login, nicht den alten Facebook-Login-/Basic-Display-Weg. Scope und Reels-Containerpfad sind vorhanden. In Prod fehlen Instagram-Credentials und Uploadbelege. Reviewstatus, Live-Modus, Zugriff für Partner außerhalb der App-Rollen und ein geeignetes professionelles Konto sind nicht nachgewiesen. Die offizielle Meta-Veröffentlichungsseite lieferte beim Abruf keinen auswertbaren Inhalt. Deshalb wird keine aktuelle Detailauflage oder konkrete erteilte Berechtigung aus Erinnerung als geprüft ausgegeben. Instagram für Go-Live entweder explizit ausnehmen oder App-Console und Testkonto zur Abnahme bereitstellen.

### C4: Produkt- und Rechtstexte

`templates/terms.html:31` verspricht automatische Veröffentlichung auch auf TikTok, obwohl der aktive Weg ein Entwurf im Postfach ist. `templates/privacy.html:43` beschreibt den Zweck als automatische Veröffentlichung; `:57` nennt Speicherung bis zur manuellen Löschung, obwohl `retention.rs:152` einen automatischen Fristenlauf besitzt. Der Hinweis „kein LLM-Key“ im Editor (`EnrichmentPanel.tsx:217`) passt außerdem nicht zu einer fehlenden Einwilligung oder der STT-Störung. Vor Freischaltung verständliche, tatsächlich zutreffende Texte verwenden. Keine Zugangsdaten im Partner-Support anfordern. Größe S/M.

### Öffentliche Vertragsquellen

Abruf am 7. Oktober 2026, ohne Nutzer-/Community-Daten in Anfragen:

- [TikTok Content Sharing Guidelines](https://developers.tiktok.com/doc/content-sharing-guidelines/): Pflicht-UX und Intended Use für Direct Post, Dokumentstand 4. August 2026.
- [TikTok Upload Content](https://developers.tiktok.com/doc/content-posting-api-get-started-upload-content/): Inbox-Entwurf und anschließende Veröffentlichung durch Nutzer.
- [YouTube videos.insert](https://developers.google.com/youtube/v3/docs/videos/insert): Projekt-Audit, Sichtbarkeit und aktuelles Kontingentmodell.
- [Meta Instagram Content Publishing](https://developers.facebook.com/docs/instagram-platform/instagram-api-with-instagram-login/content-publishing): Abruf nicht inhaltlich auswertbar, keine behauptete Abnahme.

## 7. Reproduzierbare DB-Belege

Ausführen mit `PGOPTIONS='-c default_transaction_read_only=on -c search_path=public -c statement_timeout=10000' psql -X -w -d twitch_analytics`. Keine Zugangsfelder in der Projektion. Die Daten ändern sich im laufenden Betrieb; die angegebenen Zahlen sind Snapshots, keine eingefrorene Testdatenbank.

```sql
-- D1: Umfang, Erneuerung und Ablauf, ohne Zugangsinhalte.
SELECT platform, enabled,
       CASE WHEN twitch_user_id IS NULL AND streamer_login IS NULL
            THEN 'global' ELSE 'own' END AS scope,
       count(*) AS connections,
       count(*) FILTER (WHERE refresh_token_enc IS NOT NULL) AS refresh_present,
       count(*) FILTER (WHERE nullif(token_expires_at, '')::timestamptz < now()) AS expired,
       max(last_refreshed_at) AS last_refresh
FROM social_media_platform_auth GROUP BY 1, 2, 3 ORDER BY 1, 2, 3;

-- D2: Clipbestand und Dateireferenzen.
SELECT status, count(*),
       count(*) FILTER (WHERE local_file_path IS NOT NULL) AS local_files,
       count(*) FILTER (WHERE uploaded_tiktok) AS tiktok,
       count(*) FILTER (WHERE uploaded_youtube) AS youtube,
       count(*) FILTER (WHERE uploaded_instagram) AS instagram
FROM twitch_clips_social_media GROUP BY 1;

-- D3: Anreicherung und alte Arbeit.
SELECT status, count(*), min(started_at), max(updated_at)
FROM social_media_clip_enrichment GROUP BY 1;
SELECT error_message, count(*), max(updated_at)
FROM social_media_clip_enrichment WHERE status = 'failed' GROUP BY 1;
SELECT COALESCE((SELECT value::text FROM social_media_settings
                 WHERE key = 'external_llm_consent'), 'absent') AS consent;

-- D4: Freigabe und Plattformqueue.
SELECT state, count(*) FROM social_media_clip_approval GROUP BY 1;
SELECT platform, status, count(*), max(attempts), max(last_attempt_at),
       count(*) FILTER (WHERE tiktok_publish_id IS NOT NULL) AS checkpoints
FROM twitch_clips_upload_queue GROUP BY 1, 2 ORDER BY 1, 2;

-- D5: Gespeicherte Kadenz.
SELECT platform, auto_post, count(*), min(posts_per_week), max(posts_per_week)
FROM social_media_platform_schedule GROUP BY 1, 2;

-- D6: Insights und Berichtsfallback.
SELECT platform, bucket, count(*), max(synced_at), sum(views)
FROM twitch_clips_social_analytics GROUP BY 1, 2;
SELECT kind, COALESCE(model, 'fallback'), count(*), max(created_at)
FROM social_media_reports GROUP BY 1, 2;

-- D7: Archivumfang, ohne öffentliche Video-IDs auszulesen.
SELECT enabled, privacy, count(*) FROM social_media_vod_archive GROUP BY 1, 2;
SELECT status, count(*), count(*) FILTER (WHERE youtube_video_id IS NOT NULL), max(updated_at)
FROM twitch_vod_archive_parts GROUP BY 1;
SELECT status, count(*), max(updated_at) FROM twitch_vod_archive_vods GROUP BY 1;
```

Snapshot um 05:35 CEST: D1 TikTok 1 eigener aktiver Zugang, YouTube 2, Instagram 0, global 0. D2 `approved=3`, `awaiting_approval=100`, `pending=95`; ein YouTube-Upload, kein TikTok-/Instagram-Uploadflag. D3 `transcribing=11`, `failed=49`, `done=0`; Fehler zunächst 16 Timeout, 12 Transport, 8 HTTP 401, 13 fehlende Einwilligung. Um 05:43 waren Transport/401 13/7, die Summe blieb 49. D4 TikTok `failed=3`, YouTube `completed=1`; kein offener Queuejob. D6 YouTube drei Snapshots, TikTok/Instagram keiner; 17 Admin-Fallbackberichte. Die drei TikTok-Fehler stammen vom 23./24. September und sind kein Gegenbeweis zur inzwischen erteilten Zulassung.

## Muss vor Go-Live

1. **Clip-Anreicherung wieder wirksam machen:** B1 bis B3, lokaler Rust-STT-Pfad, zulässiger Anreicherungsweg, Recovery und faire Retries. Beweis: neuer Testclip erreicht Transkript, Untertitel und Freigabe ohne Eingriff in echte Streamerzugänge.
2. **„Neu generieren“ datenerhaltend und sichtbar korrekt machen:** B4/B5. Beweis: vorhandene Segmente bleiben bei reinem Metadaten-Neulauf erhalten; Editor zeigt das tatsächlich zurückgegebene Ergebnis oder den Fehler.
3. **Fehlende Verbindungen als Handlungshindernis melden:** B6. Beweis: kein scheinbar gesunder dauerhaft wartender Job bei fehlender Verbindung. B8 zusätzlich beheben, bevor eine Sammelverbindung eingesetzt wird.
4. **Freischaltungsumfang und Texte verbindlich begrenzen:** TikTok-Inbox statt Direct-Post-Versprechen, TikTok-Insights nicht als verfügbar darstellen, Instagram ohne Reviewnachweis ausnehmen; C4 korrigieren. Eine App-Zulassung ist keine automatische Erweiterung der implementierten Fähigkeiten.
5. **Zulässige Partner-Abnahme nachholen:** zwei Testsessions für Isolation/Admin-Matrix, kompletter Clipweg bis zur gewählten Plattform, TikTok-Postfach und YouTube-Sichtbarkeit/Projektkontingent. Für Instagram bei Mitfreischaltung zusätzlich App-Review und Reel-Nachweis. Dies erfordert eine gesondert freigegebene schreibende Abnahme, nicht Änderungen aus diesem Audit.

## Kann nach Go-Live

1. TikTok Display API und echte Insights ergänzen, sofern die Beta klar ohne diese Funktion angeboten wird.
2. Direct Post mit vollständiger TikTok-Pflichtoberfläche als gesondertes Vorhaben bauen; bis dahin beim erklärten Inbox-Weg bleiben.
3. YouTube Analytics API für Watchtime/CTR und echte Zeitraumkennzahlen ergänzen; einfache Data-API-Zähler klar abgrenzen.
4. Berichte mit sauber markiertem Fallback und besserer interner Fehlerdiagnose ausbauen; keinen externen Datenversand ohne Freigabe einschalten.
5. Worker-Startzählung und Gesundheitsmetriken verbessern, damit gespeicherte Enrichmentfehler und Verbindungspausen nicht hinter einem ruhigen Journal verschwinden.

## Übergabe

Bericht als einzige Änderung auf `docs/social-golive-check`. Kein Merge nach main, kein Deploy, kein Restart und keine eigene Implementierung. Die parallele Arbeit an Ablaufanzeige/Neu-Verbinden-DM bleibt eigenständig.

Dokumentationsabschluss: `git add` für genau diesen Bericht, `git commit` auf `docs/social-golive-check`, `git push` auf denselben Branch. Kein main-Merge beauftragt.

MERGEPROTOKOLL[MS-1]: 3 Git-Schritte einzeln | Anläufe: 1 | Gate: kein main-Merge, vorhandene Branch-Hooks bleiben aktiv

TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 2 belegt | Senke: .tasks/2026-10-07-social-media-golive-check/BERICHT.md
