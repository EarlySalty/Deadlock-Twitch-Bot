import logging
import logging.handlers
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from bot import logging_setup


class LoggingSetupTests(unittest.TestCase):
    def test_looks_like_test_runtime_detects_unittest_module_target(self) -> None:
        with patch.object(
            logging_setup.sys,
            "argv",
            ["C:/Python311/Lib/unittest/__main__.py", "tests.test_logging_setup"],
        ):
            with patch.dict(logging_setup.sys.modules, {"unittest": object()}, clear=False):
                self.assertTrue(logging_setup._looks_like_test_runtime())

    def test_log_path_uses_project_logs_dir_and_strips_nested_input(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            with patch.object(logging_setup, "_LOGS_DIR", Path(tmpdir)):
                with patch.object(logging_setup, "_looks_like_test_runtime", return_value=False):
                    resolved = logging_setup.log_path("nested/twitch_autobans.log")

        self.assertEqual(resolved, Path(tmpdir) / "twitch_autobans.log")

    def test_log_path_uses_separate_test_logs_dir_during_test_runtime(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            with patch.object(logging_setup, "_LOGS_DIR", Path(tmpdir)):
                with patch.object(logging_setup, "_looks_like_test_runtime", return_value=True):
                    resolved = logging_setup.log_path("nested/twitch_autobans.log")

        self.assertEqual(resolved, Path(tmpdir) / "test" / "twitch_autobans.log")

    def test_ensure_twitch_logger_file_handler_is_idempotent(self) -> None:
        logger_name = "TwitchStreams.TestLoggingSetup"
        logger = logging.getLogger(logger_name)
        logger.handlers.clear()
        logger.setLevel(logging.NOTSET)

        with tempfile.TemporaryDirectory() as tmpdir:
            legacy_dir = Path(tmpdir) / "legacy"
            legacy_dir.mkdir()
            legacy_handler = logging.handlers.RotatingFileHandler(
                legacy_dir / "twitch_bot.log",
                maxBytes=1024,
                backupCount=1,
                encoding="utf-8",
            )
            logger.addHandler(legacy_handler)

            with patch.object(logging_setup, "_LOGS_DIR", Path(tmpdir)):
                with patch.object(logging_setup, "_TWITCH_LOGGER_NAME", logger_name):
                    with patch.object(logging_setup, "_looks_like_test_runtime", return_value=False):
                        logging_setup.ensure_twitch_logger_file_handler()
                        logging_setup.ensure_twitch_logger_file_handler()

                    handlers = [
                        handler
                        for handler in logger.handlers
                        if isinstance(handler, logging.handlers.RotatingFileHandler)
                    ]

                    self.assertEqual(len(handlers), 1)
                    self.assertEqual(
                        Path(handlers[0].baseFilename),
                        Path(tmpdir) / "twitch_bot.log",
                    )
                    self.assertEqual(logger.level, logging.INFO)

            for handler in list(logger.handlers):
                logger.removeHandler(handler)
                handler.close()
        logger.setLevel(logging.NOTSET)

    def test_dashboard_role_uses_dashboard_log_and_replaces_bot_handler(self) -> None:
        logger_name = "TwitchStreams.TestDashboardLoggingSetup"
        logger = logging.getLogger(logger_name)
        logger.handlers.clear()
        logger.setLevel(logging.NOTSET)

        with tempfile.TemporaryDirectory() as tmpdir:
            legacy_handler = logging.handlers.RotatingFileHandler(
                Path(tmpdir) / "twitch_bot.log",
                maxBytes=1024,
                backupCount=1,
                encoding="utf-8",
            )
            logger.addHandler(legacy_handler)

            with patch.object(logging_setup, "_LOGS_DIR", Path(tmpdir)):
                with patch.object(logging_setup, "_TWITCH_LOGGER_NAME", logger_name):
                    with patch.object(logging_setup, "_looks_like_test_runtime", return_value=False):
                        with patch.dict(os.environ, {"TWITCH_SPLIT_RUNTIME_ROLE": "dashboard"}, clear=False):
                            logging_setup.ensure_twitch_logger_file_handler()

                    handlers = [
                        handler
                        for handler in logger.handlers
                        if isinstance(handler, logging.handlers.RotatingFileHandler)
                    ]

                    self.assertEqual(len(handlers), 1)
                    self.assertEqual(
                        Path(handlers[0].baseFilename),
                        Path(tmpdir) / "twitch_dashboard.log",
                    )

            for handler in list(logger.handlers):
                logger.removeHandler(handler)
                handler.close()
        logger.setLevel(logging.NOTSET)

    def test_importing_bot_package_does_not_attach_file_handler_by_default(self) -> None:
        result = subprocess.run(
            [
                sys.executable,
                "-c",
                (
                    "import logging, logging.handlers, bot; "
                    "logger = logging.getLogger('TwitchStreams'); "
                    "print(sum(isinstance(h, logging.handlers.RotatingFileHandler) for h in logger.handlers))"
                ),
            ],
            cwd=Path(__file__).resolve().parents[1],
            check=True,
            capture_output=True,
            text=True,
        )

        self.assertEqual(result.stdout.strip(), "0")


if __name__ == "__main__":
    unittest.main()
