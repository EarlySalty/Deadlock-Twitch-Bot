from __future__ import annotations

import json
import unittest

from aiohttp import web

from bot.internal_api.routes import streamers as streamer_routes


class _FakeRequest:
    def __init__(
        self,
        *,
        query: dict[str, object] | None = None,
        match_info: dict[str, str] | None = None,
        body: dict[str, object] | None = None,
    ) -> None:
        self.query = query or {}
        self.match_info = match_info or {}
        self._body = dict(body or {})

    async def json(self) -> dict[str, object]:
        return dict(self._body)


class _StreamersHarness:
    def __init__(self) -> None:
        self.calls: list[tuple[str, object]] = []

    def _json_response(self, payload, status: int = 200):
        return web.json_response(payload, status=status)

    def _json_error(self, error: str, status: int, message: str):
        return web.json_response({"error": error, "message": message}, status=status)

    def _safe_bad_request(self, *, context: str, exc: Exception, message: str):
        del context, exc
        return self._json_error("bad_request", 400, message)

    def _safe_exception_error(
        self,
        *,
        context: str,
        exc: Exception,
        error: str,
        status: int,
        message: str,
    ):
        del context, exc
        return self._json_error(error, status, message)

    async def _json_body(self, request: _FakeRequest) -> dict[str, object]:
        return await request.json()

    def _prepare_idempotency(self, *, request, payload):
        del request
        self.calls.append(("idempotency", payload))
        return ("owner-key", "fingerprint", None, None, True)

    async def _wait_idempotency_result(self, *, future):
        raise AssertionError("wait path should not be used in this test")

    def _release_idempotency_owner(self, *, key, fingerprint, response, cacheable):
        self.calls.append(("release", key, fingerprint, cacheable, getattr(response, "status", None)))

    @staticmethod
    def _normalize_login(value: str):
        normalized = str(value or "").strip().lower()
        return normalized or None

    @staticmethod
    def _parse_bool(value, *, default: bool = False):
        if value is None:
            return default
        if isinstance(value, bool):
            return value
        text = str(value).strip().lower()
        if text in {"1", "true", "yes", "on"}:
            return True
        if text in {"0", "false", "no", "off"}:
            return False
        return default

    @staticmethod
    def _parse_optional_int(value, *, minimum: int | None = None):
        if value is None or value == "":
            return None
        parsed = int(value)
        if minimum is not None and parsed < minimum:
            raise ValueError("below minimum")
        return parsed

    async def _list(self):
        self.calls.append(("list", None))
        return ("alpha", "beta")

    async def _add(self, login: str, require_link: bool):
        self.calls.append(("add", login, require_link))
        return "added"

    async def _remove(self, login: str):
        self.calls.append(("remove", login))
        return "removed"

    async def _verify(self, login: str, mode: str):
        self.calls.append(("verify", login, mode))
        return "verified"

    async def _archive(self, login: str, mode: str):
        self.calls.append(("archive", login, mode))
        return "updated"

    def _enforce_discord_action_scope(self, payload: dict[str, object]) -> None:
        self.calls.append(("scope", dict(payload)))

    async def _discord_flag(self, login: str, enabled: bool):
        self.calls.append(("discord_flag", login, enabled))
        return "discord-flag-updated"

    async def _discord_profile(self, login: str, *, discord_user_id, discord_display_name, mark_member):
        self.calls.append(
            (
                "discord_profile",
                login,
                discord_user_id,
                discord_display_name,
                mark_member,
            )
        )
        return "profile-updated"

    async def _stats(self, *, hour_from, hour_to, streamer):
        self.calls.append(("stats", hour_from, hour_to, streamer))
        return {"ok": True, "streamer": streamer}

    async def _streamer_analytics(self, login: str, days: int):
        self.calls.append(("streamer_analytics", login, days))
        return {"ok": True, "login": login, "days": days}

    async def _comparison(self, days: int):
        self.calls.append(("comparison", days))
        return {"ok": True, "days": days}

    async def _session(self, session_id: int):
        self.calls.append(("session", session_id))
        return {}


class InternalApiStreamerRoutesTests(unittest.IsolatedAsyncioTestCase):
    async def test_streamers_route_normalizes_non_list_payloads(self) -> None:
        harness = _StreamersHarness()

        response = await streamer_routes.streamers(harness, _FakeRequest())

        payload = json.loads(response.text)
        self.assertEqual(response.status, 200)
        self.assertEqual(payload, ["alpha", "beta"])
        self.assertIn(("list", None), harness.calls)

    async def test_streamer_add_route_validates_and_invokes_callbacks(self) -> None:
        harness = _StreamersHarness()

        response = await streamer_routes.streamer_add(
            harness,
            _FakeRequest(body={"login": "Early_Salty", "require_link": True}),
        )

        payload = json.loads(response.text)
        self.assertEqual(response.status, 201)
        self.assertEqual(payload["login"], "early_salty")
        self.assertEqual(payload["message"], "added")
        self.assertIn(("add", "early_salty", True), harness.calls)
        self.assertIn(("release", "owner-key", "fingerprint", True, 201), harness.calls)

    async def test_session_detail_route_maps_empty_dict_to_not_found(self) -> None:
        harness = _StreamersHarness()

        response = await streamer_routes.session_detail(
            harness,
            _FakeRequest(match_info={"session_id": "42"}),
        )

        payload = json.loads(response.text)
        self.assertEqual(response.status, 404)
        self.assertEqual(payload["error"], "not_found")
        self.assertIn(("session", 42), harness.calls)

    async def test_stats_route_uses_query_normalization(self) -> None:
        harness = _StreamersHarness()

        response = await streamer_routes.stats(
            harness,
            _FakeRequest(query={"hour_from": "1", "hour_to": "5", "streamer": "Alpha"}),
        )

        payload = json.loads(response.text)
        self.assertEqual(response.status, 200)
        self.assertTrue(payload["ok"])
        self.assertEqual(payload["streamer"], "alpha")
        self.assertIn(("stats", 1, 5, "alpha"), harness.calls)


if __name__ == "__main__":
    unittest.main()
