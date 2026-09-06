---
status: aktiv
datum: 2026-09-06
contract: ./CONTRACT.md
evidence: ./EVIDENCE.md
---

# PLAN: Clips fuer Social Media aufbereiten

Maßstab ist `CONTRACT.md`. Belege in `EVIDENCE.md`. earlysalty = 1186925760.
Cargo immer `/home/nathanael/.cargo/bin/cargo`. Rust-Tests fuer tb-social-media:
`SQLX_OFFLINE=1 TB_TEST_DATABASE_URL=postgres:///tb_bb_test?host=/var/run/postgresql /home/nathanael/.cargo/bin/cargo test -p tb-social-media`.
Migrationstest gegen Docker: `sudo bash rust/scripts/test_db.sh` (Container `tb-test-postgres`).
sqlx-Offline (`rust/.sqlx/`) und `fresh_schema_snapshot.txt` bei jeder neuen Query mitziehen.

## Urteil REQ-06 (kalter Betrieb)

Verdrahtungs- und Datenbefund, KEIN gewolltes "Freigabe vor Download". Drei
zusammenwirkende Ursachen, alle belegt (EVIDENCE REQ-06):

1. Klassifizierung am Schreibweg fehlt: alle 126 Clips landen in Kategorie `other`
   mit `game_name` NULL; nur `deadlock` hat `enrichment_enabled=true`. Der
   Enrichment-Selektor trifft deshalb 0 Zeilen.
2. Henne-Ei-Sperre: `iter_pending_enrichments` verlangt eine lokale Datei, aber
   der Download passiert nur im cipher-gated Upload-Worker nach der Freigabe.
   Ohne vorgelagerten Download kann Enrichment nie starten.
3. Auto-Freigabe laeuft ins Leere (auto_platforms leer), Clips bleiben auf
   `awaiting_approval`; der einzige Download-Aufrufer (Upload-Worker) ist mangels
   FieldCipher (DB_MASTER_KEY_V1) gar nicht gespawnt.

Fix-Richtung: Download vom Freigabe- und Cipher-Pfad entkoppeln, Klassifizierung
am Schreibweg korrigieren, Consent-Key und Cipher-Praesenz als Live-Voraussetzung
pruefen (operativ, /etc/systemd und Infisical sind außerhalb des Scopes).

---

## M1 - REQ-06: Betrieb wieder scharf (Download + Enrichment automatisch)

Aenderungen:
- `rust/crates/tb-social-media/` clip-fetch (`build_clip_fetch_task`): `game_name`
  aus Helix uebernehmen und `category_key='deadlock'` fuer Deadlock-Clips beim
  Anlegen setzen (Ursache am Schreibweg, kein Poll-Nachtrag). Einmaliger
  SQL-Backfill fuer die 126 Altzeilen als Migration (`rust/crates/tb-db/migrations/`).
- Neuer cipher-freier Vor-Download: entweder eigener `clip_prep_worker` in
  tb-social-media oder ein Download-Schritt am Anfang des Enrichment-Workers, der
  `local_file_path` per yt-dlp setzt, unabhaengig von Freigabe und FieldCipher.
  Verdrahtung in `rust/bin/tb-bot/src/main.rs` (cipher-freier Block bei :1603-1627).
- Live-Voraussetzung dokumentieren (nicht im Code): Consent-Key
  `external_llm_consent=true` in `social_media_settings` setzen (Admin), DB_MASTER_KEY_V1
  fuer den Upload-Worker vorhanden. In `.tasks/.../PLAN.md`-Abschnitt "Live-Checks".

Rot-Test zuerst: Test in tb-social-media, der belegt, dass ein neuer Deadlock-Clip
ohne lokale Datei vom Prep-Schritt heruntergeladen und danach von
`iter_pending_enrichments` selektiert wird (yt-dlp gemockt/injiziert). Vor dem Fix rot.

Zwischenzustand: Neue earlysalty-Clips bekommen `game_name`+`category_key='deadlock'`,
werden geladen (`local_file_path` gesetzt), erzeugen eine `social_media_clip_enrichment`-Zeile.

Validierung: `cargo test -p tb-social-media` gruen; nach Deploy an einem echten neuen
earlysalty-Clip per SELECT belegen: `local_file_path IS NOT NULL` und Enrichment-Zeile
vorhanden; im Dashboard sichtbar.

Stop-Regel: Erst weiter, wenn an einem echten neuen Clip Download UND Enrichment-Zeile
in der DB stehen. Code-gruen allein zaehlt nicht.

Zeit: ~1,5 Tage.

## M2 - REQ-01: gespeichertes Layout wirkt im Render

Aenderungen:
- Neue Render-Funktion in `rust/crates/tb-social-media/src/upload_worker.rs`
  (bzw. video_processor.rs), die das effektive Layout laedt
  (`get_clip_effective_layout`, EVIDENCE handlers:54-56) und `compose_vertical`
  statt `convert_and_trim` aufruft; Center-Crop nur noch als Fallback, wenn kein
  Layout gespeichert ist.
- Aufrufstelle `upload_worker.rs:563-582` von `convert_to_vertical` auf die neue
  Render-Funktion umstellen.

Rot-Test zuerst: Test, der fuer einen Clip mit gespeichertem pip-Layout belegt, dass
der Render den Compose-Pfad (overlay) nimmt und nicht den Center-Crop; ohne Layout
Fallback auf Center-Crop. Vor dem Fix rot (kein Aufrufer von compose_vertical).

Zwischenzustand: Hochgeladene und vorgeschaute Videos tragen das Dashboard-Layout.

Validierung: `cargo test -p tb-social-media` gruen.

Stop-Regel: Kein Merge, solange ein Clip ohne Layout nicht sauber auf Center-Crop faellt.

Zeit: ~1 Tag.

## M3 - REQ-02: Layout-Variante "Blur-Rand"

Aenderungen:
- `rust/crates/tb-social-media/src/video_processor.rs`: in `build_compose_filter`
  einen Zweig `mode == "blur_pad"` ergaenzen: `split` -> Hintergrund `scale` auf
  1080x1920 + `boxblur`, Vordergrund 16:9 mittig per `overlay`.
- `rust/crates/tb-social-media/src/layout.rs`: `mode` um `blur_pad` erweitern
  (Validierung, Doku), Fallback-Verhalten definiert.
- `bot/dashboard_v2/src/pages/SocialMedia.tsx` + `components/socialmedia/LayoutEditor`:
  dritte Option "Blur-Rand" im Editor.

Rot-Test zuerst: `build_compose_filter(..., "blur_pad", ...)` muss `boxblur` und ein
zentriertes `overlay` enthalten; vor dem Fix rot (nur pip/stacked).

Zwischenzustand: Editor bietet drei Layouts; Blur-Rand rendert echten Blur-Hintergrund.

Validierung: `cargo test -p tb-social-media` gruen; `npm --prefix bot/dashboard_v2 run build` gruen.

Stop-Regel: Blur-Rand erst fertig, wenn der Filtergraph gegen echtes ffmpeg ein Video
ohne schwarze Balken erzeugt (Sicht in M8).

Zeit: ~1 Tag.

## M4 - REQ-03: eingebrannte Untertitel

Aenderungen:
- Neue `Transcriber`-Impl in tb-social-media, die den lokalen STT-Server nutzt
  (Client aus `tb-engagement::transcribe` wiederverwenden, EVIDENCE transcribe.rs:27-61),
  injiziert in `enrich_pipeline`/`enrichment_worker` und in den Render-Pfad.
- Neues Untertitel-Modul in tb-social-media: Segmente aus dem STT-Ergebnis in Cues
  schneiden (max 2 Zeilen, ~42 Zeichen, 1 bis 4 s), `correction.rs` auf den Text
  anwenden, ASS-Datei mit Stil Gold auf dunklem Balken erzeugen, per ffmpeg
  `subtitles` einbrennen (kein drawtext).
- Wiederverwendung: liegt schon ein Transkript aus Strang A vor, das benutzen.
- Migration `rust/crates/tb-db/migrations/`: Spalte `subtitles_enabled BOOLEAN NOT NULL
  DEFAULT TRUE` in `social_media_streamer_settings` (bestehende Tabelle, INV-03).
- Schalter im Dashboard (`SocialMedia.tsx`, `api/socialMedia.ts`, Handler
  `handlers/social_media.rs`, Route `lib.rs`).

Rot-Test zuerst: reine Segmentierungsfunktion `segment_subtitles(segments)` -> Cues
mit den Grenzen (2 Zeilen, ~42 Zeichen, 1 bis 4 s) an einem Beispiel; vor dem Fix rot.
Zusaetzlich ASS-Erzeugung: Ausgabe enthaelt Gold-Stil und Balken.

Zwischenzustand: Bei aktivem Schalter tragen gerenderte Videos korrekte Untertitel.

Validierung: `cargo test -p tb-social-media` gruen; Migrationstest `sudo bash rust/scripts/test_db.sh`
gruen; `npm --prefix bot/dashboard_v2 run build` gruen.

Stop-Regel: Untertitel erst fertig, wenn an einem echten Clip die Zeitlage passt
(nicht vor-/nachlaufend) und die Vokabel-Korrektur greift (Sicht in M8).

Zeit: ~2 Tage.

## M5 - REQ-04: Vorschau-Render als Hintergrundjob

Aenderungen:
- Migration: Vorschau-Status je Clip (neue Tabelle `social_media_clip_preview`
  oder Spalten am Clip: `preview_path`, `preview_status`, `preview_error`,
  `preview_updated_at`). Datei unter `clips_dir` (z. B. `<clip_db_id>_preview.mp4`),
  damit Retention sie mitloescht (EVIDENCE retention.rs:151-181, INV-07).
- Endpoints in `rust/crates/tb-dashboard-api/src/handlers/social_media.rs` +
  `routes`/`lib.rs`: `POST /social-media/api/admin/clips/:id/preview` (Job anstoßen),
  `GET .../preview` (Status pollen), `GET .../preview/file` (mp4 streamen, Range).
  Alle unter `/social-media/api/` -> keine Caddy-Aenderung (EVIDENCE Caddyfile:616,207).
- Hintergrundjob rendert per M2/M3/M4-Pipeline; Serverseite serverseitige Scope-/Admin-Auth
  wie die bestehenden Handler.
- Frontend `SocialMedia.tsx`: Knopf "Vorschau rendern", Polling, Video-Player statt
  Thumbnail; die Freigabe zeigt dasselbe Video.

Rot-Test zuerst: Handler-/Job-Test, der belegt, dass ein angestoßener Vorschaujob den
Status auf `rendering` setzt und nach Abschluss `preview_path` liefert; vor dem Fix rot.

Zwischenzustand: Im Dashboard ist je Clip ein echtes Hochformat-Video abspielbar.

Validierung: `cargo test` gruen; Migrationstest gruen; Frontend-Build gruen;
Vorschau-Datei taucht unter `clips_dir` auf und wird von Retention erfasst.

Stop-Regel: Kein Merge, solange die Vorschau-Datei nach Ablauf/Upload nicht geloescht wird (INV-07).

Zeit: ~1,5 Tage.

## M6 - REQ-05: Batch-Kommando fuer eigene Clips

Entscheidung: eigenes Bin in `rust/crates/tb-social-media/src/bin/render_clips.rs`
(neues `[[bin]]` in der Crate-`Cargo.toml`). Begruendung: tb-bot `main.rs:552` hat
keine clap/Subcommand-Struktur (env-flag-gesteuert); ein Subcommand im großen Binary
waere invasiv und riskant, das Bin liegt naeher an der Render-Pipeline und der Contract
formuliert "tb-social-media bietet einen Kommandoweg".

Aenderungen:
- `render_clips.rs`: nimmt eine Twitch-User-ID (INV-06), rendert alle Clips des
  Streamers nach REQ-01 bis REQ-03 in ein Zielverzeichnis, ohne Upload.

Rot-Test zuerst: Test der Batch-Kernfunktion (Auswahl der Clips je twitch_user_id +
Render-Aufruf) an einem Fixture; vor dem Fix rot.

Zwischenzustand: Der Nutzer kann `render_clips --twitch-user-id 1186925760 --out <dir>`
laufen lassen und seine Clips gesammelt sichten.

Validierung: `cargo test -p tb-social-media` gruen; ein Probelauf legt mp4-Dateien im Zielordner ab.

Stop-Regel: Batch erst fertig, wenn ein echter Lauf mit earlysalty-Clips Dateien erzeugt.

Zeit: ~0,5 Tag.

## M7 - REQ-07: Doku gegen den Code

Aenderungen:
- `docs/funktionsweise/social-media-uploads.md:10,13`: Ton-Transkription/Untertitel
  erst nach M4 als real beschreiben; falsche Aussagen bis dahin streichen.
- `docs/architecture/social-media.md` (Punkte 2,3, Abschnitt 6): Whisper/Claude/MiniMax/Ollama
  raus, tb-llm/Fireworks als einziger Weg; Layout und Untertitel gemaeß M2-M4.
- `rust/crates/tb-social-media/src/enrichment_worker.rs:80-81`: veralteten Kommentar entfernen.

Validierung: Doku nennt nur, was der Code tut; Skill `no-em-dashes` vor nutzersichtbarem Text.

Stop-Regel: Kein Doku-Merge mit einem Versprechen ohne Code-Beleg.

Zeit: ~0,5 Tag.

## M8 - Sichtpruefung der drei Layouts (Meilenstein mit Stop-Regel)

Aenderungen: keine (Verifikation).
- Mit M6 einen echten earlysalty-Clip in allen drei Layouts (pip, stacked, blur_pad)
  rendern; je ein Standbild (PNG) und ein kurzes mp4 ablegen.
- Headless-Chrome rendert in der Sandbox nicht zuverlaessig; die Dateien der Nutzer
  im eigenen Browser/Player pruefen lassen.

Validierung: drei mp4 + drei PNG liegen vor, Untertitel sitzen, Facecam sitzt, Blur-Rand
ohne schwarze Balken.

Stop-Regel: NICHT als fertig melden, bevor der Nutzer die drei Vorschauen freigegeben hat.

Zeit: ~0,5 Tag.

## Review und Abschluss

- Nach dem Bauen frischer Review-Agent (Opus 4.8) gegen Diff + Contract, adversarial;
  Frontend zusaetzlich selbst sichten. Zusammengehoerige Schreib-/Lesepfade (Render,
  Vorschau) zusammen reviewen.
- Danach Tests gegen die bekannte Baseline, Merge-Gate (`_ttb-main-deploy` vorher
  `git pull --ff-only origin main`), Migration als `postgres` anwenden und Rechte
  setzen (Docs/workspace/gates-und-merge.md), Release im isolierten Worktree bauen,
  Dienste neu starten, Live-Pruefung, Branch/Worktree loeschen.

## Umsetzungsstand (Implementierung)

Baseline vor dem Bau (origin/main, tb-social-media + tb-dashboard-api,
`SQLX_OFFLINE=1 TB_TEST_DATABASE_URL=postgres:///tb_bb_test`): 1357 passed, 0 failed.

### M1 - REQ-06 Betrieb wieder scharf: ERLEDIGT (Code + Migration; Prod/Backfill offen)

Ursache (an der Live-DB belegt, twitch_analytics): Helix `GET /clips` liefert `game_id`,
aber kein `game_name`; die Deadlock-Kategorie trug keine `twitch_game_id` (NULL). Damit
matchte `resolve_category` Deadlock nie, alle 126 Clips landeten in `other`
(enrichment_enabled=false) und wurden nie angereichert. Zweite Ursache: der einzige
Download-Aufrufer war der cipher-gated Upload-Worker NACH der Freigabe.

Fix:
- Migration `20260906120000_social_media_deadlock_game_id.sql` setzt
  `social_media_category.twitch_game_id='2132205352'` (Deadlock). Neue Clips werden damit
  am Schreibweg korrekt klassifiziert (kein Poll-Nachtrag).
- Backfill-SQL `.tasks/.../backfill-deadlock-clips.sql` reklassifiziert die 50 Deadlock-
  Altzeilen (game_id=2132205352) und setzt game_name='Deadlock'. Einmalig, kein Dauerlauf.
- Neuer cipher-freier `clip_prep_worker::ClipPrepWorker`: laedt Clips enrichment-faehiger
  Kategorien ohne lokale Datei per yt-dlp herunter (injizierbarer Downloader), setzt
  local_file_path/downloaded_at, unabhaengig von Freigabe und FieldCipher. In main.rs im
  cipher-freien Block gespawnt.

Rot-Test zuerst (dokumentierter roter Lauf):
`clip_prep_worker::tests::prep_laedt_deadlock_clip_und_macht_ihn_enrichbar`
FAILED - `assertion left==right failed: genau der Deadlock-Clip wird geladen; left:0 right:1`
(Stub `prepare_once` gab 0 zurueck). Nach der Implementierung gruen.

Was sich nach Deploy sichtbar aendert: der Prep-Worker laedt die (nach Backfill)
deadlock-klassifizierten earlysalty-Clips lokal, der Enrichment-Worker legt je Clip eine
`social_media_clip_enrichment`-Zeile an; die Clips erscheinen im Dashboard mit Transkript/
Titel statt kalt.

Live-Voraussetzungen (operativ, ausserhalb Code-Scope, durch Nutzer zu setzen):
- Backfill-SQL gegen Prod anwenden (siehe offene Schritte).
- Migration gegen Prod als `postgres` anwenden.
- `external_llm_consent=true` in `social_media_settings` (sonst bleibt der LLM-Schritt
  `skipped_no_key`; Download und Enrichment-Zeile entstehen trotzdem).
- `TB_CLIP_FETCHER_ENABLED=1`, damit neue Clips ueberhaupt geholt werden (Gate steht aus).
- `DB_MASTER_KEY_V1` fuer den Upload-Worker (nur fuer Uploads, nicht fuer Download/Enrichment).

### M2 - REQ-01 gespeichertes Layout wirkt im Render: ERLEDIGT

- `layout::get_clip_stored_layout` liefert Override > Streamer-Layout, `None` wenn keins
  gespeichert (kein Fallback auf globalen Default, damit der Render entscheiden kann).
- `video_processor::plan_vertical_render` + `VerticalRender`: Layout -> Compose (Overlay/
  Stacked), sonst Center-Crop.
- `video_processor::compose_and_trim`: schneidet auf Plattform-Laenge, dann `compose_vertical`.
- `upload_worker::convert_to_vertical` nimmt jetzt `clip_db_id`, laedt das gespeicherte
  Layout und komponiert; Center-Crop nur noch als Fallback.

Rot-Test zuerst: `layout::tests::gespeichertes_layout_waehlt_compose_ohne_center_crop`
FAILED - Panic "mit gespeichertem Layout darf NICHT Center-Crop gewaehlt werden"
(Stub `get_clip_stored_layout -> None`). Nach der Verdrahtung gruen.
Lib-Tests: 226 passed, 0 failed (Baseline 224 + M1-Prep + M2).
