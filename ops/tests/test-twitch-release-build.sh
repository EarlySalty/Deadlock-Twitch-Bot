#!/usr/bin/env bash
set -euo pipefail

repo=$(git -C "$(dirname "$(realpath "$0")")" rev-parse --show-toplevel)
test_root=$(mktemp -d)
trap 'rm -rf -- "$test_root"' EXIT
mkdir -p "$test_root/source/ops" "$test_root/bin" "$test_root/home"
cp "$repo/ops/twitch-release-build" "$test_root/source/ops/"
for path in bot/dashboard_v2 bot/admin_dashboard website rust/bin/tb-dashboard/src/bin rust/crates/tb-social-media/src/ai_editor rust/bin/tb-category-collector/src/bin; do
  mkdir -p "$test_root/source/$path"
  touch "$test_root/source/$path/input"
done
touch "$test_root/source/rust/bin/tb-dashboard/src/bin/clip_context_learn.rs"
touch "$test_root/source/rust/crates/tb-social-media/src/ai_editor/mod.rs"
touch "$test_root/source/rust/bin/tb-category-collector/Cargo.toml"
touch "$test_root/source/rust/bin/tb-category-collector/src/lib.rs"
touch "$test_root/source/rust/bin/tb-category-collector/src/bin/tb-twitch-watchdog.rs"
touch "$test_root/source/rust/bin/tb-category-collector/src/bin/tb-category-storage.rs"
printf 'target/\ndist/\n' >"$test_root/source/.gitignore"
git -C "$test_root/source" init -q
git -C "$test_root/source" -c user.name=Test -c user.email=test@localhost add .
git -C "$test_root/source" -c user.name=Test -c user.email=test@localhost commit -qm fixture
git -C "$test_root/source" remote add origin "$test_root/source"
sha=$(git -C "$test_root/source" rev-parse HEAD)
cat >"$test_root/bin/cargo-slot" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[[ $CARGO_SLOT_DEPLOY == 1 ]]
[[ $* == *'--release --locked --timings'* ]]
printf '%s\n' "$*" >>"$HOME/cargo-calls"
mkdir -p target/release
for binary in tb-bot tb-dashboard tb-stream-audit tb-config-check tb-llm-usage-recover tb-stt-server tb-twitch-watchdog tb-category-storage clip_context_learn; do
  printf '%s\n' "$(git rev-parse HEAD)" >"target/release/$binary"
  chmod +x "target/release/$binary"
done
SH
cat >"$test_root/bin/readelf" <<'SH'
#!/usr/bin/env bash
cat "${@: -1}"
SH
cat >"$test_root/bin/npm" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[[ ${FAIL_FRONTEND:-0} == 0 ]]
if [[ $1 == run ]]; then
  mkdir -p dist
  printf '%s\n' "$(git rev-parse HEAD)" >dist/index.html
fi
SH
chmod +x "$test_root/bin/"* "$test_root/source/ops/twitch-release-build"
export HOME="$test_root/home" PATH="$test_root/bin:$PATH"
script="$test_root/source/ops/twitch-release-build"
if "$script" short >/dev/null 2>&1; then exit 1; fi
"$script" "$sha" >/dev/null 2>&1
staging="$HOME/repos/twitch-release-$sha"
[[ -d "$staging/.git" ]]
[[ $(git -C "$staging" rev-parse HEAD) == "$sha" ]]
[[ -z $(git -C "$staging" status --porcelain) ]]
[[ $(cat "$staging/website/dist/index.html") == "$sha" ]]
[[ -f "$staging/rust/target/release/tb-stt-server" ]]
[[ ! -L "$staging/rust/target/release/tb-bot" ]]
[[ ! -e "$staging/rust/target/release/tb-category-collector" ]]
if "$script" "$sha" >/dev/null 2>&1; then exit 1; fi
[[ $(wc -l <"$HOME/cargo-calls") == 1 ]]
rm -rf -- "$staging"
printf 'dirty\n' >>"$HOME/repos/twitch-release-build/website/input"
if "$script" "$sha" >/dev/null 2>&1; then exit 1; fi
[[ $(wc -l <"$HOME/cargo-calls") == 1 ]]
git -C "$HOME/repos/twitch-release-build" restore website/input
if FAIL_FRONTEND=1 "$script" "$sha" >/dev/null 2>&1; then exit 1; fi
[[ ! -e "$staging" ]]
"$script" "$sha" >/dev/null 2>&1
[[ $(cat "$staging/website/dist/index.html") == "$sha" ]]
printf 'Release build contract checks passed.\n'
