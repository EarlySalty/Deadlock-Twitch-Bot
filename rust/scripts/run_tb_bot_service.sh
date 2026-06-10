#!/usr/bin/env bash
# Startet den Rust-tb-bot (interne API 8776 + Monitoring + Raid) als Service.
# Secrets kommen wie beim Python-Worker aus Infisical (export_infisical_env.py);
# Nicht-Secret-Konfiguration wird hier explizit gesetzt.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
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

  echo "Infisical not ready for tb-bot, retrying in ${INFISICAL_RETRY_DELAY}s (attempt $attempt)." >&2
  sleep "$INFISICAL_RETRY_DELAY"
done

# Nicht-Secret-Konfiguration (Werte aus dem Python-Worker übernommen):
export TWITCH_EVENTSUB_CALLBACK_URL="${TWITCH_EVENTSUB_CALLBACK_URL:-https://deutsche-deadlock-community.de/twitch/eventsub/callback}"
export TWITCH_NOTIFY_CHANNEL_ID="${TWITCH_NOTIFY_CHANNEL_ID:-1304169815505637458}"
export TWITCH_TARGET_GAME_NAME="${TWITCH_TARGET_GAME_NAME:-Deadlock}"
# Cutover-Gate: dieser Service IST der Live-Writer.
export TB_MONITORING_POLL_ENABLED="${TB_MONITORING_POLL_ENABLED:-1}"
export RUST_LOG="${RUST_LOG:-info}"

exec "$ROOT_DIR/rust/target/release/tb-bot"
