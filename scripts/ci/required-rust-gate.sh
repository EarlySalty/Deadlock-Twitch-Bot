#!/usr/bin/env bash
# Single entry point for the existing required Rust SQLx check. Never infer
# "no Rust changes" from a failed or missing change detector.
set -euo pipefail

evaluate() {
  if [[ ${CHANGES_RESULT-} != success ]]; then
    echo "Rust change detector did not succeed" >&2
    return 1
  fi
  case ${RUST_CHANGED-} in
    true)
      for result in "${AUTH_RESULT-}" "${OFFLINE_RESULT-}" "${SCHEMA_RESULT-}"; do
        if [[ $result != success ]]; then
          echo "A required Rust job did not succeed" >&2
          return 1
        fi
      done
      ;;
    false)
      for result in "${AUTH_RESULT-}" "${OFFLINE_RESULT-}" "${SCHEMA_RESULT-}"; do
        if [[ $result != skipped ]]; then
          echo "A Rust job was not skipped for a non-Rust change" >&2
          return 1
        fi
      done
      ;;
    *)
      echo "Rust change detector returned no valid scope" >&2
      return 1
      ;;
  esac
}

if [[ ${1-} == --self-test ]]; then
  check() {
    local expected=$1
    shift
    if (
      CHANGES_RESULT=$1
      RUST_CHANGED=$2
      AUTH_RESULT=$3
      OFFLINE_RESULT=$4
      SCHEMA_RESULT=$5
      evaluate >/dev/null 2>&1
    ); then
      [[ $expected == pass ]]
    else
      [[ $expected == fail ]]
    fi
  }
  check pass success true success success success
  check pass success false skipped skipped skipped
  check fail failure false skipped skipped skipped
  check fail success '' skipped skipped skipped
  check fail success true success failure success
  check fail success false success skipped skipped
  echo "Required Rust SQLx gate controls passed"
  exit 0
fi

evaluate
