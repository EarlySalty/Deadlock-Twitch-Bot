#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 [--watch-record|--vod-only|--watch-live] <twitch-channel-url-or-login> [...]" >&2
  exit 2
fi

cd "$ROOT_DIR"
# PATH des Service-Starts enthält ~/.local/bin nicht zuverlässig → Binaries explizit pinnen.
# /usr/bin/ffmpeg statt ~/.local: der statische Build segfaultet beim Twitch-HLS.
export STREAM_AUDIT_YTDLP_BIN="$ROOT_DIR/.venv/bin/yt-dlp"
export FFMPEG_BIN="/usr/bin/ffmpeg"
exec "$ROOT_DIR/scripts/run_with_infisical.sh" \
  "$ROOT_DIR/.venv/bin/python" \
  "$ROOT_DIR/scripts/audit_stream_tos.py" \
  --authorized \
  --discord-dm \
  "$@"
