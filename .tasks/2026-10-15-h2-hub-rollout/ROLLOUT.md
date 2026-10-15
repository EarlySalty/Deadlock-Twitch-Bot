# H2 hub rollout

## Scope and prerequisites

Deploy only after the integrator has merged the allowed H2 commit into `origin/main`. This package does not deploy, access production databases, send Discord messages, or change OAuth/client routing. The hub is a REST service, not a second gateway session.

Live inspection on 2026-10-15 found the user service `deadlock-bot-rust.service` running `dl-bot` on `127.0.0.1:8770`. Its Rust `dl-broker` router accepts `/internal/master/v1/*` with `X-Internal-Token`, `X-Idempotency-Key`, loopback peer enforcement, and the response envelope already used by the hub. No broker repository change or restart is needed. An authenticated GET of `/internal/master/v1/health` using the existing `TWITCH_INTERNAL_API_TOKEN` returned HTTP 200, with its body discarded and no Discord messages sent. The broker currently falls back to `TWITCH_INTERNAL_API_TOKEN`; neither `MASTER_BROKER_TOKEN` nor `MAIN_BOT_INTERNAL_TOKEN` appears in the Infisical name inventory.

Port 8771 is occupied by `dl-web`, and 8772 and 8776 are also occupied. `discord-hub.conf` therefore binds the hub to `127.0.0.1:8773`, observed free. Recheck before deployment. Do not stop or move the existing listeners. Clients must explicitly target `http://127.0.0.1:8773` when their separate routing cutover happens; the library default remains unchanged. No public Caddy route is needed for this internal API.

## Infisical wiring

The unit loads the existing encrypted bootstrap credential `/etc/credstore.encrypted/deadlock-twitch-infisical.cred`. `run-discord-hub` loads non-secret Infisical coordinates from `/etc/deadlock-twitch/infisical.conf`, uses `/usr/local/libexec/dl-infisical-env --profile all`, then removes the bootstrap variable before executing the hub. This existing profile exports all secrets in its configured path/imports, not a dedicated allowlisted profile. Confirm these exact names are visible to that credential:

| Name | Action |
| --- | --- |
| `DISCORD_DEADLOCK_TWITCH_BOT` | Reuse the existing Discord application bot token. Never generate a replacement locally. |
| `TWITCH_INTERNAL_API_TOKEN` | Reuse the existing shared broker credential. Do not rotate it for H2. |
| `DISCORD_DEADLOCK_TWITCH_BOT_INTERNAL_TOKEN` | Create once in Infisical using 48 cryptographically random bytes encoded as base64, through the approved secret-write workflow. Do not print, place in argv, commit, or write a token file. |

No `MASTER_BROKER_TOKEN`, `MAIN_BOT_INTERNAL_TOKEN`, host, or port secret must be created. The launcher deliberately maps `TWITCH_INTERNAL_API_TOKEN` to the hub process's `MASTER_BROKER_TOKEN`, matching the live broker fallback without changing the broker. It requires a dedicated hub internal token and bot token, failing startup rather than silently running forwarding-only. Future broker credential rotation must update both sides and this mapping together. Host/port belong only in the root-owned non-secret `/etc/deadlock-twitch/discord-hub.conf`, not in Infisical.

Discord client ID: `1537305565292924998`. Existing OAuth redirect: `https://deutsche-deadlock-community.de/twitch/auth/discord/hub/callback`. Do not create another application or request administrator permissions.

## Integrator commands

Run from the clean integrator checkout after merge and ALLOW. `SHA` below must be the verified current `origin/main` SHA, not the feature branch tip. The release builder and deployer perform their existing integrity checks and migration steps; only the integrator may execute them.

```bash
SHA=$(git rev-parse origin/main)
git merge-base --is-ancestor HEAD "$SHA"
ss -ltnp '( sport = :8770 or sport = :8773 )'
systemctl --user is-active deadlock-bot-rust.service
/usr/local/libexec/dl-infisical-env --help
systemd-analyze verify ops/systemd/deadlock-twitch-discord-hub-rust.service
LIVE_SHA=$(basename "$(readlink -e /opt/deadlock/twitch/current)")
if ! git merge-base --is-ancestor "$SHA" "$LIVE_SHA"; then
  ops/twitch-release-build "$SHA"
  sudo -n install -o root -g root -m 0755 ops/systemd/install-twitch-release.sh /usr/local/sbin/install-twitch-release
  sudo -n install -o root -g root -m 0755 ops/systemd/deploy-twitch-release /usr/local/bin/deploy-twitch-release
  sudo -n /usr/local/bin/deploy-twitch-release "$SHA" "$HOME/repos/twitch-release-$SHA" --restart all
  sudo -n systemctl enable deadlock-twitch-discord-hub-rust.service
fi
sudo -n systemctl is-active deadlock-twitch-discord-hub-rust.service
curl --fail --silent --show-error http://127.0.0.1:8773/health | jq -e '.ok == true and .direct == true'
test "$(curl --silent --show-error --output /dev/null --write-out '%{http_code}' http://127.0.0.1:8773/internal/master/v1/health)" = 401
sudo -n /usr/local/bin/deploy-twitch-release --pruefen
```

The deployer installs the unit and installs `discord-hub.conf` only if absent, reloads systemd, and restarts the selected services. `--restart discord-hub` is also supported for a deliberately hub-only rollout; it does not bring the other processes onto the new current SHA. Use `--restart all` for a coherent release and a green global `--pruefen`, which now checks the hub PID, executable release SHA, active state, restart count stability, and deleted executable status too. Enable is explicit so installation alone does not activate an unprovisioned unit at boot.

Verify authenticated forwarding without sending messages or reading community data. Use only the existing broker health endpoint, discard its response body, and keep credentials in process memory and curl stdin, not argv. Run without shell tracing:

```bash
eval "$(/home/nathanael/Documents/Infisical/export_claude_secret.py --secret DISCORD_DEADLOCK_TWITCH_BOT_INTERNAL_TOKEN)"
{
  printf 'header = "X-Internal-Token: %s"\n' "$DISCORD_DEADLOCK_TWITCH_BOT_INTERNAL_TOKEN"
} | curl --config - --fail --silent --show-error --output /dev/null http://127.0.0.1:8773/internal/master/v1/health
result=${PIPESTATUS[1]}
unset DISCORD_DEADLOCK_TWITCH_BOT_INTERNAL_TOKEN
test "$result" -eq 0
```

Require HTTP 401 without a token on that same internal route. `/health` alone proves configuration and listener readiness, not forwarding success, Discord permissions, OAuth completion, or a client routing cutover. Do not test a send-message route against real users.

## Rollback

Before first deployment record the previous current release SHA without reading credentials. If hub verification fails, stop and disable only `deadlock-twitch-discord-hub-rust.service`; existing clients have not been redirected by H2. Do not run the new wrapper against an older release lacking the hub artifact. A whole-release rollback must use the existing approved rollback procedure and must not undo migrations blindly. The main Discord broker remains untouched.
