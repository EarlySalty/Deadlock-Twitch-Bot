#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
installer="$repo/ops/systemd/install-twitch-release.sh"
checkout="$(mktemp -d)"
trap 'rm -rf "$checkout"' EXIT

carry_reject() { echo "REJECT: $1" >&2; exit 1; }
eval "$(sed -n '/^declare -A binary_package_dir=(/,/^verify_unchanged_sources() {/p' "$installer" | sed '$d')"

mkdir -p "$checkout/rust/bin/tb-bot/src" "$checkout/rust/crates/tb-config/src" "$checkout/rust/crates/tb-error/src" \
  "$checkout/rust/crates/tb-test-only/src" "$checkout/rust/vendor/extra/src" "$checkout/rust/migrations"
cat > "$checkout/rust/Cargo.toml" <<'TOML'
[workspace.dependencies]
tb-config = { path = "crates/tb-config" }
tb-error = { path = "crates/tb-error" }
tb-test-only = { path = "crates/tb-test-only" }
TOML
cat > "$checkout/rust/bin/tb-bot/Cargo.toml" <<'TOML'
[package]
name = "tb-bot"
version.workspace = true

[dependencies]
tb-config.workspace = true
extra = { path = "../../vendor/extra" }

[dev-dependencies]
tb-test-only.workspace = true
TOML
cat > "$checkout/rust/crates/tb-config/Cargo.toml" <<'TOML'
[dependencies.tb-error]
workspace = true
TOML
printf '[package]\nname = "tb-error"\n' > "$checkout/rust/crates/tb-error/Cargo.toml"
printf '[package]\nname = "tb-test-only"\n' > "$checkout/rust/crates/tb-test-only/Cargo.toml"
printf '[package]\nname = "extra"\n' > "$checkout/rust/vendor/extra/Cargo.toml"
printf 'const SQL: &str = include_str!("../../../migrations/one.sql");\n' > "$checkout/rust/bin/tb-bot/src/main.rs"

git_safe=(git -c user.name=test -c user.email=test@example.invalid -c commit.gpgsign=false)
commit_fixture() {
  "${git_safe[@]}" -C "$checkout" add -A
  "${git_safe[@]}" -C "$checkout" commit -q -m "$1"
  git_sha="$("${git_safe[@]}" -C "$checkout" rev-parse HEAD)"
}
"${git_safe[@]}" -C "$checkout" init -q
commit_fixture fixture

inputs="$(required_inputs binary tb-bot | LC_ALL=C sort -u | tr '\n' ' ')"
for expected in rust/bin/tb-bot/ rust/crates/tb-config/ rust/crates/tb-error/ rust/vendor/extra/ rust/migrations/one.sql; do
  [[ " $inputs " == *" $expected "* ]] || { echo "missing $expected in: $inputs" >&2; exit 1; }
done
[[ " $inputs " != *" rust/crates/tb-test-only/ "* ]] || { echo "dev dependency leaked into: $inputs" >&2; exit 1; }

printf '[package]\nname = "tb-bot"\n' > "$checkout/rust/bin/tb-bot/Cargo.toml"
: > "$checkout/rust/bin/tb-bot/src/main.rs"
tampered="$(required_inputs binary tb-bot | LC_ALL=C sort -u | tr '\n' ' ')"
[[ "$tampered" == "$inputs" ]] || { echo "working tree changed the closure: $tampered" >&2; exit 1; }
"${git_safe[@]}" -C "$checkout" checkout -q -- .

printf 'const P: &str = "../{}";\n' > "$checkout/rust/crates/tb-error/src/lib.rs"
commit_fixture bounded
[[ "$(required_inputs binary tb-bot 2>/dev/null | tr '\n' ' ')" == *"rust/crates/tb-error/"* ]] || { echo "bounded template not kept" >&2; exit 1; }
printf 'const P: &str = "../../../{root}";\n' > "$checkout/rust/crates/tb-error/src/lib.rs"
commit_fixture unbounded
if (required_inputs binary tb-bot >/dev/null 2>&1); then
  echo "unbounded reference was accepted" >&2
  exit 1
fi
if (required_inputs binary tb-unknown >/dev/null 2>&1); then
  echo "unknown binary was accepted" >&2
  exit 1
fi

echo "Install carry closure checks passed."
