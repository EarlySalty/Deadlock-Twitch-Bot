import difflib
import hashlib
import pathlib
import socket
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[4]
MIGRATIONS = ROOT / "rust" / "migrations"
PG_BIN = pathlib.Path("/usr/lib/postgresql/16/bin")
DE = 1289721245281292288
GUILD = MIGRATIONS / "20261014160000_guild_schema_shadow.sql"
BASE = "50125b0bb67fc2d32b440594a5cd5e95add21c4d"


class MultiserverSchemaContract(unittest.TestCase):
    @classmethod
    def command(cls, command):
        result = subprocess.run(command, text=True, capture_output=True, check=False)
        if result.returncode:
            raise AssertionError(
                f"Command failed ({result.returncode}):\n{result.stderr}"
            )
        return result.stdout.strip()

    @classmethod
    def sql(cls, sql, database="guild_fixture"):
        return cls.command(cls.psql + ["-d", database, "-Atc", sql])

    @classmethod
    def migration(cls, path, database="guild_fixture", succeeds=True):
        command = cls.psql + ["-d", database, "-f", str(path)]
        if not path.read_text().startswith("-- no-transaction"):
            command.append("--single-transaction")
        result = subprocess.run(command, text=True, capture_output=True, check=False)
        if succeeds and result.returncode:
            raise AssertionError(f"Migration {path.name} failed:\n{result.stderr}")
        if not succeeds and result.returncode != 3:
            raise AssertionError(
                f"Migration did not reject unsafe legacy data: {result.stderr}"
            )
        return result

    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="ttb-g0-throwaway-")
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
        cls.command(
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
        cls.command(
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
        cls.sql("CREATE DATABASE guild_main_template", "postgres")
        cls.sql("CREATE EXTENSION timescaledb", "guild_main_template")
        files = sorted(MIGRATIONS.glob("*.sql"))
        versions = [path.name.split("_", 1)[0] for path in files]
        if len(versions) != len(set(versions)):
            raise AssertionError("Duplicate migration versions")
        base_paths = cls.command(
            [
                "git",
                "-C",
                str(ROOT),
                "ls-tree",
                "-r",
                "--name-only",
                BASE,
                "rust/migrations",
            ]
        ).splitlines()
        hashes = []
        for relative in base_paths:
            if not relative.endswith(".sql"):
                continue
            original = subprocess.run(
                ["git", "-C", str(ROOT), "show", f"{BASE}:{relative}"],
                capture_output=True,
                check=True,
            ).stdout
            current = (ROOT / relative).read_bytes()
            if original != current:
                raise AssertionError(f"Applied migration modified: {relative}")
            hashes.append(hashlib.sha256(current).hexdigest())
        for path in files:
            if path != GUILD:
                cls.migration(path, "guild_main_template")
        print(
            f"Main migrations: {len(hashes)} unchanged SHA-256 hashes; versions unique: {len(versions)}",
            flush=True,
        )
        print(
            "Disposable PostgreSQL/Timescale: "
            + cls.sql(
                "SELECT current_setting('server_version') || '/' || extversion FROM pg_extension WHERE extname='timescaledb'",
                "guild_main_template",
            ),
            flush=True,
        )

        cls.sql(
            "ALTER DATABASE guild_main_template ALLOW_CONNECTIONS false; SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname='guild_main_template'",
            "postgres",
        )

    @classmethod
    def stop_database(cls):
        cls.command(
            [str(PG_BIN / "pg_ctl"), "-D", str(cls.data), "-m", "fast", "-w", "stop"]
        )

    def setUp(self):
        self.sql("DROP DATABASE IF EXISTS guild_fixture", "postgres")
        self.sql(
            "CREATE DATABASE guild_fixture TEMPLATE guild_main_template", "postgres"
        )

    def seed(self):
        self.sql("""
            INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
            SELECT (990000000000 + n)::text, 'schema_fixture_' || n FROM generate_series(1, 11) n;
            INSERT INTO twitch_partners (twitch_user_id, twitch_login, status,
                manual_partner_opt_out, technical_pause_reason, admin_archived_at, require_discord_link)
            SELECT (990000000000 + n)::text, 'schema_fixture_' || n,
                CASE WHEN n = 3 THEN 'archived' ELSE 'active' END,
                CASE WHEN n = 2 THEN 1 ELSE 0 END,
                CASE WHEN n = 4 THEN 'oauth_missing' WHEN n = 5 THEN 'bot_banned' WHEN n = 11 THEN ' BLOCKED ' ELSE NULL END,
                CASE WHEN n = 6 THEN '2026-10-01T00:00:00Z' ELSE NULL END, 1
            FROM generate_series(1, 11) n;
            UPDATE twitch_streamer_identities SET is_on_discord=1 WHERE twitch_user_id='990000000001';
            INSERT INTO twitch_exclusions (twitch_user_id, kind) VALUES ('990000000007', 'banned');
            INSERT INTO twitch_partner_signup_denylist (twitch_user_id, twitch_login, reason, added_by)
            VALUES ('990000000008', 'schema_fixture_8', 'fixture', 'fixture'),
                   ('990000099999', 'schema_fixture_9', 'fixture', 'fixture');
        """)

    def legacy_schema(self, variant):
        status = "CHECK (status IN ('active','removed'))"
        language = (
            "CHECK (language IN ('de','en'))"
            if variant == 2
            else "CHECK (language ~ '^[a-z]{2,8}(-[a-z0-9]{1,8})*$')"
        )
        timestamp_check = (
            "CHECK (active OR deactivated_at IS NOT NULL)" if variant == 1 else ""
        )
        guild_delete = "ON DELETE CASCADE" if variant == 2 else ""
        self.sql(f"""
            CREATE TABLE discord_guilds (
                guild_id bigint PRIMARY KEY, name text, language text NOT NULL DEFAULT 'en' {language},
                announce_channel_id bigint, streamer_role_id bigint, ping_role_id bigint, alert_channel_id bigint,
                promo_invite_url text, auto_partner_default boolean NOT NULL DEFAULT true,
                cross_server_raids boolean NOT NULL DEFAULT true, ai_features boolean NOT NULL DEFAULT false,
                installed_at timestamptz NOT NULL DEFAULT now(), removed_at timestamptz,
                status text NOT NULL DEFAULT 'active' {status}
            );
            CREATE TABLE streamer_guilds (
                twitch_user_id text NOT NULL, guild_id bigint NOT NULL REFERENCES discord_guilds(guild_id) {guild_delete},
                active boolean NOT NULL DEFAULT true {timestamp_check}, deactivated_by text,
                deactivated_at timestamptz, joined_at timestamptz NOT NULL DEFAULT now(), source text NOT NULL,
                PRIMARY KEY (twitch_user_id,guild_id)
            );
        """)
        if variant == 2:
            self.sql(
                "ALTER TABLE twitch_streamer_identities ADD COLUMN broadcaster_language text NOT NULL DEFAULT 'und'"
            )

    def test_clean_schema_snapshot_and_no_cutover_or_ai_install(self):
        before = self.sql(
            "SELECT pg_get_viewdef('twitch_partners_all_state'::regclass) || pg_get_viewdef('twitch_streamers_partner_state'::regclass)"
        )
        self.migration(GUILD)
        self.assertEqual(
            before,
            self.sql(
                "SELECT pg_get_viewdef('twitch_partners_all_state'::regclass) || pg_get_viewdef('twitch_streamers_partner_state'::regclass)"
            ),
        )
        self.assertEqual(
            self.sql(
                "SELECT to_regprocedure('public.twitch_streamer_ai_allowed(text,text)') IS NULL"
            ),
            "t",
        )
        actual = self.sql(
            "SELECT table_name || '|' || column_name || '|' || data_type || '|' || is_nullable || '|' || coalesce(column_default,'') FROM information_schema.columns WHERE table_schema='public' AND table_name <> '_sqlx_migrations' AND table_name !~ '^category_chat_messages_p[0-9]{8}$' ORDER BY table_name COLLATE \"C\", column_name COLLATE \"C\""
        ).splitlines()
        actual.sort()
        expected = (
            (ROOT / "rust/crates/tb-db/tests/fresh_schema_snapshot.txt")
            .read_text()
            .splitlines()
        )
        self.assertTrue(
            actual == expected, "\n".join(difflib.unified_diff(expected, actual, n=1))
        )

    def test_backfill_preserves_runtime_and_global_exclusions(self):
        self.seed()
        before = self.sql(
            "SELECT row_to_json(s) FROM twitch_partners_all_state s ORDER BY twitch_user_id"
        )
        self.migration(GUILD)
        self.assertEqual(
            self.sql(
                "SELECT count(*), count(*) FILTER (WHERE active), count(*) FILTER (WHERE NOT active AND deactivated_at IS NULL) FROM streamer_guilds"
            ),
            "11|2|0",
        )
        self.assertEqual(
            self.sql("SELECT * FROM twitch_partner_shadow_summary()"), "11|5|2|3"
        )
        self.assertEqual(
            before,
            self.sql(
                "SELECT row_to_json(s) FROM twitch_partners_all_state s ORDER BY twitch_user_id"
            ),
        )
        self.assertEqual(
            self.sql(
                "SELECT is_on_discord, require_discord_link FROM streamer_guilds WHERE twitch_user_id='990000000001'"
            ),
            "t|t",
        )
        self.sql(
            "INSERT INTO twitch_exclusions (twitch_user_id,kind) VALUES ('990000000001','banned')"
        )
        self.assertEqual(
            self.sql(
                "SELECT count(*) FROM partner_guild_active WHERE twitch_user_id='990000000001'"
            ),
            "0",
        )
        self.assertEqual(
            self.sql(
                "SELECT is_partner_active FROM twitch_partners_all_state WHERE twitch_user_id='990000000001'"
            ),
            "1",
        )

    def test_repetition_preserves_admin_deactivation_and_configuration(self):
        self.seed()
        self.migration(GUILD)
        self.sql(
            f"UPDATE streamer_guilds SET active=false, is_on_discord=false, require_discord_link=false, deactivated_at='2026-10-01Z', deactivated_by='admin' WHERE twitch_user_id='990000000001'; UPDATE discord_guilds SET name='Custom', announce_channel_id=42, updated_by='admin', ai_features=false WHERE guild_id={DE}"
        )
        before = self.sql(
            "SELECT row_to_json(s) FROM streamer_guilds s ORDER BY twitch_user_id"
        )
        self.migration(GUILD)
        self.assertEqual(
            before,
            self.sql(
                "SELECT row_to_json(s) FROM streamer_guilds s ORDER BY twitch_user_id"
            ),
        )
        self.assertEqual(
            self.sql(
                f"SELECT name, announce_channel_id, ai_features FROM discord_guilds WHERE guild_id={DE}"
            ),
            "Custom|42|f",
        )

    def test_both_prepared_schema_variants_are_compatible(self):
        for variant in (1, 2):
            with self.subTest(variant=variant):
                if variant == 2:
                    self.setUp()
                self.seed()
                self.legacy_schema(variant)
                self.sql(
                    f"INSERT INTO discord_guilds (guild_id,name,language) VALUES ({DE},'Preserved','de'); INSERT INTO streamer_guilds (twitch_user_id,guild_id,active,deactivated_by,deactivated_at,source) VALUES ('990000000001',{DE},false,'admin','2026-10-01Z','admin')"
                )
                self.migration(GUILD)
                self.migration(GUILD)
                self.assertEqual(
                    self.sql(
                        "SELECT active, deactivated_by, deactivated_at='2026-10-01Z'::timestamptz FROM streamer_guilds WHERE twitch_user_id='990000000001'"
                    ),
                    "f|admin|t",
                )
                self.assertEqual(
                    self.sql(f"SELECT name FROM discord_guilds WHERE guild_id={DE}"),
                    "Preserved",
                )
                self.assertEqual(
                    self.sql(
                        "SELECT is_on_discord, require_discord_link FROM streamer_guilds WHERE twitch_user_id='990000000001'"
                    ),
                    "t|t",
                )
                self.rejected_sql(f"DELETE FROM discord_guilds WHERE guild_id={DE}")
                self.sql("INSERT INTO discord_guilds (guild_id) VALUES (42)")
                self.assertEqual(
                    self.sql(
                        "SELECT status, ai_features FROM discord_guilds WHERE guild_id=42"
                    ),
                    "inactive|f",
                )

    def rejected_sql(self, statement):
        result = subprocess.run(
            self.psql + ["-d", "guild_fixture", "-c", statement],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertTrue("violates" in result.stderr, result.stderr)

    def test_foreign_activation_and_invalid_relationships_are_rejected(self):
        self.seed()
        self.migration(GUILD)
        self.sql("INSERT INTO discord_guilds (guild_id) VALUES (42)")
        self.rejected_sql("UPDATE discord_guilds SET status='active' WHERE guild_id=42")
        self.rejected_sql(
            "UPDATE discord_guilds SET ai_features=true WHERE guild_id=42"
        )
        self.rejected_sql(
            "INSERT INTO streamer_guilds (twitch_user_id,guild_id,active) VALUES ('990000000001',42,true)"
        )
        self.rejected_sql(
            f"INSERT INTO streamer_guilds (twitch_user_id,guild_id) VALUES ('unknown',{DE})"
        )
        self.rejected_sql(
            f"INSERT INTO streamer_guilds (twitch_user_id,guild_id) VALUES ('990000099999',{DE})"
        )
        self.rejected_sql(
            "INSERT INTO streamer_guilds (twitch_user_id,guild_id) VALUES ('990000000001',99)"
        )
        self.sql(
            "UPDATE twitch_streamer_identities SET twitch_login='renamed_fixture' WHERE twitch_user_id='990000000001'"
        )
        self.assertEqual(
            self.sql(
                "SELECT count(*) FROM streamer_guilds WHERE twitch_user_id='990000000001'"
            ),
            "1",
        )
        self.rejected_sql(
            "DELETE FROM twitch_streamer_identities WHERE twitch_user_id='990000000001'"
        )

    def test_unsafe_legacy_foreign_membership_rolls_back(self):
        self.seed()
        self.legacy_schema(1)
        self.sql(
            "INSERT INTO discord_guilds (guild_id) VALUES (42); INSERT INTO streamer_guilds (twitch_user_id,guild_id,source) VALUES ('990000000001',42,'onboarding')"
        )
        self.migration(GUILD, succeeds=False)
        self.assertEqual(
            self.sql("SELECT status FROM discord_guilds WHERE guild_id=42"), "active"
        )
        self.assertEqual(
            self.sql("SELECT to_regclass('public.partner_guild_active') IS NULL"), "t"
        )

    def test_language_backfill_is_latest_stable_and_non_overwriting(self):
        self.seed()
        self.sql(
            "INSERT INTO twitch_channel_updates (twitch_user_id,language,recorded_at) VALUES ('990000000001','DE','2026-10-01Z'),('990000000001',' EN-gb ','2026-10-02Z'),('990000000001','invalid_value','2026-10-03Z'); INSERT INTO twitch_stream_sessions (twitch_user_id,streamer_login,language,started_at) VALUES ('990000000001','schema_fixture_1','fr','2026-09-30Z')"
        )
        self.migration(GUILD)
        self.assertEqual(
            self.sql(
                "SELECT broadcaster_language FROM twitch_streamer_identities WHERE twitch_user_id='990000000001'"
            ),
            "en-gb",
        )
        self.sql(
            "UPDATE twitch_streamer_identities SET broadcaster_language='es' WHERE twitch_user_id='990000000001'"
        )
        self.migration(GUILD)
        self.assertEqual(
            self.sql(
                "SELECT broadcaster_language FROM twitch_streamer_identities WHERE twitch_user_id='990000000001'"
            ),
            "es",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
