# Evidence: Dashboard-Komponente YouTube-Live

status: aktiv
datum: 2026-09-08
contract: CONTRACT.md

## Analoge Implementierungen (wie löst das Repo so etwas schon?)

- bot/dashboard_v2/src/pages/UplinkZiel.tsx:289: `ZielKarte` mit Props je Plattform; die neue Komponente wird dort später für `youtube` eingehängt.
- bot/dashboard_v2/src/pages/UplinkZiel.tsx:640: Radio-Fieldset "Twitch-Ton" (`rounded-xl border border-border/60 bg-background/40 p-3`, Auswahl `border-primary/60 bg-primary/10`); gleiches Muster für Sichtbarkeit.
- bot/dashboard_v2/src/pages/UplinkZiel.tsx:779: Warnhinweis `rounded-xl border border-warning/30 bg-warning/10 text-warning`; Muster für Blockade und Fehler.

## Bestehende Abstraktionen (werden wiederverwendet, nicht nachgebaut)

- bot/dashboard_v2/src/api/uplink.ts:54: `UplinkVerbindungStatus`; die Komponente erhält nur `verbunden: boolean`, abgeleitet vom Aufrufer.
- bot/dashboard_v2/src/api/uplink.ts:587: `UplinkDestination` bleibt unverändert, YouTube-Live-Felder kommen als eigene Props.
- rust/crates/tb-dashboard-api/src/handlers/plattform_oauth.rs:18: `YOUTUBE_SCOPE = youtube.force-ssl`; derselbe Grant reicht für Broadcasts.
- rust/crates/tb-dashboard-api/src/handlers/plattform_oauth.rs:490: bestehender Weg liest nur `liveStreams` und speichert den Key; kein Broadcast-Lifecycle (Lücke, die der Adapter schließt).

## Relevante Tests (laufen vorher, laufen nachher)

- bot/dashboard_v2/tests/languageProvider.test.tsx:4: `renderToStaticMarkup` unter `node --test` mit globalem `React`; Muster für den Komponententest.
- bot/dashboard_v2/package.json:11: Test-Skript listet jede Testdatei explizit; neue Datei wird angehängt.
- bot/dashboard_v2/src/pages/Uplink.layout.test.tsx:13: Quelltext-Assertions als zweites Muster.

## Öffentliche Schnittstellen und Verträge (dürfen nicht brechen)

- rust/crates/tb-dashboard-api/src/handlers/platform_token.rs:190: `PlatformTokenAntwort` (`platform_user_id`, `scopes`); Kanal-ID kommt daraus, nicht aus der Komponente.

## Änderungsfläche (welche Dateien voraussichtlich angefasst werden)

- bot/dashboard_v2/src/components/uplink/UplinkYouTubeLive.tsx: neue Komponente mit Typen.
- bot/dashboard_v2/tests/uplinkYouTubeLive.test.tsx: Rendertests.
- bot/dashboard_v2/package.json: Test-Skript um die neue Datei ergänzen.

## Offene Architekturfrage

- keine
