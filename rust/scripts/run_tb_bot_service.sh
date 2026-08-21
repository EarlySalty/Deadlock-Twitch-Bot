#!/usr/bin/env bash
# Startet den Rust-tb-bot (interne API 8776 + Monitoring + Raid) als Service.
# Secrets kommen aus Infisical; Nicht-Secret-Konfiguration wird hier explizit gesetzt.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SYSTEMD_CREDENTIAL_DIR='/run/credentials/deadlock-twitch-bot-rust.service'
if [[ -r "$SYSTEMD_CREDENTIAL_DIR/infisical-token" ]]; then
  CREDENTIALS_DIRECTORY="$SYSTEMD_CREDENTIAL_DIR"
  CONFIG_FILE='/etc/deadlock-twitch/infisical.conf'
  INFISICAL_LOADER='/usr/local/libexec/dl-infisical-env'
  RUNTIME_CONFIG_FILE='/etc/deadlock-twitch/bot.conf'
else
  CONFIG_FILE="${INFISICAL_CONFIG_FILE:-$HOME/.config/deadlock-twitch-bot/infisical.conf}"
  INFISICAL_LOADER="${INFISICAL_LOADER:-/home/naniadm/.local/bin/dl-infisical-env}"
  RUNTIME_CONFIG_FILE="${TWITCH_RUNTIME_CONFIG_FILE:-}"
fi

if [[ ! -f "$CONFIG_FILE" ]]; then
  echo "Missing Infisical config: $CONFIG_FILE" >&2
  exit 1
fi

# Nicht-geheime Verbindungsparameter laden (API-URL, Project-ID, Env-Name).
# INFISICAL_SERVICE_TOKEN steht seit der systemd-creds-Migration NICHT mehr hier.
set -a
# shellcheck source=/dev/null
source "$CONFIG_FILE"
set +a

# Nicht geheime Laufzeitwerte können bei den gehärteten Systemdiensten aus
# einer normalen, root-verwalteten Konfigurationsdatei kommen. Die alte
# user-systemd-Unit setzt sie weiterhin selbst und lässt diesen Pfad leer.
if [[ -n "$RUNTIME_CONFIG_FILE" ]]; then
  if [[ ! -f "$RUNTIME_CONFIG_FILE" ]]; then
    echo "Runtime-Konfiguration fehlt: $RUNTIME_CONFIG_FILE" >&2
    exit 1
  fi
  set -a
  # shellcheck source=/dev/null
  source "$RUNTIME_CONFIG_FILE"
  set +a
fi
unset INFISICAL_SERVICE_TOKEN

# Bootstrap-Token aus systemd-Credentials übernehmen (bevorzugt).
if [[ -n "${CREDENTIALS_DIRECTORY:-}" && -f "$CREDENTIALS_DIRECTORY/infisical-token" ]]; then
  INFISICAL_SERVICE_TOKEN="$(<"$CREDENTIALS_DIRECTORY/infisical-token")"
  export INFISICAL_SERVICE_TOKEN
fi

if [[ -z "${INFISICAL_SERVICE_TOKEN:-}" ]]; then
  echo "INFISICAL_SERVICE_TOKEN nicht gesetzt — weder in $CONFIG_FILE noch via systemd-creds." >&2
  exit 1
fi

if [[ "${DL_INFISICAL_READY:-0}" != "1" ]]; then
  if [[ ! -x "$INFISICAL_LOADER" ]]; then
    echo "Infisical loader nicht gefunden oder nicht ausführbar: $INFISICAL_LOADER" >&2
    exit 1
  fi
  export DL_INFISICAL_READY=1
  exec "$INFISICAL_LOADER" --profile all -- "$0" "$@"
fi
unset DL_INFISICAL_READY
export INFISICAL_WRITE_TOKEN="${INFISICAL_WRITE_TOKEN:-$INFISICAL_SERVICE_TOKEN}"
unset INFISICAL_SERVICE_TOKEN

# Der Bulk-Load kann alte gemeinsame Nicht-Secret-Werte enthalten. Die
# root-verwaltete Dienstkonfiguration gewinnt deshalb unmittelbar vor Start.
if [[ -n "$RUNTIME_CONFIG_FILE" ]]; then
  set -a
  # shellcheck source=/dev/null
  source "$RUNTIME_CONFIG_FILE"
  set +a
fi
unset INFISICAL_SERVICE_TOKEN

# Die gemeinsame Legacy-DSN darf nach der Rollen-Trennung nicht mehr den
# Bot bestimmen. Der dienstspezifische, eingeschränkte Zugang gewinnt; die
# jeweils andere DSN wird vor dem Prozessstart aus der Umgebung entfernt.
if [[ -n "${TWITCH_BOT_ANALYTICS_DSN:-}" ]]; then
  export TWITCH_ANALYTICS_DSN="$TWITCH_BOT_ANALYTICS_DSN"
fi
unset TWITCH_BOT_ANALYTICS_DSN TWITCH_DASHBOARD_ANALYTICS_DSN

# Nicht-Secret-Konfiguration (Werte aus dem bisherigen Worker übernommen):
export TWITCH_EVENTSUB_CALLBACK_URL="${TWITCH_EVENTSUB_CALLBACK_URL:-https://deutsche-deadlock-community.de/twitch/eventsub/callback}"
export TWITCH_NOTIFY_CHANNEL_ID="${TWITCH_NOTIFY_CHANNEL_ID:-1304169815505637458}"
export TWITCH_TARGET_GAME_NAME="${TWITCH_TARGET_GAME_NAME:-Deadlock}"
# Cutover-Gate: dieser Service IST der Live-Writer.
export TB_MONITORING_POLL_ENABLED="${TB_MONITORING_POLL_ENABLED:-1}"
# Scout: entdeckt live DE-Deadlock-Streamer (auch Nicht-Partner) und nimmt sie ins Monitoring.
export TB_SCOUT_ENABLED="${TB_SCOUT_ENABLED:-1}"
# Anonymer IRC-Presence-Harvester (justinfan): joint per anonymem IRC ALLE live
# DE-Streamer aus dem Scout-Roster (auch Nicht-Partner/Ex-Partner OHNE Grant/Mod)
# und sammelt Presence (JOIN/PART/NAMES) -> twitch_session_chatters. Kein Token noetig.
export TB_IRC_LURKER_ENABLED="${TB_IRC_LURKER_ENABLED:-1}"
# Strangler-Fig-Fallback: noch nicht portierte interne API-Routen bleiben per
# Default aus. Leer bedeutet: kein Legacy-Proxy.
export TB_INTERNAL_API_LEGACY_FALLBACK_URL="${TB_INTERNAL_API_LEGACY_FALLBACK_URL:-}"
# Bot-Account für OAuth-Followups (Moderator-Einsetzung): öffentliche User-ID
# von deutschedeadlockcommunity.
export TWITCH_BOT_USER_ID="${TWITCH_BOT_USER_ID:-1422558159}"
# Chat-Cutover-Gate (Welle B): 1 = nativer Chat-Bot (Pipeline, Promos,
# Global-Ban-Sweep, Commands). Flip-Prozedur: rust/docs/04-cutover-plan.md —
# IMMER zuerst den alten Chat-Worker ausschalten, sonst Dual-Refresh-Race auf
# dem Bot-Token.
export TB_CHAT_ENABLED="${TB_CHAT_ENABLED:-0}"
# Crew-Guard (Shadow): erkennt die koordinierte Abwerbe-/Diffamierungs-Kampagne
# (Ricky/blackhusky45-Kreis) im Partner-Chat und meldet Treffer AUSSCHLIESSLICH
# nach Discord (an nani). KEIN Ban/Chat-Post/Whisper. Modell fuer den Judge der
# unbekannten Accounts via OPENAI_API_KEY (aus Infisical).
export CREW_GUARD_ENABLED="${CREW_GUARD_ENABLED:-1}"
export CREW_GUARD_MODEL="${CREW_GUARD_MODEL:-gpt-5.4-mini}"
# Ricky-Review: rein interner Shadow-Modus. Er zeichnet nur beim exakten
# Ziel-Account auf, hält Audio im RAM und sendet niemals in einen Twitch-Chat.
export RICKY_SHADOW_REVIEW_ENABLED="${RICKY_SHADOW_REVIEW_ENABLED:-1}"
export RICKY_SHADOW_REVIEW_CHANNEL_ID="${RICKY_SHADOW_REVIEW_CHANNEL_ID:-1374364800817303632}"
export RICKY_SHADOW_REVIEW_SEGMENT_SECONDS="${RICKY_SHADOW_REVIEW_SEGMENT_SECONDS:-20}"
# Reaktions-Lernmodus: zeichnet auf, worauf der Owner im Chat reagiert, und
# speist Stil und Reaktionsprofil daraus. Transkribiert wird LOKAL gegen
# ops/stt-server (Default-Endpunkt 127.0.0.1:8791); es geht kein Stream-Audio
# an einen Fremdanbieter. Sichtung: ops/learn-samples.sh
export ENGAGEMENT_LEARN_ENABLED="${ENGAGEMENT_LEARN_ENABLED:-1}"
# streamlink liegt im venv, nicht im System-PATH. Ohne diesen Pfad findet der
# Capturer nichts und der Zeitstrahl bekommt nur Chat, keinen Stream-Ton.
if [[ -x /usr/local/libexec/deadlock-streamlink ]]; then
  export VOICE_REACTION_STREAMLINK_BIN="${VOICE_REACTION_STREAMLINK_BIN:-/usr/local/libexec/deadlock-streamlink}"
else
  export VOICE_REACTION_STREAMLINK_BIN="${VOICE_REACTION_STREAMLINK_BIN:-/home/nathanael/stt-tools/bin/streamlink}"
fi
# Kein venv-Default mehr: der Deploy-Baum hat keins, der Pfad zeigte ins Leere.
# Ohne diese Variable sucht der Bot selbst (venv, ~/.local/bin, PATH).
export FFMPEG_BIN="${FFMPEG_BIN:-/usr/bin/ffmpeg}"
export FIREWORKS_BASE_URL="${FIREWORKS_BASE_URL:-https://api.fireworks.ai/inference/v1}"
export TB_LLM_MODEL_RICKY_CREW_REVIEW="${TB_LLM_MODEL_RICKY_CREW_REVIEW:-accounts/fireworks/models/deepseek-v4-flash}"
export RUST_LOG="${RUST_LOG:-info}"
# Bot-Token-Write-Back (ADR 0005): INFISICAL_WRITE_TOKEN wurde oben gesetzt.

exec "$ROOT_DIR/rust/target/release/tb-bot"
