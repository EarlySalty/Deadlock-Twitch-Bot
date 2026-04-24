#!/usr/bin/env bash
# Local security scan for Deadlock-Twitch-Bot.
# Mirrors security-fortress.yml + security-deep-scan.yml.
# Creates GitHub Issues for findings (skips duplicates).
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GH_REPO="NaniDerEchte2/Deadlock-Twitch-Bot"
REPORT_DIR="$REPO_ROOT/security-reports"
mkdir -p "$REPORT_DIR"

GREEN='\033[0;32m'; RED='\033[0;31m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'
pass=0; fail=0; warn=0

log_pass() { echo -e "${GREEN}✅ PASS${NC}  $1${2:+ — $2}"; pass=$((pass+1)); }
log_fail() { echo -e "${RED}❌ FAIL${NC}  $1${2:+ — $2}"; fail=$((fail+1)); }
log_warn() { echo -e "${YELLOW}⚠️  WARN${NC}  $1${2:+ — $2}"; warn=$((warn+1)); }

open_issue() {
    local title="$1" body="$2"
    local existing
    existing=$(gh issue list --repo "$GH_REPO" --state open --search "\"$title\"" --json title -q '.[].title' 2>/dev/null || true)
    if echo "$existing" | grep -qF "$title"; then
        echo -e "    ${YELLOW}↳ Issue already open${NC}"
    else
        if gh issue create --repo "$GH_REPO" --title "$title" --body "$body" --label "security" 2>/dev/null; then
            echo -e "    ${GREEN}↳ Issue created${NC}"
        else
            echo -e "    ${YELLOW}↳ Issue creation failed (check label/permissions)${NC}"
        fi
    fi
}

cd "$REPO_ROOT"
echo -e "\n${BLUE}═══════════════════════════════════════════${NC}"
echo -e "${BLUE}  Security Scan — Deadlock-Twitch-Bot${NC}"
echo -e "${BLUE}  $(date '+%Y-%m-%d %H:%M')${NC}"
echo -e "${BLUE}═══════════════════════════════════════════${NC}\n"

# ── 1. Bandit (exclude B608 false-positives, keep real issues) ──────────────
echo -e "${BLUE}[1/7] Bandit — Python SAST${NC}"
if command -v bandit &>/dev/null; then
    bandit -r bot/ twitch_cog/ scripts/ -ll -ii \
        --skip B608,B310 \
        -f json -o "$REPORT_DIR/bandit.json" --quiet 2>/dev/null || true
    findings=$(python3 -c "
import json, sys
try:
    d = json.load(open('$REPORT_DIR/bandit.json'))
    results = [i for i in d.get('results',[]) if i['issue_severity'] in ('HIGH','MEDIUM')]
    for i in results:
        loc = i['filename'].replace('$REPO_ROOT/','') + ':' + str(i['line_number'])
        print(i['issue_severity'] + '|' + loc + '|' + i['test_id'] + '|' + i['issue_text'][:120])
except Exception as e:
    print('ERROR:' + str(e), file=sys.stderr)
" 2>/dev/null || true)
    if [ -n "$findings" ]; then
        log_fail "Bandit" "medium/high issues found"
        while IFS='|' read -r sev loc test_id text; do
            [ -z "$sev" ] && continue
            echo "    $sev — $loc — $test_id"
            open_issue "[Security] Bandit $sev: $test_id in $loc" \
"**Tool:** Bandit
**Severity:** $sev
**Location:** \`$loc\`
**Rule:** $test_id
**Details:** $text

**Report:** \`security-reports/bandit.json\`
_Detected by local-security-scan.sh_"
        done <<< "$findings"
    else
        log_pass "Bandit" "no medium/high issues"
    fi
else
    log_warn "Bandit" "not installed — run: pip install bandit"
fi

# ── 2. Semgrep ──────────────────────────────────────────────────────────────
echo -e "\n${BLUE}[2/7] Semgrep — SAST${NC}"
if command -v semgrep &>/dev/null; then
    semgrep scan \
        --config auto \
        --json --output "$REPORT_DIR/semgrep.json" \
        --exclude-rule python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query \
        --exclude-rule python.lang.security.audit.formatted-sql-query.formatted-sql-query \
        --exclude-rule python.django.security.injection.raw-html-format.raw-html-format \
        --exclude-rule python.lang.security.audit.logging.logger-credential-leak.python-logger-credential-disclosure \
        --exclude-rule python.lang.security.audit.dynamic-urllib-use-detected.dynamic-urllib-use-detected \
        --exclude node_modules --exclude .venv --exclude venv \
        --quiet \
        bot/ twitch_cog/ 2>/dev/null || true
    findings=$(python3 -c "
import json, sys
try:
    d = json.load(open('$REPORT_DIR/semgrep.json'))
    for r in d.get('results',[]):
        path = r.get('path','?').replace('$REPO_ROOT/','')
        line = str(r.get('start',{}).get('line','?'))
        msg  = r.get('extra',{}).get('message','')[:120].replace('\n',' ')
        rule = r.get('check_id','?')
        print(path + ':' + line + '|' + rule + '|' + msg)
except Exception as e:
    print('ERROR:' + str(e), file=sys.stderr)
" 2>/dev/null || true)
    if [ -n "$findings" ]; then
        log_fail "Semgrep" "findings detected"
        while IFS='|' read -r loc rule msg; do
            [ -z "$loc" ] && continue
            echo "    $loc — $rule"
            open_issue "[Security] Semgrep: $rule in $loc" \
"**Tool:** Semgrep
**Rule:** \`$rule\`
**Location:** \`$loc\`
**Message:** $msg

**Report:** \`security-reports/semgrep.json\`
_Detected by local-security-scan.sh_"
        done <<< "$findings"
    else
        log_pass "Semgrep" "no findings"
    fi
else
    log_warn "Semgrep" "not installed — run: pip install semgrep"
fi

# ── 3. pip-audit ────────────────────────────────────────────────────────────
echo -e "\n${BLUE}[3/7] pip-audit — Dependency CVEs${NC}"
if command -v pip-audit &>/dev/null; then
    any_vuln=0
    for req in .github/requirements-test-ci-locked.txt .github/requirements-tools-locked.txt; do
        [ -f "$req" ] || continue
        out="$REPORT_DIR/pip-audit-$(basename "$req" .txt).json"
        pip-audit -r "$req" --format json --output "$out" -q 2>/dev/null || true
        findings=$(python3 -c "
import json, sys
try:
    d = json.load(open('$out'))
    for p in d.get('dependencies',[]):
        for v in p.get('vulns',[]):
            name = p['name'] + '==' + p.get('version','?')
            print(name + '|' + v['id'] + '|' + v.get('description','')[:120].replace('\n',' '))
except Exception as e:
    print('ERROR:' + str(e), file=sys.stderr)
" 2>/dev/null || true)
        if [ -n "$findings" ]; then
            any_vuln=1
            log_fail "pip-audit ($req)" "vulnerable packages"
            while IFS='|' read -r pkg cve desc; do
                [ -z "$pkg" ] && continue
                echo "    $pkg — $cve"
                open_issue "[Security] Vulnerable dependency: $pkg ($cve)" \
"**Tool:** pip-audit
**Package:** \`$pkg\`
**CVE/ID:** $cve
**Description:** $desc

**File:** \`$req\`
_Detected by local-security-scan.sh_"
            done <<< "$findings"
        fi
    done
    [ "$any_vuln" -eq 0 ] && log_pass "pip-audit" "no known vulnerabilities"
else
    log_warn "pip-audit" "not installed — run: pip install pip-audit"
fi

# ── 4. npm audit ────────────────────────────────────────────────────────────
echo -e "\n${BLUE}[4/7] npm audit — JS dependencies${NC}"
if command -v npm &>/dev/null; then
    any_npm=0
    for dir in website bot/admin_dashboard bot/dashboard_v2; do
        [ -f "$dir/package-lock.json" ] || continue
        out="$REPORT_DIR/npm-audit-$(echo "$dir" | tr '/' '-').json"
        (cd "$dir" && npm audit --audit-level=high --json 2>/dev/null) > "$out" || true
        findings=$(python3 -c "
import json, sys
try:
    d = json.load(open('$out'))
    for name, v in d.get('vulnerabilities',{}).items():
        if v.get('severity') in ('high','critical'):
            via = str(v.get('via',['?']))[0:80].replace('\n',' ')
            print(name + '|' + v.get('severity','?') + '|' + via)
except Exception as e:
    print('ERROR:' + str(e), file=sys.stderr)
" 2>/dev/null || true)
        if [ -n "$findings" ]; then
            any_npm=1
            log_fail "npm audit ($dir)" "high/critical vulnerabilities"
            while IFS='|' read -r pkg sev via; do
                [ -z "$pkg" ] && continue
                echo "    $pkg ($sev)"
                open_issue "[Security] npm vulnerability: $pkg ($sev) in $dir" \
"**Tool:** npm audit
**Package:** \`$pkg\`
**Severity:** $sev
**Directory:** \`$dir\`
**Via:** $via

_Detected by local-security-scan.sh_"
            done <<< "$findings"
        fi
    done
    [ "$any_npm" -eq 0 ] && log_pass "npm audit" "no high/critical vulnerabilities"
else
    log_warn "npm audit" "npm not found"
fi

# ── 5. Trivy ────────────────────────────────────────────────────────────────
echo -e "\n${BLUE}[5/7] Trivy — Secrets & CVEs${NC}"
if command -v trivy &>/dev/null; then
    out="$REPORT_DIR/trivy.json"
    trivy fs . \
        --format json --output "$out" \
        --scanners secret,vuln \
        --severity HIGH,CRITICAL \
        --skip-dirs node_modules,.venv,venv,dist,build \
        --quiet 2>/dev/null || true

    secrets=$(python3 -c "
import json, sys
try:
    d = json.load(open('$out'))
    for r in d.get('Results',[]):
        for s in r.get('Secrets',[]):
            print(r.get('Target','?') + '|' + s.get('RuleID','?') + '|' + s.get('Title','?'))
except: pass
" 2>/dev/null || true)

    if [ -n "$secrets" ]; then
        log_fail "Trivy secrets" "secrets found"
        while IFS='|' read -r file rule title; do
            [ -z "$file" ] && continue
            echo "    $file — $rule: $title"
            open_issue "[Security] Secret detected: $rule in $file" \
"**Tool:** Trivy secret scan
**File:** \`$file\`
**Rule:** $rule / $title

_Detected by local-security-scan.sh_"
        done <<< "$secrets"
    else
        log_pass "Trivy secrets" "no secrets detected"
    fi

    cves=$(python3 -c "
import json, sys
try:
    d = json.load(open('$out'))
    seen = set()
    for r in d.get('Results',[]):
        for v in r.get('Vulnerabilities',[]):
            if v.get('Severity') in ('HIGH','CRITICAL'):
                key = v.get('PkgName','?') + '==' + v.get('InstalledVersion','?') + '|' + v.get('VulnerabilityID','?') + '|' + v.get('Severity','?')
                if key not in seen:
                    seen.add(key)
                    print(key)
except: pass
" 2>/dev/null || true)

    if [ -n "$cves" ]; then
        log_fail "Trivy CVEs" "high/critical CVEs found"
        while IFS='|' read -r pkg cve sev; do
            [ -z "$pkg" ] && continue
            echo "    $pkg — $cve ($sev)"
            open_issue "[Security] CVE: $cve in $pkg" \
"**Tool:** Trivy
**Package:** \`$pkg\`
**CVE:** $cve
**Severity:** $sev

_Detected by local-security-scan.sh_"
        done <<< "$cves"
    else
        log_pass "Trivy CVEs" "no high/critical CVEs"
    fi
else
    log_warn "Trivy" "not installed — see https://trivy.dev"
fi

# ── 6. Ruff ─────────────────────────────────────────────────────────────────
echo -e "\n${BLUE}[6/7] Ruff — Lint${NC}"
if command -v ruff &>/dev/null; then
    out="$REPORT_DIR/ruff.json"
    ruff check bot/ twitch_cog/ scripts/ \
        --output-format=json --target-version=py311 -q > "$out" 2>/dev/null || true
    count=$(python3 -c "import json; print(len(json.load(open('$out'))))" 2>/dev/null || echo 0)
    if [ "$count" -gt 0 ]; then
        log_warn "Ruff" "$count issues → $out (fix locally, no issue created)"
    else
        log_pass "Ruff" "clean"
    fi
else
    log_warn "Ruff" "not installed — run: pip install ruff"
fi

# ── 7. Vulture ──────────────────────────────────────────────────────────────
echo -e "\n${BLUE}[7/7] Vulture — Dead code${NC}"
if command -v vulture &>/dev/null; then
    out="$REPORT_DIR/vulture.txt"
    vulture bot/ twitch_cog/ --min-confidence 80 > "$out" 2>/dev/null || true
    count=$(wc -l < "$out" 2>/dev/null || echo 0)
    if [ "$count" -gt 0 ]; then
        log_warn "Vulture" "$count dead code candidates → $out (no issue created)"
    else
        log_pass "Vulture" "clean"
    fi
else
    log_warn "Vulture" "not installed — run: pip install vulture"
fi

# ── Summary ─────────────────────────────────────────────────────────────────
echo -e "\n${BLUE}═══════════════════════════════════════════${NC}"
echo -e "  ${GREEN}$pass passed${NC}  ${RED}$fail failed${NC}  ${YELLOW}$warn warnings${NC}"
echo -e "  Reports: $REPORT_DIR/"
echo -e "${BLUE}═══════════════════════════════════════════${NC}\n"

[ "$fail" -eq 0 ]
