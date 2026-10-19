# Caster overlay: team library, coaches, Discord casters, 1440p60 cameras

Admin page: https://admin.deutsche-deadlock-community.de/twitch/admin/content/caster-overlay
Code: rust/crates/tb-dashboard-api/src/handlers/caster_overlay.rs (+ caster_*.html, caster_overlay_assets.rs), bot/admin_dashboard/src/pages/content/CasterOverlay.tsx, api client.
Previous work: .tasks/2026-10-18-scrim-overlays (live at b4938617f).

User wishes (2026-10-19):
1. Teams can be added permanently (saved team library). The existing free team editor "is built but does not quite work": find and fix what is broken.
2. Each team has a coach; the coach is shown in the overlays (scoreboard, versus, caster scene where teams appear).
3. Caster & camera assignment comes from Discord (role "Turnier Caster" 1495154811799077067 in guild 1289721245281292288), not the list of all Twitch streamers.
4. Casters have a permanent camera: they open their camera page, we send it straight to OBS in the highest quality the camera allows, target 2560x1440 at 60 fps.

Packages (each merged to main separately):
- P1 Discord casters + permanent per-caster camera.
- P2 team library (new migration) + coach + team editor fixes + coach in overlays.
- P3 camera quality 1440p60 end to end (capture, sender encoding, OBS view), actual res/fps visible.

Acceptance: admin page works in Moli at 1440 and 390, teams persist across reload, coach visible on overlays at 1920x1080, caster list only shows Discord role members, camera page publishes 1440p60 when the device supports it (getSettings proof), gate ALLOW, deployed live.
