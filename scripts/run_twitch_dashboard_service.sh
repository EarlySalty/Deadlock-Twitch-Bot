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

INFISICAL_RETRY_DELAY="${INFISICAL_RETRY_DELAY:-5}"
INFISICAL_MAX_ATTEMPTS="${INFISICAL_MAX_ATTEMPTS:-0}"
attempt=0

while true; do
  if INFISICAL_EXPORT="$("$PYTHON_BIN" "$ROOT_DIR/scripts/export_infisical_env.py" --format shell)"; then
    eval "$INFISICAL_EXPORT"
    break
  fi

  attempt=$((attempt + 1))
  if [[ "$INFISICAL_MAX_ATTEMPTS" -gt 0 && "$attempt" -ge "$INFISICAL_MAX_ATTEMPTS" ]]; then
    echo "Infisical secrets could not be loaded after $attempt attempt(s)." >&2
    exit 1
  fi

  echo "Infisical not ready for Twitch Dashboard, retrying in ${INFISICAL_RETRY_DELAY}s (attempt $attempt)." >&2
  sleep "$INFISICAL_RETRY_DELAY"
done

export PYTHONUNBUFFERED=1
export TWITCH_RUNTIME_ROLE=dashboard
export TWITCH_SPLIT_RUNTIME_ROLE=dashboard
export TWITCH_LOG_FILENAME=twitch_dashboard.log

cd "$ROOT_DIR"

WAIT_FOR_INTERNAL_HEALTH="${TWITCH_INTERNAL_API_WAIT_FOR_HEALTH:-1}"
WAIT_TIMEOUT_SEC="${TWITCH_INTERNAL_API_WAIT_TIMEOUT_SEC:-60}"
WAIT_INTERVAL_SEC="${TWITCH_INTERNAL_API_WAIT_INTERVAL_SEC:-2}"

if [[ "$WAIT_FOR_INTERNAL_HEALTH" != "0" && -n "${TWITCH_INTERNAL_API_TOKEN:-}" ]]; then
  deadline=$((SECONDS + WAIT_TIMEOUT_SEC))
  while true; do
    if "$PYTHON_BIN" - <<'PY'
import os
import sys
import urllib.request

from bot.internal_api import INTERNAL_API_BASE_PATH, INTERNAL_TOKEN_HEADER

host = (os.getenv("TWITCH_INTERNAL_API_HOST") or "127.0.0.1").strip()
port = (os.getenv("TWITCH_INTERNAL_API_PORT") or "8776").strip()
base_url = (os.getenv("TWITCH_INTERNAL_API_BASE_URL") or f"http://{host}:{port}").rstrip("/")
token = (os.getenv("TWITCH_INTERNAL_API_TOKEN") or "").strip()

if not token:
    sys.exit(1)

health_url = (
    f"{base_url}/healthz"
    if base_url.endswith(INTERNAL_API_BASE_PATH)
    else f"{base_url}{INTERNAL_API_BASE_PATH}/healthz"
)
request = urllib.request.Request(health_url, headers={INTERNAL_TOKEN_HEADER: token})
try:
    with urllib.request.urlopen(request, timeout=5) as response:
        sys.exit(0 if int(getattr(response, "status", 0) or 0) == 200 else 1)
except Exception:
    sys.exit(1)
PY
    then
      break
    fi

    if (( SECONDS >= deadline )); then
      echo "Bot internal API was not ready before dashboard startup timeout (${WAIT_TIMEOUT_SEC}s)." >&2
      exit 1
    fi

    echo "Waiting for bot internal API readiness before starting dashboard..." >&2
    sleep "$WAIT_INTERVAL_SEC"
  done
fi

exec "$PYTHON_BIN" -m bot.dashboard_service "$@"
