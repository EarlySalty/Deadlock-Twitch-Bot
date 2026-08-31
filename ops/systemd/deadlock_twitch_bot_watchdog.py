#!/usr/bin/env python3
"""Überwacht den systemweiten Twitch-Bot und alarmiert bei langen Ausfällen.

Der Watchdog läuft bewusst außerhalb von ``deadlock-twitch-bot-rust.service``:
Ein beendeter Bot kann weder ins Journal schreiben noch eine Discord-DM senden.
Die Konfiguration kommt aus einer normalen Datei. Nur das Broker-Secret wird
vom systemd-Infisical-Wrapper als Umgebungsvariable an diesen Prozess gereicht.
"""

from __future__ import annotations

import configparser
import json
import logging
import os
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

LOGGER = logging.getLogger("deadlock-twitch-bot-watchdog")
EXPECTED_BROKER_URL = "http://127.0.0.1:8770/internal/master/v1/discord/send-message"
EXPECTED_SECRET_ENV_NAME = "TWITCH_INTERNAL_API_TOKEN"
EXPECTED_STATE_FILE = Path("/var/lib/deadlock-twitch-bot-watchdog/state.json")


@dataclass(frozen=True)
class Config:
    service: str
    warning_after_seconds: int
    dm_after_seconds: int
    dm_retry_seconds: int
    discord_user_id: int
    broker_url: str
    secret_env_name: str
    state_file: Path
    config_file: Path = Path("/etc/deadlock-twitch/bot-watchdog.conf")
    secret_loader: Path = Path("/usr/local/libexec/dl-infisical-env")


def load_config(path: Path) -> Config:
    parser = configparser.ConfigParser(interpolation=None)
    with path.open(encoding="utf-8") as handle:
        parser.read_file(handle)
    if "watchdog" not in parser:
        raise ValueError("Abschnitt [watchdog] fehlt")
    section = parser["watchdog"]

    def positive(name: str) -> int:
        try:
            value = int(section[name])
        except (KeyError, ValueError) as exc:
            raise ValueError(f"{name} muss eine positive Zahl sein") from exc
        if value <= 0:
            raise ValueError(f"{name} muss eine positive Zahl sein")
        return value

    service = section.get("service", "").strip()
    broker_url = section.get("broker_url", "").strip()
    secret_env_name = section.get("secret_env_name", "").strip()
    state_file = Path(section.get("state_file", "").strip())
    try:
        user_id = int(section["discord_user_id"])
    except (KeyError, ValueError) as exc:
        raise ValueError("discord_user_id muss eine positive Zahl sein") from exc
    if not service or not state_file.is_absolute():
        raise ValueError("service, broker_url, secret_env_name und ein absoluter state_file sind Pflicht")
    if user_id <= 0:
        raise ValueError("discord_user_id muss eine positive Zahl sein")
    if broker_url != EXPECTED_BROKER_URL:
        raise ValueError("broker_url muss auf den lokalen Discord-Broker zeigen")
    if secret_env_name != EXPECTED_SECRET_ENV_NAME:
        raise ValueError("secret_env_name muss TWITCH_INTERNAL_API_TOKEN sein")
    if state_file != EXPECTED_STATE_FILE:
        raise ValueError("state_file muss im geschützten Watchdog-State-Verzeichnis liegen")

    warning_after = positive("warning_after_seconds")
    dm_after = positive("dm_after_seconds")
    if dm_after <= warning_after:
        raise ValueError("dm_after_seconds muss größer als warning_after_seconds sein")
    return Config(
        service=service,
        warning_after_seconds=warning_after,
        dm_after_seconds=dm_after,
        dm_retry_seconds=positive("dm_retry_seconds"),
        discord_user_id=user_id,
        broker_url=broker_url,
        secret_env_name=secret_env_name,
        state_file=state_file,
        config_file=path,
        secret_loader=Path(section.get("secret_loader", "/usr/local/libexec/dl-infisical-env")).resolve(),
    )


def read_service_state(service: str) -> tuple[bool, str, str]:
    result = subprocess.run(
        ["systemctl", "show", service, "--property=ActiveState,SubState"],
        capture_output=True,
        text=True,
        check=False,
        timeout=10,
    )
    if result.returncode != 0:
        raise RuntimeError(f"systemctl show {service} beendet mit {result.returncode}")
    values: dict[str, str] = {}
    for line in result.stdout.splitlines():
        key, separator, value = line.partition("=")
        if separator:
            values[key] = value
    active = values.get("ActiveState", "")
    sub_state = values.get("SubState", "")
    if not active or not sub_state:
        raise RuntimeError(f"systemctl show {service} lieferte keinen vollständigen Zustand")
    return active == "active" and sub_state == "running", active, sub_state


def load_state(path: Path) -> dict[str, object]:
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return {}
    except (OSError, json.JSONDecodeError) as exc:
        LOGGER.warning("Watchdog-Zustand unlesbar; beginne einen neuen Ausfall: %s", exc)
        return {}
    if not isinstance(raw, dict):
        LOGGER.warning("Watchdog-Zustand ist kein Objekt; beginne einen neuen Ausfall")
        return {}
    return raw


def save_state(path: Path, state: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary_name = ""
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=path.parent, prefix=f".{path.name}.", delete=False
        ) as handle:
            temporary_name = handle.name
            json.dump(state, handle, ensure_ascii=False, sort_keys=True)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.chmod(temporary_name, 0o600)
        os.replace(temporary_name, path)
    finally:
        if temporary_name:
            try:
                Path(temporary_name).unlink(missing_ok=True)
            except OSError:
                pass


def format_time(timestamp: float) -> str:
    return datetime.fromtimestamp(timestamp).astimezone().isoformat(timespec="minutes")


def threshold_minutes(seconds: int) -> int:
    return max(1, (seconds + 59) // 60)


def send_dm(config: Config, down_since: float) -> bool:
    token = os.environ.get(config.secret_env_name, "").strip()
    if not token:
        LOGGER.error("Keine Broker-Authentifizierung verfügbar; Ausfall-DM konnte nicht gesendet werden")
        return False

    hostname = socket.gethostname()
    minutes = max(1, int((time.time() - down_since) // 60))
    content = (
        "⚠️ **Deadlock-Twitch-Bot-Warnung**\n"
        f"Der Twitch-Bot läuft auf `{hostname}` seit mindestens "
        f"{threshold_minutes(config.dm_after_seconds)} Minuten nicht "
        f"(bisher etwa {minutes} Minuten; erkannt um {format_time(down_since)}).\n"
        "Bitte den Dienst `deadlock-twitch-bot-rust.service` prüfen."
    )
    payload = json.dumps(
        {
            "user_id": config.discord_user_id,
            "content": content,
            "idempotency_key": f"deadlock-twitch-bot-down-{int(down_since)}",
        },
        ensure_ascii=False,
    ).encode("utf-8")
    request = urllib.request.Request(
        config.broker_url,
        data=payload,
        headers={"Content-Type": "application/json", "X-Internal-Token": token},
        method="POST",
    )
    try:
        class NoRedirectHandler(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, req, fp, code, msg, headers, new_url):
                return None

        opener = urllib.request.build_opener(NoRedirectHandler)
        with opener.open(request, timeout=15) as response:
            status = response.status
    except urllib.error.HTTPError as exc:
        LOGGER.error("Ausfall-DM vom Discord-Broker abgelehnt (HTTP %s)", exc.code)
        return False
    except (OSError, urllib.error.URLError) as exc:
        LOGGER.error("Ausfall-DM konnte den Discord-Broker nicht erreichen: %s", exc)
        return False
    if not 200 <= status < 300:
        LOGGER.error("Ausfall-DM vom Discord-Broker mit HTTP %s beendet", status)
        return False
    LOGGER.warning(
        "Ausfall-DM nach mindestens %s Minuten erfolgreich gesendet",
        threshold_minutes(config.dm_after_seconds),
    )
    return True


def send_dm_with_infisical(config: Config, down_since: float) -> bool:
    """Lädt Secrets nur beim fälligen DM-Versuch und reicht nur das Bot-Token weiter."""
    command = (
        'exec /usr/bin/env -i '
        'TWITCH_INTERNAL_API_TOKEN="$TWITCH_INTERNAL_API_TOKEN" '
        'HOME=/root /usr/bin/python3 "$1" --send-dm "$2" "$3"'
    )
    try:
        result = subprocess.run(
            [
                str(config.secret_loader),
                "--profile",
                "all",
                "--",
                "/bin/sh",
                "-c",
                command,
                "deadlock-twitch-bot-watchdog-dm",
                str(Path(__file__).resolve()),
                str(config.config_file),
                str(down_since),
            ],
            capture_output=True,
            text=True,
            check=False,
            timeout=45,
        )
    except (OSError, subprocess.SubprocessError) as exc:
        LOGGER.error("Infisical-Loader für Ausfall-DM konnte nicht gestartet werden: %s", exc)
        return False
    if result.returncode != 0:
        LOGGER.error("Infisical-Loader für Ausfall-DM beendet mit %s", result.returncode)
        return False
    return True


def check_once(
    config: Config,
    *,
    now: float | None = None,
    service_reader=read_service_state,
    dm_sender=send_dm_with_infisical,
) -> None:
    now = time.time() if now is None else now
    try:
        running, active, sub_state = service_reader(config.service)
    except (OSError, RuntimeError, subprocess.SubprocessError) as exc:
        LOGGER.error("Bot-Zustand konnte nicht geprüft werden: %s", exc)
        return

    state = load_state(config.state_file)
    down_since_raw = state.get("down_since")
    down_since = down_since_raw if isinstance(down_since_raw, (int, float)) else None

    if running:
        if down_since is not None:
            duration_minutes = max(1, int((now - down_since) // 60))
            LOGGER.info(
                "Bot wieder online: Ausfall dauerte etwa %s Minuten (active=%s substate=%s)",
                duration_minutes,
                active,
                sub_state,
            )
            save_state(config.state_file, {})
        return

    if down_since is None or down_since > now:
        down_since = now
        state = {"down_since": down_since, "warning_sent": False, "dm_sent": False}
        save_state(config.state_file, state)
        LOGGER.error(
            "Twitch-Bot läuft nicht (active=%s substate=%s); 15-Minuten-Warnung wird überwacht",
            active,
            sub_state,
        )
    elapsed = now - down_since

    if elapsed >= config.warning_after_seconds and not state.get("warning_sent", False):
        LOGGER.warning(
            "⚠️ Twitch-Bot seit mindestens %s Minuten ausgefallen (etwa %s Minuten; active=%s substate=%s)",
            threshold_minutes(config.warning_after_seconds),
            max(threshold_minutes(config.warning_after_seconds), int(elapsed // 60)),
            active,
            sub_state,
        )
        state["warning_sent"] = True

    dm_sent = state.get("dm_sent", False) is True
    last_attempt = state.get("last_dm_attempt")
    retry_due = not isinstance(last_attempt, (int, float)) or now - last_attempt >= config.dm_retry_seconds
    if elapsed >= config.dm_after_seconds and not dm_sent and retry_due:
        state["last_dm_attempt"] = now
        if dm_sender(config, down_since):
            state["dm_sent"] = True

    save_state(config.state_file, state)


def main(argv: list[str]) -> int:
    if len(argv) == 4 and argv[1] == "--send-dm":
        logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
        try:
            config = load_config(Path(argv[2]))
            return 0 if send_dm(config, float(argv[3])) else 1
        except (OSError, ValueError, configparser.Error) as exc:
            LOGGER.error("Ausfall-DM-Konfiguration ungültig: %s", exc)
            return 2
    if len(argv) != 2:
        print(f"Aufruf: {argv[0]} /etc/deadlock-twitch/bot-watchdog.conf", file=sys.stderr)
        return 2
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    try:
        config = load_config(Path(argv[1]))
        check_once(config)
    except (OSError, ValueError, configparser.Error) as exc:
        LOGGER.error("Watchdog-Konfiguration ungültig: %s", exc)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
