#!/usr/bin/env bash
# Generischer Wrapper: lädt Infisical-Secrets in die Sub-Shell und führt
# den übergebenen Befehl aus. Für CLI-Tools / Migrations, die Secrets brauchen
# (z. B. TWITCH_ANALYTICS_DSN, MINIMAX_API_KEY) ohne dass der Aufrufer direkten
# Zugriff auf die Secret-Quellen haben muss.
#
# Beispiele:
#   scripts/run_with_infisical.sh .venv/bin/python bot/migrations/engagement_layer.py
#   scripts/run_with_infisical.sh .venv/bin/python -m bot.cli.some_tool --flag
#
# Konfig: $HOME/.config/deadlock-twitch-bot/infisical.conf (Auth-Daten für
# scripts/export_infisical_env.py). Pattern identisch zu run_twitch_bot_service.sh.

set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <command> [args...]" >&2
  exit 2
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG_FILE="${INFISICAL_CONFIG_FILE:-$HOME/.config/deadlock-twitch-bot/infisical.conf}"

if [[ ! -f "$CONFIG_FILE" ]]; then
  echo "Missing Infisical config: $CONFIG_FILE" >&2
  exit 1
fi

set -a
source "$CONFIG_FILE"
set +a

# Service-Token-Quelle (Parität zu run_twitch_bot_service.sh): unter systemd
# liefert LoadCredential ihn via $CREDENTIALS_DIRECTORY; interaktiv lesen wir
# die Credential-Datei direkt. $(<…) hält den Wert im Var (kein stdout/Log).
if [[ -z "${INFISICAL_SERVICE_TOKEN:-}" ]]; then
  if [[ -n "${CREDENTIALS_DIRECTORY:-}" && -f "$CREDENTIALS_DIRECTORY/infisical-token" ]]; then
    INFISICAL_SERVICE_TOKEN="$(<"$CREDENTIALS_DIRECTORY/infisical-token")"
  elif [[ -f "$HOME/.config/infisical-tokens/infisical-token-twitch" ]]; then
    INFISICAL_SERVICE_TOKEN="$(<"$HOME/.config/infisical-tokens/infisical-token-twitch")"
  fi
  export INFISICAL_SERVICE_TOKEN
fi

if [[ -z "${INFISICAL_SERVICE_TOKEN:-}" ]]; then
  echo "INFISICAL_SERVICE_TOKEN nicht gesetzt — weder in $CONFIG_FILE, via systemd-creds noch in ~/.config/infisical-tokens/." >&2
  exit 1
fi

if [[ -x "$ROOT_DIR/.venv/bin/python" ]]; then
  PYTHON_BIN="${PYTHON_BIN:-$ROOT_DIR/.venv/bin/python}"
else
  PYTHON_BIN="${PYTHON_BIN:-python3}"
fi

INFISICAL_EXPORT="$("$PYTHON_BIN" "$ROOT_DIR/scripts/export_infisical_env.py" --format shell)"
eval "$INFISICAL_EXPORT"

cd "$ROOT_DIR"
exec "$@"
