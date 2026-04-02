#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG_FILE="${INFISICAL_CONFIG_FILE:-$HOME/.config/deadlock-twitch-bot/infisical.env}"

if [[ ! -f "$CONFIG_FILE" ]]; then
  echo "Missing Infisical config: $CONFIG_FILE" >&2
  exit 1
fi

set -a
source "$CONFIG_FILE"
set +a

if [[ -x "$ROOT_DIR/.venv/bin/python" ]]; then
  PYTHON_BIN="${PYTHON_BIN:-$ROOT_DIR/.venv/bin/python}"
else
  PYTHON_BIN="${PYTHON_BIN:-python3}"
fi

eval "$("$PYTHON_BIN" "$ROOT_DIR/scripts/export_infisical_env.py" --format shell)"

export PYTHONUNBUFFERED=1
export TWITCH_RUNTIME_ROLE=twitch_worker
export TWITCH_SPLIT_RUNTIME_ROLE=twitch_worker

cd "$ROOT_DIR"
exec "$PYTHON_BIN" -m bot.bot_service "$@"
