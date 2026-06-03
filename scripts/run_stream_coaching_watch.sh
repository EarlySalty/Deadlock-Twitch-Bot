#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <twitch-channel-url-or-login> [...]" >&2
  exit 2
fi

cd "$ROOT_DIR"
exec "$ROOT_DIR/scripts/run_with_infisical.sh" \
  "$ROOT_DIR/.venv/bin/python" \
  "$ROOT_DIR/scripts/audit_stream_tos.py" \
  --authorized \
  --watch-live \
  --transcriber openai_api \
  --allow-remote-transcription \
  --discord-dm \
  "$@"
