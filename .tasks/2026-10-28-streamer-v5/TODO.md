# /streamer/v5/ in main-site style

Goal: new page version at /streamer/v5/ applying the design critique (main-site world.css look, colored sections, no gradients/glow/particles/icon grid, visible desktop nav plus left slide-in drawer, real hero, static and labeled raid example, one headline type, shared grid).

State: built, tests 70/70, visually checked in Moli at 1920 and 390 (Moli does not render AVIF/WebP pictures, also not on the main site).

Live proof after deploy:
- https://deutsche-deadlock-community.de/streamer/v5/ returns 200, prerendered "So läuft ein Raid."
- /brand/world.css and fonts load, partner list and ban stats fill from the public APIs.
- /streamer/ and /streamer/v4/ unchanged (shared CSS chunk renamed to gemeinsam-stil, same content).
