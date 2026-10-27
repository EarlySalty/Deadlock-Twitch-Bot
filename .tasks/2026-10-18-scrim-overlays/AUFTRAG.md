# Scrim stream overlays native and flexible

Goal: the static DACHLOCK scrim stream graphics (Drive "Vorlagen für Dach-lock stream scrims") become native overlays of the existing caster overlay system (`tb-dashboard-api` `handlers/caster_overlay.rs`, admin page `bot/admin_dashboard/src/pages/content/CasterOverlay.tsx`). Nothing team- or person-specific may be baked into images. Teams, team logos, casters, event logo and countdown time are set in the admin page.

References in `ref/` (1920x1080 unless noted): CasterView_zeroNimo.png, CasterZeroBacon.png, CasterLeoNimo.png (caster scene), SupShivVsRFDTeamsScreen.png (versus screen), streamcountdownopening.png (countdown), 064e5140...png (Fun Turnier logo), 68617624...webp (Wochenende Scrim Match logo). The user also sent a scoreboard top bar screenshot: left half amber/gold (~#E0A955) fading out to the left, right half blue (~#7F9BD8) fading out to the right, round team logo plus team name in an uppercase serif display font per side, curved centre notch with a faint DACHLOCK logo; sides can swap.

Packages:
- P1 asset store and clean caster scene: Postgres table for uploaded images (team logos, event logos) via new migration, admin upload/list/delete, public image route; scene gets free teams (name, short, logo, not tied to tournament), event logo choice, clean background without baked names/boxes/logos; label restyle as in references.
- P2 scoreboard top bar overlay page (transparent, OBS) with side swap toggle.
- P3 versus screen and countdown screen pages.

Acceptance: every page renders at 1920x1080 with data from the admin page, live checked in Moli; existing caster overlay keeps working; gate ALLOW; deployed live.
