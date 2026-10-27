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

dashboard_source=bot/dashboard_v2/dist
if [[ ! -d "$checkout/$dashboard_source" ]]; then
  dashboard_source=bot/analytics/dashboard_v2/dist
fi

generated=(
  rust/target/release/tb-bot
  rust/target/release/tb-dashboard
  rust/target/release/tb-stream-audit
  rust/target/release/tb-config-check
  rust/target/release/tb-llm-usage-recover
  rust/target/release/tb-discord-hub
  "$dashboard_source"
  bot/admin_dashboard/dist
  website/dist
)
social_ai_expected=0
social_ai_config_files=()
if "${git_safe[@]}" -C "$checkout" cat-file -e "$git_sha:rust/crates/tb-social-media/src/ai_editor/mod.rs" 2>/dev/null; then
  social_ai_expected=1
  for social_ai_config_extension in toml yaml; do
    if "${git_safe[@]}" -C "$checkout" cat-file -e "$git_sha:ops/social-ai/editor.$social_ai_config_extension" 2>/dev/null; then
      social_ai_config_files=(
        "ops/social-ai/editor.$social_ai_config_extension"
        "ops/social-ai/pricing-v1.$social_ai_config_extension"
      )
      break
    fi
  done
  generated+=(rust/target/release/tb-stt-server)
fi
collector_expected=0
if "${git_safe[@]}" -C "$checkout" cat-file -e "$git_sha:rust/bin/tb-category-collector/Cargo.toml" 2>/dev/null &&
   ! "${git_safe[@]}" -C "$checkout" cat-file -e "$git_sha:rust/bin/tb-category-collector/src/lib.rs" 2>/dev/null; then
  collector_expected=1
  generated+=(rust/target/release/tb-category-collector)
fi
clip_context_expected=0
watchdog_expected=0
if "${git_safe[@]}" -C "$checkout" cat-file -e "$git_sha:rust/bin/tb-category-collector/src/bin/tb-twitch-watchdog.rs" 2>/dev/null; then
  watchdog_expected=1
  generated+=(rust/target/release/tb-twitch-watchdog)
fi
storage_expected=0
if "${git_safe[@]}" -C "$checkout" cat-file -e "$git_sha:rust/bin/tb-category-collector/src/bin/tb-category-storage.rs" 2>/dev/null; then
  storage_expected=1
  generated+=(rust/target/release/tb-category-storage)
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

release_root=/opt/deadlock/twitch/releases
declare -A artifact_origin=()
write_provenance() {
  local stage_root="$1" binary path
  {
    for binary in "${binaries_expected[@]}"; do
      path="rust/target/release/$binary"
      if [[ -n "${artifact_origin[$path]:-}" ]]; then
        printf '%s %s carried\n' "$path" "${artifact_origin[$path]}"
      else
        printf '%s %s built\n' "$path" "$git_sha"
      fi
    done
    for path in "${dist_release_path[@]}"; do
      if [[ -n "${artifact_origin[$path]:-}" ]]; then
        printf '%s %s carried\n' "$path" "${artifact_origin[$path]}"
      else
        printf '%s %s built\n' "$path" "$git_sha"
      fi
    done
  } | LC_ALL=C sort > "$stage_root/PROVENANCE"
  chmod 0644 "$stage_root/PROVENANCE"
}
declare -A dist_release_path=(
  [bot/dashboard_v2]=bot/analytics/dashboard_v2/dist [bot/admin_dashboard]=bot/admin_dashboard/dist [website]=website/dist
)

carry_reject() {
  echo "Übernommenes Artefakt abgelehnt: $1" >&2
  exit 1
}

workspace_members() {
  "${git_safe[@]}" -C "$checkout" ls-tree -r --name-only "$git_sha" -- rust/crates rust/bin \
    | awk -F/ 'NF == 4 && $4 == "Cargo.toml" {print $1 "/" $2 "/" $3 "/"}'
}

declare -A binary_package_dir=(
  [tb-bot]=rust/bin/tb-bot [tb-dashboard]=rust/bin/tb-dashboard [clip_context_learn]=rust/bin/tb-dashboard
  [tb-stream-audit]=rust/bin/tb-stream-audit [tb-config-check]=rust/crates/tb-config
  [tb-llm-usage-recover]=rust/crates/tb-llm [tb-discord-hub]=rust/bin/tb-discord-hub
  [tb-stt-server]=rust/bin/tb-stt-server [tb-category-collector]=rust/bin/tb-category-collector
  [tb-twitch-watchdog]=rust/bin/tb-category-collector [tb-category-storage]=rust/bin/tb-category-collector
)
declare -A frontend_required_inputs=(
  [bot/dashboard_v2]="bot/dashboard_v2/ bot/shared-theme/"
  [bot/admin_dashboard]="bot/admin_dashboard/ bot/shared-theme/"
  [website]="website/"
)

normalize_reference() {
  local base="$1" reference="$2" joined part normalized prefix
  local -a pieces=() parts=()
  if [[ "$reference" == /* ]]; then joined="$base$reference"; else joined="$base/$reference"; fi
  IFS=/ read -r -a pieces <<< "$joined"
  for part in "${pieces[@]}"; do
    case "$part" in
      ''|.) ;;
      ..)
        (( ${#parts[@]} > 0 )) || return 1
        unset 'parts[-1]'
        ;;
      *) parts+=("$part") ;;
    esac
  done
  normalized=$(IFS=/; printf '%s' "${parts[*]}")
  [[ "$normalized" =~ ^[A-Za-z0-9_./-]* ]]
  prefix="${BASH_REMATCH[0]}"
  if [[ "$prefix" != "$normalized" ]]; then
    if [[ "$prefix" == */* ]]; then prefix="${prefix%/*}/"; else prefix=""; fi
  fi
  if [[ -z "$prefix" || "$prefix" == -* ]]; then
    printf 'UNBOUNDED\n'
  else
    printf '%s\n' "$prefix"
  fi
}

tracked_file() {
  "${git_safe[@]}" -C "$checkout" cat-file -e "$git_sha:$1" 2>/dev/null
}

tracked_content() {
  "${git_safe[@]}" -C "$checkout" show "$git_sha:$1"
}

declare -A workspace_crate_dir=()
load_workspace_map() {
  local manifest entries name path
  manifest="$(tracked_content rust/Cargo.toml)" || carry_reject "Arbeitsbereich nicht lesbar"
  entries="$(sed -nE 's/^[[:space:]]*([A-Za-z0-9_-]+)[[:space:]]*=[[:space:]]*\{.*path[[:space:]]*=[[:space:]]*"([^"]+)".*/\1 \2/p' <<< "$manifest")"
  while read -r name path; do
    [[ -n "$name" ]] || continue
    workspace_crate_dir[$name]="rust/${path%/}/"
  done <<< "$entries"
}

manifest_dependencies() {
  awk '
    /^[[:space:]]*\[/ {
      header = $0
      gsub(/[[:space:]]|\[|\]/, "", header)
      section = ""
      if (header ~ /(^|\.)dev-dependencies(\.|$)/) { next }
      if (header ~ /(^|\.)(build-)?dependencies$/) { section = "table"; next }
      if (header ~ /(^|\.)(build-)?dependencies\.[A-Za-z0-9_-]+$/) {
        section = "entry"
        name = header
        sub(/.*dependencies\./, "", name)
        print "name " name
      }
      next
    }
    section == "table" && match($0, /^[[:space:]]*[A-Za-z0-9_-]+/) {
      name = substr($0, RSTART, RLENGTH)
      gsub(/[[:space:]]/, "", name)
      print "name " name
    }
    section != "" && match($0, /path[[:space:]]*=[[:space:]]*"[^"]+"/) {
      value = substr($0, RSTART, RLENGTH)
      sub(/^path[[:space:]]*=[[:space:]]*"/, "", value)
      sub(/"$/, "", value)
      print "path " value
    }
  '
}

crate_dependency_dirs() {
  local crate="$1" manifest entries kind value resolved
  manifest="$(tracked_content "${crate%/}/Cargo.toml")" || return 1
  entries="$(manifest_dependencies <<< "$manifest")" || return 1
  while read -r kind value; do
    case "$kind" in
      name)
        if [[ -n "${workspace_crate_dir[$value]:-}" ]]; then
          printf '%s\n' "${workspace_crate_dir[$value]}"
        fi
        ;;
      path)
        resolved=$(normalize_reference "${crate%/}" "$value") || continue
        if [[ "$resolved" != UNBOUNDED ]] && tracked_file "${resolved%/}/Cargo.toml"; then
          printf '%s/\n' "${resolved%/}"
        fi
        ;;
    esac
  done <<< "$entries"
}

crate_file_references() {
  local crate="$1" matches status=0 line file reference file_dir base resolved
  matches="$("${git_safe[@]}" -C "$checkout" grep -I -o -E -e '"[^"\\]*\.\./[^"\\]*"' "$git_sha" -- "${crate%/}/*.rs")" || status=$?
  (( status <= 1 )) || return 1
  while IFS= read -r line; do
    [[ -n "$line" ]] || continue
    line="${line#"$git_sha":}"
    file="${line%%:*}"
    reference="${line#*:}"
    reference="${reference#\"}"
    reference="${reference%\"}"
    file_dir="$(dirname -- "$file")"
    for base in "$file_dir" "${crate%/}"; do
      resolved=$(normalize_reference "$base" "$reference") || continue
      printf '%s\n' "$resolved"
    done
  done <<< "$matches"
}

binary_required_inputs() {
  local name="$1" root crate dependencies references dependency reference
  local -A seen=()
  local -a pending=()
  root="${binary_package_dir[$name]:-}"
  [[ -n "$root" ]] || carry_reject "$name hat kein bekanntes Paket"
  load_workspace_map
  pending=("$root/")
  while (( ${#pending[@]} > 0 )); do
    crate="${pending[-1]}"
    unset 'pending[-1]'
    [[ -z "${seen[$crate]:-}" ]] || continue
    seen[$crate]=1
    tracked_file "${crate%/}/Cargo.toml" || carry_reject "$name: Paket $crate fehlt in $git_sha"
    printf '%s\n' "$crate"
    dependencies="$(crate_dependency_dirs "$crate")" || carry_reject "$name: Abhängigkeiten von $crate nicht lesbar"
    references="$(crate_file_references "$crate")" || carry_reject "$name: Dateiverweise von $crate nicht lesbar"
    while read -r dependency; do
      [[ -n "$dependency" && -z "${seen[$dependency]:-}" ]] || continue
      pending+=("$dependency")
    done <<< "$dependencies"
    while read -r reference; do
      [[ -n "$reference" ]] || continue
      [[ "$reference" != UNBOUNDED ]] || carry_reject "$name: $crate liest Dateien über einen offenen Pfad"
      printf '%s\n' "$reference"
    done <<< "$references"
  done
}

required_inputs() {
  local kind="$1" name="$2"
  case "$kind" in
    binary) binary_required_inputs "$name" ;;
    dist)
      [[ -n "${frontend_required_inputs[$name]:-}" ]] || carry_reject "$name hat keine bekannten Eingaben"
      tr ' ' '\n' <<< "${frontend_required_inputs[$name]}"
      ;;
  esac
}

verify_unchanged_sources() {
  local kind="$1" name="$2" origin="$3" inputs="$4" changed member input inside
  local -a members=() input_list=()
  mapfile -t members < <(workspace_members)
  read -r -a input_list <<< "$inputs"
  (( ${#input_list[@]} > 0 )) || carry_reject "$name ohne Eingaben"
  for input in "${input_list[@]}"; do
    [[ "$input" =~ ^[A-Za-z0-9_.][A-Za-z0-9_./-]*$ && "$input" != *..* ]] || carry_reject "$name mit ungültiger Eingabe $input"
  done
  while IFS= read -r -d '' changed; do
    for input in "${input_list[@]}"; do
      if [[ "$changed" == "${input%/}" || "$changed" == "${input%/}/"* ]]; then
        carry_reject "$name: Eingabe $changed hat sich seit $origin geändert"
      fi
    done
    if [[ "$kind" == binary && "$changed" == rust/* && "$changed" != rust/scripts/* && "$changed" != rust/docs/* && "$changed" != *.md ]]; then
      inside=0
      for member in "${members[@]}"; do
        if [[ "$changed" == "$member"* ]]; then
          inside=1
          break
        fi
      done
      [[ $inside -eq 1 ]] || carry_reject "$name: arbeitsbereichsweite Datei $changed hat sich seit $origin geändert"
    fi
  done < <("${git_safe[@]}" -C "$checkout" diff --no-renames -z --name-only "$origin" "$git_sha" --)
}

trusted_parent() {
  local parent="$1" directory="$release_root/$1"
  [[ -d "$directory" && ! -L "$directory" && "$(stat -c '%U:%G' -- "$directory")" == root:root ]] || carry_reject "Parent-Release $parent fehlt oder ist nicht root-eigen"
  [[ -z "$(find "$directory" -maxdepth 0 -perm /022 -print -quit)" ]] || carry_reject "Parent-Release $parent ist beschreibbar"
}

parent_origin() {
  local parent="$1" artifact="$2" provenance="$release_root/$1/PROVENANCE" origin=""
  if [[ -f "$provenance" && ! -L "$provenance" ]]; then
    origin="$(awk -v path="$artifact" '$1 == path {print $2}' "$provenance")"
  fi
  printf '%s\n' "${origin:-$parent}"
}

verify_carried_entry() {
  local kind="$1" name="$2" origin="$3" parent="$4" inputs="$5" artifact parent_path required
  [[ "$origin" =~ ^[0-9a-f]{40}$ && "$parent" =~ ^[0-9a-f]{40}$ ]] || carry_reject "$name mit ungültiger Herkunft"
  "${git_safe[@]}" -C "$checkout" merge-base --is-ancestor "$origin" "$git_sha" || carry_reject "$name: $origin ist kein Vorfahr von $git_sha"
  "${git_safe[@]}" -C "$checkout" merge-base --is-ancestor "$parent" "$git_sha" || carry_reject "$name: Parent $parent ist kein Vorfahr von $git_sha"
  trusted_parent "$parent"
  case "$kind" in
    binary)
      [[ " ${binaries_expected[*]} " == *" $name "* ]] || carry_reject "$name ist kein Release-Binary"
      artifact="rust/target/release/$name"
      parent_path="$artifact"
      cmp --silent -- "$checkout/$artifact" "$release_root/$parent/$parent_path" || carry_reject "$name weicht vom Parent-Release ab"
      ;;
    dist)
      [[ -n "${dist_release_path[$name]:-}" ]] || carry_reject "$name ist kein Release-Frontend"
      artifact="$name/dist"
      if [[ "$name" == bot/dashboard_v2 ]]; then
        artifact="$dashboard_source"
      fi
      parent_path="${dist_release_path[$name]}"
      diff -r --no-dereference -- "$checkout/$artifact" "$release_root/$parent/$parent_path" >/dev/null || carry_reject "$name weicht vom Parent-Release ab"
      ;;
    *) carry_reject "unbekannte Art $kind" ;;
  esac
  [[ "$(parent_origin "$parent" "$parent_path")" == "$origin" ]] || carry_reject "$name: Herkunft $origin passt nicht zum Parent-Release"
  required="$(required_inputs "$kind" "$name" | LC_ALL=C sort -u | tr '\n' ' ')" || carry_reject "$name: Eingaben nicht bestimmbar"
  verify_unchanged_sources "$kind" "$name" "$origin" "$inputs $required"
  artifact_origin[$parent_path]="$origin"
}

expected_binaries() {
  binaries_expected=(tb-bot tb-dashboard tb-stream-audit tb-config-check tb-llm-usage-recover tb-discord-hub)
  if [[ "$social_ai_expected" == 1 ]]; then binaries_expected+=(tb-stt-server); fi
  if [[ "$collector_expected" == 1 ]]; then binaries_expected+=(tb-category-collector); fi
  if [[ "$clip_context_expected" == 1 ]]; then binaries_expected+=(clip_context_learn); fi
  if [[ "$watchdog_expected" == 1 ]]; then binaries_expected+=(tb-twitch-watchdog); fi
  if [[ "$storage_expected" == 1 ]]; then binaries_expected+=(tb-category-storage); fi
}
declare -a binaries_expected=()
expected_binaries

carried_list="$checkout/rust/target/release/carried.list"
if [[ -e "$carried_list" || -L "$carried_list" ]]; then
  [[ -f "$carried_list" && ! -L "$carried_list" ]] || carry_reject "carried.list ist keine reguläre Datei"
  while read -r carry_kind carry_name carry_origin carry_parent carry_inputs; do
    [[ -n "$carry_kind" ]] || continue
    verify_carried_entry "$carry_kind" "$carry_name" "$carry_origin" "$carry_parent" "$carry_inputs"
    printf 'Übernommen geprüft: %s %s Herkunft %s\n' "$carry_kind" "$carry_name" "$carry_origin"
  done < "$carried_list"
fi

load_release_provenance() {
  local provenance="$1/PROVENANCE" path origin state
  [[ -f "$provenance" ]] || return 0
  while read -r path origin state; do
    [[ "$origin" =~ ^[0-9a-f]{40}$ ]] || carry_reject "PROVENANCE enthält eine ungültige Herkunft"
    if [[ "$state" == carried ]]; then
      artifact_origin[$path]="$origin"
    fi
  done < "$provenance"
}

check_binary_revisions() {
  local source_root="$1" binary embedded_revision expected_revision
  local binaries=(tb-bot tb-dashboard tb-stream-audit tb-config-check tb-llm-usage-recover tb-discord-hub)
  if [[ "$social_ai_expected" == 1 ]]; then binaries+=(tb-stt-server); fi
  if [[ "$collector_expected" == 1 ]]; then binaries+=(tb-category-collector); fi
  if [[ "$clip_context_expected" == 1 ]]; then binaries+=(clip_context_learn); fi
  if [[ "$watchdog_expected" == 1 ]]; then binaries+=(tb-twitch-watchdog); fi
  if [[ "$storage_expected" == 1 ]]; then binaries+=(tb-category-storage); fi
  for binary in "${binaries[@]}"; do
    expected_revision="${artifact_origin[rust/target/release/$binary]:-$git_sha}"
    embedded_revision="$(readelf --string-dump=.twitch_build "$source_root/rust/target/release/$binary" 2>/dev/null | awk '/\[/{print $NF}')" || embedded_revision=""
    if [[ "$embedded_revision" != "$expected_revision" ]]; then
      echo "Build-Herkunft stimmt nicht: $binary muss aus dem sauberen Commit $git_sha neu gebaut werden." >&2
      exit 1
    fi
  done
}
check_binary_revisions "$checkout"

required_files=(
  rust/scripts/run_tb_bot_service.sh
  rust/scripts/run_tb_dashboard_service.sh
  rust/scripts/run_stream_audit_service.sh
  ops/systemd/twitch-runtime-roles.sql
  ops/systemd/run-discord-hub
  ops/systemd/deadlock-twitch-discord-hub-rust.service
  ops/systemd/discord-hub.conf
)
if [[ "$social_ai_expected" == 1 ]]; then
  required_files+=(
    "${social_ai_config_files[@]}"
    ops/social-ai/skills/deadlock-understanding.md
    ops/social-ai/skills/highlight-analysis.md
    ops/social-ai/skills/editing-strategy.md
    ops/social-ai/skills/video-layout.md
    ops/social-ai/skills/subtitles.md
    ops/social-ai/skills/copywriting.md
    ops/social-ai/skills/quality-validation.md
    ops/social-ai/skills/feedback-learning.md
  )
fi
check_required_sources() {
  local source_root="$1" required_file
  for required_file in "${required_files[@]}"; do
    if [[ ! -f "$source_root/$required_file" || -L "$source_root/$required_file" ]]; then
      echo "Erforderliche Release-Quelle fehlt oder ist ein Symlink: $required_file" >&2
      exit 1
    fi
  done
}

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
  install -m 0755 "$checkout/rust/target/release/tb-llm-usage-recover" "$stage/rust/target/release/tb-llm-usage-recover"
  install -m 0755 "$checkout/rust/target/release/tb-discord-hub" "$stage/rust/target/release/tb-discord-hub"
  if [[ "$social_ai_expected" == 1 ]]; then
    install -m 0755 "$checkout/rust/target/release/tb-stt-server" "$stage/rust/target/release/tb-stt-server"
  fi
  if [[ "$collector_expected" == 1 ]]; then
    install -m 0755 "$checkout/rust/target/release/tb-category-collector" "$stage/rust/target/release/tb-category-collector"
  fi
  if [[ "$clip_context_expected" == 1 ]]; then
    install -m 0755 "$checkout/rust/target/release/clip_context_learn" "$stage/rust/target/release/clip_context_learn"
  fi
  if [[ "$watchdog_expected" == 1 ]]; then
    install -m 0755 "$checkout/rust/target/release/tb-twitch-watchdog" "$stage/rust/target/release/tb-twitch-watchdog"
  fi
  if [[ "$storage_expected" == 1 ]]; then
    install -m 0755 "$checkout/rust/target/release/tb-category-storage" "$stage/rust/target/release/tb-category-storage"
  fi

  archived_roots=(
    rust/scripts/run_tb_bot_service.sh
    rust/scripts/run_tb_dashboard_service.sh
    rust/scripts/run_stream_audit_service.sh
    rust/migrations
    rust/knowledge
    ops/systemd
  )
  if [[ "$social_ai_expected" == 1 ]]; then
    archived_roots+=(ops/social-ai)
  fi
  "${git_safe[@]}" -C "$checkout" archive --format=tar "$git_sha" -- \
    "${archived_roots[@]}" \
    | tar --extract --file=- --directory="$stage" --no-same-owner --no-same-permissions

  unsafe_archived="$({
    cd "$stage"
    find "${archived_roots[@]}" -xdev ! \( -type f -o -type d \) -print -quit
  })"
  if [[ -n "$unsafe_archived" ]]; then
    echo "Archivierte Release-Quelle ist keine reguläre Datei: $unsafe_archived" >&2
    exit 1
  fi
  check_required_sources "$stage"
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
  cp -a "$checkout/$dashboard_source/." "$stage/bot/analytics/dashboard_v2/dist/"
  cp -a "$checkout/bot/admin_dashboard/dist/." "$stage/bot/admin_dashboard/dist/"
  cp -a "$checkout/website/dist/." "$stage/website/dist/"

  write_provenance "$stage"

  chown -R root:root "$stage"
  chmod -R go-w "$stage"
  chmod 0755 "$stage"
  if [[ -n "$(find "$stage" -xdev ! \( -type f -o -type d \) -print -quit)" ]]; then
    echo "Der erzeugte Release-Baum enthält einen Symlink oder Sonderdateityp." >&2
    exit 1
  fi
  (
    cd "$stage"
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
artifact_origin=()
load_release_provenance "$release"
check_required_sources "$release"
check_binary_revisions "$release"

current_tmp=/opt/deadlock/twitch/.current-next
if [[ -e "$current_tmp" || -L "$current_tmp" ]]; then
  echo "Temporärer Current-Link existiert bereits: $current_tmp" >&2
  exit 1
fi
ln -s "releases/$git_sha" "$current_tmp"
mv -Tf -- "$current_tmp" /opt/deadlock/twitch/current

echo "Twitch-Release aktiviert: $git_sha"
