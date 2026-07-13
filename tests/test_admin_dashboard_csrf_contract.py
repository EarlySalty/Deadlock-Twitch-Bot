import unittest
from pathlib import Path


CLIENT_SOURCE = (
    Path(__file__).resolve().parents[1]
    / "bot"
    / "admin_dashboard"
    / "src"
    / "api"
    / "client.ts"
)


class AdminDashboardCsrfContractTests(unittest.TestCase):
    def test_csrf_kommt_nur_aus_der_admin_session(self) -> None:
        source = CLIENT_SOURCE.read_text(encoding="utf-8")

        self.assertNotIn("LEGACY_CSRF_PAGE", source)
        self.assertNotIn("fetchLegacyCsrfToken", source)
        self.assertIn("await resolveJsonCsrfToken(fields)", source)
        self.assertIn("throw new ApiError('CSRF-Token fehlt.', 403)", source)


if __name__ == "__main__":
    unittest.main()
