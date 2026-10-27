#!/usr/bin/env bash
set -euo pipefail
source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
binary=${1:?Aufruf: install.sh <deploy-queue-binary>}
install -D -m 0755 "$binary" "$HOME/.local/bin/deploy-queue"
install -D -m 0644 "$source_dir/config.toml" "$HOME/.config/deploy-queue/config.toml"
install -D -m 0644 "$source_dir/deploy-queue.service" "$HOME/.config/systemd/user/deploy-queue.service"
systemctl --user daemon-reload
systemctl --user enable deploy-queue.service
systemctl --user restart deploy-queue.service
