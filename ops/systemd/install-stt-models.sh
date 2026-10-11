#!/usr/bin/env bash
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH

stt_source=/home/nathanael/.cache/deadlock-stt
stt_parent=/opt/deadlock
stt_target=$stt_parent/stt-models
stt_owner=0
stt_group=0
stt_reader=nathanael

stt_fail() {
  printf 'STT-Modellinstallation blockiert: %s\n' "$1" >&2
  return 1
}

stt_directory() {
  local directory=$1
  [[ -d "$directory" && ! -L "$directory" ]] || { stt_fail "Verzeichnis fehlt oder ist ein Symlink: $directory"; return 1; }
  [[ $(stat -c %u -- "$directory") == "$stt_owner" && $(stat -c %g -- "$directory") == "$stt_group" ]] || {
    stt_fail "Verzeichnis hat einen unerwarteten Eigentümer: $directory"; return 1;
  }
  [[ -z $(find "$directory" -maxdepth 0 -perm /022 -print -quit) ]] || {
    stt_fail "Verzeichnis ist durch andere beschreibbar: $directory"; return 1;
  }
}

stt_check_file() {
  local path=$1 expected=$2
  [[ -f "$path" && ! -L "$path" ]] || { stt_fail "Modell fehlt oder ist keine reguläre Datei: $path"; return 1; }
  [[ $(stat -c %u -- "$path") == "$stt_owner" && $(stat -c %g -- "$path") == "$stt_group" && $(stat -c %a -- "$path") == 444 ]] || {
    stt_fail "Modell ist nicht root-eigen und schreibgeschützt: $path"; return 1;
  }
  printf '%s  %s\n' "$expected" "$path" | sha256sum --check --strict --status || {
    stt_fail "Modell hat eine falsche SHA256-Prüfsumme: $path"; return 1;
  }
}

stt_copy_model() {
  local expected=$2 source="$stt_source/$1" destination="$stt_target/$1" temporary directory
  if [[ -e "$destination" || -L "$destination" ]]; then
    stt_check_file "$destination" "$expected"
    return
  fi
  directory=$stt_source
  while :; do
    [[ -d "$directory" && ! -L "$directory" ]] || { stt_fail "Quellverzeichnis fehlt oder ist unsicher: $directory"; return 1; }
    [[ "$directory" == / ]] && break
    directory=$(dirname -- "$directory")
  done
  [[ -f "$source" && ! -L "$source" ]] || { stt_fail "Quellmodell fehlt oder ist ein Symlink: $source"; return 1; }
  temporary=$(mktemp "$stt_target/.incoming-XXXXXXXX")
  if [[ $EUID -eq 0 ]]; then
    if ! runuser -u "$stt_reader" -- /usr/bin/dd "if=$source" iflag=nofollow,nonblock status=none > "$temporary"; then
      rm -f -- "$temporary"
      stt_fail "Quellmodell konnte nicht sicher gelesen werden: $source"
      return 1
    fi
  elif ! /usr/bin/dd "if=$source" iflag=nofollow,nonblock status=none > "$temporary"; then
    rm -f -- "$temporary"
    stt_fail "Quellmodell konnte nicht sicher gelesen werden: $source"
    return 1
  fi
  if ! printf '%s  %s\n' "$expected" "$temporary" | sha256sum --check --strict --status; then
    rm -f -- "$temporary"
    stt_fail "Quellmodell hat eine falsche SHA256-Prüfsumme: $source"
    return 1
  fi
  chmod 0444 "$temporary"
  if [[ -e "$destination" || -L "$destination" ]]; then
    rm -f -- "$temporary"
    stt_fail "Modellziel wurde während der Installation angelegt: $destination"
    return 1
  fi
  mv -T --no-clobber -- "$temporary" "$destination"
  if [[ -e "$temporary" || -L "$temporary" ]]; then
    rm -f -- "$temporary"
    stt_fail "Modellziel konnte nicht exklusiv angelegt werden: $destination"
    return 1
  fi
  stt_check_file "$destination" "$expected"
}

install_stt_models() {
  local entry
  stt_directory "$(dirname -- "$stt_parent")"
  stt_directory "$stt_parent"
  if [[ ! -e "$stt_target" && ! -L "$stt_target" ]]; then
    install -d -m 0755 "$stt_target"
  fi
  stt_directory "$stt_target"
  while IFS= read -r entry; do
    case "$entry" in
      ggml-large-v3-turbo-q5_0.bin|ggml-silero-v6.2.0.bin) ;;
      *) stt_fail "Unbekannter Eintrag im Modellverzeichnis: $entry"; return 1 ;;
    esac
  done < <(find "$stt_target" -mindepth 1 -maxdepth 1 -printf '%f\n')
  stt_copy_model ggml-large-v3-turbo-q5_0.bin 394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2
  stt_copy_model ggml-silero-v6.2.0.bin 2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  [[ $EUID -eq 0 ]] || { stt_fail 'Der Modellinstaller muss als root laufen.'; exit 1; }
  [[ $# -eq 0 ]] || { stt_fail 'Der Modellinstaller nimmt keine Pfadangaben an.'; exit 1; }
  install_stt_models
fi
