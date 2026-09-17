#!/usr/bin/env bash
# One-time installation on v50671. Requires an explicitly authorized root shell.
# Does not modify Caddy, authentication, the bot, dashboard binaries or database.
set -euo pipefail
if [[ ${EUID} -ne 0 ]]; then
  echo 'Diese einmalige Installation benötigt root. Der Update-Dienst läuft danach als nathanael.' >&2
  exit 1
fi
src="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
repo=/home/nathanael/repos/Deadlock-Twitch-Bot
assets=/opt/deadlock-roadmap/tools/roadmap-history
webroot=/srv/deadlock-roadmap

[[ -d "$repo/.git" ]] || { echo "Git-Quelle fehlt: $repo" >&2; exit 1; }
id nathanael >/dev/null
for file in features.json index.html style.css model.js app.js history_data.py; do
  [[ -f "$src/tools/roadmap-history/$file" ]] || { echo "Asset fehlt: $file" >&2; exit 1; }
done

# Preserve the original page (including its undated curated notes) outside the
# web root before the first replacement. Never put private backups in public assets.
install -d -o root -g root -m 0750 /var/backups/deadlock-roadmap
if [[ -f "$webroot/index.html" ]]; then
  backup="$(mktemp /var/backups/deadlock-roadmap/index.XXXXXXXX.html)"
  cp -- "$webroot/index.html" "$backup"
  chmod 0640 "$backup"
  printf 'Vorherige Ansicht gesichert: %s\n' "$backup"
fi
install -d -o root -g root -m 0755 /opt/deadlock-roadmap/tools "$assets"
install -m 0644 "$src/tools/generate_roadmap.py" /opt/deadlock-roadmap/tools/generate_roadmap.py
for file in features.json index.html style.css model.js app.js history_data.py; do
  install -m 0644 "$src/tools/roadmap-history/$file" "$assets/$file"
done
install -d -o nathanael -g nathanael -m 0755 "$webroot"
install -m 0644 "$src/ops/systemd/deadlock-roadmap-history.service" /etc/systemd/system/deadlock-roadmap-history.service
install -m 0644 "$src/ops/systemd/deadlock-roadmap-history.timer" /etc/systemd/system/deadlock-roadmap-history.timer
systemctl daemon-reload
# The generator pins the ref and finishes before replacing index.html atomically.
# A failed fetch or build leaves the previous published HTML intact.
systemctl start deadlock-roadmap-history.service
systemctl enable --now deadlock-roadmap-history.timer
printf 'Feature-Historie veröffentlicht; stündlicher Update-Dienst aktiviert.\n'
