#!/home/naniadm/Documents/Deadlock-Twitch-Bot/.venv/bin/python
from __future__ import annotations

import argparse
import hashlib
import getpass
import json
import os
import secrets
import shlex
import socket
import string
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from urllib import error, parse, request

import psycopg
from psycopg import sql


SECRET_NAME = "TWITCH_ANALYTICS_DSN"
DEFAULT_SECRET_PATH = "/"
ROOT_DIR = Path(__file__).resolve().parents[1]
DEFAULT_AUDIT_LOG = ROOT_DIR / "logs/secret_rotation_audit.jsonl"
DEFAULT_GENERATED_PASSWORD_LENGTH = 48


def _redact_dsn(dsn: str) -> str:
    parsed = parse.urlsplit(dsn)
    if not parsed.scheme or not parsed.hostname:
        return "<invalid dsn>"

    username = parse.unquote(parsed.username or "")
    host = parsed.hostname or ""
    port = f":{parsed.port}" if parsed.port else ""
    path = parsed.path or ""
    query = f"?{parsed.query}" if parsed.query else ""
    auth = f"{username}:***@" if username else ""
    return f"{parsed.scheme}://{auth}{host}{port}{path}{query}"


def _host_for_url(parsed: parse.SplitResult) -> str:
    host = parsed.hostname or ""
    if ":" in host and not host.startswith("["):
        return f"[{host}]"
    return host


def _replace_dsn_password(dsn: str, new_password: str) -> str:
    parsed = parse.urlsplit(dsn)
    if not parsed.username:
        raise SystemExit("Current DSN is missing a database user")

    user = parse.quote(parse.unquote(parsed.username), safe="")
    password = parse.quote(new_password, safe="")
    host = _host_for_url(parsed)
    port = f":{parsed.port}" if parsed.port else ""
    netloc = f"{user}:{password}@{host}{port}"
    return parse.urlunsplit(
        (parsed.scheme, netloc, parsed.path, parsed.query, parsed.fragment)
    )


def _generate_password(length: int = DEFAULT_GENERATED_PASSWORD_LENGTH) -> str:
    if length < 32:
        raise SystemExit("Generated password length must be at least 32 characters")
    alphabet = string.ascii_letters + string.digits
    return "".join(secrets.choice(alphabet) for _ in range(length))


def _parse_dsn(dsn: str) -> dict[str, str]:
    parsed = parse.urlsplit(dsn)
    if parsed.scheme not in {"postgres", "postgresql"}:
        raise SystemExit("New DSN must start with postgres:// or postgresql://")
    if not parsed.hostname:
        raise SystemExit("New DSN is missing a host")
    if not parsed.username:
        raise SystemExit("New DSN is missing a database user")
    if parsed.password is None:
        raise SystemExit("New DSN is missing a password")
    if not parsed.path or parsed.path == "/":
        raise SystemExit("New DSN is missing a database name")
    return {
        "user": parse.unquote(parsed.username),
        "password": parse.unquote(parsed.password),
        "host": parsed.hostname,
        "port": str(parsed.port or 5432),
        "database": parsed.path.lstrip("/"),
    }


def _dsn_fingerprint(dsn: str) -> str:
    parsed = parse.urlsplit(dsn)
    material = "|".join(
        [
            parsed.scheme,
            parse.unquote(parsed.username or ""),
            parsed.hostname or "",
            str(parsed.port or 5432),
            parsed.path.lstrip("/"),
            parsed.query,
        ]
    )
    return hashlib.sha256(material.encode("utf-8")).hexdigest()[:16]


def _append_audit_event(
    path: Path,
    *,
    event: str,
    context: dict[str, str],
    current_dsn: str,
    new_dsn: str | None = None,
    status: str = "ok",
    detail: str | None = None,
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "ts": datetime.now(timezone.utc).isoformat(),
        "event": event,
        "status": status,
        "project_id": context["project_id"],
        "environment": context["environment"],
        "secret_path": context["secret_path"],
        "secret_name": SECRET_NAME,
        "current_dsn": _redact_dsn(current_dsn),
        "current_dsn_fingerprint": _dsn_fingerprint(current_dsn),
        "actor_uid": os.getuid(),
        "actor_user": getpass.getuser(),
    }
    if new_dsn is not None:
        payload["new_dsn"] = _redact_dsn(new_dsn)
        payload["new_dsn_fingerprint"] = _dsn_fingerprint(new_dsn)
    if detail:
        payload["detail"] = detail[:500]

    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(payload, sort_keys=True) + "\n")
    try:
        path.chmod(0o600)
    except OSError:
        pass


def _same_connection_target(left: dict[str, str], right: dict[str, str]) -> bool:
    keys = ("user", "host", "port", "database")
    return all(left[key] == right[key] for key in keys)


def _load_shell_env_file(path: Path) -> None:
    if not path.exists():
        raise SystemExit(f"Infisical config file not found: {path}")

    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("export "):
            line = line[len("export ") :].strip()
        if "=" not in line:
            continue

        key, value = line.split("=", 1)
        key = key.strip()
        if not key:
            continue

        value = value.strip()
        if value:
            try:
                parts = shlex.split(value, posix=True)
                value = parts[0] if parts else ""
            except ValueError:
                value = value.strip("'\"")
        os.environ.setdefault(key, value)


def _resolve_config_file(explicit: str | None) -> Path:
    candidates: list[Path] = []
    if explicit:
        candidates.append(Path(explicit).expanduser())
    if os.getenv("INFISICAL_CONFIG_FILE"):
        candidates.append(Path(os.environ["INFISICAL_CONFIG_FILE"]).expanduser())
    candidates.extend(
        [
            Path.home() / ".config/deadlock-twitch-bot/infisical.env",
            Path("/etc/deadlock-bots/twitch-bot.env"),
        ]
    )

    for candidate in candidates:
        if candidate.exists():
            return candidate
    return candidates[0]


def _required_env(name: str) -> str:
    value = (os.getenv(name) or "").strip()
    if not value:
        raise SystemExit(f"Missing required environment variable: {name}")
    return value


def _normalize_service_token(token: str) -> str:
    """Infisical API expects the access-token part for legacy service tokens."""
    token = token.strip()
    if token.startswith("st.") and token.count(".") >= 3:
        return ".".join(token.split(".")[:3])
    return token


def _service_token_candidates(token: str) -> list[str]:
    token = token.strip()
    candidates: list[str] = []
    normalized = _normalize_service_token(token)
    if normalized:
        candidates.append(normalized)
    if token and token not in candidates:
        candidates.append(token)
    return candidates


def _infisical_context(*, service_token_override: str | None = None) -> dict[str, str]:
    service_token = (
        service_token_override
        or os.getenv("ROTATION_INFISICAL_SERVICE_TOKEN")
        or _required_env("INFISICAL_SERVICE_TOKEN")
    )
    return {
        "base_url": _required_env("INFISICAL_API_URL").rstrip("/"),
        "project_id": _required_env("INFISICAL_PROJECT_ID"),
        "environment": _required_env("INFISICAL_ENV"),
        "service_token": _normalize_service_token(service_token),
        "secret_path": (os.getenv("INFISICAL_SECRET_PATH") or DEFAULT_SECRET_PATH).strip()
        or DEFAULT_SECRET_PATH,
        "timeout": os.getenv("INFISICAL_HTTP_TIMEOUT", "10"),
    }


def _context_with_service_token(context: dict[str, str], service_token: str) -> dict[str, str]:
    next_context = dict(context)
    next_context["service_token"] = service_token
    return next_context


def _infisical_request(
    method: str,
    path: str,
    *,
    context: dict[str, str],
    query: dict[str, str] | None = None,
    body: dict[str, Any] | None = None,
) -> dict[str, Any]:
    url = f"{context['base_url']}{path}"
    if query:
        url = f"{url}?{parse.urlencode(query)}"

    data = None
    headers = {
        "Authorization": f"Bearer {context['service_token']}",
        "Accept": "application/json",
        "User-Agent": "deadlock-twitch-bot-dsn-rotator/1.0",
    }
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        headers["Content-Type"] = "application/json"

    req = request.Request(url, data=data, headers=headers, method=method)
    try:
        with request.urlopen(req, timeout=float(context["timeout"])) as resp:
            raw = resp.read().decode("utf-8")
    except error.HTTPError as exc:
        response_body = exc.read().decode("utf-8", errors="replace")
        raise RuntimeError(
            f"Infisical request failed with HTTP {exc.code}: {response_body}"
        ) from exc
    except (error.URLError, ConnectionResetError, TimeoutError, socket.timeout, OSError) as exc:
        raise RuntimeError(f"Infisical request failed: {exc}") from exc

    return json.loads(raw) if raw.strip() else {}


def _fetch_current_dsn(context: dict[str, str]) -> str:
    env_map = _fetch_secret_map(context)
    current_dsn = env_map.get(SECRET_NAME)
    if current_dsn is None:
        raise SystemExit(f"Secret not found in Infisical: {SECRET_NAME}")
    return current_dsn


def _fetch_secret_map(context: dict[str, str]) -> dict[str, str]:
    payload = _infisical_request(
        "GET",
        "/api/v4/secrets/",
        context=context,
        query={
            "projectId": context["project_id"],
            "environment": context["environment"],
            "secretPath": context["secret_path"],
            "viewSecretValue": "true",
            "includeImports": "true",
            "recursive": "false",
        },
    )

    secrets = list(payload.get("secrets") or [])
    for imported in payload.get("imports") or []:
        secrets.extend(imported.get("secrets") or [])

    env_map: dict[str, str] = {}
    for item in secrets:
        key = str(item.get("secretKey") or "").strip()
        if not key:
            continue
        value = item.get("secretValue")
        env_map[key] = "" if value is None else str(value)
    return env_map


def _update_infisical_dsn(context: dict[str, str], new_dsn: str) -> None:
    _infisical_request(
        "PATCH",
        f"/api/v4/secrets/{parse.quote(SECRET_NAME, safe='')}",
        context=context,
        body={
            "projectId": context["project_id"],
            "environment": context["environment"],
            "secretPath": context["secret_path"],
            "secretValue": new_dsn,
        },
    )


def _preflight_infisical_write(context: dict[str, str], current_dsn: str) -> None:
    """Fail before touching Postgres if the token cannot edit secrets."""
    _update_infisical_dsn(context, current_dsn)


def _is_infisical_write_denied(exc: Exception) -> bool:
    message = str(exc).lower()
    return (
        "http 403" in message
        and ("permissiondenied" in message or "not allowed to edit" in message)
    )


def _prompt_for_write_context(context: dict[str, str]) -> dict[str, str]:
    print("The configured Infisical token is read-only.")
    token = getpass.getpass("Temporary Infisical write token: ").strip()
    if not token:
        raise SystemExit("No temporary Infisical write token provided.")
    next_context = dict(context)
    next_context["service_token"] = _normalize_service_token(token)
    next_context["_prompted_raw_service_token"] = token
    return next_context


def _ensure_infisical_write_context(
    context: dict[str, str],
    current_dsn: str,
) -> dict[str, str]:
    try:
        _preflight_infisical_write(context, current_dsn)
        return context
    except RuntimeError as exc:
        if not _is_infisical_write_denied(exc):
            raise
        prompted_context = _prompt_for_write_context(context)
        raw_token = str(prompted_context.pop("_prompted_raw_service_token", ""))
        last_error: RuntimeError | None = None
        for candidate in _service_token_candidates(raw_token):
            candidate_context = _context_with_service_token(prompted_context, candidate)
            try:
                _preflight_infisical_write(candidate_context, current_dsn)
                return candidate_context
            except RuntimeError as candidate_exc:
                last_error = candidate_exc
        if last_error is not None:
            raise last_error
        raise


def _run_preflight_only(
    *,
    current_dsn: str,
    context: dict[str, str],
    skip_function_test: bool,
) -> None:
    print(f"Infisical API: {context['base_url']}")
    print(f"Infisical project: {context['project_id']}")
    print(f"Infisical environment: {context['environment']}")
    print(f"Infisical secret path: {context['secret_path']}")
    print(f"Current DSN: {_redact_dsn(current_dsn)}")

    print("Checking current PostgreSQL DSN...")
    _verify_connection(current_dsn)

    print("Checking Infisical write permission...")
    context = _ensure_infisical_write_context(context, current_dsn)

    if not skip_function_test:
        _run_function_test(current_dsn, context)

    print("Preflight OK.")


def _alter_role_password(connect_dsn: str, role_name: str, new_password: str) -> None:
    with psycopg.connect(connect_dsn, autocommit=True) as conn:
        with conn.cursor() as cur:
            cur.execute(
                sql.SQL("ALTER ROLE {} WITH PASSWORD {}").format(
                    sql.Identifier(role_name),
                    sql.Literal(new_password),
                )
            )


def _verify_connection(dsn: str) -> None:
    with psycopg.connect(dsn, connect_timeout=10) as conn:
        with conn.cursor() as cur:
            cur.execute("SELECT 1")
            cur.fetchone()


def _restart_services() -> None:
    subprocess.run(
        ["/home/naniadm/Documents/manage-twitch-services.sh", "restart", "all"],
        cwd=str(ROOT_DIR),
        check=True,
    )


def _http_json(
    url: str,
    *,
    headers: dict[str, str] | None = None,
    timeout: float = 5,
) -> tuple[int, dict[str, Any]]:
    req = request.Request(url, headers=headers or {}, method="GET")
    with request.urlopen(req, timeout=timeout) as resp:
        raw = resp.read().decode("utf-8")
        payload = json.loads(raw) if raw.strip() else {}
        return int(getattr(resp, "status", 0) or 0), payload


def _wait_for_health(
    label: str,
    url: str,
    *,
    headers: dict[str, str] | None = None,
    timeout_seconds: int = 75,
) -> None:
    deadline = time.monotonic() + timeout_seconds
    last_error: str | None = None
    while time.monotonic() < deadline:
        try:
            status, payload = _http_json(url, headers=headers)
            if status == 200 and bool(payload.get("ok")):
                print(f"{label} health OK.")
                return
            last_error = f"status={status}, payload={payload}"
        except Exception as exc:
            last_error = str(exc)
        time.sleep(2)
    raise RuntimeError(f"{label} health check failed: {last_error}")


def _service_health_settings(env_map: dict[str, str]) -> dict[str, str]:
    return {
        "dashboard_host": env_map.get("TWITCH_DASHBOARD_HOST", "127.0.0.1").strip()
        or "127.0.0.1",
        "dashboard_port": env_map.get("TWITCH_DASHBOARD_PORT", "8765").strip() or "8765",
        "internal_host": env_map.get("TWITCH_INTERNAL_API_HOST", "127.0.0.1").strip()
        or "127.0.0.1",
        "internal_port": env_map.get("TWITCH_INTERNAL_API_PORT", "8776").strip() or "8776",
        "internal_token": env_map.get("TWITCH_INTERNAL_API_TOKEN", "").strip(),
    }


def _run_function_test(new_dsn: str, context: dict[str, str]) -> None:
    print("Running post-rotation function test...")
    _verify_connection(new_dsn)
    env_map = _fetch_secret_map(context)
    settings = _service_health_settings(env_map)

    internal_token = settings["internal_token"]
    if not internal_token:
        raise RuntimeError("Missing TWITCH_INTERNAL_API_TOKEN for internal API health check")

    internal_url = (
        f"http://{settings['internal_host']}:{settings['internal_port']}"
        "/internal/twitch/v1/healthz"
    )
    dashboard_url = (
        f"http://{settings['dashboard_host']}:{settings['dashboard_port']}/healthz"
    )
    _wait_for_health(
        "Twitch internal API",
        internal_url,
        headers={"X-Internal-Token": internal_token},
    )
    _wait_for_health("Twitch dashboard", dashboard_url)


def _rollback_rotation(
    *,
    current_dsn: str,
    current_password: str,
    new_dsn: str,
    role_name: str,
    context: dict[str, str],
    restart: bool,
) -> None:
    print("Rolling DB password and Infisical secret back...", file=sys.stderr)
    _alter_role_password(new_dsn, role_name, current_password)
    _update_infisical_dsn(context, current_dsn)
    if restart:
        _restart_services()


def _confirm(args: argparse.Namespace, current_dsn: str, new_dsn: str) -> None:
    if args.yes:
        return

    print("About to rotate Twitch PostgreSQL credentials:")
    print(f"  current Infisical DSN: {_redact_dsn(current_dsn)}")
    print(f"  new Infisical DSN:     {_redact_dsn(new_dsn)}")
    print(f"  restart services:      {'no' if args.no_restart else 'yes'}")
    print(f"  function test:         {'no' if args.skip_function_test else 'yes'}")
    answer = input("Type 'rotate' to continue: ").strip().lower()
    if answer != "rotate":
        raise SystemExit("Cancelled.")


def _self_test() -> None:
    current = "postgresql://postgres:old%23pass@localhost:5433/twitch_analytics?sslmode=disable"
    new_dsn = _replace_dsn_password(current, "new#pass")
    assert (
        new_dsn
        == "postgresql://postgres:new%23pass@localhost:5433/twitch_analytics?sslmode=disable"
    )
    assert _parse_dsn(new_dsn)["password"] == "new#pass"
    assert "new#pass" not in _redact_dsn(new_dsn)
    assert _same_connection_target(_parse_dsn(current), _parse_dsn(new_dsn))


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Rotate the PostgreSQL password behind TWITCH_ANALYTICS_DSN and "
            "save the new DSN back to Infisical."
        )
    )
    parser.add_argument(
        "--current-dsn",
        help=(
            "Current working postgresql:// DSN. If omitted, the script reads "
            "TWITCH_ANALYTICS_DSN from Infisical."
        ),
    )
    parser.add_argument(
        "--new-password",
        help=(
            "New PostgreSQL password. If omitted and --new-dsn is not set, "
            "prompt securely."
        ),
    )
    parser.add_argument(
        "--generate-password",
        action="store_true",
        help="Generate a new PostgreSQL password instead of prompting",
    )
    parser.add_argument(
        "--password-length",
        type=int,
        default=DEFAULT_GENERATED_PASSWORD_LENGTH,
        help="Length for --generate-password, minimum 32",
    )
    parser.add_argument(
        "--new-dsn",
        help=(
            "Full new postgresql:// DSN. Normally not needed; use --new-password."
        ),
    )
    parser.add_argument("--config-file", help="Infisical env config file")
    parser.add_argument(
        "--prompt-write-token",
        action="store_true",
        help="Prompt securely for a temporary Infisical read/write service token",
    )
    parser.add_argument(
        "--audit-log",
        default=str(DEFAULT_AUDIT_LOG),
        help="Secret-free JSONL audit log for rotation metadata",
    )
    parser.add_argument(
        "--restart",
        action="store_true",
        help="Restart Twitch worker and dashboard. This is now the default.",
    )
    parser.add_argument(
        "--no-restart",
        action="store_true",
        help="Do not restart Twitch worker and dashboard after rotation",
    )
    parser.add_argument(
        "--skip-function-test",
        action="store_true",
        help="Skip DB and service health checks after rotation",
    )
    parser.add_argument(
        "--preflight-only",
        action="store_true",
        help="Check current DB, Infisical write permission, and health checks without rotating",
    )
    parser.add_argument("--yes", action="store_true", help="Skip confirmation prompt")
    parser.add_argument(
        "--no-rollback",
        action="store_true",
        help="Do not restore the old DB password if the Infisical update fails",
    )
    parser.add_argument("--self-test", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()

    if args.self_test:
        _self_test()
        print("Self-test OK.")
        return 0

    config_file = _resolve_config_file(args.config_file)
    _load_shell_env_file(config_file)
    service_token_override = None
    if args.prompt_write_token:
        service_token_override = getpass.getpass("Temporary Infisical write token: ").strip()
        if not service_token_override:
            raise SystemExit("No temporary Infisical write token provided.")
    context = _infisical_context(service_token_override=service_token_override)
    audit_log = Path(args.audit_log).expanduser()

    current_dsn = (
        args.current_dsn or os.getenv("CURRENT_TWITCH_ANALYTICS_DSN") or ""
    ).strip()
    if not current_dsn:
        current_dsn = _fetch_current_dsn(context)
    current_info = _parse_dsn(current_dsn)

    if args.preflight_only:
        try:
            _run_preflight_only(
                current_dsn=current_dsn,
                context=context,
                skip_function_test=args.skip_function_test,
            )
        except RuntimeError as exc:
            raise SystemExit(f"Preflight failed: {exc}") from exc
        return 0

    new_password = (
        args.new_password or os.getenv("NEW_TWITCH_ANALYTICS_PASSWORD") or ""
    ).strip()
    new_dsn = (args.new_dsn or os.getenv("NEW_TWITCH_ANALYTICS_DSN") or "").strip()
    if new_dsn and (new_password or args.generate_password):
        raise SystemExit("Use either --new-dsn, --new-password, or --generate-password.")
    if args.generate_password and new_password:
        raise SystemExit("Use either --new-password or --generate-password, not both.")
    if not new_dsn:
        if args.generate_password:
            new_password = _generate_password(args.password_length)
            print("Generated new PostgreSQL password.")
        elif not new_password:
            new_password = getpass.getpass("New PostgreSQL password: ").strip()
        if not new_password:
            raise SystemExit("No new password provided.")
        new_dsn = _replace_dsn_password(current_dsn, new_password)

    new_info = _parse_dsn(new_dsn)
    if not _same_connection_target(current_info, new_info):
        raise SystemExit(
            "Refusing to change DSN target. This script only rotates the password "
            "for the same user/host/port/database."
        )

    _confirm(args, current_dsn, new_dsn)
    _append_audit_event(
        audit_log,
        event="rotation_confirmed",
        context=context,
        current_dsn=current_dsn,
        new_dsn=new_dsn,
    )

    print("Checking Infisical write permission...")
    try:
        context = _ensure_infisical_write_context(context, current_dsn)
    except RuntimeError as exc:
        _append_audit_event(
            audit_log,
            event="infisical_write_preflight",
            context=context,
            current_dsn=current_dsn,
            new_dsn=new_dsn,
            status="failed",
            detail=str(exc),
        )
        raise SystemExit(
            "Infisical write preflight failed before changing PostgreSQL. "
            "Use an Infisical service token with edit permission for secrets, "
            f"then run this script again. Details: {exc}"
        ) from exc
    _append_audit_event(
        audit_log,
        event="infisical_write_preflight",
        context=context,
        current_dsn=current_dsn,
        new_dsn=new_dsn,
    )

    print("Rotating PostgreSQL role password...")
    _alter_role_password(current_dsn, current_info["user"], new_info["password"])
    _append_audit_event(
        audit_log,
        event="postgres_password_rotated",
        context=context,
        current_dsn=current_dsn,
        new_dsn=new_dsn,
    )

    try:
        print("Verifying new PostgreSQL DSN...")
        _verify_connection(new_dsn)

        print("Updating Infisical secret TWITCH_ANALYTICS_DSN...")
        _update_infisical_dsn(context, new_dsn)
        _append_audit_event(
            audit_log,
            event="infisical_secret_updated",
            context=context,
            current_dsn=current_dsn,
            new_dsn=new_dsn,
        )
    except Exception:
        if args.no_rollback:
            raise
        print("Infisical update or verification failed; rolling DB password back...", file=sys.stderr)
        _alter_role_password(new_dsn, current_info["user"], current_info["password"])
        _append_audit_event(
            audit_log,
            event="rotation_rollback_after_update_failure",
            context=context,
            current_dsn=current_dsn,
            new_dsn=new_dsn,
            status="rolled_back",
        )
        raise

    try:
        should_restart = not args.no_restart
        if should_restart:
            print("Restarting Twitch services...")
            _restart_services()

        if not args.skip_function_test:
            _run_function_test(new_dsn, context)
        _append_audit_event(
            audit_log,
            event="rotation_completed",
            context=context,
            current_dsn=current_dsn,
            new_dsn=new_dsn,
        )
    except Exception:
        if args.no_rollback:
            raise
        _rollback_rotation(
            current_dsn=current_dsn,
            current_password=current_info["password"],
            new_dsn=new_dsn,
            role_name=current_info["user"],
            context=context,
            restart=should_restart,
        )
        _append_audit_event(
            audit_log,
            event="rotation_rollback_after_health_failure",
            context=context,
            current_dsn=current_dsn,
            new_dsn=new_dsn,
            status="rolled_back",
        )
        raise

    print("Rotation complete.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
