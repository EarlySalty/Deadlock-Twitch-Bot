#!/usr/bin/env bash
set -euo pipefail

repo=$(git -C "$(dirname "$(realpath "$0")")" rev-parse --show-toplevel)
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT
cp "$repo/ops/systemd/discord-hub.conf" "$scratch/config"
sed -e "s|/etc/deadlock-twitch/discord-hub.conf|$scratch/config|g" \
  -e "s|/opt/deadlock/twitch/current/rust/target/release/tb-discord-hub|$scratch/binary|g" \
  "$repo/ops/systemd/run-discord-hub" >"$scratch/launcher"
cat >"$scratch/binary" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[[ $DISCORD_DEADLOCK_TWITCH_BOT == synthetic-bot ]]
[[ $DISCORD_DEADLOCK_TWITCH_BOT_INTERNAL_TOKEN == synthetic-hub ]]
[[ $MASTER_BROKER_TOKEN == synthetic-twitch ]]
[[ $MASTER_BROKER_HOST == 127.0.0.1 && $MASTER_BROKER_PORT == 8770 ]]
[[ $DISCORD_DEADLOCK_TWITCH_BOT_HOST == 127.0.0.1 && $DISCORD_DEADLOCK_TWITCH_BOT_PORT == 8773 ]]
[[ ! -v INFISICAL_SERVICE_TOKEN && ! -v DL_DISCORD_HUB_SECRETS_READY && ! -v MASTER_BROKER_BASE_URL ]]
[[ $* == --build-revision ]]
SH
chmod +x "$scratch/binary"
run_launcher() {
  env -i PATH="$PATH" DL_DISCORD_HUB_SECRETS_READY=1 \
    DISCORD_DEADLOCK_TWITCH_BOT=synthetic-bot \
    DISCORD_DEADLOCK_TWITCH_BOT_INTERNAL_TOKEN=synthetic-hub \
    TWITCH_INTERNAL_API_TOKEN=synthetic-twitch \
    INFISICAL_SERVICE_TOKEN=synthetic-bootstrap \
    MASTER_BROKER_TOKEN=synthetic-stale MASTER_BROKER_HOST=192.0.2.1 \
    MASTER_BROKER_BASE_URL=https://invalid.example \
    DISCORD_DEADLOCK_TWITCH_BOT_PORT=8771 \
    "$@" /bin/bash "$scratch/launcher" --build-revision
}
run_launcher
for name in DISCORD_DEADLOCK_TWITCH_BOT DISCORD_DEADLOCK_TWITCH_BOT_INTERNAL_TOKEN TWITCH_INTERNAL_API_TOKEN; do
  if run_launcher /usr/bin/env -u "$name" >"$scratch/output" 2>&1; then
    printf 'Fehlender Pflichtzugang wurde akzeptiert: %s\n' "$name" >&2
    exit 1
  fi
done
printf 'Discord hub launcher contract checks passed.\n'
