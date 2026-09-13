#!/usr/bin/env bash
set -euo pipefail
repo="$(cd -- "$(dirname -- "$0")/.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf -- "$scratch"' EXIT
mkdir -p "$scratch/src"
cp "$repo/rust/bin/build_revision.rs" "$scratch/build.rs"
cat > "$scratch/Cargo.toml" <<'EOF'
[package]
name = "revision-check"
version = "0.1.0"
edition = "2021"
EOF
cat > "$scratch/src/main.rs" <<'EOF'
include!(concat!(env!("OUT_DIR"), "/build_revision.rs"));
fn main() { assert!(print_build_revision()); }
EOF
printf '/target/\n' > "$scratch/.gitignore"
git -C "$scratch" init -q
git -C "$scratch" add .
git -C "$scratch" -c user.name=Test -c user.email=test@example.invalid commit -qm initial
build_and_check() {
  cargo build --quiet --manifest-path "$scratch/Cargo.toml"
  local actual
  actual="$("$scratch/target/debug/revision-check" --build-revision)"
  [[ "$actual" == "$1" ]]
  [[ "$(readelf --string-dump=.twitch_build "$scratch/target/debug/revision-check" | awk '/\[/{print $NF}')" == "$1" ]]
}
first="$(git -C "$scratch" rev-parse HEAD)"
# Cargo.lock muss vor dem sauberen Vergleich vorhanden und versioniert sein.
cargo generate-lockfile --quiet --manifest-path "$scratch/Cargo.toml"
git -C "$scratch" add Cargo.lock
git -C "$scratch" -c user.name=Test -c user.email=test@example.invalid commit -qm lock
first="$(git -C "$scratch" rev-parse HEAD)"
build_and_check "$first"
printf '// lokale Änderung\n' >> "$scratch/src/main.rs"
build_and_check "$first-dirty"
git -C "$scratch" add src/main.rs
git -C "$scratch" -c user.name=Test -c user.email=test@example.invalid commit -qm next
second="$(git -C "$scratch" rev-parse HEAD)"
build_and_check "$second"
# Auch ein reiner Commit-Wechsel ohne geänderte Rust-Datei aktualisiert den Cache.
git -C "$scratch" -c user.name=Test -c user.email=test@example.invalid commit --allow-empty -qm empty
third="$(git -C "$scratch" rev-parse HEAD)"
build_and_check "$third"
git -C "$scratch" checkout -q "$second"
build_and_check "$second"
# Ein tatsächlich mitkompiliertes, unversioniertes Modul macht den Build dirty.
printf '\nmod extra;\n' >> "$scratch/src/main.rs"
git -C "$scratch" add src/main.rs
git -C "$scratch" -c user.name=Test -c user.email=test@example.invalid commit -qm module
module_revision="$(git -C "$scratch" rev-parse HEAD)"
printf 'const _: () = ();\n' > "$scratch/src/extra.rs"
build_and_check "$module_revision-dirty"
git -C "$scratch" add src/extra.rs
git -C "$scratch" -c user.name=Test -c user.email=test@example.invalid commit -qm track-module
tracked_revision="$(git -C "$scratch" rev-parse HEAD)"
build_and_check "$tracked_revision"
git -C "$scratch" checkout -q "$second"
build_and_check "$second"
# Exakt die Installer-Prüfung testen, ohne root, Dienste oder Release-Verzeichnis.
sed -n '/^check_binary_revisions() {$/,/^}$/p' "$repo/ops/systemd/install-twitch-release.sh" > "$scratch/check.sh"
source "$scratch/check.sh"
mkdir -p "$scratch/package/rust/target/release"
for name in tb-bot tb-dashboard tb-stream-audit; do
  cp "$scratch/target/debug/revision-check" "$scratch/package/rust/target/release/$name"
done
git_sha="$second"
check_binary_revisions "$scratch/package"
git_sha="$third"
if (check_binary_revisions "$scratch/package") 2>/dev/null; then
  echo 'Veraltete Binaries wurden akzeptiert' >&2; exit 1
fi
git_sha="$second"
cp /usr/bin/true "$scratch/package/rust/target/release/tb-bot"
if (check_binary_revisions "$scratch/package") 2>/dev/null; then
  echo 'Binary ohne Herkunft wurde akzeptiert' >&2; exit 1
fi
echo 'Build-Herkunft: sauber, dirty, Commit-/Branch-Wechsel und Installer-Ablehnung geprüft.'
