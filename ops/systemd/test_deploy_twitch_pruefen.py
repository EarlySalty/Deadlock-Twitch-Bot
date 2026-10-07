import os
from pathlib import Path
import subprocess
import unittest


WRAPPER = Path(__file__).with_name("deploy-twitch-release")
SHA = "a" * 40
MOCKS = r'''
systemctl() {
  [[ "$1" == show ]] || exit 90
  if [[ "$SCENARIO" == show_failure && "$2" == tb-category-collector.service ]]; then return 1; fi
  local pid=123 state=active restarts=0
  if [[ "$2" == tb-category-collector.service ]]; then
    case "$SCENARIO" in
      no_pid) pid=0 ;;
      inactive) state=inactive ;;
      bad_restarts) restarts=unknown ;;
      race) if [[ "${SECOND_SHOW:-}" == yes ]]; then pid=124; fi ;;
    esac
  fi
  printf 'MainPID=%s\nNRestarts=%s\nActiveState=%s\n' "$pid" "$restarts" "$state"
}
readlink() {
  local sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  if [[ "${*: -1}" == /opt/deadlock/twitch/current ]]; then
    case "$SCENARIO" in
      missing_current) return 1 ;;
      invalid_current) printf '/tmp/release\n'; return ;;
    esac
    printf '/opt/deadlock/twitch/releases/%s\n' "$sha"
  else
    case "$SCENARIO" in
      unreadable) return 1 ;;
      wrong_sha) sha=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb ;;
      outside_release) printf '/usr/bin/bash\n'; return ;;
    esac
    printf '/opt/deadlock/twitch/releases/%s/rust/target/release/tb-bot' "$sha"
    if [[ "$SCENARIO" == deleted ]]; then printf ' (deleted)'; fi
    printf '\n'
  fi
}
flock() { exit 91; }
mv() { exit 92; }
chown() { exit 93; }
chmod() { exit 94; }
'''


class PruefenTests(unittest.TestCase):
    def run_wrapper(self, scenario="healthy", args=("--pruefen",)):
        source = WRAPPER.read_text().replace("if [[ $EUID -ne 0 ]]; then", "if false; then", 1)
        source = source.replace("export PATH\n", "export PATH\n" + MOCKS, 1)
        if scenario == "race":
            source = source.replace('if properties=$(systemctl show', 'SECOND_SHOW=no\n    if properties=$(systemctl show', 1)
            source = source.replace('if ! observed=$(systemctl show', 'SECOND_SHOW=yes\n    if ! observed=$(systemctl show', 1)
        if scenario == "current_race":
            source = source.replace('if ! observed=$(readlink -e', 'SCENARIO=invalid_current\n  if ! observed=$(readlink -e', 1)
        return subprocess.run(
            ["/bin/bash", "-c", source, "deploy-twitch-release", *args],
            env={**os.environ, "SCENARIO": scenario},
            capture_output=True,
            text=True,
            timeout=5,
        )

    def test_healthy_units_are_reported_without_mutations(self):
        result = self.run_wrapper()
        self.assertEqual(result.returncode, 0, result.stderr)
        lines = result.stdout.splitlines()
        self.assertEqual(len(lines), 5)
        self.assertIn(f"current=/opt/deadlock/twitch/releases/{SHA}", lines[0])
        for unit, line in zip(("deadlock-twitch-bot-rust", "deadlock-twitch-dashboard-rust", "deadlock-twitch-stream-coaching-watch", "tb-category-collector"), lines[1:]):
            for field in (f"Unit={unit}", "MainPID=123", "exe=/opt/deadlock/twitch/releases/", "deleted=nein", f"Release-SHA={SHA}", "NRestarts=0", "ActiveState=active"):
                self.assertIn(field, line)

    def test_failures_return_one_and_keep_all_unit_reports(self):
        for scenario in ("deleted", "wrong_sha", "outside_release", "no_pid", "inactive", "bad_restarts", "show_failure", "unreadable", "missing_current", "invalid_current", "race", "current_race"):
            with self.subTest(scenario=scenario):
                result = self.run_wrapper(scenario)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(result.stdout.count("Unit="), 4)
                self.assertTrue(result.stderr)

    def test_extra_arguments_are_rejected(self):
        for args in (("--pruefen", SHA), ("--pruefen", "--restart", "all")):
            with self.subTest(args=args):
                result = self.run_wrapper(args=args)
                self.assertEqual(result.returncode, 1)
                self.assertNotIn("Unit=", result.stdout)


if __name__ == "__main__":
    unittest.main()
