#!/usr/bin/env -S -i PATH=/usr/sbin:/usr/bin:/sbin:/bin LC_ALL=C /usr/bin/bash --noprofile --norc
# shellcheck shell=bash
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
LC_ALL=C
builtin export PATH LC_ALL

NFT_PATH=/usr/sbin/nft
JQ_PATH=/usr/bin/jq
TABLE_FAMILY=inet
TABLE_NAME=deadlock_twitch_downloader
EXPECTED_RULE_COUNT=12

validate_fixed_root_tool() {
  local tool="$1"
  if [[ -L "$tool" || ! -f "$tool" || ! -x "$tool" ]] ||
     [[ "$(/usr/bin/stat -c '%u:%g:%h' -- "$tool")" != '0:0:1' ]] ||
     [[ -n "$(/usr/bin/find "$tool" -maxdepth 0 -perm /022 -print -quit)" ]] ||
     [[ -u "$tool" || -g "$tool" ]] ||
     [[ -n "$(/usr/sbin/getcap -- "$tool")" ]]; then
    echo "Firewall-Prüfwerkzeug ist nicht vertrauenswürdig: $tool" >&2
    return 1
  fi
}

validate_table_json() {
  local mode="$1"
  local json_file="${2:-/dev/stdin}"
  # Das jq-Programm ist absichtlich einfach zitiert; Shellvariablen kommen nur
  # über die beiden typisierten --arg-Parameter hinein.
  # shellcheck disable=SC2016
  "$JQ_PATH" --exit-status --arg mode "$mode" \
    --argjson expected_rules "$EXPECTED_RULE_COUNT" '
      def entries($name): [.nftables[] | select(has($name)) | .[$name]];
      (.nftables | type == "array")
      and all(.nftables[];
        ((keys_unsorted | length) == 1)
        and (has("metainfo") or has("table") or has("chain") or has("rule")))
      and ((entries("table")) as $tables
        | ($tables | length) == 1
        and $tables[0].family == "inet"
        and $tables[0].name == "deadlock_twitch_downloader"
        and (($tables[0] | keys_unsorted - ["family", "name", "handle"]) | length) == 0)
      and ((entries("chain")) as $chains
        | ($chains | length) == 1
        and $chains[0].family == "inet"
        and $chains[0].table == "deadlock_twitch_downloader"
        and $chains[0].name == "output"
        and $chains[0].type == "filter"
        and $chains[0].hook == "output"
        and ($chains[0].prio == 0 or $chains[0].prio == "filter")
        and $chains[0].policy == "accept"
        and (($chains[0] | keys_unsorted
          - ["family", "table", "name", "handle", "type", "hook", "prio", "policy"])
          | length) == 0)
      and ((entries("rule")) as $rules
        | all($rules[];
            .family == "inet"
            and .table == "deadlock_twitch_downloader"
            and .chain == "output")
        and ($mode == "before" or ($rules | length) == $expected_rules))
    ' "$json_file" >/dev/null
}

validate_live_table() {
  local mode="$1"
  local table_count
  table_count="$(
    "$NFT_PATH" --json --numeric --stateless list tables \
      | "$JQ_PATH" --raw-output \
          '[.nftables[] | select(has("table")) | .table
            | select(.family == "inet" and .name == "deadlock_twitch_downloader")]
           | length'
  )"
  if [[ "$table_count" == 0 ]]; then
    if [[ "$mode" == before ]]; then
      return 0
    fi
    echo "Downloader-Firewalltabelle fehlt nach dem atomaren Laden." >&2
    return 1
  fi
  if [[ "$table_count" != 1 ]]; then
    echo "Downloader-Firewalltabelle ist nicht eindeutig." >&2
    return 1
  fi
  if ! "$NFT_PATH" --json --numeric --stateless \
      list table "$TABLE_FAMILY" "$TABLE_NAME" \
      | validate_table_json "$mode"; then
    echo "Downloader-Firewalltabelle weicht von der exakten Topologie ab." >&2
    return 1
  fi
}

if [[ "${1:-}" == --validate-json-for-test ]]; then
  if [[ $# -ne 3 || $EUID -eq 0 ||
        ("$2" != before && "$2" != after) ]] ||
     [[ -L "$3" || ! -f "$3" ]] ||
     [[ "$(/usr/bin/stat -c '%u:%g:%h' -- "$3")" != "$EUID:$(/usr/bin/id -g):1" ]]; then
    echo "Der Firewall-JSON-Testmodus darf nur unprivilegiert eigene reguläre Daten prüfen." >&2
    exit 1
  fi
  validate_fixed_root_tool "$JQ_PATH"
  validate_table_json "$2" "$3"
  exit 0
fi

if [[ $# -ne 1 || ("$1" != --before-load && "$1" != --after-load) ]] ||
   [[ $EUID -ne 0 ]]; then
  echo "Aufruf nur als root: $0 --before-load|--after-load" >&2
  exit 1
fi
validate_fixed_root_tool "$NFT_PATH"
validate_fixed_root_tool "$JQ_PATH"
if [[ "$1" == --before-load ]]; then
  validate_live_table before
else
  validate_live_table after
fi
