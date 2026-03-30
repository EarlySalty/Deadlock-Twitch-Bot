# Split Runtime Runbook (Deadlock Twitch Bot)

## Scope
This runbook installs and operates split runtime services:
- `twitch-bot-service`: bot runtime, internal API (`127.0.0.1:8776`), private EventSub callback listener (`127.0.0.1:8768`)
- `twitch-dashboard-service`: standalone dashboard module (`python -m bot.dashboard_service`) on `127.0.0.1:8765`

Public routing is handled by `C:/caddy/Caddyfile`.

## File Locations
- Caddy config: `C:/caddy/Caddyfile`
- NSSM install script: `C:/nssm/install-deadlock-twitch-bot-services.ps1`
- NSSM update script: `C:/nssm/update-deadlock-twitch-bot-services.ps1`
- Restart dashboard only: `C:/nssm/restart-dashboard-only.ps1`
- Restart bot only: `C:/nssm/restart-bot-only.ps1`
- Healthcheck: `C:/nssm/healthcheck-deadlock-twitch-bot.ps1`

## Required Env (Admin PowerShell Session)
Set one shared token that both services use:
```powershell
$env:TWITCH_INTERNAL_API_TOKEN = 'replace-with-strong-random-token'
```
Set Twitch OAuth credentials for the standalone dashboard service:
```powershell
$env:TWITCH_CLIENT_ID = 'replace-with-twitch-client-id'
$env:TWITCH_CLIENT_SECRET = 'replace-with-twitch-client-secret'
```
Set Discord Admin OAuth credentials for `/twitch/admin`:
```powershell
$env:DISCORD_OAUTH_CLIENT_ID = 'replace-with-discord-application-id'
$env:DISCORD_OAUTH_CLIENT_SECRET = 'replace-with-discord-client-secret'
$env:TWITCH_ADMIN_PUBLIC_URL = 'https://admin.earlysalty.de'
$env:TWITCH_ADMIN_DISCORD_REDIRECT_URI = 'https://admin.earlysalty.de/twitch/auth/discord/callback'
```
Optional recovery override for the owner account:
```powershell
$env:TWITCH_ADMIN_OWNER_USER_ID = 'replace-with-discord-user-id'
```
Optional (defaults to `https://twitch.earlysalty.com/twitch/auth/callback`):
```powershell
$env:TWITCH_DASHBOARD_AUTH_REDIRECT_URI = 'https://twitch.earlysalty.com/twitch/auth/callback'
```
Optional: you can override host/port defaults before install/update:
```powershell
$env:TWITCH_INTERNAL_API_HOST = '127.0.0.1'
$env:TWITCH_INTERNAL_API_PORT = '8776'
$env:TWITCH_INTERNAL_API_BASE_URL = 'http://127.0.0.1:8776'
```

## Script Security Baseline (Admin PowerShell)
Use signed scripts where possible and run with a restrictive policy:
```powershell
# Default for local operations
$ExecutionPolicy = 'RemoteSigned'
# Stricter option when all scripts are signed
# $ExecutionPolicy = 'AllSigned'
```
Verify script signature before execution when `AllSigned` is used:
```powershell
Get-AuthenticodeSignature C:/nssm/*.ps1 | Select-Object Path, Status, SignerCertificate
```
Keep script ACLs restricted (admins/system write access only):
```powershell
icacls C:/nssm
```

## Install (Admin PowerShell)
```powershell
powershell -NoProfile -ExecutionPolicy $ExecutionPolicy -File C:/nssm/install-deadlock-twitch-bot-services.ps1
```

## Update Service Definitions (Admin PowerShell)
Use this after changing command paths, env vars, ports, or log settings.
```powershell
powershell -NoProfile -ExecutionPolicy $ExecutionPolicy -File C:/nssm/update-deadlock-twitch-bot-services.ps1
```

## Restart Commands (Admin PowerShell)
```powershell
powershell -NoProfile -ExecutionPolicy $ExecutionPolicy -File C:/nssm/restart-dashboard-only.ps1
powershell -NoProfile -ExecutionPolicy $ExecutionPolicy -File C:/nssm/restart-bot-only.ps1
```

## Legal Access Gate

Die Legal-Seiten `/twitch/impressum` und `/twitch/datenschutz` laufen ueber ein Human-Gate mit Cloudflare Turnstile.

Pfadfluss:

- `GET /twitch/impressum` oder `GET /twitch/datenschutz`
- Redirect nach `GET /twitch/legal/access?next=...`
- Formular-Submit nach `POST /twitch/legal/verify`

Der Dashboard-Service auf `127.0.0.1:8765` muss diese Pfade registriert haben, und Caddy muss sie oeffentlich explizit durchlassen.

Erforderliche Secrets:

- `TWITCH_LEGAL_TURNSTILE_SITE_KEY`
- `TWITCH_LEGAL_TURNSTILE_SECRET_KEY`
- `TWITCH_LEGAL_GATE_COOKIE_SECRET`

Zusetzlich muss die CSP auf `twitch.earlysalty.com` Turnstile erlauben:

- `script-src ... https://challenges.cloudflare.com`
- `frame-src https://challenges.cloudflare.com`

Ohne diese CSP-Freigaben erscheint spaeter serverseitig nur `Turnstile verification failed.`

Die Detaildoku steht in [`docs/LEGAL_ACCESS_GATE.md`](../docs/LEGAL_ACCESS_GATE.md).

## EventSub Routing Decision
- Public `/twitch*` traffic is routed to `twitch-dashboard-service` on `127.0.0.1:8765`.
- Exact path `/twitch/eventsub/callback` is routed to `twitch-bot-service` on `127.0.0.1:8768`.
- This is intentional: webhook signature verification and callback handling remain in bot runtime.
- No conflict: Caddy matches the exact callback path first, then the `/twitch*` matcher.

## Internal API Exposure Policy
- Internal API lives at `http://127.0.0.1:8776/internal/twitch/v1/*`.
- It requires `X-Internal-Token` and is intended for local service-to-service traffic only.
- Caddy does not proxy `/internal/twitch/v1/*` publicly.
- Official raid-integration read surface:
  - `GET /internal/twitch/v1/raid/auth-url?login=discord:<discord_id>`
  - `GET /internal/twitch/v1/raid/auth-state?discord_user_id=<discord_id>`
  - `GET /internal/twitch/v1/raid/block-state?discord_user_id=<discord_id>&twitch_login=<optional>`
- Raid state responses stay small and stable: `ok`, `discord_user_id`, `twitch_login`, `twitch_user_id`, `authorized`, `partner_opt_out`, `token_blacklisted`, `raid_blacklisted`, `blocked`.
- Discord role sync mode is controlled via `TWITCH_DISCORD_ROLE_SYNC_MODE`:
  - `local` keeps the historic Twitch-Bot role behavior (default).
  - `external` disables local streamer-role mutations so an external owner can handle role + notify.
- Keep `127.0.0.1:8766` free for the master admin dashboard service.

## Verification
1. Service-level healthcheck:
```powershell
powershell -NoProfile -ExecutionPolicy $ExecutionPolicy -File C:/nssm/healthcheck-deadlock-twitch-bot.ps1
```
2. Direct local checks:
```powershell
Invoke-WebRequest -UseBasicParsing http://127.0.0.1:8765/twitch | Select-Object StatusCode
Invoke-WebRequest -UseBasicParsing http://127.0.0.1:8765/twitch/legal/access?next=/twitch/impressum | Select-Object StatusCode
Invoke-WebRequest -UseBasicParsing -Headers @{ 'X-Internal-Token' = $env:TWITCH_INTERNAL_API_TOKEN } http://127.0.0.1:8776/internal/twitch/v1/healthz | Select-Object StatusCode
Invoke-WebRequest -UseBasicParsing -Method Get http://127.0.0.1:8768/twitch/eventsub/callback -ErrorAction SilentlyContinue | Select-Object StatusCode
```
3. Caddy validation/reload (if included by main `C:/caddy/Caddyfile`):
```powershell
C:/caddy/caddy.exe validate --config C:/caddy/Caddyfile
C:/caddy/caddy.exe reload --config C:/caddy/Caddyfile
```

4. Public legal gate checks:
```powershell
curl.exe -i "https://twitch.earlysalty.com/twitch/impressum"
curl.exe -i "https://twitch.earlysalty.com/twitch/legal/access?next=/twitch/impressum"
```

Erwartung:

- `/twitch/impressum` -> `302 Found` nach `/twitch/legal/access?...`
- `/twitch/legal/access?...` -> `200 OK`

## Exception (Break-Glass Only)
Only if operations are blocked and there is no signed/remotesigned path available:
```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File C:/nssm/<script>.ps1
```
Risk: `Bypass` disables script trust enforcement and increases the chance of running tampered code. Require explicit approval, short-lived use, and post-incident review.

## Rollback
1. Restore previous Caddy config:
```powershell
Copy-Item C:/caddy/Caddyfile.bak C:/caddy/Caddyfile -Force
```
2. Restore previous NSSM script revisions if you keep backups.
3. Restart services:
```powershell
powershell -NoProfile -ExecutionPolicy $ExecutionPolicy -File C:/nssm/restart-dashboard-only.ps1
powershell -NoProfile -ExecutionPolicy $ExecutionPolicy -File C:/nssm/restart-bot-only.ps1
```
