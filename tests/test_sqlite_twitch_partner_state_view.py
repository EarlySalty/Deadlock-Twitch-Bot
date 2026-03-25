import sqlite3
import unittest

from tests.sqlite_twitch_schema import ensure_sqlite_twitch_schema


class TwitchPartnerStateViewTests(unittest.TestCase):
    def test_manual_partner_opt_out_clears_is_partner_active(self) -> None:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        try:
            ensure_sqlite_twitch_schema(conn)
            conn.execute(
                """
                INSERT INTO twitch_partners (
                    twitch_user_id,
                    twitch_login,
                    manual_verified_permanent,
                    manual_partner_opt_out,
                    raid_bot_enabled,
                    status
                ) VALUES (?, ?, ?, ?, ?, ?)
                """,
                ("22482316", "jekoz42", 1, 1, 0, "active"),
            )

            row = conn.execute(
                """
                SELECT manual_partner_opt_out, is_partner, is_partner_active
                FROM twitch_streamers_partner_state
                WHERE twitch_user_id = ?
                """,
                ("22482316",),
            ).fetchone()

            self.assertIsNotNone(row)
            self.assertEqual(int(row["manual_partner_opt_out"]), 1)
            self.assertEqual(int(row["is_partner"]), 1)
            self.assertEqual(int(row["is_partner_active"]), 0)
        finally:
            conn.close()


if __name__ == "__main__":
    unittest.main()
