from __future__ import annotations

import json
import subprocess
import sys
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]


class DashboardImportBoundaryTests(unittest.TestCase):
    def _run_import(self, statement: str) -> dict[str, list[str]]:
        code = (
            "import json\n"
            "import sys\n\n"
            "before = set(sys.modules)\n"
            f"{statement.rstrip()}\n"
            "loaded = sorted(name for name in sys.modules if name not in before)\n"
            "print(json.dumps({\n"
            '    "dashboard": [name for name in loaded if name.startswith("bot.dashboard")],\n'
            '    "all": loaded,\n'
            "}))\n"
        )
        completed = subprocess.run(
            [sys.executable, "-c", code],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
        return json.loads(completed.stdout)

    def test_root_dashboard_import_is_lazy(self) -> None:
        loaded = self._run_import("import bot.dashboard")

        self.assertEqual(loaded["dashboard"], ["bot.dashboard"])

    def test_auth_package_import_does_not_pull_other_dashboard_features(self) -> None:
        loaded = self._run_import("import bot.dashboard.auth")

        self.assertIn("bot.dashboard", loaded["dashboard"])
        self.assertIn("bot.dashboard._compat", loaded["dashboard"])
        self.assertIn("bot.dashboard.auth", loaded["dashboard"])
        self.assertNotIn("bot.dashboard.affiliate", loaded["dashboard"])
        self.assertNotIn("bot.dashboard.billing.billing_mixin", loaded["dashboard"])
        self.assertNotIn(
            "bot.dashboard.live.live_announcement_mixin", loaded["dashboard"]
        )
        self.assertNotIn("bot.dashboard.raids.raid_mixin", loaded["dashboard"])

    def test_live_package_import_stays_lightweight(self) -> None:
        loaded = self._run_import("import bot.dashboard.live")

        self.assertIn("bot.dashboard", loaded["dashboard"])
        self.assertIn("bot.dashboard._compat", loaded["dashboard"])
        self.assertIn("bot.dashboard.live", loaded["dashboard"])
        self.assertNotIn("bot.dashboard.live.live", loaded["dashboard"])
        self.assertNotIn(
            "bot.dashboard.live.live_announcement_mixin", loaded["dashboard"]
        )
        self.assertNotIn("bot.dashboard.billing.billing_mixin", loaded["dashboard"])

    def test_live_compat_exports_do_not_pull_announcement_modules(self) -> None:
        loaded = self._run_import(
            "import bot.dashboard.live as live_pkg\n"
            "assert hasattr(live_pkg, 'DashboardLiveMixin')\n"
            "assert hasattr(live_pkg, '_storage')\n"
            "assert hasattr(live_pkg, '_BILLING_PLANS')\n"
        )

        self.assertIn("bot.dashboard.live.live", loaded["dashboard"])
        self.assertIn("bot.dashboard.billing.billing_plans", loaded["dashboard"])
        self.assertNotIn("bot.dashboard.billing.billing_mixin", loaded["dashboard"])
        self.assertNotIn(
            "bot.dashboard.live.announcement_mode_mixin", loaded["dashboard"]
        )
        self.assertNotIn(
            "bot.dashboard.live.live_announcement_mixin", loaded["dashboard"]
        )

    def test_legacy_auth_shim_loads_only_target_module(self) -> None:
        loaded = self._run_import(
            "import bot.dashboard.auth_mixin as legacy_auth\n"
            "assert hasattr(legacy_auth, '_DashboardAuthMixin')\n"
            "assert hasattr(legacy_auth, 'secrets')\n"
        )

        self.assertIn("bot.dashboard.auth_mixin", loaded["dashboard"])
        self.assertIn("bot.dashboard.auth.auth_mixin", loaded["dashboard"])
        self.assertNotIn("bot.dashboard.affiliate.affiliate_mixin", loaded["dashboard"])
        self.assertNotIn("bot.dashboard.billing.billing_mixin", loaded["dashboard"])
        self.assertNotIn(
            "bot.dashboard.live.live_announcement_mixin", loaded["dashboard"]
        )
        self.assertNotIn("bot.dashboard.raids.raid_mixin", loaded["dashboard"])

    def test_live_announcement_mode_shim_loads_admin_target(self) -> None:
        loaded = self._run_import(
            "import bot.dashboard.live.announcement_mode_mixin as legacy_announcement\n"
            "assert hasattr(legacy_announcement, 'DashboardAdminAnnouncementMixin')\n"
        )

        self.assertIn("bot.dashboard.live.announcement_mode_mixin", loaded["dashboard"])
        self.assertIn("bot.dashboard.admin.announcement_mode_mixin", loaded["dashboard"])
        self.assertNotIn("bot.dashboard.live.live_announcement_mixin", loaded["dashboard"])
        self.assertNotIn("bot.dashboard.billing.billing_mixin", loaded["dashboard"])


if __name__ == "__main__":
    unittest.main()
