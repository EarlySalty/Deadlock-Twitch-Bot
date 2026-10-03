#!/usr/bin/env bash
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
unset GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES

if [[ $EUID -ne 0 ]]; then
  echo "Der Release-Installer muss als root laufen." >&2
  exit 1
fi
brain_editor_only=0
if [[ $# -eq 3 && $3 == --brain-editor-only ]]; then
  brain_editor_only=1
elif [[ $# -ne 2 ]]; then
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

if [[ $brain_editor_only -eq 1 ]]; then
  release_root=/opt/deadlock/twitch/releases
  current=/opt/deadlock/twitch/current
  for directory in /opt /opt/deadlock /opt/deadlock/twitch "$release_root"; do
    if [[ ! -d "$directory" || -L "$directory" || $(stat -c %u "$directory") != 0 ]] ||
       [[ -n $(find "$directory" -maxdepth 0 -perm /022 -print) ]]; then
      echo "Release-Elternverzeichnis ist nicht ausschließlich rootkontrolliert." >&2
      exit 1
    fi
  done
  if [[ ! -L "$current" || $(stat -c %u "$current") != 0 ]]; then
    echo "Der Editorrelease benötigt einen root-eigenen current-Link." >&2
    exit 1
  fi
  previous_link=$(readlink -- "$current")
  previous_release=$(realpath -- "$current")
  previous_name=$(basename -- "$previous_release")
  if [[ $(dirname -- "$previous_release") != "$release_root" ||
        ! "$previous_name" =~ ^[0-9a-f]{8,40}(-dashboard-[0-9a-f]{8,40})?(-brain-[0-9a-f]{40})?$ ||
        ! -d "$previous_release" || -L "$previous_release" ||
        ! -f "$previous_release/SHA256SUMS" || -L "$previous_release/SHA256SUMS" ]] ||
     [[ -n $(find "$previous_release" -xdev \( ! -user root -o -perm /022 \) -print -quit) ]] ||
     [[ -n $(find "$previous_release" -xdev ! \( -type f -o -type d \) -print -quit) ]]; then
    echo "Der bestehende Release ist nicht sicher kopierbar." >&2
    exit 1
  fi
  (cd "$previous_release" && sha256sum --check --strict SHA256SUMS >/dev/null)
  editor=rust/target/release/tb-config-check
  if [[ ! -f "$checkout/$editor" || -L "$checkout/$editor" ]] ||
     [[ $(readelf --string-dump=.twitch_build "$checkout/$editor" 2>/dev/null | awk '/\[/{print $NF}') != "$git_sha" ]]; then
    echo "Der Konfigeditor stammt nicht aus dem angegebenen sauberen SHA." >&2
    exit 1
  fi
  baseline_revision=$(readelf --string-dump=.twitch_build "$previous_release/rust/target/release/tb-bot" 2>/dev/null | awk '/\[/{print $NF}')
  if [[ ! "$baseline_revision" =~ ^[0-9a-f]{40}$ ||
        "$previous_name" != "$baseline_revision"* && "$previous_name" != "${baseline_revision:0:8}"* ]]; then
    echo "Die bisherige Bot-Herkunft passt nicht zum Release." >&2
    exit 1
  fi
  previous_editor_revision=$baseline_revision
  if [[ "$previous_name" == *-brain-* ]]; then previous_editor_revision=${previous_name##*-brain-}; fi
  for binary in "$previous_release"/rust/target/release/*; do
    revision=$(readelf --string-dump=.twitch_build "$binary" 2>/dev/null | awk '/\[/{print $NF}')
    expected_revision=$baseline_revision
    if [[ $(basename -- "$binary") == tb-config-check ]]; then expected_revision=$previous_editor_revision; fi
    if [[ "$revision" != "$expected_revision" ]]; then
      echo "Ein vorhandenes Binary verletzt den bisherigen SHA-Herkunftsvertrag." >&2
      exit 1
    fi
  done
  base_name=${previous_name%-brain-*}
  release="$release_root/$base_name-brain-$git_sha"
  if [[ -e "$release" || -L "$release" ]]; then
    echo "Der Editorrelease existiert bereits." >&2
    exit 1
  fi
  stage=$(mktemp -d "$release_root/.stage-brain-$git_sha-XXXXXXXX")
  cleanup_editor_stage() {
    if [[ -d "$stage" && "$stage" == "$release_root"/.stage-brain-"$git_sha"-* ]]; then
      find "$stage" -xdev -depth -delete
    fi
  }
  trap cleanup_editor_stage EXIT
  cp -a "$previous_release/." "$stage/"
  install -o root -g root -m 0755 "$checkout/$editor" "$stage/$editor"
  while IFS= read -r -d '' relative; do
    if [[ "$relative" != ./SHA256SUMS && "$relative" != "./$editor" ]]; then
      cmp --silent "$previous_release/$relative" "$stage/$relative"
    fi
  done < <(cd "$previous_release" && find . -type f -print0)
  if [[ $(readelf --string-dump=.twitch_build "$stage/$editor" 2>/dev/null | awk '/\[/{print $NF}') != "$git_sha" ]]; then
    echo "Die installierte Editorherkunft stimmt nicht." >&2
    exit 1
  fi
  (cd "$stage" && find . -type f ! -path ./SHA256SUMS -print0 | sort -z | xargs -0 sha256sum >SHA256SUMS && sha256sum --check --strict SHA256SUMS >/dev/null)
  chmod 0755 "$stage"
  chmod 0644 "$stage/SHA256SUMS"
  if [[ $(readlink -- "$current") != "$previous_link" ]]; then
    echo "current wurde während der Editorinstallation geändert." >&2
    exit 1
  fi
  mv -- "$stage" "$release"
  temporary=$(mktemp -d /opt/deadlock/twitch/.brain-current.XXXXXXXX)
  ln -s -- "releases/$(basename -- "$release")" "$temporary/current"
  mv -Tf -- "$temporary/current" "$current"
  rmdir -- "$temporary"
  trap - EXIT
  printf 'Twitch-Editorrelease aktiviert: %s\n' "$release"
  exit 0
fi

generated=(
  rust/target/release/tb-bot
  rust/target/release/tb-dashboard
  rust/target/release/tb-stream-audit
  rust/target/release/tb-config-check
  bot/analytics/dashboard_v2/dist
  bot/admin_dashboard/dist
  website/dist
)
collector_expected=0
if "${git_safe[@]}" -C "$checkout" cat-file -e "$git_sha:rust/bin/tb-category-collector/Cargo.toml" 2>/dev/null; then
  collector_expected=1
  generated+=(rust/target/release/tb-category-collector)
fi
clip_context_expected=0
watchdog_expected=0
if "${git_safe[@]}" -C "$checkout" cat-file -e "$git_sha:rust/bin/tb-category-collector/src/bin/tb-twitch-watchdog.rs" 2>/dev/null; then
  watchdog_expected=1
  generated+=(rust/target/release/tb-twitch-watchdog)
fi
if "${git_safe[@]}" -C "$checkout" cat-file -e "$git_sha:rust/bin/tb-dashboard/src/bin/clip_context_learn.rs" 2>/dev/null; then
  clip_context_expected=1
  generated+=(rust/target/release/clip_context_learn)
fi
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

# Herkunft direkt aus ELF-Daten lesen; niemals ein Build-Artefakt als root
# ausführen. Ein neuer Checkout-Name macht eine kopierte alte Binary nicht neu.
check_binary_revisions() {
  local source_root="$1" binary embedded_revision
  local binaries=(tb-bot tb-dashboard tb-stream-audit tb-config-check)
  if [[ "$collector_expected" == 1 ]]; then binaries+=(tb-category-collector); fi
  if [[ "$clip_context_expected" == 1 ]]; then binaries+=(clip_context_learn); fi
  if [[ "$watchdog_expected" == 1 ]]; then binaries+=(tb-twitch-watchdog); fi
  for binary in "${binaries[@]}"; do
    embedded_revision="$(readelf --string-dump=.twitch_build "$source_root/rust/target/release/$binary" 2>/dev/null | awk '/\[/{print $NF}')" || embedded_revision=""
    if [[ "$embedded_revision" != "$git_sha" ]]; then
      echo "Build-Herkunft stimmt nicht: $binary muss aus dem sauberen Commit $git_sha neu gebaut werden." >&2
      exit 1
    fi
  done
}
check_binary_revisions "$checkout"

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
  install -m 0755 "$checkout/rust/target/release/tb-config-check" "$stage/rust/target/release/tb-config-check"
  if [[ "$collector_expected" == 1 ]]; then
    install -m 0755 "$checkout/rust/target/release/tb-category-collector" "$stage/rust/target/release/tb-category-collector"
  fi
  if [[ "$clip_context_expected" == 1 ]]; then
    install -m 0755 "$checkout/rust/target/release/clip_context_learn" "$stage/rust/target/release/clip_context_learn"
  fi
  if [[ "$watchdog_expected" == 1 ]]; then
    install -m 0755 "$checkout/rust/target/release/tb-twitch-watchdog" "$stage/rust/target/release/tb-twitch-watchdog"
  fi

  # Skripte, Migrationen und Rollen-SQL kommen direkt aus dem Git-Objekt des
  # angegebenen SHA. Unversionierte Dateien aus dem Build-Baum werden niemals
  # mit postgres- oder Dienstrechten ausgeführt.
  "${git_safe[@]}" -C "$checkout" archive --format=tar "$git_sha" -- \
    rust/scripts/run_tb_bot_service.sh \
    rust/scripts/run_tb_dashboard_service.sh \
    rust/scripts/run_stream_audit_service.sh \
    rust/migrations \
    rust/knowledge \
    ops/systemd \
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
    ops/systemd
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
  # mktemp legt die Stage-Wurzel mit 0700 an. Die Laufzeitnutzer brauchen
  # Traversal-Rechte, dürfen den root-eigenen Release-Baum aber nie ändern.
  chmod 0755 "$stage"
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
check_binary_revisions "$release"

current_tmp=/opt/deadlock/twitch/.current-next
if [[ -e "$current_tmp" || -L "$current_tmp" ]]; then
  echo "Temporärer Current-Link existiert bereits: $current_tmp" >&2
  exit 1
fi
ln -s "releases/$git_sha" "$current_tmp"
mv -Tf -- "$current_tmp" /opt/deadlock/twitch/current

echo "Twitch-Release aktiviert: $git_sha"
