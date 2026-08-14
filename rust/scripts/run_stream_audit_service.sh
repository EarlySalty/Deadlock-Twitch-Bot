#!/usr/bin/env bash
# Startet das Coaching-Audit mit Infisical-Secrets, nach demselben Muster wie
# run_tb_bot_service.sh. Der Service-Token kommt aus systemd-Credentials und
# steht bewusst nicht in der Config-Datei.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONFIG_FILE="${INFISICAL_CONFIG_FILE:-$HOME/.config/deadlock-twitch-bot/infisical.conf}"
INFISICAL_LOADER="${INFISICAL_LOADER:-$HOME/.local/bin/dl-infisical-env}"

if [[ ! -f "$CONFIG_FILE" ]]; then
  echo "Infisical-Config fehlt: $CONFIG_FILE" >&2
  exit 1
fi

# set -a, sonst bleiben die Werte Shell-Variablen und der Loader sieht sie
# nicht - er liest ausschliesslich die Umgebung.
set -a
# shellcheck disable=SC1090
source "$CONFIG_FILE"
set +a

# Das systemd-Credential gewinnt gegen einen Wert aus der Config-Datei: es ist
# die Stelle, die rotiert wird. Ein alter Token in infisical.conf hat den
# Loader sonst still scheitern lassen.
if [[ -n "${CREDENTIALS_DIRECTORY:-}" && -r "$CREDENTIALS_DIRECTORY/infisical-token" ]]; then
  INFISICAL_SERVICE_TOKEN="$(<"$CREDENTIALS_DIRECTORY/infisical-token")"
  export INFISICAL_SERVICE_TOKEN
fi

if [[ -z "${INFISICAL_SERVICE_TOKEN:-}" ]]; then
  echo "INFISICAL_SERVICE_TOKEN nicht gesetzt - weder in $CONFIG_FILE noch via systemd-creds." >&2
  exit 3
fi

# Einmal durch den Loader, dann sich selbst erneut aufrufen. DL_INFISICAL_READY
# verhindert die Endlosschleife.
if [[ "${DL_INFISICAL_READY:-0}" != "1" ]]; then
  if [[ ! -x "$INFISICAL_LOADER" ]]; then
    echo "Infisical loader nicht gefunden oder nicht ausfuehrbar: $INFISICAL_LOADER" >&2
    exit 4
  fi
  export DL_INFISICAL_READY=1
  exec "$INFISICAL_LOADER" --profile all -- "$0" "$@"
fi

# Der Service-Token hat hier seine Aufgabe erfuellt. Bliebe er exportiert,
# stuende er in der Umgebung des Audits - und damit in der jedes von ihm
# gestarteten streamlink-Prozesses, lesbar in /proc.
unset INFISICAL_SERVICE_TOKEN
# Die Marke hat ihren Zweck erfuellt; sie muss nicht in streamlink und ffmpeg
# weiterleben.
unset DL_INFISICAL_READY

exec "$ROOT_DIR/rust/target/release/tb-stream-audit" "$@"
