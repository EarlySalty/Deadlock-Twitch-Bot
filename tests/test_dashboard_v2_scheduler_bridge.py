import unittest
from types import SimpleNamespace

from bot.dashboard.server_v2 import DashboardV2Server
from bot.runtime.contracts import DashboardBotService


class DashboardV2SchedulerBridgeTests(unittest.TestCase):
    def test_dashboard_schedule_background_uses_resolved_scheduler(self) -> None:
        handler = DashboardV2Server.__new__(DashboardV2Server)
        scheduled: list[tuple[object, str]] = []

        def scheduler(coro, name: str):
            scheduled.append((coro, name))
            coro.close()
            return SimpleNamespace(name=name)

        handler._dashboard_bot_service_view = DashboardBotService(
            _schedule_background=scheduler,
        )

        async def followup():
            return "ok"

        coro = followup()
        result = DashboardV2Server._dashboard_schedule_background(
            handler,
            coro,
            "twitch.raid.complete_setup",
        )

        self.assertEqual(result.name, "twitch.raid.complete_setup")
        self.assertEqual(len(scheduled), 1)
        self.assertEqual(scheduled[0][1], "twitch.raid.complete_setup")
        self.assertIs(scheduled[0][0], coro)


if __name__ == "__main__":
    unittest.main()
