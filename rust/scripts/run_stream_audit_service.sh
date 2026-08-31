#!/usr/bin/env bash
# Startet das Coaching-Audit mit Infisical-Secrets, nach demselben Muster wie
# run_tb_bot_service.sh. Der Service-Token kommt aus systemd-Credentials und
# steht bewusst nicht in der Config-Datei.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SYSTEMD_CREDENTIAL_DIR='/run/credentials/deadlock-twitch-stream-coaching-watch.service'
if [[ -r "$SYSTEMD_CREDENTIAL_DIR/infisical-token" ]]; then
  CREDENTIALS_DIRECTORY="$SYSTEMD_CREDENTIAL_DIR"
  CONFIG_FILE='/etc/deadlock-twitch/infisical.conf'
  INFISICAL_LOADER='/usr/local/libexec/dl-infisical-env'
  RUNTIME_CONFIG_FILE='/etc/deadlock-twitch/audit.conf'
else
  CONFIG_FILE="${INFISICAL_CONFIG_FILE:-$HOME/.config/deadlock-twitch-bot/infisical.conf}"
  INFISICAL_LOADER="${INFISICAL_LOADER:-$HOME/.local/bin/dl-infisical-env}"
  RUNTIME_CONFIG_FILE="${TWITCH_RUNTIME_CONFIG_FILE:-}"
fi

if [[ ! -f "$CONFIG_FILE" ]]; then
  echo "Infisical-Config fehlt: $CONFIG_FILE" >&2
  # Eigener Code: Exit 1 gehoert dem Programm ("eine Schleife ist gestorben").
  # Beides auf 1 zu legen hiess, den haeufigsten Einrichtungsfehler als
  # Laufzeitfehler zu lesen.
  exit 6
fi

# set -a, sonst bleiben die Werte Shell-Variablen und der Loader sieht sie
# nicht - er liest ausschliesslich die Umgebung.
set -a
# shellcheck disable=SC1090
source "$CONFIG_FILE"
set +a

if [[ -n "$RUNTIME_CONFIG_FILE" ]]; then
  if [[ ! -f "$RUNTIME_CONFIG_FILE" ]]; then
    echo "Runtime-Konfiguration fehlt: $RUNTIME_CONFIG_FILE" >&2
    exit 7
  fi
  set -a
  # shellcheck source=/dev/null
  source "$RUNTIME_CONFIG_FILE"
  set +a
fi
unset INFISICAL_SERVICE_TOKEN

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

# Nicht-Secret-Werte nach dem Bulk-Load erneut aus der root-verwalteten
# Audit-Konfiguration übernehmen. Das rclone-Credential ist unverrückbar an
# das von systemd bereitgestellte, nur für diesen Dienst lesbare File gebunden.
if [[ -n "$RUNTIME_CONFIG_FILE" ]]; then
  set -a
  # shellcheck source=/dev/null
  source "$RUNTIME_CONFIG_FILE"
  set +a
fi
unset INFISICAL_SERVICE_TOKEN
if [[ -z "${CREDENTIALS_DIRECTORY:-}" || ! -r "$CREDENTIALS_DIRECTORY/rclone-config" ]]; then
  echo "Verschlüsseltes rclone-Credential fehlt." >&2
  exit 8
fi
export RCLONE_CONFIG="$CREDENTIALS_DIRECTORY/rclone-config"

BINARY="$ROOT_DIR/rust/target/release/tb-stream-audit"
# Gebaut wird nicht hier, sondern beim Deploy (siehe docs/STREAM_COACHING_AUDIT.md).
# Ohne diese Pruefung endet ein fehlendes Binary als nacktes 203/EXEC im
# Journal - genau der Fehler, an dem der Vorgaengerdienst monatelang haengen
# blieb, ohne dass jemand den Grund sah.
if [[ ! -x "$BINARY" ]]; then
  echo "tb-stream-audit fehlt oder ist nicht ausfuehrbar: $BINARY" >&2
  echo "Bauen: SQLX_OFFLINE=true cargo build --release -p tb-stream-audit-bin" >&2
  exit 5
fi

exec "$BINARY" "$@"
