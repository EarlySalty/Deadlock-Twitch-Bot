#!/usr/bin/bash
# Legt ein ausdrücklich übergebenes yt-dlp-Artefakt am festen Release-Buildpfad ab.
# Das Skript lädt nichts herunter und sucht weder in PATH noch in HOME.
set -euo pipefail

PINNED_VERSION='2026.08.19'

if [[ $# -ne 1 ]]; then
  echo "Aufruf: $0 <ausführbares-yt-dlp-artefakt>" >&2
  exit 1
fi

SOURCE_ARTIFACT="$1"
if [[ -L "$SOURCE_ARTIFACT" ]]; then
  echo "yt-dlp-Quellartefakt darf kein Symlink sein: $SOURCE_ARTIFACT" >&2
  exit 1
fi
if [[ ! -e "$SOURCE_ARTIFACT" ]]; then
  echo "yt-dlp-Quellartefakt fehlt: $SOURCE_ARTIFACT" >&2
  exit 1
fi
if [[ ! -f "$SOURCE_ARTIFACT" ]]; then
  echo "yt-dlp-Quellartefakt ist keine reguläre Datei: $SOURCE_ARTIFACT" >&2
  exit 1
fi
if [[ ! -x "$SOURCE_ARTIFACT" ]]; then
  echo "yt-dlp-Quellartefakt ist nicht ausführbar: $SOURCE_ARTIFACT" >&2
  exit 1
fi
if [[ "$(basename -- "$SOURCE_ARTIFACT")" != 'yt-dlp_linux' ]]; then
  echo "Erwartet wird das offizielle Standalone-Artefakt yt-dlp_linux." >&2
  exit 1
fi
SOURCE_LEXICAL="$(realpath -s -e -- "$SOURCE_ARTIFACT")"
SOURCE_RESOLVED="$(realpath -e -- "$SOURCE_ARTIFACT")"
if [[ "$SOURCE_LEXICAL" != "$SOURCE_RESOLVED" ]]; then
  echo "yt-dlp-Quellpfad enthält einen Symlink: $SOURCE_ARTIFACT" >&2
  exit 1
fi
SOURCE_ARTIFACT="$SOURCE_RESOLVED"
exec {SOURCE_FD}<"$SOURCE_ARTIFACT"
SOURCE_FD_PATH="/proc/$$/fd/$SOURCE_FD"
if [[ -L "$SOURCE_ARTIFACT" ]] ||
   [[ ! -f "$SOURCE_FD_PATH" ]] ||
   [[ "$(stat -Lc '%d:%i' -- "$SOURCE_ARTIFACT")" != "$(stat -Lc '%d:%i' -- "$SOURCE_FD_PATH")" ]]; then
  echo "yt-dlp-Quellartefakt wurde während der Prüfung ausgetauscht." >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
CHECKSUM_FILE="$ROOT_DIR/ops/systemd/yt-dlp-linux-$PINNED_VERSION.sha256"
if [[ -L "$CHECKSUM_FILE" || ! -f "$CHECKSUM_FILE" ]]; then
  echo "Gepinntes yt-dlp-Prüfsummenmanifest fehlt oder ist ein Symlink: $CHECKSUM_FILE" >&2
  exit 1
fi
mapfile -t CHECKSUM_LINES <"$CHECKSUM_FILE"
if [[ ${#CHECKSUM_LINES[@]} -ne 1 ]] ||
   [[ ! "${CHECKSUM_LINES[0]}" =~ ^([0-9a-f]{64})[[:space:]][[:space:]]yt-dlp_linux$ ]]; then
  echo "Gepinntes yt-dlp-Prüfsummenmanifest ist ungültig: $CHECKSUM_FILE" >&2
  exit 1
fi
EXPECTED_CHECKSUM="${BASH_REMATCH[1]}"
TARGET_PARENT="$ROOT_DIR/rust/target"
TARGET_DIR="$ROOT_DIR/rust/target/release"
TARGET_ARTIFACT="$TARGET_DIR/yt-dlp"
if [[ -L "$TARGET_PARENT" || -L "$TARGET_DIR" ]]; then
  echo "Build-Zielpfad enthält einen Symlink: $TARGET_DIR" >&2
  exit 1
fi
install -d -m 0755 "$TARGET_DIR"
if [[ "$(realpath -e -- "$TARGET_DIR")" != "$TARGET_DIR" ]]; then
  echo "Build-Zielpfad enthält einen Symlink: $TARGET_DIR" >&2
  exit 1
fi

STAGED_ARTIFACT="$(mktemp "$TARGET_DIR/.yt-dlp-stage-XXXXXXXX")"
cleanup() {
  if [[ -n "${STAGED_ARTIFACT:-}" && -f "$STAGED_ARTIFACT" ]]; then
    unlink -- "$STAGED_ARTIFACT"
  fi
}
trap cleanup EXIT

install -m 0755 -- "$SOURCE_FD_PATH" "$STAGED_ARTIFACT"
exec {SOURCE_FD}<&-
if [[ -L "$STAGED_ARTIFACT" || ! -f "$STAGED_ARTIFACT" || ! -x "$STAGED_ARTIFACT" ]]; then
  echo "Gestuftes yt-dlp-Artefakt ist nicht regulär und ausführbar: $STAGED_ARTIFACT" >&2
  exit 1
fi
ACTUAL_CHECKSUM="$(sha256sum -- "$STAGED_ARTIFACT")"
ACTUAL_CHECKSUM="${ACTUAL_CHECKSUM%% *}"
if [[ "$ACTUAL_CHECKSUM" != "$EXPECTED_CHECKSUM" ]]; then
  echo "yt-dlp-Prüfsumme stimmt nicht; erwartet wird das gepinnte yt-dlp_linux $PINNED_VERSION." >&2
  exit 1
fi
if ! ACTUAL_VERSION="$("$STAGED_ARTIFACT" --version 2>/dev/null)" ||
   [[ "$ACTUAL_VERSION" != "$PINNED_VERSION" ]]; then
  echo "yt-dlp-Version stimmt nicht; erwartet: $PINNED_VERSION." >&2
  exit 1
fi

mv -Tf -- "$STAGED_ARTIFACT" "$TARGET_ARTIFACT"
STAGED_ARTIFACT=
echo "yt-dlp für Release-Build bereit: $TARGET_ARTIFACT"
