#!/usr/bin/env bash
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
unset GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES

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
if [[ "$(stat -c '%U:%G' -- "$build_root")" != "root:root" ]] ||
   [[ -n "$(find "$build_root" -maxdepth 0 -perm /022 -print -quit)" ]]; then
  echo "Build-Wurzel ist nicht root-eigen und schreibgeschützt: $build_root" >&2
  exit 1
fi

git_dir="$checkout/.git"
if [[ ! -d "$git_dir" || -L "$git_dir" ]]; then
  echo "Der Release-Checkout muss ein eigenständiger Clone mit internem .git-Verzeichnis sein." >&2
  exit 1
fi
unsafe_git_entry="$(find "$git_dir" -xdev ! \( -type f -o -type d \) -print -quit)"
if [[ -n "$unsafe_git_entry" ]]; then
  echo "Git-Metadaten enthalten einen Symlink oder Sonderdateityp: $unsafe_git_entry" >&2
  exit 1
fi
unsafe_checkout="$(find "$checkout" -xdev \( ! -user root -o \( ! -type l -perm /022 \) \) -print -quit)"
if [[ -n "$unsafe_checkout" ]]; then
  echo "Checkout oder Git-Metadaten sind nicht vollständig eingefroren: $unsafe_checkout" >&2
  exit 1
fi
if [[ -f "$git_dir/objects/info/alternates" ]] ||
   [[ -e "$git_dir/info/attributes" || -L "$git_dir/info/attributes" ]]; then
  echo "Externe Git-Objekte oder unverfolgte Git-Attribute sind für Releases nicht erlaubt." >&2
  exit 1
fi

# Ersetzungsobjekte könnten selbst einen expliziten SHA transparent umbiegen.
export GIT_NO_REPLACE_OBJECTS=1
export GIT_OPTIONAL_LOCKS=0
export GIT_NO_LAZY_FETCH=1
export GIT_ATTR_NOSYSTEM=1
git_safe=(
  git
  -c core.fsmonitor=false
  -c core.hooksPath=/dev/null
  -c core.attributesFile=/dev/null
  -c core.sshCommand=/usr/bin/false
  -c credential.helper=
  -c protocol.allow=never
)
if [[ "$("${git_safe[@]}" -C "$checkout" rev-parse --absolute-git-dir)" != "$git_dir" ]] ||
   [[ "$("${git_safe[@]}" -C "$checkout" rev-parse HEAD)" != "$git_sha" ]]; then
  echo "Checkout und angegebener Git-SHA stimmen nicht überein." >&2
  exit 1
fi
# Kein `git status` als root: lokale Filter/Attribute des zuvor unprivilegierten
# Builders könnten dabei Programme ausführen. Vertrauenswürdige Skripte und SQL
# kommen unten ausschließlich per `git archive` aus dem expliziten SHA;
# generierte Artefakte werden separat auf Typ, Eigentum und Schreibschutz geprüft.

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
  "${git_safe[@]}" -C "$checkout" archive --format=tar "$git_sha" -- \
    rust/scripts/run_tb_bot_service.sh \
    rust/scripts/run_tb_dashboard_service.sh \
    rust/scripts/run_stream_audit_service.sh \
    rust/migrations \
    rust/knowledge \
    ops/systemd/twitch-runtime-roles.sql \
    | tar --extract --file=- --directory="$stage" --no-same-owner --no-same-permissions

  # Vor jedem root-seitigen chmod/chown müssen archivierte Quellen echte
  # reguläre Dateien sein. Ein getrackter Symlink dürfte sonst beim chmod sein
  # Ziel außerhalb des Releases verändern. Außerdem muss der extrahierte
  # Umfang exakt dem Git-Baum entsprechen; export-ignore darf nichts verbergen.
  archived_roots=(
    rust/scripts/run_tb_bot_service.sh
    rust/scripts/run_tb_dashboard_service.sh
    rust/scripts/run_stream_audit_service.sh
    rust/migrations
    rust/knowledge
    ops/systemd/twitch-runtime-roles.sql
  )
  unsafe_archived="$({
    cd "$stage"
    find "${archived_roots[@]}" -xdev ! \( -type f -o -type d \) -print -quit
  })"
  if [[ -n "$unsafe_archived" ]]; then
    echo "Archivierte Release-Quelle ist keine reguläre Datei: $unsafe_archived" >&2
    exit 1
  fi
  for required_file in \
    rust/scripts/run_tb_bot_service.sh \
    rust/scripts/run_tb_dashboard_service.sh \
    rust/scripts/run_stream_audit_service.sh \
    ops/systemd/twitch-runtime-roles.sql; do
    if [[ ! -f "$stage/$required_file" || -L "$stage/$required_file" ]]; then
      echo "Erforderliche Release-Quelle fehlt oder ist ein Symlink: $required_file" >&2
      exit 1
    fi
  done
  expected_archive=
  actual_archive=
  cleanup_archive_lists() {
    if [[ -n "$expected_archive" ]]; then
      unlink -- "$expected_archive" 2>/dev/null || true
    fi
    if [[ -n "$actual_archive" ]]; then
      unlink -- "$actual_archive" 2>/dev/null || true
    fi
  }
  trap 'cleanup_archive_lists; cleanup_stage' EXIT
  expected_archive="$(mktemp "$release_root/.expected-$git_sha-XXXXXXXX")"
  actual_archive="$(mktemp "$release_root/.actual-$git_sha-XXXXXXXX")"
  "${git_safe[@]}" -C "$checkout" ls-tree -rz --name-only "$git_sha" -- \
    "${archived_roots[@]}" | LC_ALL=C sort -z >"$expected_archive"
  (
    cd "$stage"
    find "${archived_roots[@]}" -type f -print0 | LC_ALL=C sort -z
  ) >"$actual_archive"
  if ! cmp --silent "$expected_archive" "$actual_archive"; then
    echo "Archivierter Release-Umfang stimmt nicht mit dem Git-SHA überein." >&2
    exit 1
  fi
  cleanup_archive_lists
  trap cleanup_stage EXIT

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
    # Die Ausgabedatei existiert durch die Shell-Umleitung bereits, bevor
    # `find` startet. Sie darf deshalb nicht ihren eigenen Vorzustand hashen.
    find . -type f ! -path ./SHA256SUMS -print0 \
      | sort -z \
      | xargs -0 sha256sum > SHA256SUMS
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
