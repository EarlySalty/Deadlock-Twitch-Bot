#!/usr/bin/env bash
set -euo pipefail

installer=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../systemd" && pwd)/install-twitch-contest-peer
temporary_root=$(mktemp -d)
trap 'rm -r -- "$temporary_root"' EXIT

runuser() {
    printf '%s\n' "$FIXTURE_METADATA"
}

functions=$(sed -n \
    -e '/^resolve_peer_rule_file() {/,/^}/p' \
    -e '/^backup_peer_files() {/,/^}/p' \
    -e '/^restore_peer_files() {/,/^}/p' \
    -e '/^install_local_peer_before_reject() {/,/^}/p' \
    "$installer")
eval "$functions"

assert_contains_once() {
    local expected=$1 file=$2
    [[ $(rg --fixed-strings --line-regexp --count "$expected" "$file") == 1 ]]
}

assert_unchanged() {
    local expected=$1 file=$2
    [[ $(cat -- "$file") == "$expected" ]]
}

exercise_layout() {
    local layout=$1 main_file
    local rule_file ident_file backup_dir selected original_main original_rules original_ident
    main_file="$temporary_root/$layout/pg_hba.conf"
    mkdir -p -- "$(dirname -- "$main_file")"
    ident_file="$(dirname -- "$main_file")/pg_ident.conf"
    backup_dir="$(dirname -- "$main_file")/backup"
    mkdir -- "$backup_dir"

    if [[ "$layout" == inline ]]; then
        rule_file=$main_file
        cat > "$main_file" <<'EOF'
local all twitchbot reject
local all twitchdash reject
EOF
        FIXTURE_METADATA="1	1	$main_file	$main_file"
    else
        rule_file="$(dirname -- "$main_file")/pg_hba-twitch.conf"
        cat > "$main_file" <<'EOF'
include 'pg_hba-twitch.conf'
local twitch_analytics twitchcontest peer map=twitch_contest_writer
EOF
        cat > "$rule_file" <<'EOF'
local all twitchbot reject
local all twitchdash reject
EOF
        FIXTURE_METADATA="1	1	$rule_file	$rule_file"
    fi
    printf 'twitch_contest_writer twitchdash twitchcontest\n' > "$ident_file"
    original_main=$(cat -- "$main_file")
    original_rules=$(cat -- "$rule_file")
    original_ident=$(cat -- "$ident_file")

    selected=$(resolve_peer_rule_file "$main_file")
    [[ "$selected" == "$rule_file" ]]
    backup_peer_files "$main_file" "$rule_file" "$ident_file" "$backup_dir"
    install_local_peer_before_reject "$rule_file" deadlock twitchbot
    install_local_peer_before_reject "$rule_file" deadlock twitchdash
    assert_contains_once 'local deadlock twitchbot peer' "$rule_file"
    assert_contains_once 'local deadlock twitchdash peer' "$rule_file"
    install_local_peer_before_reject "$rule_file" deadlock twitchbot
    install_local_peer_before_reject "$rule_file" deadlock twitchdash
    assert_contains_once 'local deadlock twitchbot peer' "$rule_file"
    assert_contains_once 'local deadlock twitchdash peer' "$rule_file"

    restore_peer_files "$main_file" "$rule_file" "$ident_file" "$backup_dir"
    assert_unchanged "$original_main" "$main_file"
    assert_unchanged "$original_rules" "$rule_file"
    assert_unchanged "$original_ident" "$ident_file"
    printf 'PASS %s selection, insertion, idempotence, backup and rollback\n' "$layout"
}

exercise_layout inline
exercise_layout include

main_file="$temporary_root/include/pg_hba.conf"
include_file="$temporary_root/include/pg_hba-twitch.conf"
FIXTURE_METADATA="2	1	$include_file	$include_file"
if resolve_peer_rule_file "$main_file" >/dev/null 2>&1; then
    echo 'FAIL duplicate reject rules were accepted' >&2
    exit 1
fi
FIXTURE_METADATA="1	1	$main_file	$include_file"
if resolve_peer_rule_file "$main_file" >/dev/null 2>&1; then
    echo 'FAIL reject rules from different files were accepted' >&2
    exit 1
fi
FIXTURE_METADATA="1	1	$temporary_root/outside.conf	$temporary_root/outside.conf"
if resolve_peer_rule_file "$main_file" >/dev/null 2>&1; then
    echo 'FAIL unexpected HBA topology was accepted' >&2
    exit 1
fi
symlink_dir="$temporary_root/symlink"
mkdir -- "$symlink_dir"
symlink_main="$symlink_dir/pg_hba.conf"
symlink_rules="$symlink_dir/pg_hba-twitch.conf"
printf 'include pg_hba-twitch.conf\n' > "$symlink_main"
printf 'local all twitchbot reject\nlocal all twitchdash reject\n' > "$temporary_root/external-rules.conf"
ln -s "$temporary_root/external-rules.conf" "$symlink_rules"
FIXTURE_METADATA="1\t1\t$symlink_rules\t$symlink_rules"
if resolve_peer_rule_file "$symlink_main" >/dev/null 2>&1; then
    echo 'FAIL symlinked HBA rule file was accepted' >&2
    exit 1
fi
missing_reject="$temporary_root/missing-reject.conf"
printf 'local deadlock twitchbot peer\n' > "$missing_reject"
if install_local_peer_before_reject "$missing_reject" deadlock twitchdash >/dev/null 2>&1; then
    echo 'FAIL missing reject anchor was accepted' >&2
    exit 1
fi
printf 'PASS ambiguous and unexpected PostgreSQL HBA metadata fails closed\n'
