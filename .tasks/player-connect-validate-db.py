"""Run migrations and role assertions exclusively in a private temporary PostgreSQL cluster."""
import os
import subprocess
import tempfile
import time
from pathlib import Path

root = Path(__file__).resolve().parents[1]
env = os.environ.copy()
env.update(PATH='/opt/deadlock/twitch/toolchains/stable/bin:' + env['PATH'], SQLX_OFFLINE='true',
           CARGO_TARGET_DIR='/home/nathanael/repos/twitch-rank-friend-state/rust/target')
with tempfile.TemporaryDirectory(prefix='player-connect-private-pg-') as directory:
    data = Path(directory) / 'data'
    subprocess.run(['/usr/lib/postgresql/16/bin/initdb', '-A', 'trust', '-U', 'postgres', '--no-locale', '-D', str(data)],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    with open(root / '.tasks/player-connect-private-pg.log', 'w') as pglog:
        process = subprocess.Popen(['/usr/lib/postgresql/16/bin/postgres', '-D', str(data), '-h', '', '-k', directory,
                                    '-c', 'shared_preload_libraries=timescaledb', '-c', 'unix_socket_permissions=0700',
                                    '-c', 'shared_buffers=32MB', '-c', 'max_connections=30', '-c', 'fsync=off'], stdout=pglog, stderr=pglog)
        try:
            base = ['psql', '-X', '-h', directory, '-U', 'postgres', '-v', 'ON_ERROR_STOP=1']
            for _ in range(100):
                if subprocess.run(base + ['-d', 'postgres', '-Atc', 'SELECT 1'], capture_output=True).returncode == 0:
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError('Private database did not start')
            subprocess.run(base + ['-d', 'postgres', '-c', 'CREATE DATABASE twitch_analytics'], check=True, capture_output=True)
            env['TB_TEST_DATABASE_URL'] = f'postgresql:///twitch_analytics?host={directory}&user=postgres'
            env['TB_TEST_REQUIRE_DB'] = '1'
            with open(root / '.tasks/player-connect-schema-tests.log', 'w') as log:
                result = subprocess.run(['cargo', 'test', '-p', 'tb-db', '--test', 'fresh_migrations_schema', '-j', '2', '--',
                                         'fresh_migrations_match_committed_schema_snapshot', '--exact'],
                                        cwd=root / 'rust', env=env, stdout=log, stderr=subprocess.STDOUT, timeout=100)
            print('SCHEMA_TEST_EXIT', result.returncode)
            print((root / '.tasks/player-connect-schema-tests.log').read_text()[-6000:])
            if result.returncode:
                raise SystemExit(result.returncode)
            result = subprocess.run(base + ['-d', 'twitch_analytics', '-f', str(root / 'ops/systemd/twitch-runtime-roles.sql')],
                                    text=True, capture_output=True, timeout=20)
            if result.returncode:
                print(result.stderr[-3000:])
                raise SystemExit(result.returncode)
            sql = """
                SET ROLE twitchdash;
                INSERT INTO twitch_player_steam_links(twitch_user_id,steam_id64) VALUES('111',76561197960265770);
                RESET ROLE;
                SET ROLE twitchbot;
                INSERT INTO twitch_player_steam_links(twitch_user_id,lookup_enabled,revision) VALUES('111',false,1)
                  ON CONFLICT(twitch_user_id) DO UPDATE SET lookup_enabled=false,revision=twitch_player_steam_links.revision+1,updated_at=now();
                SELECT steam_id64 IS NULL AND NOT lookup_enabled FROM twitch_player_steam_links WHERE twitch_user_id='111';
                RESET ROLE;
                SELECT NOT has_column_privilege('twitchbot','twitch_player_steam_links','steam_id64','INSERT')
                  AND NOT has_column_privilege('twitchbot','twitch_player_steam_links','steam_id64','UPDATE')
                  AND NOT has_table_privilege('twitchbot','twitch_steam_openid_nonces','SELECT')
                  AND NOT has_table_privilege('twitchlegacy','twitch_player_steam_links','SELECT');
            """
            result = subprocess.run(base + ['-d', 'twitch_analytics', '-Atc', sql], text=True, capture_output=True, timeout=15)
            assert result.returncode == 0, result.stderr
            assert result.stdout.splitlines().count('t') == 2, result.stdout
            print('ROLE_ASSERTIONS: PASS (web assigns, bot clears, bot cannot assign, nonce store private)')
        finally:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
