# Auftrag: Clips sauber und gebrandet fürs Hochformat (Paket A)

## Ziel in Nutzerworten

Die Clips aus der Social-Media-Pipeline sollen so geschnitten und gestaltet sein, dass sie auf TikTok, YouTube Shorts und Instagram Reels gut aussehen und funktionieren, und die Marke der Deutschen Deadlock Community ist drin.

## Befund der Hauptsession (Stand origin/main cf3d7708, Prod-DB 2026-09-29)

- Render-Kette: `rust/crates/tb-social-media/src/render.rs` (`render_clip_vertical`) -> `video_processor.rs` (`build_compose_filter`, `compose_and_trim`, `convert_and_trim`, `burn_subtitles`) mit Layout aus `layout.rs` (`social_media_streamer_layout`, Modi pip, stacked, blur_pad). Untertitel aus `subtitles.rs` (ASS, Arial 54, Gold auf dunklem Balken).
- Einziges gespeichertes Layout: earlysalty, Modus `pip`, `game_crop` 1080x1080, `cam_position` 636x454 oben links. Ergebnis im Test-Render (`cur_grid.png` in diesem Ordner): Game-Bild wird auf ein 608 px breites Mittelstück hochskaliert, HUD links und rechts abgeschnitten (Lebensbalken halbiert, Minimap weg), die Cam liegt als harte Kachel über dem Spiel. Alle anderen Streamer fallen auf Center-Crop zurück.
- Keine Markenelemente im Video: kein Logo, keine Goldkante, kein Kanalname, kein Hook-Text.
- Encode: libx264 `-preset medium -crf 23`, Audio-`loudnorm` nur im Compose-Pfad, Framerate nicht festgelegt. Untertitel werden in einem zweiten Encode-Durchgang eingebrannt (doppelte Qualitätsverluste).
- Untertitel: nur 14 von 43 Clips mit Enrichment haben ein Transkript.
- TikTok: alle drei TikTok-Uploads scheitern mit `403 unaudited_client_can_only_post_to_private_accounts` (Tabelle `twitch_clips_upload_queue`). Die TikTok-App ist nicht auditiert. YouTube-Upload lief einmal erfolgreich.
- Vorschau: `preview_path` zeigt auf einen release-relativen Pfad (`/opt/deadlock/twitch/releases/<sha>/data/clips/...`), der beim nächsten Deploy verschwindet; Clip-Downloads liegen dagegen schon dauerhaft unter `/var/lib/deadlock-twitch-media/clips/`.
- Entwurf des Zielbilds (von der Hauptsession per ffmpeg gerendert): `new_grid.png` in diesem Ordner: Cam als volle Breite oben (rund ein Drittel), Goldlinie, Gameplay darunter, Kanalname, Hook-Text in den ersten Sekunden. Das DDC-Logo war im Entwurf nicht sichtbar, das ist zu lösen.

## Arbeitsschritte

1. **Neues Standard-Format "Stacked Brand"** in `video_processor.rs`/`layout.rs` als Default für alle Streamer mit Facecam-Layout, bestehendes pip und blur_pad bleiben wählbar:
   - 1080x1920, Cam-Streifen oben in voller Breite (Höhe aus dem Layout, Default rund 600 px), 6 bis 8 px Goldkante (Markengold aus `dl-brand`/Dashboard-Tokens, nicht erfinden), Gameplay darunter.
   - Gameplay-Ausschnitt so wählen, dass Fadenkreuz und Kampfgeschehen mittig bleiben und möglichst wenig HUD halb abgeschnitten wird; keine unnötig starke Hochskalierung.
   - Ohne Facecam-Layout: blur_pad statt hartem Center-Crop als Fallback.
2. **Markenelemente** als Overlay im selben Encode: kleines DDC-Logo (vorhandenes Asset aus `bot/dashboard_v2/public/brand/`, als PNG mit Alpha ins Repo, sichtbar auf dunklem und hellem Grund), Kanalname `@<login>` in Gold, beides außerhalb der TikTok-UI-Zonen (unten rund 20 %, rechts rund 12 % frei halten, oben die Statusleiste). Keine Em-Dashes, echte Umlaute in allen Texten.
3. **Hook-Text**: Clip-Titel (bzw. `custom_title`) groß und gut lesbar in den ersten 2 bis 3 Sekunden, auf dem Übergang zwischen Cam und Gameplay; lange Titel umbrechen, Emojis und Sonderzeichen sicher behandeln (drawtext-Escaping bzw. ASS).
4. **Untertitel**: Stil auf Social-Media-Standard heben (fette, gut lesbare Schrift im Repo mitliefern statt System-Arial, Weiß mit schwarzer Kontur und Gold für betonte Wörter oder Gold auf Balken beibehalten, Position im mittleren Drittel über dem TikTok-UI), kurze Cues mit wenigen Wörtern. Hook, Marke und Untertitel in **einem** Encode-Durchgang (ASS für alles oder ein Filtergraph), kein zweites Neukodieren.
5. **Encode**: H.264 High, yuv420p, `-crf 18` oder vergleichbare Bitrate (TikTok/Shorts vertragen 8 bis 12 Mbit/s), Framerate der Quelle bis 60 fps, AAC 192k 48 kHz, `loudnorm` in jedem Pfad, `+faststart`. Werte als Konfig-Konstanten an einer Stelle.
6. **TikTok ohne Audit nutzbar machen**: in `uploaders/tiktok.rs` den Upload-Modus "an TikTok-Entwürfe/Posteingang senden" (Content Posting API, Inbox-Upload mit `video.upload`) als Weg für nicht auditierte Apps einbauen bzw. als Fallback bei `unaudited_client_can_only_post_to_private_accounts`, damit der Streamer den Clip in der App mit aktuellem Sound freigibt. Vorher gegen die offizielle TikTok-Doku prüfen, welche Scopes und Endpunkte das sind, und bestehenden OAuth-Weg erweitern (kein zweiter OAuth-Weg). Den Fehler dem Streamer im Dashboard verständlich zeigen statt stumm `failed`.
7. **Vorschaupfad**: Vorschau-Renders dauerhaft unter dem Media-Verzeichnis (`/var/lib/deadlock-twitch-media/`, derselbe Konfigweg wie die Downloads) ablegen statt release-relativ.
8. **Sichtbeweis**: mit `render_clips` (Bin in `tb-social-media`) mindestens die Clips 124534 und 124453 (Quelle `/var/lib/deadlock-twitch-media/clips/`, lesbar per sudo) rendern, Einzelbilder bei 1 s und 9 s als PNG ins Task-Verzeichnis legen, dazu ein Clip ohne Layout (Fallback).

## Nicht anfassen

- Highlight-Erkennung, `korpus_ernte`, `ops/highlight-detector/` (Paket B).
- Kein Python. Keine Code-Kommentare.
- Keine Modellwechsel, keine neuen LLM-Aufrufe.
- Keine Uploads auf echte Konten während der Arbeit; TikTok nur gegen Sandbox oder mit ausdrücklich privatem Ziel.

## Fertig-Kriterium

- Neues Standardformat mit Marke, Hook und Untertiteln in einem Encode, sichtbar belegt durch die PNGs.
- TikTok-Upload scheitert nicht mehr am fehlenden Audit (Entwurfsweg) oder meldet den Grund sichtbar.
- `cargo fmt`, `cargo clippy -p tb-social-media`, `cargo test -p tb-social-media` grün bzw. nicht schlechter als Baseline; sqlx-Offline-Dateien und Schema-Snapshot mitziehen, falls Queries/Migrationen dazukommen.

## Deploy-Weg

Nach Review und Merge-Gate: Release per `/usr/local/bin/deploy-twitch-release <sha>` (Skill `deploy-restart-selbstdienst`), Migrationen von Hand als postgres (Memory `twitch-connect-rank-live-und-migrator-luecke`). Das macht die Hauptsession, nicht der Worker.
