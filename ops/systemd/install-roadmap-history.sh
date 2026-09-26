#!/usr/bin/env bash
# Install a complete, versioned Roadmap renderer and publish only after validation.
set -euo pipefail
if [[ ${EUID} -ne 0 ]]; then
  echo 'Die Installation benötigt root.' >&2
  exit 1
fi
src="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
repo=/home/nathanael/repos/Deadlock-Twitch-Bot
base=/opt/deadlock-roadmap
webroot=/srv/deadlock-roadmap
unit=/etc/systemd/system/deadlock-roadmap-history.service
timer=/etc/systemd/system/deadlock-roadmap-history.timer
files=(features.json index.html style.css app.js history_data.py feature_graph.py legacy-history.json.gz)
if [[ $src != "$repo" ]] || [[ $(git -C "$src" branch --show-current) != main ]]; then
  echo 'Nur der gemergte main-Checkout darf veröffentlicht werden.' >&2
  exit 1
fi
runuser -u nathanael -- git -C "$repo" fetch --quiet origin main
sha="$(git -C "$src" rev-parse HEAD)"
[[ $sha == "$(git -C "$src" rev-parse origin/main)" ]] || { echo 'main und origin/main stimmen nicht überein.' >&2; exit 1; }
paths=(tools/generate_roadmap.py ops/systemd/deadlock-roadmap-history.service ops/systemd/deadlock-roadmap-history.timer)
for file in "${files[@]}"; do paths+=("tools/roadmap-history/$file"); done
git -C "$src" diff --quiet HEAD -- "${paths[@]}" || { echo 'Roadmap-Quellen sind geändert und uncommitted.' >&2; exit 1; }
for file in "${paths[@]}"; do git -C "$src" ls-files --error-unmatch "$file" >/dev/null; done
install -d -o root -g root -m 0755 "$base/releases"
install -d -o nathanael -g nathanael -m 0755 "$webroot"
install -d -o root -g root -m 0750 /var/backups/deadlock-roadmap
release="$base/releases/$sha"
if [[ ! -d $release ]]; then
  stage="$(mktemp -d "$base/releases/.staging.XXXXXXXX")"
  trap 'rm -rf -- "${stage:-}"' EXIT
  install -d -m 0755 "$stage/tools/roadmap-history"
  install -m 0644 "$src/tools/generate_roadmap.py" "$stage/tools/generate_roadmap.py"
  for file in "${files[@]}"; do install -m 0644 "$src/tools/roadmap-history/$file" "$stage/tools/roadmap-history/$file"; done
  # mktemp -d starts at 0700. The oneshot service runs as nathanael and must
  # traverse the immutable release after the atomic rename.
  chmod 0755 "$stage"
  mv -T -- "$stage" "$release"
  stage=''
  trap - EXIT
fi
for file in "${files[@]}"; do cmp -s "$src/tools/roadmap-history/$file" "$release/tools/roadmap-history/$file" || { echo "Release-Asset abweichend: $file" >&2; exit 1; }; done
cmp -s "$src/tools/generate_roadmap.py" "$release/tools/generate_roadmap.py" || { echo 'Release-Generator abweichend.' >&2; exit 1; }
# Prove the complete legacy cache and current Git history before touching the live page.
candidate="$(mktemp "$webroot/.roadmap-candidate.XXXXXXXX")"
chown nathanael:nathanael "$candidate"
if ! runuser -u nathanael -- python3 "$release/tools/generate_roadmap.py" --repo "$repo" --ref origin/main --output "$candidate"; then
  rm -f -- "$candidate"
  exit 1
fi
old_link="$(readlink "$base/current" 2>/dev/null || true)"
old_timer_active="$(systemctl is-active deadlock-roadmap-history.timer 2>/dev/null || true)"
old_timer_enabled="$(systemctl is-enabled deadlock-roadmap-history.timer 2>/dev/null || true)"
backup="$(mktemp /var/backups/deadlock-roadmap/index.XXXXXXXX.html)"
old_page_present=0
if [[ -f $webroot/index.html ]]; then
  old_page_present=1
  cp -- "$webroot/index.html" "$backup"
fi
chmod 0640 "$backup"
old_unit="$(mktemp /var/backups/deadlock-roadmap/service.XXXXXXXX)"
if [[ -f $unit ]]; then cp -- "$unit" "$old_unit"; else : > "$old_unit"; fi
old_timer="$(mktemp /var/backups/deadlock-roadmap/timer.XXXXXXXX)"
if [[ -f $timer ]]; then cp -- "$timer" "$old_timer"; else : > "$old_timer"; fi
rollback() {
  local status=$?
  if (( status != 0 )); then
    systemctl stop deadlock-roadmap-history.timer deadlock-roadmap-history.service || true
    if [[ -n $old_link ]]; then ln -s "$old_link" "$base/.current-rollback"; mv -Tf "$base/.current-rollback" "$base/current"; else rm -f "$base/current"; fi
    rm -f -- "$base/.current-next"
    if [[ -s $old_unit ]]; then cp -- "$old_unit" "$unit"; else rm -f "$unit"; fi
    if [[ -s $old_timer ]]; then cp -- "$old_timer" "$timer"; else rm -f "$timer"; fi
    if (( old_page_present )); then
      restore="$(mktemp "$webroot/.roadmap-restore.XXXXXXXX")"
      cp -- "$backup" "$restore"
      chown nathanael:nathanael "$restore"
      chmod 0644 "$restore"
      mv -Tf -- "$restore" "$webroot/index.html"
    else rm -f -- "$webroot/index.html"; fi
    systemctl daemon-reload || true
    if [[ $old_timer_enabled == enabled ]]; then systemctl enable deadlock-roadmap-history.timer || true
    else systemctl disable deadlock-roadmap-history.timer || true; fi
    if [[ $old_timer_active == active ]]; then systemctl start deadlock-roadmap-history.timer || true; fi
    echo 'Roadmap-Installation zurückgesetzt; vorherige Seite bleibt erhalten.' >&2
  fi
  rm -f -- "$candidate"
}
trap rollback EXIT
for running_unit in deadlock-roadmap-history.timer deadlock-roadmap-history.service; do
  if systemctl cat "$running_unit" >/dev/null 2>&1; then systemctl stop "$running_unit"; fi
done
install -m 0644 "$src/ops/systemd/deadlock-roadmap-history.service" "$unit"
install -m 0644 "$src/ops/systemd/deadlock-roadmap-history.timer" "$timer"
ln -s "$release" "$base/.current-next"
mv -Tf "$base/.current-next" "$base/current"
mv -Tf -- "$candidate" "$webroot/index.html"
systemctl daemon-reload
systemctl start deadlock-roadmap-history.service
systemctl enable --now deadlock-roadmap-history.timer
trap - EXIT
printf 'Feature-Stammbaum veröffentlicht: %s\n' "$sha"
