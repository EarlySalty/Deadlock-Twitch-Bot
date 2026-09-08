#!/bin/sh
# Overlay für ein vollständiges normales Release, kein Ersatz aller Botdateien.
# Paketiert beide gekoppelten Rust-Prozesse und dieselben Dashboard-Assets.
# Keine Installation, Migration, Credentialkopie oder Dienstumschaltung.
set -eu
if [ "$#" -ne 2 ]; then
  echo 'Aufruf: prepare_uplink_release.sh /absolutes/cargo-profil /neues/paket' >&2
  exit 2
fi
case "$1:$2" in /*:/*) ;; *) exit 2 ;; esac
if [ -e "$2" ] || [ -L "$2" ]; then
  echo 'Das Releasepaket darf noch nicht existieren.' >&2
  exit 2
fi
for binary in tb-bot tb-dashboard; do
  if [ ! -x "$1/$binary" ] || [ ! -f "$1/$binary" ] || [ -L "$1/$binary" ]; then
    echo 'Beide gekoppelten Rust-Binaries müssen gebaut sein.' >&2
    exit 1
  fi
done
source_root=$(dirname -- "$(dirname -- "$(dirname -- "$(realpath -- "$0")")")")
if [ ! -f "$source_root/bot/analytics/dashboard_v2/dist/index.html" ]; then
  echo 'Gebautes Dashboard fehlt.' >&2
  exit 1
fi
umask 077
mkdir -- "$2"
mkdir -p -- "$2/rust/target/release" "$2/rust/scripts" "$2/rust/deployment" "$2/rust/migrations" "$2/bot/analytics/dashboard_v2"
for binary in tb-bot tb-dashboard; do install -m 0755 -- "$1/$binary" "$2/rust/target/release/$binary"; done
for launcher in run_tb_bot_service.sh run_tb_dashboard_service.sh; do
  install -m 0755 -- "$source_root/rust/scripts/$launcher" "$2/rust/scripts/$launcher"
done
install -m 0600 -- "$source_root/rust/deployment/uplink.json.example" "$2/rust/deployment/uplink.json.example"
for migration in 20260908210000_twitch_uplink_intent.sql 20260908220000_uplink_target_generations.sql; do
  install -m 0644 -- "$source_root/rust/migrations/$migration" "$2/rust/migrations/$migration"
done
cp -R -- "$source_root/bot/analytics/dashboard_v2/dist" "$2/bot/analytics/dashboard_v2/dist"
(cd -- "$2" && find rust bot -type f -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS)
echo 'Gekoppeltes Bot-/Dashboard-Paket erstellt; nichts aktiviert.'
