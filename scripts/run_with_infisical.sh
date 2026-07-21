#!/usr/bin/env bash
# Generischer Wrapper: lädt Infisical-Secrets in den übergebenen Befehl.
# Konfig: $HOME/.config/deadlock-twitch-bot/infisical.conf.

set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <command> [args...]" >&2
  exit 2
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG_FILE="${INFISICAL_CONFIG_FILE:-$HOME/.config/deadlock-twitch-bot/infisical.conf}"
INFISICAL_LOADER="${INFISICAL_LOADER:-/home/naniadm/.local/bin/dl-infisical-env}"

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

if [[ ! -x "$INFISICAL_LOADER" ]]; then
  echo "Infisical loader nicht gefunden oder nicht ausführbar: $INFISICAL_LOADER" >&2
  exit 1
fi

cd "$ROOT_DIR"
exec "$INFISICAL_LOADER" --profile all -- "$@"
