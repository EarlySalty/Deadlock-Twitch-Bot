from __future__ import annotations

import json
import unittest

from aiohttp import web

from bot.internal_api.routes import raid as raid_routes


class _FakeRequest:
    def __init__(
        self,
        *,
        query: dict[str, object] | None = None,
        body: dict[str, object] | None = None,
    ) -> None:
        self.query = query or {}
        self.match_info = {}
        self._body = dict(body or {})

    async def json(self) -> dict[str, object]:
        return dict(self._body)


class _RaidHarness:
    def __init__(self) -> None:
        self.calls: list[tuple[str, object]] = []

    def _json_response(self, payload, status: int = 200):
        return web.json_response(payload, status=status)

    def _json_error(self, error: str, status: int, message: str):
        return web.json_response({"error": error, "message": message}, status=status)

    def _safe_bad_request(self, *, context: str, exc: Exception, message: str):
        del context, exc
        return self._json_error("bad_request", 400, message)

    def _safe_exception_error(self, *, context: str, exc: Exception, error: str, status: int, message: str):
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

    def _normalize_login(self, value: str):
        normalized = str(value or "").strip().lower()
        return normalized or None

    def _normalize_raid_auth_target(self, value: str):
        text = str(value or "").strip()
        if text.startswith("discord:"):
            suffix = text.split(":", 1)[1]
            return text if suffix.isdigit() else None
        return text.lower() if text else None

    def _normalize_discord_user_id_param(self, value, *, required: bool):
        text = str(value or "").strip()
        if not text:
            return None if not required else (_ for _ in ()).throw(ValueError("missing discord_user_id"))
        if not text.isdigit():
            raise ValueError("invalid discord_user_id")
        return text

    def _normalize_raid_state_payload(self, payload, *, discord_user_id, twitch_login):
        result = dict(payload or {})
        result.setdefault("discord_user_id", discord_user_id)
        result.setdefault("twitch_login", twitch_login)
        return result

    def _enforce_discord_action_scope(self, payload):
        self.calls.append(("scope", dict(payload)))

    async def _invoke_raid_auth_url(self, login: str, *, discord_user_id=None, scope_profile=None):
        self.calls.append(("auth_url", login, discord_user_id, scope_profile))
        return f"https://auth.example/{login}"

    async def _raid_auth_state(self, discord_user_id: str):
        self.calls.append(("auth_state", discord_user_id))
        return {"authorized": True}

    async def _raid_block_state(self, *, discord_user_id=None, twitch_login=None):
        self.calls.append(("block_state", discord_user_id, twitch_login))
        return {"blocked": bool(twitch_login)}

    async def _raid_go_url(self, state: str):
        self.calls.append(("go_url", state))
        return f"https://raid.example/{state}"

    async def _raid_requirements(self, login: str):
        self.calls.append(("requirements", login))
        return "sent"

    async def _raid_oauth_callback(self, *, code: str, state: str, error: str):
        self.calls.append(("oauth_callback", code, state, error))
        return {"status": 201, "ok": True}


class InternalApiRaidRoutesTests(unittest.IsolatedAsyncioTestCase):
    async def test_raid_auth_url_normalizes_discord_target(self) -> None:
        harness = _RaidHarness()

        response = await raid_routes.raid_auth_url(
            harness,
            _FakeRequest(query={"login": "discord:123456789"}),
        )

        payload = json.loads(response.text)
        self.assertEqual(response.status, 200)
        self.assertEqual(payload["login"], "discord:123456789")
        self.assertIn(("auth_url", "discord:123456789", "123456789", None), harness.calls)

    async def test_raid_requirements_route_validates_and_releases_idempotency(self) -> None:
        harness = _RaidHarness()

        response = await raid_routes.raid_requirements(
            harness,
            _FakeRequest(body={"login": "Partner_One"}),
        )

        payload = json.loads(response.text)
        self.assertEqual(response.status, 200)
        self.assertEqual(payload["login"], "partner_one")
        self.assertIn(("requirements", "partner_one"), harness.calls)
        self.assertIn(("release", "owner-key", "fingerprint", True, 200), harness.calls)

    async def test_raid_oauth_callback_normalizes_result_status(self) -> None:
        harness = _RaidHarness()

        response = await raid_routes.raid_oauth_callback(
            harness,
            _FakeRequest(body={"code": "abc", "state": "xyz"}),
        )

        payload = json.loads(response.text)
        self.assertEqual(response.status, 200)
        self.assertEqual(payload["status"], 201)
        self.assertTrue(payload["ok"])


if __name__ == "__main__":
    unittest.main()
