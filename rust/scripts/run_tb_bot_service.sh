#!/usr/bin/env bash
# Startet den Rust-tb-bot (interne API 8776 + Monitoring + Raid) als Service.
# Secrets kommen wie beim Python-Worker aus Infisical (export_infisical_env.py);
# Nicht-Secret-Konfiguration wird hier explizit gesetzt.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONFIG_FILE="${INFISICAL_CONFIG_FILE:-$HOME/.config/deadlock-twitch-bot/infisical.conf}"

if [[ ! -f "$CONFIG_FILE" ]]; then
  echo "Missing Infisical config: $CONFIG_FILE" >&2
  exit 1
fi

# Nicht-geheime Verbindungsparameter laden (API-URL, Project-ID, Env-Name).
# INFISICAL_SERVICE_TOKEN steht seit der systemd-creds-Migration NICHT mehr hier.
set -a
source "$CONFIG_FILE"
set +a

# Bootstrap-Token aus systemd-Credentials übernehmen (bevorzugt).
if [[ -n "${CREDENTIALS_DIRECTORY:-}" && -f "$CREDENTIALS_DIRECTORY/infisical-token" ]]; then
  INFISICAL_SERVICE_TOKEN="$(<"$CREDENTIALS_DIRECTORY/infisical-token")"
  export INFISICAL_SERVICE_TOKEN
fi

if [[ -z "${INFISICAL_SERVICE_TOKEN:-}" ]]; then
  echo "INFISICAL_SERVICE_TOKEN nicht gesetzt — weder in $CONFIG_FILE noch via systemd-creds." >&2
  exit 1
fi

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
# Scout: entdeckt live DE-Deadlock-Streamer (auch Nicht-Partner) und nimmt sie ins Monitoring (Python-Parität).
export TB_SCOUT_ENABLED="${TB_SCOUT_ENABLED:-1}"
# Anonymer IRC-Presence-Harvester (justinfan): joint per anonymem IRC ALLE live
# DE-Streamer aus dem Scout-Roster (auch Nicht-Partner/Ex-Partner OHNE Grant/Mod)
# und sammelt Presence (JOIN/PART/NAMES) -> twitch_session_chatters. Kein Token noetig.
export TB_IRC_LURKER_ENABLED="${TB_IRC_LURKER_ENABLED:-1}"
# Strangler-Fig-Fallback: noch nicht portierte interne-API-Routen (Raid-OAuth,
# Blacklist, Analytics, …) gehen an die Legacy-Python-API auf Seitenport 8779
# (Python-Worker, TWITCH_INTERNAL_API_LEGACY_PORT im Takeover-Drop-in).
export TB_INTERNAL_API_LEGACY_FALLBACK_URL="${TB_INTERNAL_API_LEGACY_FALLBACK_URL:-}"
# Bot-Account für OAuth-Followups (Moderator-Einsetzung): öffentliche User-ID
# von deutschedeadlockcommunity — Python löst sie zur Laufzeit aus dem
# Chat-Token auf, Rust besitzt den Chat-Token (noch) nicht.
export TWITCH_BOT_USER_ID="${TWITCH_BOT_USER_ID:-1422558159}"
# Chat-Cutover-Gate (Welle B): 1 = nativer Chat-Bot (Pipeline, Promos,
# Global-Ban-Sweep, Commands). Flip-Prozedur: rust/docs/04-cutover-plan.md —
# IMMER zuerst den Python-Chat ausschalten (TWITCH_RUST_CHAT_TAKEOVER=1 im
# Worker-Drop-in), sonst Dual-Refresh-Race auf dem Bot-Token.
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
export RICKY_SHADOW_REVIEW_YTDLP_BIN="${RICKY_SHADOW_REVIEW_YTDLP_BIN:-$ROOT_DIR/.venv/bin/yt-dlp}"
export FFMPEG_BIN="${FFMPEG_BIN:-/usr/bin/ffmpeg}"
export FIREWORKS_BASE_URL="${FIREWORKS_BASE_URL:-https://api.fireworks.ai/inference/v1}"
export FIREWORKS_RICKY_REVIEW_MODEL="${FIREWORKS_RICKY_REVIEW_MODEL:-accounts/fireworks/models/deepseek-v4-flash}"
export RUST_LOG="${RUST_LOG:-info}"
# Bot-Token-Write-Back (ADR 0005): mangels reinem Write-Token in Infisical
# nutzt der Bot den ohnehin vorhandenen all-rights Service-Token. Ein explizit
# gesetztes INFISICAL_WRITE_TOKEN (z. B. künftige dedizierte Identity) gewinnt.
export INFISICAL_WRITE_TOKEN="${INFISICAL_WRITE_TOKEN:-$INFISICAL_SERVICE_TOKEN}"

exec "$ROOT_DIR/rust/target/release/tb-bot"
