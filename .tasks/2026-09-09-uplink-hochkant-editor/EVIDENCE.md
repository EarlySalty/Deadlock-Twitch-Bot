# Evidence: Uplink-Hochkant-Editor

status: aktiv
datum: 2026-09-09
contract: CONTRACT.md

## Analoge Implementierungen (wie löst das Repo so etwas schon?)

- bot/dashboard_v2/src/components/socialmedia/LayoutEditor.tsx:23: drei benannte Boxen (`game_crop`, `cam_crop`, `cam_position`) mit Eckgriffen (`CORNER_HANDLES`, Zeile 38) und Pointer-Drag mit Clamping (Zeilen 101 bis 135); Bedienmuster wird übernommen, Datenmodell nicht (Pixel in zwei festen Räumen, clipgebunden).
- bot/dashboard_v2/src/components/socialmedia/LayoutEditor.tsx:307: `SourcePreview` und `TargetPreview` (Zeile 361) nebeneinander; gleiche Zweiteilung für Uplink.
- bot/dashboard_v2/src/utils/socialMediaLayout.ts:177: `ausschnittRahmen` rechnet den Renderer nach (crop, scale increase, mittiger Nachschnitt); die Uplink-Variante rechnet normiert und ohne Nachschnitt, weil das Seitenverhältnis gesperrt wird.
- bot/dashboard_v2/src/pages/UplinkZiel.tsx:640: Radio-Fieldset und Warnhinweis-Muster (Zeile 779) der Uplink-Karten.

## Bestehende Abstraktionen (werden wiederverwendet, nicht nachgebaut)

- rust/crates/tb-social-media/src/layout.rs:202: `StreamerLayout` (Pixel, `TARGET_WIDTH=1080`, Zeile 35); bleibt Social-Media-Format, kein Uplink-Format.
- bot/dashboard_v2/src/types/socialMedia.ts:15: `LayoutPayload` mit `version: 1`; die Uplink-Typen leben getrennt in `components/uplink/hochkantLayout.ts`.
- bot/dashboard_v2/src/api/uplink.ts:421: `UplinkProfilAnsicht {width,height,fps,bitrate_kbps}`; Zielprofil kommt als Prop `ziel`.

## Relevante Tests (laufen vorher, laufen nachher)

- bot/dashboard_v2/tests/socialMediaLayout.test.ts:39: Clamping mit Mindestgröße, Drag an der Ecke (Zeile 91), gerade Kantenlängen (Zeile 125); Muster für die normierten Tests.
- bot/dashboard_v2/tests/ausschnittRahmen.test.ts:13: Zielvorschau-Rechnung; Muster für `vorschauAusschnitt`.
- bot/dashboard_v2/tests/uplinkYouTubeLive.test.tsx:1: `renderToStaticMarkup` unter `node --test` mit globalem React (Branch `feat/uplink-youtube-live-lifecycle`); gleiches Muster.
- bot/dashboard_v2/package.json:11: Test-Skript listet jede Testdatei explizit.

## Öffentliche Schnittstellen und Verträge (dürfen nicht brechen)

- rust/crates/tb-social-media/src/video_processor.rs:47: `build_compose_filter` (1080×1920 fest) bleibt unverändert.
- bot/dashboard_v2/src/pages/SocialMedia.tsx:583: Einsatzstellen des Social-Media-Editors bleiben unverändert.

## Änderungsfläche (welche Dateien voraussichtlich angefasst werden)

- bot/dashboard_v2/src/components/uplink/hochkantLayout.ts: Typen und reine Funktionen.
- bot/dashboard_v2/src/components/uplink/UplinkHochkantEditor.tsx: Komponente.
- bot/dashboard_v2/tests/uplinkHochkantLayout.test.ts, tests/uplinkHochkant.test.tsx: Tests.
- bot/dashboard_v2/package.json: Test-Skript.

## Offene Architekturfrage

- keine
