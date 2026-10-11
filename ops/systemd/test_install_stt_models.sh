#!/usr/bin/env bash
set -euo pipefail
repo="$(git -C "$(dirname -- "$0")/../.." rev-parse --show-toplevel)"
cd "$repo"
source ./ops/systemd/install-stt-models.sh
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT
stt_parent=$scratch/parent
stt_target=$stt_parent/stt-models
stt_source=$scratch/source
stt_owner=$(id -u)
stt_group=$(id -g)
stt_reader=$(id -un)
mkdir -p "$stt_target" "$stt_source"
chmod 0755 "$stt_parent" "$stt_target"
stt_directory "$stt_parent"
stt_directory "$stt_target"
name=ggml-silero-v6.2.0.bin
printf 'fixture' > "$stt_source/$name"
expected=$(sha256sum "$stt_source/$name")
expected=${expected%% *}
expected_size=$(stat -c %s -- "$stt_source/$name")
stt_copy_model "$name" "$expected" "$expected_size"
[[ $(stat -c %a "$stt_target/$name") == 444 ]]
first=$(stat -c '%i:%Y' "$stt_target/$name")
rm -- "$stt_source/$name"
stt_copy_model "$name" "$expected" "$expected_size"
[[ $(stat -c '%i:%Y' "$stt_target/$name") == "$first" ]]
printf 'unexpected' > "$stt_target/another-model.bin"
if output=$(install_stt_models 2>&1); then
  printf 'Ein fremdes Modell wurde akzeptiert.\n' >&2
  exit 1
fi
[[ "$output" == *'Unbekannter Eintrag im Modellverzeichnis'* ]]
rm -- "$stt_target/another-model.bin"
chmod 0644 "$stt_target/$name"
if (stt_copy_model "$name" "$expected" "$expected_size") 2>/dev/null; then
  printf 'Ein beschreibbares Ziel wurde akzeptiert.\n' >&2
  exit 1
fi
rm -- "$stt_target/$name"
ln -s /dev/null "$stt_target/$name"
if (stt_copy_model "$name" "$expected" "$expected_size") 2>/dev/null; then
  printf 'Ein Ziel-Symlink wurde akzeptiert.\n' >&2
  exit 1
fi
rm -- "$stt_target/$name"
ln -s /dev/null "$stt_source/$name"
if (stt_copy_model "$name" "$expected" "$expected_size") 2>/dev/null; then
  printf 'Ein Quell-Symlink wurde akzeptiert.\n' >&2
  exit 1
fi
rm -- "$stt_source/$name"
printf 'corrupt' > "$stt_source/$name"
if (stt_copy_model "$name" "$expected" "$expected_size") 2>/dev/null; then
  printf 'Eine falsche Prüfsumme wurde akzeptiert.\n' >&2
  exit 1
fi
[[ ! -e "$stt_target/$name" ]]
if find "$stt_target" -mindepth 1 -print -quit | grep -q .; then
  printf 'Eine fehlgeschlagene Kopie hinterließ Dateien.\n' >&2
  exit 1
fi
printf 'short' > "$stt_source/$name"
if output=$(stt_copy_model "$name" "$expected" "$expected_size" 2>&1); then
  printf 'Ein zu kurzes Quellmodell wurde akzeptiert.\n' >&2
  exit 1
fi
[[ "$output" == *'falsche Größe'* ]]
printf 'wrong!!' > "$stt_source/$name"
if output=$(stt_copy_model "$name" "$expected" "$expected_size" 2>&1); then
  printf 'Ein gleich großes Modell mit falschem Hash wurde akzeptiert.\n' >&2
  exit 1
fi
[[ "$output" == *'SHA256-Prüfsumme'* ]]
truncate -s 1048576 "$stt_source/$name"
if output=$( (ulimit -f 128; stt_copy_model "$name" "$expected" "$expected_size") 2>&1 ); then
  printf 'Ein übergroßes Quellmodell wurde akzeptiert.\n' >&2
  exit 1
fi
[[ "$output" == *'falsche Größe'* ]]
[[ ! -e "$stt_target/$name" ]]
if find "$stt_target" -mindepth 1 -print -quit | grep -q .; then
  printf 'Eine fehlgeschlagene Größenprüfung hinterließ Dateien.\n' >&2
  exit 1
fi
printf 'fixture' > "$stt_source/$name"
source_link=$scratch/source-link
ln -s "$stt_source" "$source_link"
stt_source=$source_link
if (stt_copy_model "$name" "$expected" "$expected_size") 2>/dev/null; then
  printf 'Ein Quellverzeichnis-Symlink wurde akzeptiert.\n' >&2
  exit 1
fi
stt_source=$scratch/source
printf 'fixture' > "$stt_target/$name"
chmod 0444 "$stt_target/$name"
if (stt_copy_model "$name" "$expected" "$((expected_size + 1))") 2>/dev/null; then
  printf 'Ein Ziel mit falscher Größe wurde akzeptiert.\n' >&2
  exit 1
fi
[[ $(stat -c %s -- "$stt_target/$name") == "$expected_size" ]]
printf 'STT-Modellinstallation: Einmalkopie, Wiederverwendung, Größenlimit und Abbruchbedingungen geprüft.\n'
