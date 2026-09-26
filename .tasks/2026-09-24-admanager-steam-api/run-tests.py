"""Run ad-manager tests on an isolated disposable database, without Steam tables."""
import os
import subprocess
import time
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2] / "rust"
NAME = "tb-admanager-test-" + uuid.uuid4().hex[:12]
CARGO = "/home/nathanael/.cargo/bin/cargo"

def docker(*args):
    return subprocess.check_output(["docker", *args], text=True, stderr=subprocess.DEVNULL).strip()

try:
    docker("run", "--rm", "-d", "--name", NAME, "-e", "POSTGRES_DB=tb_adm_test",
           "-e", "POSTGRES_HOST_AUTH_METHOD=trust", "-p", "127.0.0.1:0:5432",
           "timescale/timescaledb:2.17.2-pg16")
    port = docker("port", NAME, "5432/tcp").rsplit(":", 1)[1]
    assert port.isdigit()
    for _ in range(90):
        logs = subprocess.run(["docker", "logs", NAME], capture_output=True, text=True, check=True)
        if (logs.stdout + logs.stderr).count("database system is ready to accept connections") >= 2:
            break
        time.sleep(1)
    else:
        raise RuntimeError("Disposable database did not become ready")
    env = os.environ.copy()
    for name in ("DEADLOCK_CENTRAL_DSN", "TWITCH_ANALYTICS_DSN", "DATABASE_URL", "CENTRAL_TEST_DSN"):
        env.pop(name, None)
    env["TB_TEST_DATABASE_URL"] = f"postgresql://postgres@127.0.0.1:{port}/tb_adm_test"
    env["SQLX_OFFLINE"] = "true"
    env["PATH"] = "/home/nathanael/.cargo/bin:" + env.get("PATH", "")
    commands = [
        ["--lib", "ad_manager"],
        ["--test", "ad_manager_decision", "--test", "ad_manager_safety", "--test", "ad_manager_store"],
    ]
    print("Disposable Twitch test database ready; no activity schema created.", flush=True)
    results = []
    for args in commands:
        result = subprocess.run([CARGO, "+1.98.0", "test", "--locked", "-j", "2", "-p", "tb-analytics", *args], cwd=ROOT, env=env)
        results.append(result.returncode)
    raise SystemExit(next((code for code in results if code), 0))
finally:
    subprocess.run(["docker", "rm", "-f", NAME], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
