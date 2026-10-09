import os
from pathlib import Path
import subprocess
import tempfile
import unittest


WRAPPER = Path(__file__).with_name("deploy-twitch-release")
SHA = "a" * 40
LEGACY_SHA = "12987b689642b630f9b62b786f2002a6843add73"
MOCKS = r'''
systemctl() {
  [[ "$1" == show ]] || exit 90
  if [[ "$SCENARIO" == show_failure && "$2" == deadlock-twitch-bot-rust.service ]]; then return 1; fi
  local pid=123 state=active restarts=0
  if [[ "$2" == deadlock-twitch-bot-rust.service ]]; then
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
        self.assertEqual(len(lines), 4)
        self.assertIn(f"current=/opt/deadlock/twitch/releases/{SHA}", lines[0])
        for unit, line in zip(("deadlock-twitch-bot-rust", "deadlock-twitch-dashboard-rust", "deadlock-twitch-stream-coaching-watch"), lines[1:]):
            for field in (f"Unit={unit}", "MainPID=123", "exe=/opt/deadlock/twitch/releases/", "deleted=nein", f"Release-SHA={SHA}", "NRestarts=0", "ActiveState=active"):
                self.assertIn(field, line)

    def test_failures_return_one_and_keep_all_unit_reports(self):
        for scenario in ("deleted", "wrong_sha", "outside_release", "no_pid", "inactive", "bad_restarts", "show_failure", "unreadable", "missing_current", "invalid_current", "race", "current_race"):
            with self.subTest(scenario=scenario):
                result = self.run_wrapper(scenario)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(result.stdout.count("Unit="), 3)
                self.assertTrue(result.stderr)

    def test_extra_arguments_are_rejected(self):
        for args in (("--pruefen", SHA), ("--pruefen", "--restart", "all")):
            with self.subTest(args=args):
                result = self.run_wrapper(args=args)
                self.assertEqual(result.returncode, 1)
                self.assertNotIn("Unit=", result.stdout)


class CutoverTests(unittest.TestCase):
    def run_cutover(self, scenario):
        body = WRAPPER.read_text().split("native_cutover() {\n", 1)[1].split("\n}\n\nif [[ -n", 1)[0]
        mocks = r'''
set -euo pipefail
systemctl() {
  case "$1" in
    show)
      if [[ "$2" == deadlock-twitch-bot-rust.service ]]; then printf '123\n';
      else printf 'loaded\n'; fi ;;
    is-active) [[ "$SCENARIO" == collision ]] ;;
    *) printf 'systemctl %s\n' "$*" ;;
  esac
}
local_psql() {
  case "$*" in
    *"FROM category_native_processes"*)
      if [[ "$SCENARIO" != not_ready ]]; then printf '123-1000000\n'; fi ;;
    *"SELECT now()"*) printf '2026-10-08 12:00:00+00\n' ;;
    *"SELECT poll_seconds"*) printf '60\n' ;;
    *"cutoff="*)
      printf 'snapshot proof checked\n' >&2
      if [[ "$SCENARIO" == no_snapshots ]]; then printf 'f\n'; else printf 't\n'; fi ;;
    *) return 90 ;;
  esac
}
sleep() { :; }
rm() { printf 'obsolete files removed\n'; }
'''
        return subprocess.run(
            ["/bin/bash", "-c", mocks + "\nnative_cutover() {\n" + body + "\n}\nnative_cutover"],
            env={**os.environ, "SCENARIO": scenario}, capture_output=True, text=True, timeout=10,
        )

    def test_old_files_are_removed_after_new_snapshots_are_proved(self):
        result = self.run_cutover("healthy")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("systemctl stop tb-category-collector.service", result.stdout)
        self.assertIn("systemctl disable tb-category-collector.service", result.stdout)
        self.assertIn("snapshot proof checked", result.stderr)
        self.assertIn("obsolete files removed", result.stdout)
        self.assertLess(result.stdout.index("systemctl stop"), result.stdout.index("obsolete files removed"))

    def test_missing_readiness_keeps_old_service_and_files(self):
        result = self.run_cutover("not_ready")
        self.assertEqual(result.returncode, 1)
        self.assertNotIn("systemctl stop", result.stdout)
        self.assertNotIn("obsolete files removed", result.stdout)

    def test_missing_snapshots_keep_old_files(self):
        result = self.run_cutover("no_snapshots")
        self.assertEqual(result.returncode, 1)
        self.assertIn("systemctl stop tb-category-collector.service", result.stdout)
        self.assertIn("snapshot proof checked", result.stderr)
        self.assertNotIn("obsolete files removed", result.stdout)

    def test_still_active_old_service_prevents_file_removal(self):
        result = self.run_cutover("collision")
        self.assertEqual(result.returncode, 1)
        self.assertNotIn("snapshot proof checked", result.stderr)
        self.assertNotIn("obsolete files removed", result.stdout)


class SnapshotProofTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.dsn = os.environ.get("TB_TEST_DATABASE_URL")
        if not cls.dsn:
            if os.environ.get("TB_TEST_REQUIRE_DB") == "1":
                raise RuntimeError("Die synthetische PostgreSQL-Test-DB muss angegeben werden.")
            raise unittest.SkipTest("TB_TEST_DATABASE_URL für den PostgreSQL-Präzisionsbeweis fehlt.")
        if cls.dsn != "postgres://postgres:tbtest@127.0.0.1:33100/postgres":
            raise RuntimeError("Der Präzisionsbeweis verwendet ausschließlich die isolierte Test-DB auf Port 33100.")
        body = WRAPPER.read_text().split("native_cutover() {\n", 1)[1].split("\n}\n\nif [[ -n", 1)[0]
        cls.proof_sql = body.split("<<'SQL'\n", 1)[1].split("\nSQL", 1)[0]

    def snapshot_proof(self, snapshot_text, stored_snapshot, **overrides):
        values = {
            "snapshot_text": snapshot_text,
            "stored_snapshot": stored_snapshot,
            "pid": "123",
            "lease_id": "123-1000000",
            "status_pid": "123",
            "runtime_pid": "123",
            "status_lease": "123-1000000",
            "runtime_lease": "123-1000000",
            "runtime": "tb-bot",
            "native_lease_active": "true",
            "lease_state": "active",
            "status_heartbeat_offset": "0 seconds",
            "runtime_heartbeat_offset": "0 seconds",
            "has_snapshot": "true",
            "cutoff": "2026-10-09 00:19:00+00",
            **overrides,
        }
        fixtures = r'''
BEGIN READ ONLY;
SET LOCAL statement_timeout = '5s';
WITH category_collector_status AS (
    SELECT true AS singleton, now() + :'status_heartbeat_offset'::interval AS heartbeat_at,
        jsonb_build_object('runtime', :'runtime', 'process_id', :'status_pid',
            'lease_id', :'status_lease', 'native_lease_active', :'native_lease_active'::boolean,
            'last_discovery_snapshot_at', NULLIF(:'snapshot_text', '')) AS details
), category_native_runtime AS (
    SELECT true AS singleton, now() + :'runtime_heartbeat_offset'::interval AS heartbeat_at,
        jsonb_build_object('process_id', :'runtime_pid', 'lease_id', :'runtime_lease',
            'lease_state', :'lease_state') AS details
), category_collection_runs AS (
    SELECT :'stored_snapshot'::timestamptz AS snapshot_at WHERE :'has_snapshot'::boolean
)
'''
        raw_comparison = "\nSELECT NULLIF(:'snapshot_text', '')::timestamptz = :'stored_snapshot'::timestamptz;\nROLLBACK;\n"
        args = ["/usr/bin/psql", "--no-psqlrc", "--no-password", "--quiet", "-At",
                "--dbname", self.dsn + "?connect_timeout=5", "-v", "ON_ERROR_STOP=1"]
        for name, value in values.items():
            args.extend(("-v", f"{name}={value}"))
        result = subprocess.run(
            args, input=fixtures + self.proof_sql + raw_comparison,
            env={}, capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.splitlines()

    def test_postgresql_proves_the_exact_stored_microsecond(self):
        cases = (
            ("2026-10-09T00:19:40Z", "2026-10-09 00:19:40+00", "t"),
            ("2026-10-09T00:19:40.1Z", "2026-10-09 00:19:40.100000+00", "t"),
            ("2026-10-09T00:19:40.978Z", "2026-10-09 00:19:40.978000+00", "t"),
            ("2026-10-09T00:19:40.978276Z", "2026-10-09 00:19:40.978276+00", "t"),
            ("2026-10-09T00:19:40.978276499Z", "2026-10-09 00:19:40.978276+00", "t"),
            ("2026-10-09T00:19:40.978276500Z", "2026-10-09 00:19:40.978276+00", "t"),
            ("2026-10-09T00:19:40.978275500Z", "2026-10-09 00:19:40.978275+00", "f"),
            ("2026-10-09T00:19:40.978276501Z", "2026-10-09 00:19:40.978276+00", "f"),
            ("2026-10-09T00:19:40.978276730Z", "2026-10-09 00:19:40.978276+00", "f"),
            ("2026-10-09T00:19:40.999999999Z", "2026-10-09 00:19:40.999999+00", "f"),
            ("2026-10-09T02:19:40.978276730+02:00", "2026-10-09 00:19:40.978276+00", "f"),
        )
        for snapshot_text, stored_snapshot, raw_match in cases:
            with self.subTest(snapshot_text=snapshot_text):
                self.assertEqual(self.snapshot_proof(snapshot_text, stored_snapshot), ["t", raw_match])

    def test_postgresql_rejects_other_measurements_and_invalid_native_state(self):
        snapshot_text = "2026-10-09T00:19:40.978276730Z"
        stored_snapshot = "2026-10-09 00:19:40.978276+00"
        cases = {
            "next_microsecond": {"stored_snapshot": "2026-10-09 00:19:40.978277+00"},
            "previous_microsecond": {"stored_snapshot": "2026-10-09 00:19:40.978275+00"},
            "missing_snapshot": {"has_snapshot": "false"},
            "missing_status_timestamp": {"snapshot_text": ""},
            "wrong_runtime": {"runtime": "standalone"},
            "wrong_status_pid": {"status_pid": "124"},
            "wrong_runtime_pid": {"runtime_pid": "124"},
            "wrong_status_lease": {"status_lease": "123-1000001"},
            "wrong_runtime_lease": {"runtime_lease": "123-1000001"},
            "inactive_native_lease": {"native_lease_active": "false"},
            "waiting_runtime_lease": {"lease_state": "waiting"},
            "stale_status": {"status_heartbeat_offset": "-31 seconds"},
            "stale_runtime": {"runtime_heartbeat_offset": "-31 seconds"},
            "status_at_freshness_boundary": {"status_heartbeat_offset": "-30 seconds"},
            "runtime_at_freshness_boundary": {"runtime_heartbeat_offset": "-30 seconds"},
            "snapshot_at_cutover": {"cutoff": stored_snapshot},
            "snapshot_before_cutover": {"cutoff": "2026-10-09 00:19:40.978277+00"},
        }
        for name, overrides in cases.items():
            with self.subTest(scenario=name):
                values = {"snapshot_text": snapshot_text, "stored_snapshot": stored_snapshot, **overrides}
                self.assertEqual(self.snapshot_proof(**values)[0], "f")


class ArtifactPathTests(unittest.TestCase):
    def check_layout(self, dashboards):
        wrapper = WRAPPER.read_text()
        preflight = wrapper.split("dashboard_source=bot/dashboard_v2/dist\n", 1)[1].split('\nif [[ -e "$dest"', 1)[0]
        installer = WRAPPER.with_name("install-twitch-release.sh").read_text()
        selection = installer.split("dashboard_source=bot/dashboard_v2/dist\n", 1)[1].split('\nfor relative in "${generated[@]}";', 1)[0]
        copy = next(line for line in installer.splitlines() if line.strip().startswith('cp -a "$checkout/$dashboard_source/."'))
        with tempfile.TemporaryDirectory(prefix="twitch-artifact-paths-") as directory:
            checkout = Path(directory) / "checkout"
            for relative in (
                "rust/target/release/tb-bot", "rust/target/release/tb-dashboard",
                "rust/target/release/tb-stream-audit", "rust/target/release/tb-config-check",
                "rust/target/release/tb-llm-usage-recover",
            ):
                artifact = checkout / relative
                artifact.parent.mkdir(parents=True, exist_ok=True)
                artifact.write_text("synthetic artifact")
            for relative in (*dashboards, "bot/admin_dashboard/dist", "website/dist"):
                output = checkout / relative
                output.mkdir(parents=True, exist_ok=True)
                (output / "index.html").write_text(relative)
            package = checkout / "bot/dashboard_v2/package.json"
            package.parent.mkdir(parents=True, exist_ok=True)
            package.write_text('{}')
            wrapper_result = subprocess.run(
                ["/bin/bash", "-c", 'set -euo pipefail\nsrc="$1"\nnative_expected=0\ndashboard_source=bot/dashboard_v2/dist\n' + preflight + '\nprintf "%s\\n" "${required_artifacts[@]}"', "wrapper", str(checkout)],
                capture_output=True, text=True, timeout=5,
            )
            if not dashboards:
                self.assertEqual(wrapper_result.returncode, 1)
                self.assertIn("bot/analytics/dashboard_v2/dist", wrapper_result.stderr)
                return
            self.assertEqual(wrapper_result.returncode, 0, wrapper_result.stderr)
            expected = "bot/dashboard_v2/dist" if "bot/dashboard_v2/dist" in dashboards else "bot/analytics/dashboard_v2/dist"
            self.assertIn(expected, wrapper_result.stdout.splitlines())
            stage = Path(directory) / "stage"
            installed = stage / "bot/analytics/dashboard_v2/dist"
            installed.mkdir(parents=True)
            installer_result = subprocess.run(
                ["/bin/bash", "-c", 'set -euo pipefail\ncheckout="$1"\nstage="$2"\ngit_sha="$3"\ngit_safe=(/bin/false)\ndashboard_source=bot/dashboard_v2/dist\n' + selection + '\n' + copy + '\nprintf "%s\\n" "${generated[@]}"', "installer", str(checkout), str(stage), SHA],
                capture_output=True, text=True, timeout=5,
            )
            self.assertEqual(installer_result.returncode, 0, installer_result.stderr)
            self.assertEqual(wrapper_result.stdout.splitlines(), installer_result.stdout.splitlines())
            self.assertEqual((installed / "index.html").read_text(), expected)

    def test_current_build_path_is_required_and_installed(self):
        self.check_layout(("bot/dashboard_v2/dist",))

    def test_base_build_path_is_required_and_installed_despite_new_package_location(self):
        self.check_layout(("bot/analytics/dashboard_v2/dist",))

    def test_current_path_has_the_same_precedence_in_both_scripts(self):
        self.check_layout(("bot/dashboard_v2/dist", "bot/analytics/dashboard_v2/dist"))

    def test_missing_dashboard_is_rejected_before_installation(self):
        self.check_layout(())


class CollectorPackagingTests(unittest.TestCase):
    def check_target(self, revision, native, scenario="complete", target="tb-category-collector"):
        repository = WRAPPER.parents[2]
        sha = subprocess.check_output(
            ["git", "-C", str(repository), "rev-parse", revision], text=True,
        ).strip()
        wrapper = WRAPPER.read_text()
        preflight = "native_expected=0\n" + wrapper.split("native_expected=0\n", 1)[1].split('\nif [[ -e "$dest"', 1)[0]
        installer = WRAPPER.with_name("install-twitch-release.sh").read_text()
        selection = "dashboard_source=bot/dashboard_v2/dist\n" + installer.split("dashboard_source=bot/dashboard_v2/dist\n", 1)[1].split("\ncheck_binary_revisions() {", 1)[0]
        selection = selection.replace("! -user root", f"! -user {os.getuid()}")
        revisions = "check_binary_revisions() {" + installer.split("check_binary_revisions() {", 1)[1].split('\ncheck_binary_revisions "$checkout"', 1)[0]
        copies = '  install -m 0755 "$checkout/rust/target/release/tb-bot"' + installer.split('  install -m 0755 "$checkout/rust/target/release/tb-bot"', 1)[1].split('\n  "${git_safe[@]}" -C "$checkout" archive', 1)[0]
        with tempfile.TemporaryDirectory(prefix="twitch-collector-packaging-") as directory:
            checkout = Path(directory) / "checkout"
            stage = Path(directory) / "stage"
            (stage / "rust/target/release").mkdir(parents=True)
            for relative in (
                "rust/bin/tb-category-collector/Cargo.toml",
                "rust/bin/tb-category-collector/src/lib.rs",
                "rust/bin/tb-category-collector/src/bin/tb-twitch-watchdog.rs",
                "rust/bin/tb-category-collector/src/bin/tb-category-storage.rs",
                "rust/bin/tb-dashboard/src/bin/clip_context_learn.rs",
            ):
                exists = subprocess.run(
                    ["git", "-C", str(repository), "cat-file", "-e", f"{sha}:{relative}"],
                    capture_output=True,
                ).returncode == 0
                if exists:
                    source = checkout / relative
                    source.parent.mkdir(parents=True, exist_ok=True)
                    source.write_text("synthetic source marker")
            for relative in ("bot/dashboard_v2/dist", "bot/admin_dashboard/dist", "website/dist"):
                (checkout / relative).mkdir(parents=True)
            stamp = Path(directory) / "revision"
            for binary in (
                "tb-bot", "tb-dashboard", "tb-stream-audit", "tb-config-check",
                "tb-llm-usage-recover", "tb-category-collector", "tb-twitch-watchdog", "tb-category-storage", "clip_context_learn",
            ):
                if binary == target and scenario == "missing":
                    continue
                stamp.write_bytes((("0" * 40 if scenario == "wrong_revision" and binary == target else sha) + "\0").encode())
                artifact = checkout / "rust/target/release" / binary
                artifact.parent.mkdir(parents=True, exist_ok=True)
                subprocess.run(
                    ["objcopy", "--add-section", f".twitch_build={stamp}", "/usr/bin/true", str(artifact)],
                    check=True, capture_output=True,
                )
            for path in checkout.rglob("*"):
                path.chmod(path.stat().st_mode & ~0o022)
            wrapper_result = subprocess.run(
                ["/bin/bash", "-c", 'set -euo pipefail\nsrc="$1"\ndeclare -A restart_seen=([twitch-bot]=1)\n' + preflight + '\nprintf "%s\\n" "${required_artifacts[@]}"', "wrapper", str(checkout)],
                capture_output=True, text=True, timeout=5,
            )
            prefix = 'set -euo pipefail\ncheckout="$1"\nstage="$2"\ngit_sha="$3"\nrepository="$4"\ntarget_git() { shift 2; /usr/bin/git -C "$repository" "$@"; }\ngit_safe=(target_git)\n'
            command = prefix + selection + "\n" + revisions + '\ncheck_binary_revisions "$checkout"\n' + copies
            if scenario == "incomplete_release":
                command += f'\nrm -- "$stage/rust/target/release/{target}"'
            command += '\ncheck_binary_revisions "$stage"\nprintf "%s\\n" "${generated[@]}"'
            installer_result = subprocess.run(
                ["/bin/bash", "-c", command, "installer", str(checkout), str(stage), sha, str(repository)],
                capture_output=True, text=True, timeout=5,
            )
            collector = f"rust/target/release/{target}"
            if scenario == "missing":
                self.assertEqual(wrapper_result.returncode, 1, wrapper_result.stderr)
                self.assertEqual(installer_result.returncode, 1, installer_result.stderr)
                self.assertIn(collector, wrapper_result.stderr)
                self.assertIn(collector, installer_result.stderr)
            elif scenario in ("wrong_revision", "incomplete_release"):
                self.assertEqual(wrapper_result.returncode, 0, wrapper_result.stderr)
                self.assertEqual(installer_result.returncode, 1, installer_result.stderr)
                self.assertIn(target, installer_result.stderr)
            else:
                self.assertEqual(wrapper_result.returncode, 0, wrapper_result.stderr)
                self.assertEqual(installer_result.returncode, 0, installer_result.stderr)
                self.assertEqual(wrapper_result.stdout.splitlines(), installer_result.stdout.splitlines())
                expected = not native or target == "tb-category-storage"
                self.assertEqual(collector in installer_result.stdout.splitlines(), expected)
                self.assertEqual((stage / collector).exists(), expected)
                if expected:
                    self.assertEqual((checkout / collector).read_bytes(), (stage / collector).read_bytes())

    def test_storage_binary_is_required_checked_and_copied(self):
        self.check_target("HEAD", native=True, target="tb-category-storage")

    def test_missing_storage_binary_is_rejected(self):
        self.check_target("HEAD", native=True, scenario="missing", target="tb-category-storage")

    def test_storage_binary_from_another_revision_is_rejected(self):
        self.check_target("HEAD", native=True, scenario="wrong_revision", target="tb-category-storage")

    def test_existing_release_without_storage_binary_is_rejected(self):
        self.check_target("HEAD", native=True, scenario="incomplete_release", target="tb-category-storage")

    def test_base_revision_requires_checks_and_copies_standalone_collector(self):
        self.check_target(LEGACY_SHA, native=False)

    def test_native_revision_never_packages_a_stale_standalone_binary(self):
        self.check_target("HEAD", native=True)

    def test_missing_legacy_collector_is_rejected_by_both_scripts(self):
        self.check_target(LEGACY_SHA, native=False, scenario="missing")

    def test_legacy_collector_from_another_revision_is_rejected(self):
        self.check_target(LEGACY_SHA, native=False, scenario="wrong_revision")

    def test_existing_release_without_expected_collector_is_rejected(self):
        self.check_target(LEGACY_SHA, native=False, scenario="incomplete_release")


if __name__ == "__main__":
    unittest.main()
