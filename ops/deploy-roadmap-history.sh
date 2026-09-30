#!/usr/bin/env bash
# Deploy der Git-History-Roadmap hinter das Admin-Gate des Dashboards.
#
# Macht sichtbar:  https://admin.deutsche-deadlock-community.de/twitch/admin/roadmap-history/
# Quellen:         /home/nathanael/Documents/Caddy/hosts/v50671/Caddyfile  (Route, validiert)
#                  /home/nathanael/Documents/Deadlock-Twitch-Bot/docs/roadmap/index.html
# Ausfuehren als root:  sudo bash ops/deploy-roadmap-history.sh
#
# Ablauf: Backup der Live-Config, Validierung, Deploy, Inhaltskopie nach
# /srv/deadlock-roadmap, Caddy neu laden (graceful, ohne Unterbrechung),
# Gegenprobe des Session-Gates. Rueckweg: letzte Caddyfile.bak-Datei nach
# /etc/caddy/Caddyfile kopieren und systemctl reload caddy.
set -euo pipefail

caddy_repo_conf="/home/nathanael/Documents/Caddy/hosts/v50671/Caddyfile"
page="/home/nathanael/Documents/Deadlock-Twitch-Bot/docs/roadmap/index.html"
live="/etc/caddy/Caddyfile"
srv="/srv/deadlock-roadmap"
url="https://admin.deutsche-deadlock-community.de/twitch/admin/roadmap-history/"

[[ $EUID -eq 0 ]] || { echo "Bitte als root ausfuehren: sudo bash $0"; exit 1; }
[[ -f $caddy_repo_conf ]] || { echo "Fehlt: $caddy_repo_conf"; exit 1; }
grep -q "roadmap-history" "$caddy_repo_conf" || { echo "Route fehlt in $caddy_repo_conf"; exit 1; }
[[ -f $page ]] || { echo "Fehlt: $page (erst python3 tools/generate_roadmap.py im Twitch-Bot-Repo)"; exit 1; }

backup="$live.bak-$(date +%Y%m%d-%H%M%S)"
cp -a "$live" "$backup"
echo "Backup: $backup"

caddy validate --config "$caddy_repo_conf" >/dev/null
cp "$caddy_repo_conf" "$live"
caddy validate --config "$live" >/dev/null
echo "Config deployt und validiert."

mkdir -p "$srv"
cp "$page" "$srv/index.html"
chmod 0755 "$srv"
chmod 0644 "$srv/index.html"
sudo -u caddy head -c 30 "$srv/index.html" >/dev/null \
	|| { echo "caddy-User kann die Datei nicht lesen"; exit 1; }
echo "Inhalt: $srv/index.html"

systemctl reload caddy
sleep 1
systemctl is-active caddy

echo
echo "Gegenprobe Gate (erwartet: 302 auf /twitch/auth/discord/login):"
curl -sk --resolve admin.deutsche-deadlock-community.de:443:127.0.0.1 \
	-o /dev/null -w '%{http_code} %{redirect_url}\n' "$url"
echo "Gegenprobe Admin-Panel Regression (erwartet: ebenfalls 302):"
curl -sk --resolve admin.deutsche-deadlock-community.de:443:127.0.0.1 \
	-o /dev/null -w '%{http_code} %{redirect_url}\n' \
	https://admin.deutsche-deadlock-community.de/twitch/admin
echo
echo "Fertig. Seite anschliessend mit dem gewohnten Admin-Login oeffnen: $url"
