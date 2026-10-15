set -euo pipefail
root=/home/nathanael/.worktrees/ttb-opus-r1-20261015
if test -e "$root/rust/test-database.json"; then
  printf 'Refusing a preconfigured database file; isolated DB only\n'
  exit 1
fi
dir=$(mktemp -d /tmp/ttb-r1-tests.XXXXXX)
cleanup() {
  /usr/lib/postgresql/16/bin/pg_ctl -D "$dir/data" -m immediate -w stop >/dev/null 2>&1 || true
  rm -rf "$dir"
}
trap cleanup EXIT
/usr/lib/postgresql/16/bin/initdb --no-locale -A trust -U r1_test -D "$dir/data" >/dev/null
/usr/lib/postgresql/16/bin/pg_ctl -D "$dir/data" -l "$dir/postgres.log" -o "-h '' -k $dir -c fsync=off -c shared_buffers=32MB" -w start >/dev/null
export TB_TEST_DATABASE_URL="postgresql://r1_test@localhost/postgres?host=$dir"
export TB_TEST_REQUIRE_DB=1
export CARGO_SLOT_REMOTE=0
cd "$root/rust"
cargo-slot test -p tb-chat --lib promos
cargo-slot test -p tb-internal-api --lib chat_command
cargo-slot test -p tb-dashboard-api --lib admin_guild_raids
cargo-slot test -p tb-bot --bin tb-bot invite
