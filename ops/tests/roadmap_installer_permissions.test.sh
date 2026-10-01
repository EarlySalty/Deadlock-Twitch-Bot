#!/usr/bin/env bash
# Prove the installer staging mode and candidate preflight with the service user.
set -euo pipefail
if [[ ${EUID} -ne 0 ]]; then
  echo 'Test als root starten, damit der Dienstnutzer getrennt geprüft wird.' >&2
  exit 2
fi
repo="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
service_user="${SUDO_USER:-nathanael}"
id "$service_user" >/dev/null
scratch="$(mktemp -d /tmp/roadmap-installer-test.XXXXXXXX)"
trap 'rm -rf -- "$scratch"' EXIT
chmod 0755 "$scratch"
stage="$(mktemp -d "$scratch/.staging.XXXXXXXX")"
install -d -m 0755 "$stage/tools/roadmap-history"
install -m 0644 "$repo/tools/generate_roadmap.py" "$stage/tools/generate_roadmap.py"
for file in features.json index.html style.css app.js history_data.py feature_graph.py legacy-history.json.gz; do
  install -m 0644 "$repo/tools/roadmap-history/$file" "$stage/tools/roadmap-history/$file"
done
if runuser -u "$service_user" -- test -r "$stage/tools/generate_roadmap.py"; then
  echo 'Negativtest fehlgeschlagen: Dienstnutzer kommt durch 0700-Staging.' >&2
  exit 1
fi
chmod 0755 "$stage"
release="$scratch/release"
mv -T -- "$stage" "$release"
runuser -u "$service_user" -- test -r "$release/tools/generate_roadmap.py"
runuser -u "$service_user" -- test -r "$release/tools/roadmap-history/legacy-history.json.gz"
install -d -o "$service_user" -g "$(id -gn "$service_user")" -m 0755 "$scratch/webroot"
output="$scratch/webroot/index.html"
runuser -u "$service_user" -- python3 -B "$release/tools/generate_roadmap.py" --repo "$repo" --ref origin/main --output "$output" >/dev/null
[[ -s $output ]]
printf 'vorherige veröffentlichte Seite' > "$output"
cp -- "$release/tools/roadmap-history/legacy-history.json.gz" "$scratch/cache.backup"
printf 'beschädigt' > "$release/tools/roadmap-history/legacy-history.json.gz"
if runuser -u "$service_user" -- python3 -B "$release/tools/generate_roadmap.py" --repo "$repo" --ref origin/main --output "$output" >/dev/null 2>&1; then
  echo 'Negativtest fehlgeschlagen: beschädigter Snapshot wurde veröffentlicht.' >&2
  exit 1
fi
[[ $(cat "$output") == 'vorherige veröffentlichte Seite' ]]
echo 'Release-Durchgang, Dienstnutzer-Zugriff und Fehler-Preflight bestanden.'
