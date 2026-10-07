# Fixer, Folgeprüfung zu Gate-Runde 1

## Gesicherter Stand

Der erhaltene Implementierungsstand f637d138 wurde vor Änderungen mit normalem Push auf origin/feat/social-tiktok-direct-post gesichert. Fixcommit: 59c904d8. Origin/main wurde frisch geholt; Rebase meldete unveränderte Basis e0b0dbaf. Fremde Änderungen im gemeinsamen Checkout wurden nicht angefasst.

## Korrigierte Befunde

1. queue_upload_handler prüft den Zeitplan vor save_choice. Ein abgelehnter Zeitplan erreicht keine TikTok-Schreiboperation. PostgreSQL-Regression prüft invalid_schedule sowie unveränderte Clip-Angaben, Job-Angaben, Termin und Jobanzahl.
2. Beide JSON-Feldabfragen im Upload-Worker normalisieren JSON-null über NULLIF(..., 'null'::jsonb) zu SQL-NULL. Frühere Postfach-Jobs verwenden damit wieder den bisherigen Resolver. Neue Uploads bleiben am getrennten Direct-Post-Resolver gegen fehlende oder unvollständige Angaben gesperrt.
3. Der gemeinsame Freigabeguard erlaubt einen neuen Auftrag nach status=failed und bestätigtem tiktok_publish_status=FAILED. Die frühere Vorgangsnummer und ihre Angaben bleiben erhalten. NULL, STATUS_UNAVAILABLE, laufende Übertragung und PUBLISH_COMPLETE öffnen keine neue Freigabe. Dieselbe Funktion gilt für Queue- und Approval-Handler. PostgreSQL-Regression prüft neue Job-ID, erhaltene Historie, neuen Snapshot und acht gesperrte Zustände.

Zwillingssuche: save_choice im Queue- und Approval-Handler, die beiden JSON-Abfragen im Worker, queue_upload und upload_already_exists in der Approval-Verarbeitung geprüft. Änderungen bleiben auf drei Rust-Dateien begrenzt. Kein produktiver Post und kein Deploy vor ALLOW.

## Prüfungen

Arbeitsverzeichnis: /home/nathanael/.worktrees/tb-social-tiktok-direct/rust. Eigener PostgreSQL-16-Cluster unter /tmp/tb-tiktok-db-666eaa47 auf Port 59471. Wiederanlauf benötigte zusätzlich -k /tmp/tb-tiktok-db-666eaa47, weil der Standard-Socketordner nicht beschreibbar ist. Keine Produktionsdatenbank für Tests.

```bash
SQLX_OFFLINE=true TB_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:59471/postgres /home/nathanael/.cargo/bin/cargo test -j 2 -p tb-dashboard-api --offline social_media -- --include-ignored --test-threads=1
```

Exit 0: 55 bestanden, 0 fehlgeschlagen, 0 ignoriert, 1278 Bibliothekstests ausgefiltert. Zwei neue PostgreSQL-Regressionen liefen erfolgreich. Log: /tmp/tb-tiktok-fixer-api-tests.log.

```bash
SQLX_OFFLINE=true TB_TEST_DATABASE_URL=postgres://postgres@127.0.0.1:59471/postgres /home/nathanael/.cargo/bin/cargo test -j 2 -p tb-social-media --offline -- --include-ignored --test-threads=1
```

Exit 0: 305 Bibliothekstests und 1 Integrationstest bestanden, 0 fehlgeschlagen, 0 ignoriert. Log: /tmp/tb-tiktok-fixer-social-tests.log.

## Folgeprüfung und anschließende Integration

Gate-Runde 2 mit `--model gpt-6.1-sol` ergab ALLOW auf 59c904d8 und bestätigte jeden der drei alten Befunde als FIXED. Log: /tmp/tb-tiktok-gate-round-2.log.

Vor dem tatsächlichen Merge hatte sich origin/main auf 07f511a3 weiterbewegt. Der verpflichtende zweite Rebase integrierte D2. Konflikte wurden unter Erhaltung beider Funktionen gelöst: YouTube-Sichtbarkeit und TikTok-Veröffentlichungsstand in API und Clipkarte; D2-Wartezustand und faire Verbindungsprüfung neben dem TikTok-Resolver im Worker. Fehlende TikTok-Verbindungen mit vorhandener Freigabe erhalten den D2-Wartezustand. Fehlende Freigaben erlauben weiterhin keinen neuen Upload. Die beiden D2-Workerfixtures tragen jetzt eine vollständige individuelle TikTok-Freigabe und video.publish. Die Plattformfähigkeit nennt den neuen Uploadmodus direct_post statt inbox. Der integrierte Stand wird erneut geprüft; ALLOW von Runde 2 wird nicht als Freigabe des neuen Standes ausgegeben.

## Tatsächliche Bildbelege

Die vom ursprünglichen Implementierer aufgenommenen PNGs wurden vom Fixer angesehen:

- /home/nathanael/.claude/sichtpruefung/tb-social-tiktok-direct/tiktok-desktop.png
- /home/nathanael/.claude/sichtpruefung/tb-social-tiktok-direct/tiktok-mobile-bottom.png
- Weitere obere Mobilaufnahme: /home/nathanael/.claude/sichtpruefung/tb-social-tiktok-direct/tiktok-mobile-top.png

Desktop zeigt Kontonamen, Beschreibung, Vorschau, leere Sichtbarkeitsauswahl, Interaktionen, Werbung, Musikbestätigung und gesperrten Einplanen-Button. Die gescrollte Mobilaufnahme zeigt die unteren Einstellungen und Aktionen im Dialog. Die Videos sind hier Testfixtures. Das ist eine eigene Sichtprüfung, keine unabhängige visuelle Zustimmung des Gate und kein Live-Beweis eines echten Clips.

## Weiter gültige Freigabe

Genau ein SELF_ONLY-Test auf earlysalty nach ALLOW, Merge und Deploy. Eigenes Deadlock-Video zuerst ansehen, keine erkennbar fremde Musik, Beschreibung und Musikbestätigung im Dashboard. Quelldateien vor einer Fehlmeldung auch über den Dienst-Bind-Mount unter /proc/<PID>/root prüfen. Clip-ID im Abschluss dokumentieren. Kein anderer Post.

Bei erneutem BLOCK gilt die inzwischen ausdrücklich erteilte autonome Fixschleife: neuer nativer Fixer-Subagent je Runde, derselbe Gate-Prüfer gpt-6.1-sol, kein neuer T3-Thread. Nach fünf erfolglosen Runden oder echtem Blocker melden.

TESTNACHWEIS[TW-1]: 434 passed, 0 ignored | Baseline: nicht neu gemessen rot
WIRKUNGSPRUEFUNG[WP-1]: 3 Befunde korrigiert | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 2/2 geprüft
TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 1 belegt | Senke: FIXER-EVIDENCE.md

Beleg zum begrenzten Netzwerkziel: `live-api-readonly.mjs:5-11` erlaubt drei feste Pfade; `live-api-readonly.mjs:17-19` verwendet den fest eingetragenen Loopback-Host und manuelle Weiterleitungen. Befehlsflags bleiben als Code erhalten und sind keine Satzzeichen.

## Integrierter Abschlussstand

Runde 3 prüfte 9315b3cf gegen origin/main 07f511a3 mit gpt-6.1-sol und ergab Exit 0, ALLOW. Fast-Forward und normaler Push HEAD:main erfolgten in einem eigenen detached Merge-Worktree. Origin/main bestätigte danach 9315b3cf7e4a6feeffc32b603db26eca78b03a2c.

Die vier tatsächlichen integrierten Läufe: tb-social-media 315 Bibliothekstests und 1 Integrationstest; betroffene tb-dashboard-api 55 Tests; Frontend-Verträge 37 Tests; Chromium 26 Tests. Summe 434 bestanden, 0 fehlgeschlagen, 0 ignoriert oder übersprungen. Die Befehle für Rust blieben wie oben, mit dem eigenen Testcluster und `--include-ignored --test-threads=1`. Der frische Schema-Test wurde vom Fixer nicht erneut gefahren. Breite rote Läufe aus EVIDENCE.md werden nicht als eigene Baseline übernommen.

Logs: /tmp/tb-tiktok-fixer-integrated-social-tests.log, /tmp/tb-tiktok-fixer-integrated-api-tests.log, /tmp/tb-tiktok-fixer-frontend-contract-tests-final.log, /tmp/tb-tiktok-fixer-browser-tests.log.

## Release-Vorbereitung und echte Medien

Hauptdashboard, Admin-Dashboard und Website wurden aus dem freigegebenen eigenen Quellstand gebaut, jeweils Exit 0. Der Rust-Release-Build für 9315b3cf endete nach 53m 32s mit Exit 0; jedes der acht Programme gab den vollständigen sauberen `--build-revision` 9315b3cf7e4a6feeffc32b603db26eca78b03a2c aus. Vor dem Deploy zeigte ein frisches Fetch jedoch origin/main 0452e03c mit den inzwischen gemergten A3-Änderungen. Dieser alte geprüfte Build wurde nicht deployt. Der eigenständige Clone wurde per Fast-Forward auf das aktuelle main gebracht; Rust und Hauptdashboard wurden dort erneut gebaut. Das Hauptdashboard endete mit Exit 0; der aktuelle Rust-Build mit Exit 0 nach 30m 09s. Die acht neu gebauten und später die acht installierten Programme bestätigten exakt 0452e03cb7eab42d9e08ee5d39bde380514f1cd3. Beide Rust-Läufe verwenden SQLX_OFFLINE=true, `--release --locked --offline -j 2` und die acht erforderlichen Binärziele. Zusätzlich bestanden die bestehenden Frontend-Verträge auf 0452e03c: 38 bestanden, 0 fehlgeschlagen, 0 übersprungen. Die ältere Summe 434 bezeichnet ausdrücklich den integrierten E/D2-Stand 9315b3cf, nicht einen erneuten vollständigen Backend-Lauf auf A3.

Clip 124589 wurde im tatsächlichen Dienst-Mount gefunden: Quelle 23.177.417 Bytes, Vorschau 26.670.801 Bytes. ffprobe bestätigte 30 Sekunden, H.264 1080×1920 und AAC. Die autorisierte lokale schreibgeschützte Administrationsroute bestätigte earlysalty, preview_status=ready und uploaded_tiktok=false. music_track=NULL beweist keine Musikfreiheit im Originalton. Die ursprüngliche Meldung einer fehlenden Datei berücksichtigte den Dienst-Mount nicht und ist damit korrigiert.

Der T3-Browser meldete, dass kein Automationshost verfügbar ist. Unangemeldete Social-Media-Aufrufe bleiben mit 401 gesperrt. Ein frischer eigener Chromium-Kontext wurde zusätzlich über den vorhandenen internen Zugang versucht. Die Webseite /social-media-admin verlangt trotzdem eine gültige Dashboard-Sitzung und antwortet 303 zum Login; die Weiterleitung wurde vor Chromium und ohne Weitergabe des internen Zugangs an einen anderen Host abgebrochen. Der schreibgeschützte API-Prüfer verwendet die vorhandene Dienstidentität, sendet den Zugang ausschließlich an 127.0.0.1 und folgt keinen Weiterleitungen. /social-media/api/access/me bestätigte HTTP 200, application/json und allowed=true, isAdmin=true. Das ist echte interne API-Anmeldung, keine nachgebildete Browser-Sitzung. Zugangsdaten wurden nicht ausgegeben oder gespeichert. Noch kein produktiver Post.

Die tatsächliche frühere Vorschau wurde als drei Videoframes angesehen: Deadlock-Gameplay und Kameraausschnitt. Beleg: /home/nathanael/.claude/sichtpruefung/tb-social-tiktok-direct/clip-124589-preview-frames.png. Ihr Audiotrack ist nicht digital stumm: astats Peak -2,45 dB, RMS -26,76 dB. Diese Messung ist keine Hörprüfung und sagt nicht, ob fremde Musik enthalten ist.

## Produktiver Deploy und nachgewiesene Wirkung

Vor dem Deploy bestätigte ein frisches Fetch erneut origin/main 0452e03cb7eab42d9e08ee5d39bde380514f1cd3. Der saubere eigenständige Clone und seine acht geprüften Programme wurden über deploy-twitch-release mit genau diesem SHA ausgeliefert, Exit 0. Keine fremden Build-Artefakte verwendet. Aktiver Release: /opt/deadlock/twitch/releases/0452e03cb7eab42d9e08ee5d39bde380514f1cd3.

Das tatsächlich lesbare Dienstjournal belegt Applied 20261007042000 um 10:09:27 CEST. Erst danach starteten Dashboard um 10:09:32, Bot um 10:09:33, Coaching-Watch und Kategorie-Collector um 10:09:44. Die Migration ist ab diesem Lauf eingefroren. Die schreibgeschützte Administrationsroute bestätigt die drei neuen Spalten und den BEFORE INSERT-Snapshot-Trigger. Der Anwendungszugang darf _sqlx_migrations nicht lesen und lieferte dafür 400; diese Metadatenprüfung wird nicht als erfolgreich ausgegeben. Der Migrationserfolg ist stattdessen direkt im Dienstjournal belegt.

| Dienst | Vorher | Nachher | Tatsächliches Programm |
| --- | --- | --- | --- |
| Bot | 1878256 | 3057449 | tb-bot |
| Dashboard | 1875039 | 3057112 | tb-dashboard |
| Coaching-Watch | 2946363 | 3059286 | tb-stream-audit |
| Kategorie-Collector | 2945531 | 3059387 | tb-category-collector |
| Watchdog, One-shot | inaktiv | Start 10:12:08, Exit 0 | tb-twitch-watchdog |

/proc/exe der vier laufenden Dienste zeigt den angegebenen Release ohne deleted. NRestarts jeweils 0. Der Watchdog wurde zusätzlich über bot-restart neu gestartet und lief erfolgreich als One-shot. Seine MainPID 0 nach dem Ende ist kein laufender Prozess. Die tatsächlichen Bot- und Dashboard-Prozessbilder enthalten die neuen Anker tiktok_post_choice beziehungsweise tiktok_post_options; beide Kontrollanker `--build-revision` wurden ebenfalls gefunden. Das lesbare Journal mit -p err blieb seit 10:09:00 auch nach den Vorschau- und API-Aufrufen leer. Der erste Journalaufruf ohne Leserechte galt ausdrücklich nicht als Beweis; der folgende Zugriff unter Dienstidentität mit Journal-Lesegruppe zeigte echte Migrator-Einträge und keine Fehler.

Im installierten Hauptdashboard-Bundle index-05wmVRCU.js sind SELF_ONLY, approved_video_sha256 und consent vorhanden. Das ist ein Beleg am gebauten Artefakt, kein behaupteter produktiver DOM-Test.

## Aktuelle vorbereitete Vorschau und verbleibender Testblocker

Der erste produktive Creator-Aufruf lieferte korrekt 400, weil der vorhandene alte Vorschauname nicht zur aktuellen manuellen Renderfassung passt. Die Quelldatei und frühere Vorschau waren im Dienst-Mount vorhanden; die neue Datei fehlte tatsächlich. Der gespeicherte manuelle Ausschnitt wurde deshalb über die reguläre POST-Vorschauaktion für Clip 124589 neu gerendert. Kein neuer Ausschnitt erfunden, keine TikTok-Freigabe gespeichert und kein Upload eingereiht.

Anschließend bestätigten echte GET-Aufrufe HTTP 200 und application/json: preview ready=true; TikTok-Konto earlysalty, individuelle Verbindung 7, 30 Sekunden, maximale Dauer 3600 Sekunden und SELF_ONLY als erlaubte Auswahl. Die Videobindung 132384ae48ff5bfb89631aba46935e57fcaa6402632c82d5da6861933c10d47e entspricht exakt der Prüfsumme der lokal gesicherten Vorschau. Die Datenbank bestätigt tiktok_post_options SQL-NULL, uploaded_tiktok=false, zwei vorhandene Jobs und keine gespeicherte TikTok-Vorgangsnummer.

Die neue Vorschau und ihre tatsächlichen Frames liegen unter /home/nathanael/.claude/sichtpruefung/tb-social-tiktok-direct/clip-124589-current-preview.mp4 und clip-124589-current-preview-frames.png. Die Frames wurden angesehen. ffprobe: H.264, AAC 48 kHz Stereo, 30 Sekunden. Der aktuelle Audiotrack ist nicht stumm: Peak -3,01 dB, RMS -26,73 dB. Das ersetzt keine Hörprüfung.

Der produktive Browserzugang wurde nach dem Deploy erneut mit vorhandenem internem Zugang geprüft: /social-media-admin antwortet weiterhin 303 zum regulären Login. Die JSON-Verwaltung funktioniert, aber es liegt keine gültige Dashboard-Browsersitzung vor. Keine Sitzung wurde nachgebildet. Ohne Hörprüfung und ohne diesen Browserzugang werden Beschreibung, Sichtbarkeit und Musikbestätigung nicht stellvertretend als im Dashboard bestätigt ausgegeben. Der genau eine genehmigte SELF_ONLY-Test wurde nicht initialisiert. Keine weitere Veröffentlichung und keine öffentliche Sichtbarkeit gewählt.

TESTNACHWEIS[TW-1]: 38 passed, 0 ignored | Baseline: nicht neu gemessen rot
WIRKUNGSPRUEFUNG[WP-1]: 3 Befunde | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 2/2 geprüft
LIVEBEWEIS[DV-1]: PID 1878256->3057449 und 1875039->3057112 | exe ohne (deleted) | journal -p err leer | Anker "tiktok_post_choice" in Binary | Funktion: Vorschau bereit und Creator-Bindung bestätigt, privater Test blockiert | Ort: http://127.0.0.1:8769/social-media-admin, Clip 124589, TikTok-Dialog; Browserlogin erforderlich

Abschlussunterlagen werden separat gesichert. Dieser Nachtrag ändert Aufgabenunterlagen und Verwaltungsprüfer, nicht produktiven Rust-, Frontend- oder Migrationscode. Der Release-SHA oben bleibt die nachgewiesene ausgelieferte Fassung. Der eigene PostgreSQL-Testcluster ist gestoppt; Browserbelege und Logs sind außerhalb der zu löschenden Worktrees gesichert.
