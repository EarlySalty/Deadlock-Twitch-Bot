import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("deadlock_twitch_bot_watchdog.py")
SPEC = importlib.util.spec_from_file_location("deadlock_twitch_bot_watchdog", MODULE_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class WatchdogTests(unittest.TestCase):
    def make_config(self, state_file: Path):
        return MODULE.Config(
            service="deadlock-twitch-bot-rust.service",
            warning_after_seconds=900,
            dm_after_seconds=3600,
            dm_retry_seconds=900,
            discord_user_id=1,
            broker_url="http://127.0.0.1:8770/internal/master/v1/discord/send-message",
            secret_env_name="TWITCH_INTERNAL_API_TOKEN",
            state_file=state_file,
        )

    def test_warns_at_15_minutes_and_sends_dm_after_one_hour(self):
        with tempfile.TemporaryDirectory() as directory:
            config = self.make_config(Path(directory) / "state.json")
            calls = []

            def reader(_service):
                return False, "inactive", "dead"

            def sender(_config, down_since):
                calls.append(down_since)
                return True

            MODULE.check_once(config, now=1000, service_reader=reader, dm_sender=sender)
            MODULE.check_once(config, now=1899, service_reader=reader, dm_sender=sender)
            state = MODULE.load_state(config.state_file)
            self.assertFalse(state["warning_sent"])
            self.assertEqual(calls, [])

            MODULE.check_once(config, now=1900, service_reader=reader, dm_sender=sender)
            state = MODULE.load_state(config.state_file)
            self.assertTrue(state["warning_sent"])
            self.assertEqual(calls, [])

            MODULE.check_once(config, now=4600, service_reader=reader, dm_sender=sender)
            state = MODULE.load_state(config.state_file)
            self.assertTrue(state["dm_sent"])
            self.assertEqual(calls, [1000])

            MODULE.check_once(config, now=5000, service_reader=reader, dm_sender=sender)
            self.assertEqual(calls, [1000])

    def test_online_resets_a_previous_incident(self):
        with tempfile.TemporaryDirectory() as directory:
            config = self.make_config(Path(directory) / "state.json")
            MODULE.check_once(
                config,
                now=1000,
                service_reader=lambda _service: (False, "failed", "failed"),
                dm_sender=lambda *_args: False,
            )
            MODULE.check_once(
                config,
                now=1100,
                service_reader=lambda _service: (True, "active", "running"),
                dm_sender=lambda *_args: False,
            )
            self.assertEqual(MODULE.load_state(config.state_file), {})


if __name__ == "__main__":
    unittest.main()
