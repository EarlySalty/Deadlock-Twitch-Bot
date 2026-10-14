import pathlib
import socket
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[4]
MIGRATIONS = ROOT / "rust" / "migrations"
PG_BIN = pathlib.Path("/usr/lib/postgresql/16/bin")
DE = 1289721245281292288
CUTOVER = MIGRATIONS / "20261013150300_multiserver_partner_cutover.sql"


class MultiserverSchemaContract(unittest.TestCase):
    @classmethod
    def run_command(cls, command, **kwargs):
        result = subprocess.run(
            command, text=True, capture_output=True, check=False, **kwargs
        )
        if result.returncode:
            raise AssertionError(
                f"Command failed ({result.returncode}): {command}\n{result.stderr}"
            )
        return result.stdout.strip()

    @classmethod
    def sql(cls, sql, database="twitch_analytics"):
        return cls.run_command(cls.psql + ["-d", database, "-Atc", sql])

    @classmethod
    def migration(cls, path, database="twitch_analytics", succeeds=True):
        command = cls.psql + ["-d", database, "-f", str(path)]
        if not path.read_text().startswith("-- no-transaction"):
            command.append("--single-transaction")
        result = subprocess.run(command, text=True, capture_output=True, check=False)
        if succeeds and result.returncode:
            raise AssertionError(
                f"Migration {path.name} failed ({result.returncode}):\n{result.stderr}"
            )
        if not succeeds and (
            result.returncode != 3 or "verhindert die Umschaltung" not in result.stderr
        ):
            raise AssertionError(
                f"Cutover did not refuse the mismatch: {result.returncode}\n{result.stderr}"
            )
        return result

    @classmethod
    def seed(cls):
        cls.sql("""
            INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
            SELECT (990000000000 + n)::text, 'schema_fixture_' || n FROM generate_series(1, 9) n;
            INSERT INTO twitch_partners (twitch_user_id, twitch_login, status,
                manual_partner_opt_out, technical_pause_reason, admin_archived_at)
            SELECT (990000000000 + n)::text, 'schema_fixture_' || n,
                CASE WHEN n IN (3, 7, 8) THEN 'archived' ELSE 'active' END,
                CASE WHEN n = 2 THEN 1 ELSE 0 END,
                CASE WHEN n = 4 THEN 'oauth_missing' WHEN n = 5 THEN 'bot_banned' ELSE NULL END,
                CASE WHEN n = 6 THEN '2026-10-01T00:00:00Z' ELSE NULL END
            FROM generate_series(1, 9) n;
            INSERT INTO twitch_exclusions (twitch_user_id, kind) VALUES ('990000000007', 'banned');
            INSERT INTO twitch_partner_signup_denylist (twitch_user_id, twitch_login, reason, added_by)
            VALUES ('990000000008', 'schema_fixture_8', 'fixture', 'fixture');
        """)

    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="ttb-schema-r2-throwaway-")
        cls.addClassCleanup(cls.temporary.cleanup)
        directory = pathlib.Path(cls.temporary.name)
        cls.data = directory / "pgdata"
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        cls.psql = [
            "psql",
            "-X",
            "-q",
            "-v",
            "ON_ERROR_STOP=1",
            "-h",
            str(directory),
            "-p",
            str(port),
            "-U",
            "postgres",
        ]
        cls.run_command(
            [
                str(PG_BIN / "initdb"),
                "-D",
                str(cls.data),
                "-U",
                "postgres",
                "--auth=trust",
                "--encoding=UTF8",
                "--no-locale",
            ]
        )
        cls.run_command(
            [
                str(PG_BIN / "pg_ctl"),
                "-D",
                str(cls.data),
                "-l",
                str(directory / "postgres.log"),
                "-o",
                f"-p {port} -k {directory} -c listen_addresses='' -c shared_preload_libraries=timescaledb -c timescaledb.telemetry_level=off",
                "-w",
                "start",
            ]
        )
        cls.addClassCleanup(cls.stop_database)
        cls.sql("CREATE DATABASE twitch_analytics", "postgres")
        cls.sql("CREATE EXTENSION timescaledb")
        files = sorted(MIGRATIONS.glob("*.sql"))
        versions = [path.name.split("_", 1)[0] for path in files]
        if len(versions) != len(set(versions)):
            raise AssertionError("Duplicate migration versions")
        for path in files:
            if path.name == "20261012120000_multiserver_guilds_language.sql":
                cls.seed()
            if path == CUTOVER:
                continue
            cls.migration(path)
            if path.name == "20261012120000_multiserver_guilds_language.sql":
                actual = cls.sql(
                    "SELECT COUNT(*), COUNT(*) FILTER (WHERE active), COUNT(*) FILTER (WHERE NOT active AND deactivated_at IS NULL) FROM streamer_guilds"
                )
                if actual != "9|2|0":
                    raise AssertionError(
                        f"Bootstrap order or inactive timestamps failed: {actual}"
                    )
        cls.applied_count = len(files) - 1
        cls.sql(
            "CREATE TABLE fixture_before_cutover AS SELECT * FROM twitch_partners_all_state"
        )
        cls.sql(
            "CREATE DATABASE schema_fixture_template TEMPLATE twitch_analytics",
            "postgres",
        )
        print(
            f"Executed {cls.applied_count} actual migrations before cutover; PostgreSQL/Timescale "
            + cls.sql(
                "SELECT current_setting('server_version') || '/' || extversion FROM pg_extension WHERE extname='timescaledb'"
            )
        )

    @classmethod
    def stop_database(cls):
        cls.run_command(
            [str(PG_BIN / "pg_ctl"), "-D", str(cls.data), "-m", "fast", "-w", "stop"]
        )

    def setUp(self):
        self.sql("DROP DATABASE twitch_analytics", "postgres")
        self.sql(
            "CREATE DATABASE twitch_analytics TEMPLATE schema_fixture_template",
            "postgres",
        )

    def test_bootstrap_shadow_and_canonical_cutover(self):
        self.assertEqual(
            self.sql("SELECT * FROM twitch_partner_shadow_summary()"), "9|2|2|0"
        )
        self.assertEqual(
            self.sql(
                "SELECT COUNT(*) FROM ((SELECT * FROM fixture_before_cutover EXCEPT ALL SELECT * FROM twitch_partners_legacy_state) UNION ALL (SELECT * FROM twitch_partners_legacy_state EXCEPT ALL SELECT * FROM fixture_before_cutover)) d"
            ),
            "0",
        )
        self.migration(CUTOVER)
        self.assertEqual(
            self.sql(
                "SELECT COUNT(*), COUNT(*) FILTER (WHERE is_partner_active=1) FROM twitch_partners_all_state"
            ),
            "9|2",
        )
        self.assertEqual(
            self.sql(
                "SELECT COUNT(*) FROM twitch_partners_all_state WHERE twitch_user_id IN ('990000000002','990000000003','990000000004','990000000005','990000000006','990000000007','990000000008') AND is_partner_active=1"
            ),
            "0",
        )
        self.assertEqual(
            self.sql(
                "SELECT COUNT(*) FROM ((SELECT twitch_user_id, manual_partner_opt_out, archived_at, technical_pause_reason, status FROM fixture_before_cutover EXCEPT ALL SELECT twitch_user_id, manual_partner_opt_out, archived_at, technical_pause_reason, status FROM twitch_partners_all_state) UNION ALL (SELECT twitch_user_id, manual_partner_opt_out, archived_at, technical_pause_reason, status FROM twitch_partners_all_state EXCEPT ALL SELECT twitch_user_id, manual_partner_opt_out, archived_at, technical_pause_reason, status FROM fixture_before_cutover)) d"
            ),
            "0",
        )

    def test_legacy_projection_drift_refuses_before_activation(self):
        definition = self.sql(
            "SELECT pg_get_viewdef('twitch_partners_legacy_state'::regclass)"
        )
        altered = definition.replace("1 AS is_partner", "0 AS is_partner")
        self.assertNotEqual(definition, altered)
        self.sql("CREATE OR REPLACE VIEW twitch_partners_legacy_state AS " + altered)
        self.assertEqual(
            self.sql("SELECT mismatches FROM twitch_partner_shadow_summary()"), "0"
        )
        self.migration(CUTOVER, succeeds=False)
        self.assertEqual(
            self.sql(
                "SELECT COUNT(*) FROM twitch_partners_all_state WHERE is_partner=1"
            ),
            "9",
        )

    def test_membership_mismatch_refuses_before_activation(self):
        self.sql(
            f"UPDATE streamer_guilds SET active=false, deactivated_at=now(), deactivated_by='admin' WHERE twitch_user_id='990000000001' AND guild_id={DE}"
        )
        self.assertEqual(
            self.sql("SELECT mismatches FROM twitch_partner_shadow_summary()"), "1"
        )
        self.migration(CUTOVER, succeeds=False)
        self.assertEqual(
            self.sql(
                "SELECT is_partner_active FROM twitch_partners_all_state WHERE twitch_user_id='990000000001'"
            ),
            "1",
        )
        self.assertNotIn(
            "twitch_partners_guild_state",
            self.sql("SELECT pg_get_viewdef('twitch_partners_all_state'::regclass)"),
        )

    def test_global_exclusion_refuses_and_ai_fails_closed(self):
        self.sql(
            "INSERT INTO twitch_exclusions (twitch_user_id, kind) VALUES ('990000000001', 'banned')"
        )
        self.assertEqual(
            self.sql("SELECT mismatches FROM twitch_partner_shadow_summary()"), "1"
        )
        self.assertEqual(
            self.sql(
                "SELECT twitch_streamer_ai_allowed('990000000001', 'schema_fixture_1')"
            ),
            "f",
        )
        self.migration(CUTOVER, succeeds=False)

    def test_invalid_identity_mismatch_is_not_hidden(self):
        self.sql(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) VALUES ('invalid-fixture', 'invalid_fixture', 'active')"
        )
        self.assertEqual(
            self.sql("SELECT mismatches FROM twitch_partner_shadow_summary()"), "1"
        )
        self.migration(CUTOVER, succeeds=False)

    def test_mixed_membership_ai_and_unknown_identity(self):
        self.migration(CUTOVER)
        self.assertEqual(
            self.sql(
                "SELECT twitch_streamer_ai_allowed('990000000001','schema_fixture_1')"
            ),
            "t",
        )
        self.sql(
            "INSERT INTO discord_guilds (guild_id, language, ai_features) VALUES (900000000000000001, 'en', false); INSERT INTO streamer_guilds (twitch_user_id, guild_id, source) VALUES ('990000000001', 900000000000000001, 'onboarding')"
        )
        self.assertEqual(
            self.sql(
                "SELECT twitch_streamer_ai_allowed('990000000001','schema_fixture_1')"
            ),
            "f",
        )
        self.sql(
            "UPDATE discord_guilds SET status='removed', removed_at=now() WHERE guild_id=900000000000000001"
        )
        self.assertEqual(
            self.sql(
                "SELECT twitch_streamer_ai_allowed('990000000001','schema_fixture_1')"
            ),
            "t",
        )
        self.assertEqual(
            self.sql(
                "SELECT twitch_streamer_ai_allowed('990000000001','schema_fixture_9'), twitch_streamer_ai_allowed(NULL,'unknown_fixture'), twitch_streamer_ai_allowed('invalid-fixture', NULL)"
            ),
            "f|f|f",
        )

    def test_inactive_rebootstrap_and_orphan_no_false_positive(self):
        self.sql(
            f"UPDATE streamer_guilds SET active=false, deactivated_at='2026-10-01T00:00:00Z', deactivated_by='admin' WHERE twitch_user_id='990000000001' AND guild_id={DE}"
        )
        self.migration(MIGRATIONS / "20261012120000_multiserver_guilds_language.sql")
        self.migration(MIGRATIONS / "20261013150000_guilds_partner_membership.sql")
        self.assertEqual(
            self.sql(
                "SELECT active, deactivated_by, deactivated_at='2026-10-01T00:00:00Z'::timestamptz FROM streamer_guilds WHERE twitch_user_id='990000000001'"
            ),
            "f|admin|t",
        )
        self.sql(
            f"INSERT INTO streamer_guilds (twitch_user_id, guild_id, source) VALUES ('990000009999', {DE}, 'onboarding')"
        )
        self.assertEqual(
            self.sql(
                "SELECT COUNT(*) FROM twitch_partners_guild_state WHERE twitch_user_id='990000009999' AND is_partner_active=1"
            ),
            "0",
        )

    def test_runtime_roles_allow_effects_and_refuse_forbidden_writes(self):
        self.migration(CUTOVER)
        roles = ROOT / "ops" / "systemd" / "twitch-runtime-roles.sql"
        self.migration(roles)
        self.migration(roles)
        self.sql(
            "SET ROLE twitchhub; INSERT INTO discord_guilds (guild_id,name,status,removed_at,updated_at) VALUES (900000000000000001,'Fixture','active',NULL,now()); UPDATE discord_guilds SET language='en', updated_by='setup' WHERE guild_id=900000000000000001; UPDATE streamer_guilds SET active=false,deactivated_by='discord_leave',deactivated_at=now() WHERE twitch_user_id='990000000001'; RESET ROLE"
        )
        self.assertEqual(
            self.sql(
                "SELECT COUNT(*) FROM streamer_guilds WHERE twitch_user_id='990000000001' AND NOT active AND deactivated_by='discord_leave'"
            ),
            "1",
        )
        self.assertEqual(
            self.sql(
                "SELECT rolcanlogin,rolinherit,rolsuper,rolcreatedb,rolcreaterole,rolreplication,rolbypassrls FROM pg_roles WHERE rolname='twitchhub'"
            ),
            "t|f|f|f|f|f|f",
        )
        self.assertEqual(
            self.sql(
                "SELECT has_column_privilege('twitchhub','discord_guilds','ai_features','UPDATE'), has_column_privilege('twitchdash','discord_guilds','ai_features','UPDATE'), has_column_privilege('twitchhub','twitch_streamer_identities','twitch_login','SELECT'), has_table_privilege('twitchhub','twitch_partners','UPDATE'), has_table_privilege('twitchhub','server_admin_audit_log','SELECT')"
            ),
            "f|f|f|f|f",
        )
        for role, statement in [
            ("twitchhub", "UPDATE discord_guilds SET ai_features=true"),
            ("twitchdash", "UPDATE discord_guilds SET ai_features=true"),
            ("twitchhub", "DELETE FROM streamer_guilds"),
            ("twitchhub", "SELECT twitch_login FROM twitch_streamer_identities"),
            ("twitchhub", "CREATE TABLE hub_unauthorized (id int)"),
            ("twitchdash", "DELETE FROM server_admin_audit_log"),
        ]:
            result = subprocess.run(
                self.psql
                + ["-d", "twitch_analytics", "-c", f"SET ROLE {role}; {statement}"],
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("permission denied", result.stderr)
        self.sql(
            "SET ROLE twitchdash; INSERT INTO server_admin_audit_log (guild_id,actor,action) VALUES (900000000000000001,'fixture','fixture'); RESET ROLE"
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
