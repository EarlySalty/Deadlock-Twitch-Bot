from __future__ import annotations

import logging
import logging.handlers
import os
import sys
from pathlib import Path

_PROJECT_ROOT = Path(__file__).resolve().parents[1]
_LOGS_DIR = _PROJECT_ROOT / "logs"
_TEST_LOGS_SUBDIR = "test"
_TWITCH_LOGGER_NAME = "TwitchStreams"
_DEFAULT_TWITCH_LOG_FILENAME = "twitch_bot.log"
_DASHBOARD_TWITCH_LOG_FILENAME = "twitch_dashboard.log"
_MANAGED_TWITCH_LOG_FILENAMES = frozenset(
    {
        _DEFAULT_TWITCH_LOG_FILENAME,
        _DASHBOARD_TWITCH_LOG_FILENAME,
    }
)
_DEFAULT_LOG_FORMAT = "%(asctime)s - %(name)s - %(levelname)s - %(message)s"


def project_root() -> Path:
    return _PROJECT_ROOT


def _looks_like_test_runtime() -> bool:
    argv = [str(arg or "") for arg in sys.argv]
    argv_lower = [arg.strip().lower() for arg in argv]
    argv_normalized = [arg.replace("\\", "/").strip().lower() for arg in argv]
    entrypoint = Path(argv[0]).as_posix().lower() if argv else ""

    if any(name == "pytest" or name.startswith("pytest.") for name in sys.modules):
        return True
    if any("pytest" in arg for arg in argv_lower):
        return True
    if "/tests/" in entrypoint or entrypoint.endswith("/tests") or Path(entrypoint).name.startswith("test_"):
        return True
    if "unittest" in sys.modules:
        if "discover" in argv_lower:
            return True
        if any(
            arg == "tests"
            or arg.startswith("tests.")
            or "/tests/" in arg
            or arg.endswith("/tests")
            or Path(arg).name.startswith("test_")
            for arg in argv_normalized
        ):
            return True
    return False


def logs_dir() -> Path:
    target_dir = _LOGS_DIR / _TEST_LOGS_SUBDIR if _looks_like_test_runtime() else _LOGS_DIR
    target_dir.mkdir(parents=True, exist_ok=True)
    return target_dir


def log_path(filename: str) -> Path:
    normalized = Path(str(filename or "")).name
    if not normalized:
        raise ValueError("filename is required")
    return logs_dir() / normalized


def _same_file_handler_target(handler: logging.Handler, expected_path: Path) -> bool:
    if not isinstance(handler, logging.handlers.RotatingFileHandler):
        return False
    base_filename = getattr(handler, "baseFilename", "")
    if not base_filename:
        return False
    try:
        return Path(base_filename).resolve() == expected_path.resolve()
    except OSError:
        return str(base_filename) == str(expected_path)


def _handler_file_name(handler: logging.Handler) -> str:
    base_filename = getattr(handler, "baseFilename", "")
    if not base_filename:
        return ""
    return Path(str(base_filename)).name


def current_twitch_log_filename() -> str:
    explicit_value = Path(str(os.getenv("TWITCH_LOG_FILENAME") or "")).name
    if explicit_value:
        return explicit_value

    split_runtime_role = str(os.getenv("TWITCH_SPLIT_RUNTIME_ROLE") or "").strip().lower()
    if split_runtime_role == "dashboard":
        return _DASHBOARD_TWITCH_LOG_FILENAME
    return _DEFAULT_TWITCH_LOG_FILENAME


def ensure_twitch_logger_file_handler(*, level: int = logging.INFO) -> logging.Logger:
    logger = logging.getLogger(_TWITCH_LOGGER_NAME)
    if logger.level == logging.NOTSET or logger.level > level:
        logger.setLevel(level)

    target_filename = current_twitch_log_filename()
    file_path = log_path(target_filename)
    formatter = logging.Formatter(_DEFAULT_LOG_FORMAT)
    stale_handlers: list[logging.Handler] = []

    for handler in logger.handlers:
        handler_filename = _handler_file_name(handler)
        if handler_filename != target_filename:
            if handler_filename in _MANAGED_TWITCH_LOG_FILENAMES:
                stale_handlers.append(handler)
            continue
        if not _same_file_handler_target(handler, file_path):
            stale_handlers.append(handler)
            continue
        handler.setFormatter(formatter)
        if handler.level > level:
            handler.setLevel(level)
        return logger

    for handler in stale_handlers:
        logger.removeHandler(handler)
        handler.close()

    handler = logging.handlers.RotatingFileHandler(
        file_path,
        maxBytes=5 * 1024 * 1024,
        backupCount=5,
        encoding="utf-8",
    )
    handler.setFormatter(formatter)
    handler.setLevel(level)
    logger.addHandler(handler)
    return logger
