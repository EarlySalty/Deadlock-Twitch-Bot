import os
import shlex
import subprocess
from urllib.parse import quote, unquote, urlparse, urlunparse

INFISICAL = "/home/nathanael/Documents/Infisical"
LOADER = f"{INFISICAL}/export_claude_secret.py"
UPDATER = f"{INFISICAL}/update_gpt_secret.py"


def load_secret(name):
    result = subprocess.run([LOADER, "--secret", name], capture_output=True, text=True)
    if result.returncode != 0:
        raise SystemExit(f"loading {name} failed")
    tokens = shlex.split(result.stdout.strip())
    if len(tokens) != 2 or not tokens[1].startswith(name + "="):
        raise SystemExit(f"unexpected loader output for {name}")
    return tokens[1][len(name) + 1:]


def write_secret(name, value, create=False):
    args = ["python3", "-I", UPDATER, "--secret", name]
    if create:
        args.append("--anlegen")
    result = subprocess.run(args, input=value, capture_output=True, text=True)
    if result.returncode != 0:
        raise SystemExit(f"writing {name} failed with exit {result.returncode}")
    if load_secret(name) != value:
        raise SystemExit(f"read-back of {name} differs")


def password_of(dsn):
    return unquote(urlparse(dsn).password or "")


def with_password(dsn, password):
    parts = urlparse(dsn)
    host = parts.hostname or ""
    netloc = f"{quote(unquote(parts.username or ''), safe='')}:{quote(password, safe='')}@{host}"
    if parts.port:
        netloc += f":{parts.port}"
    return urlunparse(parts._replace(netloc=netloc))


def tcp_login_ok(dsn):
    parts = urlparse(dsn)
    env = {
        "PATH": "/usr/bin:/bin",
        "PGHOST": parts.hostname or "127.0.0.1",
        "PGPORT": str(parts.port or 5432),
        "PGUSER": unquote(parts.username or ""),
        "PGDATABASE": parts.path.lstrip("/"),
        "PGPASSWORD": password_of(dsn),
        "PGCONNECT_TIMEOUT": "10",
    }
    result = subprocess.run(
        ["psql", "-X", "-At", "--no-psqlrc", "-c", "select current_user"],
        env=env,
        capture_output=True,
        text=True,
    )
    return result.returncode == 0 and result.stdout.strip() == env["PGUSER"]


def alter_role_verifier(role, verifier):
    if not role.isidentifier():
        raise SystemExit("bad role name")
    if "'" in verifier or not verifier.startswith("SCRAM-SHA-256$"):
        raise SystemExit("bad verifier")
    sql = f"ALTER ROLE {role} PASSWORD '{verifier}';\n"
    result = subprocess.run(
        ["sudo", "-n", "-u", "postgres", "psql", "-X", "-q", "-v", "ON_ERROR_STOP=1", "-d", "postgres"],
        input=sql,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise SystemExit(f"ALTER ROLE failed with exit {result.returncode}")
