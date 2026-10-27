# Dono goal overlay (StreamElements)

Goal: every network streamer can show a live donation goal in OBS, served by the Twitch bot like `/twitch/overlay`. Amount comes from the streamer's StreamElements tips.

## Contract
- Postgres migration (new file): per streamer goal config: title, target amount, currency, count-from timestamp (reset), StreamElements channel id, StreamElements JWT encrypted at rest with the existing token encryption the bot uses for OAuth tokens. JWT is write-only: never returned by any API, never logged.
- Backend (tb-dashboard-api, Rust): fetch tips from StreamElements REST (`GET https://api.streamelements.com/kappa/v2/tips/{channelId}` with `Authorization: Bearer <jwt>`, paginate, only tips after count-from), sum amounts, latest tip (username + amount only, no message). Per-streamer cache ~15 s with in-flight dedup like overlay.rs. Errors degrade to last known value, logged without token.
- Public JSON: `GET /twitch/api/v2/public/dono-goal?streamer=<login>` -> `{ok,title,goal,current,currency,last:{id,name,amount}|null}`. No secrets, unknown streamer -> ok:false.
- Public HTML: `GET /twitch/dono-goal?streamer=<login>` based on `prototype.html` (same look), polls the JSON every 10 s, toast on new tip id, `?demo=1` keeps working.
- Dashboard (bot/dashboard_v2, next to OverlayBuilder, same auth level as the overlay builder): set title, target, currency, reset button (count-from = now), StreamElements channel id + JWT field (shows only "verbunden"/"nicht verbunden"), connection test, copyable OBS URL with recommended size 900 x 150, live preview iframe. All UI text natural German with real umlauts.

## Rules
No code comments. No .unwrap() in production paths. Rust checks via cargo-slot. Frontend checked in browser with Moli at 390 and 1440. Secrets never printed.
