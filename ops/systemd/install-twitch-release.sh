#!/usr/bin/env bash
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
  echo "Der Release-Installer muss als root laufen." >&2
  exit 1
fi
if [[ $# -ne 2 ]]; then
  echo "Aufruf: $0 <sauberer-checkout> <vollständiger-git-sha>" >&2
  exit 1
fi

checkout="$(realpath -- "$1")"
git_sha="$2"
if [[ ! "$git_sha" =~ ^[0-9a-f]{40}$ ]]; then
  echo "Ungültiger Git-SHA: erwartet werden 40 kleine Hexzeichen." >&2
  exit 1
fi
if [[ "$(git -C "$checkout" rev-parse HEAD)" != "$git_sha" ]]; then
  echo "Checkout und angegebener Git-SHA stimmen nicht überein." >&2
  exit 1
fi
if [[ -n "$(git -C "$checkout" status --porcelain --untracked-files=no)" ]]; then
  echo "Der Checkout enthält versionierte Änderungen; Release abgebrochen." >&2
  exit 1
fi

required=(
  rust/target/release/tb-bot
  rust/target/release/tb-dashboard
  rust/scripts/run_tb_bot_service.sh
  rust/scripts/run_tb_dashboard_service.sh
  rust/migrations
  rust/knowledge
  ops/systemd/twitch-runtime-roles.sql
  bot/analytics/dashboard_v2/dist
  bot/admin_dashboard/dist
  website/dist
)
for relative in "${required[@]}"; do
  if [[ ! -e "$checkout/$relative" ]]; then
    echo "Release-Artefakt fehlt: $relative" >&2
    exit 1
  fi
done

release_root=/opt/deadlock/twitch/releases
release="$release_root/$git_sha"
if [[ ! -e "$release" ]]; then
  stage="$(mktemp -d "$release_root/.stage-$git_sha-XXXXXXXX")"
  cleanup_stage() {
    if [[ -d "$stage" && "$stage" == "$release_root"/.stage-"$git_sha"-* ]]; then
      find "$stage" -xdev -depth -delete
    fi
  }
  trap cleanup_stage EXIT

  install -d -m 0755 \
    "$stage/rust/target/release" \
    "$stage/rust/scripts" \
    "$stage/rust/migrations" \
    "$stage/rust/knowledge" \
    "$stage/ops/systemd" \
    "$stage/bot/analytics/dashboard_v2/dist" \
    "$stage/bot/admin_dashboard/dist" \
    "$stage/website/dist" \
    "$stage/data/clips" \
    "$stage/logs"

  install -m 0755 "$checkout/rust/target/release/tb-bot" "$stage/rust/target/release/tb-bot"
  install -m 0755 "$checkout/rust/target/release/tb-dashboard" "$stage/rust/target/release/tb-dashboard"
  install -m 0755 "$checkout/rust/scripts/run_tb_bot_service.sh" "$stage/rust/scripts/"
  install -m 0755 "$checkout/rust/scripts/run_tb_dashboard_service.sh" "$stage/rust/scripts/"
  cp -a "$checkout/rust/migrations/." "$stage/rust/migrations/"
  cp -a "$checkout/rust/knowledge/." "$stage/rust/knowledge/"
  install -m 0644 "$checkout/ops/systemd/twitch-runtime-roles.sql" "$stage/ops/systemd/"
  cp -a "$checkout/bot/analytics/dashboard_v2/dist/." "$stage/bot/analytics/dashboard_v2/dist/"
  cp -a "$checkout/bot/admin_dashboard/dist/." "$stage/bot/admin_dashboard/dist/"
  cp -a "$checkout/website/dist/." "$stage/website/dist/"

  chown -R root:root "$stage"
  chmod -R go-w "$stage"
  mv -- "$stage" "$release"
  trap - EXIT
fi

current_tmp=/opt/deadlock/twitch/.current-next
if [[ -e "$current_tmp" || -L "$current_tmp" ]]; then
  echo "Temporärer Current-Link existiert bereits: $current_tmp" >&2
  exit 1
fi
ln -s "releases/$git_sha" "$current_tmp"
mv -Tf -- "$current_tmp" /opt/deadlock/twitch/current

echo "Twitch-Release aktiviert: $git_sha"
