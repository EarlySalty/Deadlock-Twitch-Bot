#!/usr/bin/env bash
# Offline checks only. No bot/dashboard launch, provider calls or message relay.
set -uo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
MODE="${1:-knowledge}"
case "$MODE" in knowledge|self-explainer) ;; *) echo 'Usage: check-brain-consumer.sh knowledge|self-explainer' >&2; exit 64 ;; esac
LOGS="$ROOT/.consumer-ci-reports"
mkdir -p "$LOGS/test-home"
CARGO_CACHE="${CARGO_HOME:-$HOME/.cargo}"
RUSTUP_CACHE="${RUSTUP_HOME:-$HOME/.rustup}"
printf 'check\texit_code\n' > "$LOGS/$MODE-results.tsv"
{ git -C "$ROOT" rev-parse HEAD; cargo --version; rustc --version; } > "$LOGS/$MODE-provenance.txt"
FAILED=0
run() {
  local label="$1"; shift
  local result=0
  (cd "$ROOT" && env -i PATH="$PATH" HOME="$LOGS/test-home" CARGO_HOME="$CARGO_CACHE" RUSTUP_HOME="$RUSTUP_CACHE" CARGO_BUILD_JOBS=2 CARGO_NET_OFFLINE=true SQLX_OFFLINE=true LC_ALL=C.UTF-8 TZ=UTC "$@") > "$LOGS/$MODE-$label.log" 2>&1 || result=$?
  printf '%s\t%s\n' "$label" "$result" >> "$LOGS/$MODE-results.tsv"
  printf '%s %s exit=%s\n' "$MODE" "$label" "$result"
  if ((result != 0)); then FAILED=1; tail -n 60 "$LOGS/$MODE-$label.log"; fi
}
case "$MODE" in
  knowledge)
    run fmt cargo fmt --manifest-path rust/Cargo.toml -p tb-knowledge -- --check
    run test cargo test --manifest-path rust/Cargo.toml -p tb-knowledge --all-targets --locked --offline
    run clippy cargo clippy --manifest-path rust/Cargo.toml -p tb-knowledge --all-targets --locked --offline -- -D warnings
    ;;
  self-explainer)
    run fmt rustfmt --edition 2021 --check rust/crates/tb-dashboard-api/src/handlers/self_explainer.rs
    run test cargo test --manifest-path rust/Cargo.toml -p tb-dashboard-api --lib self_explainer::tests --locked --offline
    if ! grep -q 'typed_port_does_not_drop_history_or_invoke_legacy_fallbacks ... ok' "$LOGS/$MODE-test.log"; then
      echo 'Required typed-port regression did not pass.' >&2
      FAILED=1
    fi
    ;;
esac
exit "$FAILED"
