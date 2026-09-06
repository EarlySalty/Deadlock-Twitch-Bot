#!/usr/bin/env bash
set -uo pipefail

ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"

export PATH="${HOME}/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:${HOME}/.cargo/bin:${HOME}/.local/bin:/usr/bin:${PATH}"

STRICT=0
if [ "${1:-}" = "--strict" ]; then
  STRICT=1
fi

FAILED=0
WARNED=0

say() {
  printf '%s\n' "$*"
}

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    say "FEHLER: $1 fehlt im PATH"
    FAILED=1
    return 1
  fi
  return 0
}

block() {
  say "FEHLER: $*"
  FAILED=1
}

note() {
  say "HINWEIS: $*"
  WARNED=1
}

pass() {
  say "OK: $*"
}

soft_fail() {
  if [ "$STRICT" -eq 1 ]; then
    block "$*"
  else
    note "$*"
  fi
}

CARGO_DIR=""
if [ -f rust/Cargo.toml ]; then
  CARGO_DIR=rust
elif [ -f Cargo.toml ]; then
  CARGO_DIR=.
fi

DENY_CONFIG=""
if [ -f .github/cargo-deny.toml ]; then
  DENY_CONFIG="$ROOT/.github/cargo-deny.toml"
elif [ -f deny.toml ]; then
  DENY_CONFIG="$ROOT/deny.toml"
fi

GITLEAKS_CONFIG=""
if [ -f .gitleaks.toml ]; then
  GITLEAKS_CONFIG=.gitleaks.toml
fi

TRIVY_IGNORE=""
if [ -f .trivyignore.yaml ]; then
  TRIVY_IGNORE=.trivyignore.yaml
fi

RUST_CHANGED=1
CHANGED_FILES=""
if [ ! -t 0 ]; then
  CHANGED_FILES=$(
    while read -r _local_ref local_sha _remote_ref remote_sha; do
      [ -n "${local_sha:-}" ] || continue
      [ "$local_sha" != "0000000000000000000000000000000000000000" ] || continue
      if [ "${remote_sha:-}" = "0000000000000000000000000000000000000000" ] || [ -z "${remote_sha:-}" ]; then
        git diff-tree --no-commit-id --name-only -r "$local_sha"
      else
        git diff --name-only "$remote_sha" "$local_sha"
      fi
    done
  )
  if [ -n "$CHANGED_FILES" ]; then
    if ! printf '%s\n' "$CHANGED_FILES" | grep -qE '\.(rs)$|^Cargo\.(toml|lock)$|^rust/'; then
      RUST_CHANGED=0
    fi
  fi
fi

say "== Security-Scan lokal (kein GitHub, keine Lizenz) =="
if [ "$STRICT" -eq 1 ]; then
  say "Modus: strict (wie der Wochenjob auf GitHub)"
else
  say "Modus: Push-Gate (Secrets und RustSec blocken, Rest nur melden)"
fi

forbidden=$(git ls-files | grep -E '(^|/)(\.env(\..*)?|id_rsa|id_dsa|machine_auth_token\.txt|refresh\.token|oauth_tokens?.*|.*\.pem|.*\.key)$' || true)
if [ -n "$forbidden" ]; then
  block "versionierte Secret-Dateien:"
  printf '%s\n' "$forbidden"
fi

if need gitleaks; then
  gitleaks_cfg=$(mktemp)
  if [ -n "$GITLEAKS_CONFIG" ]; then
    cat "$GITLEAKS_CONFIG" > "$gitleaks_cfg"
  else
    printf 'title = "local"\n[extend]\nuseDefault = true\n' > "$gitleaks_cfg"
  fi
  cat >> "$gitleaks_cfg" <<'EOF'

[[allowlists]]
description = "Testfixtures und Demo-Chiffrate, keine echten Secrets."
paths = [
  '''src/auth\.rs''',
  '''src/transcode/encoder\.rs''',
  '''rust/crates/tb-dashboard-api/src/handlers/ad_manager\.rs''',
  '''rust/crates/tb-dashboard-api/src/obs/ws\.rs''',
  '''website/src/components/partner-clean/Security\.tsx''',
]
targetRules = ["generic-api-key"]
EOF
  gitleaks_args=(detect --source "$ROOT" --no-git --no-banner --redact --exit-code 1 --config "$gitleaks_cfg")
  if gitleaks "${gitleaks_args[@]}"; then
    pass "gitleaks"
  else
    block "gitleaks"
  fi
  rm -f "$gitleaks_cfg"
fi

if [ -n "$CARGO_DIR" ] && need cargo && need cargo-audit; then
  if (
    cd "$CARGO_DIR"
    if [ -f .cargo/audit.toml ]; then
      cargo audit
    else
      cargo audit --ignore RUSTSEC-2023-0071
    fi
  ); then
    pass "cargo-audit"
  else
    block "cargo-audit"
  fi
fi

if [ -n "$CARGO_DIR" ] && need cargo && need cargo-deny; then
  deny_args=(check --hide-inclusion-graph advisories bans sources)
  if [ -n "$DENY_CONFIG" ]; then
    deny_args=(--config "$DENY_CONFIG" "${deny_args[@]}")
  fi
  if (cd "$CARGO_DIR" && cargo deny "${deny_args[@]}"); then
    pass "cargo-deny"
  else
    block "cargo-deny"
  fi
fi

if need osv-scanner; then
  osv_args=(scan source -r)
  if [ -f osv-scanner.toml ]; then
    osv_args+=(--config "$ROOT/osv-scanner.toml")
  fi
  if osv-scanner "${osv_args[@]}" "$ROOT"; then
    pass "osv-scanner"
  else
    soft_fail "osv-scanner hat Treffer oder ist fehlgeschlagen"
  fi
fi

if need trivy; then
  trivy_args=(fs --scanners vuln,secret --severity HIGH,CRITICAL --skip-dirs node_modules,target,dist,graphify-out,.git)
  if [ -n "$TRIVY_IGNORE" ]; then
    trivy_args+=(--ignorefile "$TRIVY_IGNORE")
  fi
  if [ "$STRICT" -eq 1 ]; then
    trivy_args+=(--exit-code 1)
  else
    trivy_args+=(--exit-code 0)
  fi
  if trivy "${trivy_args[@]}" "$ROOT"; then
    pass "trivy"
  else
    soft_fail "trivy"
  fi
fi

if [ "$STRICT" -eq 1 ] && need semgrep; then
  semgrep_args=(scan --metrics=off --config p/rust --config p/security-audit --config p/owasp-top-ten --config p/github-actions --severity ERROR --error)
  if [ -d bot ] || [ -d website ]; then
    semgrep_args+=(--config p/javascript --config p/python)
  fi
  if semgrep "${semgrep_args[@]}" "$ROOT"; then
    pass "semgrep"
  else
    block "semgrep"
  fi
else
  say "SKIP: semgrep (nur mit --strict)"
fi

if [ "$STRICT" -eq 1 ] && [ -n "$CARGO_DIR" ] && [ "$RUST_CHANGED" -eq 1 ] && need cargo && command -v cargo-clippy >/dev/null 2>&1; then
  if (
    cd "$CARGO_DIR"
    SQLX_OFFLINE=true cargo clippy --workspace --all-targets --locked -- -D warnings
  ); then
    pass "clippy"
  else
    block "clippy"
  fi
elif [ -n "$CARGO_DIR" ]; then
  say "SKIP: clippy (nur mit --strict, sonst zu langsam für jeden Push)"
fi

say ""
if [ "$FAILED" -ne 0 ]; then
  say "Push gestoppt: Secrets oder RustSec. Remote-GitHub bleibt wöchentlich."
  exit 1
fi
if [ "$WARNED" -ne 0 ]; then
  say "Push erlaubt, aber es gab Hinweise. Mit --strict wie der Wochenjob werten."
  exit 0
fi
say "Alles grün."
exit 0
