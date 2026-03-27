from __future__ import annotations

import logging
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock

from bot.raid.raid_pipeline import (
    RaidPipelineDependencies,
    RaidPipelineRequest,
    RaidPipelineService,
    execute_raid_pipeline,
    is_retryable_raid_error,
)


class RaidPipelineTests(unittest.IsolatedAsyncioTestCase):
    def _make_deps(self, **overrides):
        logger = MagicMock(spec=logging.Logger)
        monotonic_value = {"current": 100.0}

        def _monotonic() -> float:
            monotonic_value["current"] += 1.0
            return monotonic_value["current"]

        deps = RaidPipelineDependencies(
            load_raid_blacklist=MagicMock(return_value=(set(), set())),
            add_to_blacklist=MagicMock(),
            select_partner_candidate_by_score=AsyncMock(),
            select_fairest_candidate=AsyncMock(),
            ensure_raid_arrival_subscription_ready=AsyncMock(return_value=True),
            start_raid=AsyncMock(return_value=(True, None)),
            register_pending_raid=AsyncMock(),
            mark_manual_raid_started=MagicMock(),
            logger=logger,
            next_raid_observability_flow_id=lambda prefix: f"{prefix}-1",
            increment_raid_observability_counter=MagicMock(return_value=1),
            log_raid_observability_event=MagicMock(),
            monotonic=_monotonic,
            to_thread=AsyncMock(side_effect=lambda func: func()),
        )
        for key, value in overrides.items():
            setattr(deps, key, value)
        return deps

    def _make_request(self, **overrides):
        request = RaidPipelineRequest(
            broadcaster_id="1001",
            broadcaster_login="source_login",
            viewer_count=7,
            stream_duration_sec=600,
            online_partners=[],
            session=object(),
            reason="auto_raid_on_offline",
        )
        for key, value in overrides.items():
            setattr(request, key, value)
        return request

    async def test_retryable_markers(self) -> None:
        self.assertTrue(is_retryable_raid_error("Raids are disabled for this channel"))
        self.assertTrue(is_retryable_raid_error("This channel does not allow you to raid"))
        self.assertFalse(is_retryable_raid_error("something else"))
        self.assertFalse(is_retryable_raid_error(None))

    async def test_unavailable_without_session(self) -> None:
        deps = self._make_deps()
        request = self._make_request(session=None)

        result = await RaidPipelineService(deps).execute(request)

        self.assertEqual(result, {"status": "unavailable", "error": "no_active_session"})
        deps.load_raid_blacklist.assert_not_called()

    async def test_blocks_when_blacklist_load_fails(self) -> None:
        deps = self._make_deps(load_raid_blacklist=MagicMock(side_effect=RuntimeError("db down")))
        request = self._make_request()

        result = await RaidPipelineService(deps).execute(request)

        self.assertEqual(result, {"status": "blocked", "error": "blacklist_unavailable"})
        deps.start_raid.assert_not_awaited()

    async def test_successful_partner_raid_registers_pending_and_manual_suppression(self) -> None:
        target = {
            "user_id": "2002",
            "user_login": "targetlogin",
            "started_at": "2026-03-10T18:00:00+00:00",
            "raid_enabled": True,
        }
        deps = self._make_deps(
            select_partner_candidate_by_score=AsyncMock(return_value=target),
            start_raid=AsyncMock(return_value=(True, None)),
        )
        request = self._make_request(
            online_partners=[target],
            set_manual_suppression=True,
        )

        result = await RaidPipelineService(deps).execute(request)

        self.assertEqual(result["status"], "started")
        self.assertEqual(result["target_login"], "targetlogin")
        self.assertTrue(result["is_partner_raid"])
        deps.ensure_raid_arrival_subscription_ready.assert_awaited_once()
        deps.start_raid.assert_awaited_once()
        deps.register_pending_raid.assert_awaited_once()
        deps.mark_manual_raid_started.assert_called_once_with(
            broadcaster_id="1001",
            ttl_seconds=180.0,
        )
        deps.increment_raid_observability_counter.assert_any_call("raid_flow_started_total", 1)

    async def test_falls_back_to_fairest_candidate_when_partner_selection_returns_none(self) -> None:
        partner = {
            "user_id": "2002",
            "user_login": "partner",
            "started_at": "2026-03-10T18:00:00+00:00",
            "raid_enabled": True,
        }
        fallback = {
            "user_id": "3003",
            "user_login": "fallback",
            "started_at": "2026-03-10T19:00:00+00:00",
            "viewer_count": 1,
        }
        api = SimpleNamespace(
            get_streams_by_category=AsyncMock(return_value=[fallback]),
        )
        deps = self._make_deps(
            select_partner_candidate_by_score=AsyncMock(return_value=None),
            select_fairest_candidate=AsyncMock(return_value=fallback),
            start_raid=AsyncMock(return_value=(True, None)),
        )
        request = self._make_request(
            online_partners=[partner],
            api=api,
            category_id="deadlock",
        )

        result = await RaidPipelineService(deps).execute(request)

        self.assertEqual(result["status"], "started")
        self.assertEqual(result["target_login"], "fallback")
        self.assertFalse(result["is_partner_raid"])
        register_kwargs = deps.register_pending_raid.await_args.kwargs
        self.assertFalse(register_kwargs["is_partner_raid"])
        api.get_streams_by_category.assert_awaited_once_with(
            "deadlock",
            language="de",
            limit=50,
        )

    async def test_invalid_selected_target_identity_is_rejected_before_start(self) -> None:
        invalid_target = {
            "user_id": "",
            "user_login": "target_without_id",
            "started_at": "2026-03-10T19:00:00+00:00",
        }
        deps = self._make_deps(
            select_fairest_candidate=AsyncMock(return_value=invalid_target),
        )
        request = self._make_request(
            online_partners=[],
            api=SimpleNamespace(get_streams_by_category=AsyncMock(return_value=[invalid_target])),
            category_id="deadlock",
        )

        result = await RaidPipelineService(deps).execute(request)

        self.assertEqual(
            result,
            {"status": "no_target", "error": "invalid_target_identity"},
        )
        deps.start_raid.assert_not_awaited()
        deps.register_pending_raid.assert_not_awaited()

    async def test_retryable_non_partner_failure_blacklists_and_retries(self) -> None:
        first = {
            "user_id": "2002",
            "user_login": "first",
            "started_at": "2026-03-10T18:00:00+00:00",
        }
        second = {
            "user_id": "3003",
            "user_login": "second",
            "started_at": "2026-03-10T19:00:00+00:00",
        }
        start_calls: list[str] = []

        async def _start_raid(**kwargs):
            start_calls.append(kwargs["to_broadcaster_login"])
            if kwargs["to_broadcaster_login"] == "first":
                return False, "Raids are disabled"
            return True, None

        deps = self._make_deps(
            select_fairest_candidate=AsyncMock(side_effect=[first, second]),
            start_raid=AsyncMock(side_effect=_start_raid),
        )
        request = self._make_request(
            online_partners=[],
            api=SimpleNamespace(get_streams_by_category=AsyncMock(return_value=[first, second])),
            category_id="deadlock",
        )

        result = await RaidPipelineService(deps).execute(request)

        self.assertEqual(result["status"], "started")
        self.assertEqual(start_calls, ["first", "second"])
        deps.add_to_blacklist.assert_called_once_with("2002", "first", "Raids are disabled")
        deps.register_pending_raid.assert_awaited_once()

    async def test_retryable_partner_failure_does_not_blacklist(self) -> None:
        partner = {
            "user_id": "2002",
            "user_login": "partner",
            "started_at": "2026-03-10T18:00:00+00:00",
            "raid_enabled": True,
        }
        fallback = {
            "user_id": "3003",
            "user_login": "fallback",
            "started_at": "2026-03-10T19:00:00+00:00",
        }
        deps = self._make_deps(
            select_partner_candidate_by_score=AsyncMock(side_effect=[partner, None]),
            select_fairest_candidate=AsyncMock(return_value=fallback),
            start_raid=AsyncMock(side_effect=[(False, "does not allow you to raid"), (True, None)]),
        )
        request = self._make_request(
            online_partners=[partner],
            api=SimpleNamespace(get_streams_by_category=AsyncMock(return_value=[fallback])),
            category_id="deadlock",
        )

        result = await RaidPipelineService(deps).execute(request)

        self.assertEqual(result["status"], "started")
        deps.add_to_blacklist.assert_not_called()

    async def test_non_retryable_failure_stops_pipeline(self) -> None:
        target = {
            "user_id": "2002",
            "user_login": "target",
            "started_at": "2026-03-10T18:00:00+00:00",
            "raid_enabled": True,
        }
        deps = self._make_deps(
            select_partner_candidate_by_score=AsyncMock(return_value=target),
            start_raid=AsyncMock(return_value=(False, "unexpected 500")),
        )
        request = self._make_request(online_partners=[target])

        result = await RaidPipelineService(deps).execute(request)

        self.assertEqual(result, {"status": "raid_failed", "error": "unexpected 500"})
        deps.add_to_blacklist.assert_not_called()
        deps.register_pending_raid.assert_not_awaited()

    async def test_no_target_after_three_attempts(self) -> None:
        deps = self._make_deps(
            select_partner_candidate_by_score=AsyncMock(return_value=None),
            select_fairest_candidate=AsyncMock(return_value=None),
        )
        request = self._make_request(online_partners=[], api=None, category_id=None)

        result = await RaidPipelineService(deps).execute(request)

        self.assertEqual(result, {"status": "no_target"})
        deps.start_raid.assert_not_awaited()

    async def test_execute_raid_pipeline_helper(self) -> None:
        target = {
            "user_id": "2002",
            "user_login": "target",
            "started_at": "2026-03-10T18:00:00+00:00",
            "raid_enabled": True,
        }
        deps = self._make_deps(
            select_partner_candidate_by_score=AsyncMock(return_value=target),
            start_raid=AsyncMock(return_value=(True, None)),
        )
        request = self._make_request(online_partners=[target])

        result = await execute_raid_pipeline(deps, request)

        self.assertEqual(result["status"], "started")
