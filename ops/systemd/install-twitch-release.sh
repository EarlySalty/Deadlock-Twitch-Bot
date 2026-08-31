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
build_root=/opt/deadlock/twitch/builds
if [[ "$(dirname -- "$checkout")" != "$build_root" ]]; then
  echo "Der Build muss vor dem Installieren unter $build_root eingefroren werden." >&2
  exit 1
fi
if [[ "$(basename -- "$checkout")" != "$git_sha" ]]; then
  echo "Das Build-Verzeichnis muss exakt den vollständigen Git-SHA tragen." >&2
  exit 1
fi
for frozen_path in "$build_root" "$checkout"; do
  if [[ "$(stat -c '%U:%G' -- "$frozen_path")" != "root:root" ]] ||
     [[ -n "$(find "$frozen_path" -maxdepth 0 -perm /022 -print -quit)" ]]; then
    echo "Build-Pfad ist nicht root-eigen und schreibgeschützt: $frozen_path" >&2
    exit 1
  fi
done

# Ersetzungsobjekte könnten selbst einen expliziten SHA transparent umbiegen.
export GIT_NO_REPLACE_OBJECTS=1
if [[ "$(git -C "$checkout" rev-parse HEAD)" != "$git_sha" ]]; then
  echo "Checkout und angegebener Git-SHA stimmen nicht überein." >&2
  exit 1
fi
if [[ -n "$(git -C "$checkout" status --porcelain --untracked-files=normal)" ]]; then
  echo "Der Checkout enthält Änderungen oder unversionierte Dateien; Release abgebrochen." >&2
  exit 1
fi

generated=(
  rust/target/release/tb-bot
  rust/target/release/tb-dashboard
  rust/target/release/tb-stream-audit
  bot/analytics/dashboard_v2/dist
  bot/admin_dashboard/dist
  website/dist
)
for relative in "${generated[@]}"; do
  if [[ ! -e "$checkout/$relative" ]]; then
    echo "Release-Artefakt fehlt: $relative" >&2
    exit 1
  fi
  unsafe_entry="$(find "$checkout/$relative" -xdev ! \( -type f -o -type d \) -print -quit)"
  if [[ -n "$unsafe_entry" ]]; then
    echo "Release-Artefakt enthält einen Symlink oder Sonderdateityp: $unsafe_entry" >&2
    exit 1
  fi
  unsafe_owner="$(find "$checkout/$relative" -xdev \( ! -user root -o -perm /022 \) -print -quit)"
  if [[ -n "$unsafe_owner" ]]; then
    echo "Release-Artefakt ist nicht eingefroren: $unsafe_owner" >&2
    exit 1
  fi
done

release_root=/opt/deadlock/twitch/releases
install -d -o root -g root -m 0755 "$release_root"
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
  install -m 0755 "$checkout/rust/target/release/tb-stream-audit" "$stage/rust/target/release/tb-stream-audit"

  # Skripte, Migrationen und Rollen-SQL kommen direkt aus dem Git-Objekt des
  # angegebenen SHA. Unversionierte Dateien aus dem Build-Baum werden niemals
  # mit postgres- oder Dienstrechten ausgeführt.
  git -C "$checkout" archive --format=tar "$git_sha" -- \
    rust/scripts/run_tb_bot_service.sh \
    rust/scripts/run_tb_dashboard_service.sh \
    rust/scripts/run_stream_audit_service.sh \
    rust/migrations \
    rust/knowledge \
    ops/systemd/twitch-runtime-roles.sql \
    | tar --extract --file=- --directory="$stage" --no-same-owner --no-same-permissions

  chmod 0755 \
    "$stage/rust/scripts/run_tb_bot_service.sh" \
    "$stage/rust/scripts/run_tb_dashboard_service.sh" \
    "$stage/rust/scripts/run_stream_audit_service.sh"
  chmod 0644 "$stage/ops/systemd/twitch-runtime-roles.sql"
  cp -a "$checkout/bot/analytics/dashboard_v2/dist/." "$stage/bot/analytics/dashboard_v2/dist/"
  cp -a "$checkout/bot/admin_dashboard/dist/." "$stage/bot/admin_dashboard/dist/"
  cp -a "$checkout/website/dist/." "$stage/website/dist/"

  chown -R root:root "$stage"
  chmod -R go-w "$stage"
  if [[ -n "$(find "$stage" -xdev ! \( -type f -o -type d \) -print -quit)" ]]; then
    echo "Der erzeugte Release-Baum enthält einen Symlink oder Sonderdateityp." >&2
    exit 1
  fi
  (
    cd "$stage"
    find . -type f -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
    sha256sum --check --strict SHA256SUMS >/dev/null
  )
  chmod 0644 "$stage/SHA256SUMS"
  mv -- "$stage" "$release"
  trap - EXIT
fi

if [[ ! -d "$release" ]] || [[ -L "$release" ]] ||
   [[ "$(stat -c '%U:%G' -- "$release")" != "root:root" ]] ||
   [[ -n "$(find "$release" -xdev \( ! -user root -o -perm /022 \) -print -quit)" ]] ||
   [[ -n "$(find "$release" -xdev ! \( -type f -o -type d \) -print -quit)" ]] ||
   [[ ! -f "$release/SHA256SUMS" ]]; then
  echo "Bestehender Release-Baum ist nicht vertrauenswürdig: $release" >&2
  exit 1
fi
(
  cd "$release"
  sha256sum --check --strict SHA256SUMS >/dev/null
)

current_tmp=/opt/deadlock/twitch/.current-next
if [[ -e "$current_tmp" || -L "$current_tmp" ]]; then
  echo "Temporärer Current-Link existiert bereits: $current_tmp" >&2
  exit 1
fi
ln -s "releases/$git_sha" "$current_tmp"
mv -Tf -- "$current_tmp" /opt/deadlock/twitch/current

echo "Twitch-Release aktiviert: $git_sha"
