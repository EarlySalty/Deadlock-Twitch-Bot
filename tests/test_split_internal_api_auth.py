import asyncio
import json
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from aiohttp.test_utils import TestClient, TestServer

from bot.internal_api import INTERNAL_API_BASE_PATH, INTERNAL_TOKEN_HEADER, build_internal_api_app
from bot.internal_api.app import IDEMPOTENCY_KEY_HEADER, InternalApiServer


class InternalApiAuthTests(unittest.IsolatedAsyncioTestCase):
    async def test_healthz_rejects_missing_token(self) -> None:
        app = build_internal_api_app(token="secret-token")
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(f"{INTERNAL_API_BASE_PATH}/healthz")
                payload = await response.json()

        self.assertEqual(response.status, 401)
        self.assertEqual(payload.get("error"), "unauthorized")

    async def test_healthz_rejects_invalid_token(self) -> None:
        app = build_internal_api_app(token="secret-token")
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/healthz",
                    headers={INTERNAL_TOKEN_HEADER: "wrong-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 401)
        self.assertEqual(payload.get("error"), "unauthorized")

    async def test_healthz_allows_valid_token(self) -> None:
        app = build_internal_api_app(token="secret-token")
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/healthz",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertTrue(payload.get("ok"))

    async def test_observability_debug_returns_scrubbed_payload(self) -> None:
        async def _observability_snapshot_cb() -> dict[str, object]:
            return {
                "chat": {"counters": {"chat_join_attempt_total": 3}},
                "raid": {"pending_count": 1},
            }

        app = build_internal_api_app(
            token="secret-token",
            observability_snapshot_cb=_observability_snapshot_cb,
        )
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/debug/observability",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertTrue(payload.get("ok"))
        self.assertEqual(
            payload.get("observability", {}).get("chat", {}).get("counters", {}).get("chat_join_attempt_total"),
            3,
        )

    async def test_observability_debug_returns_callback_payload(self) -> None:
        async def _observability_snapshot_cb() -> dict[str, object]:
            return {
                "chat": {"joinAttempts": 4},
                "raid": {"pendingCount": 2},
            }

        app = build_internal_api_app(
            token="secret-token",
            observability_snapshot_cb=_observability_snapshot_cb,
        )
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/debug/observability",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertTrue(payload.get("ok"))
        self.assertEqual(payload.get("observability", {}).get("chat", {}).get("joinAttempts"), 4)
        self.assertEqual(payload.get("observability", {}).get("raid", {}).get("pendingCount"), 2)

    async def test_healthz_rejects_non_loopback_host_header(self) -> None:
        app = build_internal_api_app(token="secret-token")
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/healthz",
                    headers={
                        INTERNAL_TOKEN_HEADER: "secret-token",
                        "Host": "dashboard.example",
                    },
                )
                payload = await response.json()

        self.assertEqual(response.status, 403)
        self.assertEqual(payload.get("error"), "forbidden")

    async def test_healthz_rejects_non_loopback_origin_header(self) -> None:
        app = build_internal_api_app(token="secret-token")
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/healthz",
                    headers={
                        INTERNAL_TOKEN_HEADER: "secret-token",
                        "Origin": "https://dashboard.example",
                    },
                )
                payload = await response.json()

        self.assertEqual(response.status, 403)
        self.assertEqual(payload.get("error"), "forbidden")

    async def test_healthz_fails_closed_when_server_token_is_missing(self) -> None:
        app = build_internal_api_app(token=None)
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/healthz",
                    headers={INTERNAL_TOKEN_HEADER: "any-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 401)
        self.assertEqual(payload.get("error"), "unauthorized")

    async def test_streamer_add_invalid_json_uses_stable_safe_error_message(self) -> None:
        app = build_internal_api_app(token="secret-token")
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/streamers",
                    data='{"login"',
                    headers={
                        INTERNAL_TOKEN_HEADER: "secret-token",
                        "Content-Type": "application/json",
                    },
                )
                payload = await response.json()

        self.assertEqual(response.status, 400)
        self.assertEqual(payload.get("error"), "bad_request")
        self.assertEqual(payload.get("message"), "invalid request body")

    async def test_stats_invalid_query_uses_stable_safe_error_message(self) -> None:
        app = build_internal_api_app(token="secret-token")
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/stats?hour_from=abc",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 400)
        self.assertEqual(payload.get("error"), "bad_request")
        self.assertEqual(payload.get("message"), "invalid query parameters")

    async def test_safe_bad_request_logs_do_not_leak_sensitive_values(self) -> None:
        async def _raise_add_cb(login: str, require_link: bool) -> str:
            del login, require_link
            raise ValueError("token=streamer-secret-123")

        async def _raise_stats_cb(**kwargs) -> dict[str, object]:
            del kwargs
            raise ValueError("dsn=postgres://internal-user:db-secret@example/db")

        async def _raise_live_link_click_cb(**kwargs) -> dict[str, object]:
            del kwargs
            raise ValueError("secret=discord-click-secret")

        cases = (
            {
                "context": "add streamer",
                "method": "post",
                "path": f"{INTERNAL_API_BASE_PATH}/streamers",
                "app_kwargs": {"add_cb": _raise_add_cb},
                "request_kwargs": {"json": {"login": "some_streamer"}},
                "expected_message": "invalid request body",
                "secret_fragment": "streamer-secret-123",
            },
            {
                "context": "stats query",
                "method": "get",
                "path": f"{INTERNAL_API_BASE_PATH}/stats",
                "app_kwargs": {"stats_cb": _raise_stats_cb},
                "request_kwargs": {},
                "expected_message": "invalid query parameters",
                "secret_fragment": "db-secret",
            },
            {
                "context": "live link click",
                "method": "post",
                "path": f"{INTERNAL_API_BASE_PATH}/live/link-click",
                "app_kwargs": {"live_link_click_cb": _raise_live_link_click_cb},
                "request_kwargs": {
                    "json": {
                        "streamer_login": "partner_one",
                        "tracking_token": "deadbeef1234",
                        "discord_user_id": "12345",
                        "discord_username": "Viewer One",
                        "guild_id": "111",
                        "channel_id": "222",
                        "message_id": "333",
                        "source_hint": "discord_button",
                    }
                },
                "expected_message": "invalid request body",
                "secret_fragment": "discord-click-secret",
            },
        )

        for case in cases:
            with self.subTest(context=case["context"]):
                app = build_internal_api_app(token="secret-token", **case["app_kwargs"])
                with self.assertLogs("TwitchStreams", level="WARNING") as captured:
                    async with TestServer(app) as server:
                        async with TestClient(server) as client:
                            request = getattr(client, case["method"])
                            response = await request(
                                case["path"],
                                headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                                **case["request_kwargs"],
                            )
                            raw_text = await response.text()
                            payload = json.loads(raw_text)

                self.assertEqual(response.status, 400)
                self.assertEqual(payload.get("error"), "bad_request")
                self.assertEqual(payload.get("message"), case["expected_message"])
                self.assertNotIn(case["secret_fragment"], raw_text)
                self.assertTrue(
                    any(
                        f"internal api {case['context']} bad request (ValueError" in line
                        for line in captured.output
                    )
                )
                self.assertTrue(
                    all(case["secret_fragment"] not in line for line in captured.output)
                )

    async def test_raid_requirements_safe_bad_request_logs_sanitized_reason(self) -> None:
        async def _raise_raid_requirements_cb(login: str) -> str:
            del login
            raise ValueError("Invalid Discord user id")

        app = build_internal_api_app(
            token="secret-token",
            raid_requirements_cb=_raise_raid_requirements_cb,
        )

        with self.assertLogs("TwitchStreams", level="WARNING") as captured:
            async with TestServer(app) as server:
                async with TestClient(server) as client:
                    response = await client.post(
                        f"{INTERNAL_API_BASE_PATH}/raid/requirements",
                        headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                        json={"login": "partner_one"},
                    )
                    payload = await response.json()

        self.assertEqual(response.status, 400)
        self.assertEqual(payload.get("error"), "bad_request")
        self.assertEqual(payload.get("message"), "invalid request body")
        self.assertTrue(
            any(
                "internal api raid requirements bad request (ValueError: Invalid Discord user id)"
                in line
                for line in captured.output
            )
        )

    async def test_raid_auth_url_allows_discord_state_target(self) -> None:
        seen_logins: list[str] = []

        async def _raid_auth_url_cb(login: str) -> str:
            seen_logins.append(login)
            return f"https://auth.example/{login}"

        app = build_internal_api_app(token="secret-token", raid_auth_url_cb=_raid_auth_url_cb)
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/raid/auth-url?login=discord:123456789",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertEqual(payload.get("login"), "discord:123456789")
        self.assertEqual(payload.get("auth_url"), "https://auth.example/discord:123456789")
        self.assertEqual(seen_logins, ["discord:123456789"])

    async def test_raid_auth_url_allows_public_onboarding_target(self) -> None:
        seen_logins: list[str] = []

        async def _raid_auth_url_cb(login: str) -> str:
            seen_logins.append(login)
            return f"https://auth.example/{login}"

        app = build_internal_api_app(token="secret-token", raid_auth_url_cb=_raid_auth_url_cb)
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/raid/auth-url?login=public:website_onboarding",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertEqual(payload.get("login"), "public:website_onboarding")
        self.assertEqual(
            payload.get("auth_url"),
            "https://auth.example/public:website_onboarding",
        )
        self.assertEqual(seen_logins, ["public:website_onboarding"])

    async def test_raid_auth_url_rejects_mismatched_discord_target_and_query_id(self) -> None:
        seen_calls: list[tuple[str, str | None]] = []

        async def _raid_auth_url_cb(login: str, discord_user_id: str | None = None) -> str:
            seen_calls.append((login, discord_user_id))
            return f"https://auth.example/{login}/{discord_user_id or 'none'}"

        app = build_internal_api_app(token="secret-token", raid_auth_url_cb=_raid_auth_url_cb)
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    (
                        f"{INTERNAL_API_BASE_PATH}/raid/auth-url"
                        "?login=discord:123456789&discord_user_id=987654321"
                    ),
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 400)
        self.assertEqual(payload.get("error"), "bad_request")
        self.assertEqual(payload.get("message"), "invalid request parameters")
        self.assertEqual(seen_calls, [])

    async def test_raid_auth_url_passes_discord_user_id_when_callback_supports_it(self) -> None:
        seen_calls: list[tuple[str, str | None]] = []

        async def _raid_auth_url_cb(login: str, discord_user_id: str | None = None) -> str:
            seen_calls.append((login, discord_user_id))
            return f"https://auth.example/{login}/{discord_user_id or 'none'}"

        app = build_internal_api_app(token="secret-token", raid_auth_url_cb=_raid_auth_url_cb)
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/raid/auth-url?login=partner_one&discord_user_id=123456789",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertEqual(payload.get("auth_url"), "https://auth.example/partner_one/123456789")
        self.assertEqual(seen_calls, [("partner_one", "123456789")])

    async def test_raid_auth_url_passes_scope_profile_when_callback_supports_it(self) -> None:
        seen_calls: list[tuple[str, str | None]] = []

        async def _raid_auth_url_cb(login: str, scope_profile: str | None = None) -> str:
            seen_calls.append((login, scope_profile))
            return f"https://auth.example/{login}/{scope_profile or 'none'}"

        app = build_internal_api_app(token="secret-token", raid_auth_url_cb=_raid_auth_url_cb)
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/raid/auth-url?login=public:website_onboarding&scope_profile=base",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertEqual(
            payload.get("auth_url"),
            "https://auth.example/public:website_onboarding/base",
        )
        self.assertEqual(seen_calls, [("public:website_onboarding", "base")])

    async def test_raid_auth_url_legacy_callback_still_works_without_discord_kwarg(self) -> None:
        seen_logins: list[str] = []

        async def _raid_auth_url_cb(login: str) -> str:
            seen_logins.append(login)
            return f"https://auth.example/{login}"

        app = build_internal_api_app(token="secret-token", raid_auth_url_cb=_raid_auth_url_cb)
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/raid/auth-url?login=partner_one&discord_user_id=123456789",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertEqual(payload.get("auth_url"), "https://auth.example/partner_one")
        self.assertEqual(seen_logins, ["partner_one"])

    async def test_raid_auth_url_rejects_invalid_discord_state_target(self) -> None:
        async def _raid_auth_url_cb(login: str) -> str:
            return f"https://auth.example/{login}"

        app = build_internal_api_app(token="secret-token", raid_auth_url_cb=_raid_auth_url_cb)
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/raid/auth-url?login=discord:not-a-number",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 400)
        self.assertEqual(payload.get("error"), "bad_request")
        self.assertEqual(payload.get("message"), "invalid or missing login")

    async def test_raid_auth_state_returns_stable_payload(self) -> None:
        seen_ids: list[str] = []

        async def _raid_auth_state_cb(discord_user_id: str) -> dict[str, object]:
            seen_ids.append(discord_user_id)
            return {
                "discord_user_id": discord_user_id,
                "twitch_login": "partner_one",
                "twitch_user_id": "1001",
                "authorized": True,
                "partner_opt_out": False,
            }

        app = build_internal_api_app(token="secret-token", raid_auth_state_cb=_raid_auth_state_cb)
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/raid/auth-state?discord_user_id=123456789",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertEqual(
            payload,
            {
                "ok": True,
                "discord_user_id": "123456789",
                "twitch_login": "partner_one",
                "twitch_user_id": "1001",
                "authorized": True,
                "partner_opt_out": False,
                "token_blacklisted": False,
                "raid_blacklisted": False,
                "blocked": False,
            },
        )
        self.assertEqual(seen_ids, ["123456789"])

    async def test_raid_auth_state_rejects_invalid_discord_user_id(self) -> None:
        app = build_internal_api_app(token="secret-token")
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/raid/auth-state?discord_user_id=abc",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 400)
        self.assertEqual(payload.get("error"), "bad_request")
        self.assertEqual(payload.get("message"), "invalid query parameters")

    async def test_raid_block_state_supports_login_only_and_computes_blocked(self) -> None:
        seen_queries: list[tuple[str | None, str | None]] = []

        async def _raid_block_state_cb(
            *,
            discord_user_id: str | None = None,
            twitch_login: str | None = None,
        ) -> dict[str, object]:
            seen_queries.append((discord_user_id, twitch_login))
            return {
                "discord_user_id": None,
                "twitch_login": twitch_login,
                "twitch_user_id": "2002",
                "authorized": True,
                "partner_opt_out": False,
                "token_blacklisted": True,
                "raid_blacklisted": False,
            }

        app = build_internal_api_app(token="secret-token", raid_block_state_cb=_raid_block_state_cb)
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/raid/block-state?twitch_login=Partner_One",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertTrue(payload.get("ok"))
        self.assertEqual(payload.get("twitch_login"), "partner_one")
        self.assertEqual(payload.get("twitch_user_id"), "2002")
        self.assertTrue(payload.get("authorized"))
        self.assertTrue(payload.get("token_blacklisted"))
        self.assertTrue(payload.get("blocked"))
        self.assertEqual(seen_queries, [(None, "partner_one")])

    async def test_raid_block_state_requires_identifier(self) -> None:
        app = build_internal_api_app(token="secret-token")
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/raid/block-state",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 400)
        self.assertEqual(payload.get("error"), "bad_request")
        self.assertEqual(payload.get("message"), "invalid query parameters")

    async def test_streamer_add_replays_idempotent_request(self) -> None:
        seen: list[tuple[str, bool]] = []

        async def _add_cb(login: str, require_link: bool) -> str:
            seen.append((login, require_link))
            return "added"

        app = build_internal_api_app(token="secret-token", add_cb=_add_cb)
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                headers = {
                    INTERNAL_TOKEN_HEADER: "secret-token",
                    IDEMPOTENCY_KEY_HEADER: "idem-streamer-add-1",
                }
                first = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/streamers",
                    headers=headers,
                    json={"login": "some_streamer", "require_link": True},
                )
                first_payload = await first.json()

                second = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/streamers",
                    headers=headers,
                    json={"login": "some_streamer", "require_link": True},
                )
                second_payload = await second.json()

        self.assertEqual(first.status, 201)
        self.assertEqual(second.status, 201)
        self.assertEqual(first_payload, second_payload)
        self.assertEqual(second.headers.get("X-Idempotency-Replayed"), "1")
        self.assertEqual(seen, [("some_streamer", True)])

    async def test_idempotency_key_is_scoped_per_endpoint(self) -> None:
        add_seen: list[tuple[str, bool]] = []
        click_seen: list[dict[str, str | None]] = []

        async def _add_cb(login: str, require_link: bool) -> str:
            add_seen.append((login, require_link))
            return "added"

        async def _live_link_click_cb(**kwargs) -> dict[str, object]:
            click_seen.append(kwargs)
            return {"ok": True}

        app = build_internal_api_app(
            token="secret-token",
            add_cb=_add_cb,
            live_link_click_cb=_live_link_click_cb,
        )
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                headers = {
                    INTERNAL_TOKEN_HEADER: "secret-token",
                    IDEMPOTENCY_KEY_HEADER: "idem-shared-across-endpoints",
                }
                add_response = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/streamers",
                    headers=headers,
                    json={"login": "some_streamer"},
                )
                add_payload = await add_response.json()

                click_response = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/live/link-click",
                    headers=headers,
                    json={
                        "streamer_login": "partner_one",
                        "tracking_token": "deadbeef1234",
                        "discord_user_id": "12345",
                        "discord_username": "Viewer One",
                        "guild_id": "111",
                        "channel_id": "222",
                        "message_id": "333",
                        "source_hint": "discord_button",
                    },
                )
                click_payload = await click_response.json()

        self.assertEqual(add_response.status, 201)
        self.assertEqual(add_payload.get("ok"), True)
        self.assertIsNone(add_response.headers.get("X-Idempotency-Replayed"))
        self.assertEqual(click_response.status, 200)
        self.assertEqual(click_payload, {"ok": True})
        self.assertIsNone(click_response.headers.get("X-Idempotency-Replayed"))
        self.assertEqual(add_seen, [("some_streamer", False)])
        self.assertEqual(
            click_seen,
            [
                {
                    "streamer_login": "partner_one",
                    "tracking_token": "deadbeef1234",
                    "discord_user_id": "12345",
                    "discord_username": "Viewer One",
                    "guild_id": "111",
                    "channel_id": "222",
                    "message_id": "333",
                    "source_hint": "discord_button",
                }
            ],
        )

    async def test_streamer_add_rejects_idempotency_key_reuse_with_different_payload(self) -> None:
        seen: list[tuple[str, bool]] = []

        async def _add_cb(login: str, require_link: bool) -> str:
            seen.append((login, require_link))
            return "added"

        app = build_internal_api_app(token="secret-token", add_cb=_add_cb)
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                headers = {
                    INTERNAL_TOKEN_HEADER: "secret-token",
                    IDEMPOTENCY_KEY_HEADER: "idem-streamer-add-2",
                }
                first = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/streamers",
                    headers=headers,
                    json={"login": "alpha_streamer"},
                )
                self.assertEqual(first.status, 201)

                second = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/streamers",
                    headers=headers,
                    json={"login": "beta_streamer"},
                )
                second_payload = await second.json()

        self.assertEqual(second.status, 409)
        self.assertEqual(second_payload.get("error"), "idempotency_conflict")
        self.assertEqual(seen, [("alpha_streamer", False)])

    async def test_prepare_idempotency_discards_expired_cache_and_inflight_entries(self) -> None:
        server = InternalApiServer(token="secret-token")
        server._idempotency_ttl_seconds = 30
        request = SimpleNamespace(
            headers={IDEMPOTENCY_KEY_HEADER: "idem-ttl-expired-1"},
            method="POST",
            path=f"{INTERNAL_API_BASE_PATH}/streamers",
            path_qs=f"{INTERNAL_API_BASE_PATH}/streamers",
        )
        scope_key = server._idempotency_scope_key(request=request, key="idem-ttl-expired-1")
        stale_future = asyncio.get_running_loop().create_future()
        server._idempotency_cache[scope_key] = {
            "fingerprint": "stale-cache-fingerprint",
            "status": 201,
            "payload": {"ok": True, "message": "stale"},
            "created_at": 100.0,
        }
        server._idempotency_inflight[scope_key] = SimpleNamespace(
            fingerprint="stale-inflight-fingerprint",
            future=stale_future,
            created_at=100.0,
        )

        with patch("bot.internal_api.app.time_module.time", return_value=200.0):
            key, fingerprint, replay, wait_future, is_owner = server._prepare_idempotency(
                request=request,
                payload={"login": "some_streamer"},
            )

        self.assertEqual(key, "")
        self.assertEqual(fingerprint, "")
        self.assertIsNotNone(replay)
        self.assertIsNone(wait_future)
        self.assertFalse(is_owner)
        self.assertTrue(stale_future.done())
        self.assertEqual(
            stale_future.result(),
            (
                503,
                {
                    "error": "upstream_unavailable",
                    "message": "idempotent request timed out",
                },
            ),
        )
        self.assertEqual(replay.status, 409)
        self.assertIn(scope_key, server._idempotency_cache)
        self.assertNotIn(scope_key, server._idempotency_inflight)

    async def test_prepare_idempotency_replays_timeout_for_same_request_after_inflight_expiry(self) -> None:
        server = InternalApiServer(token="secret-token")
        server._idempotency_ttl_seconds = 30
        request = SimpleNamespace(
            headers={IDEMPOTENCY_KEY_HEADER: "idem-ttl-expired-2"},
            method="POST",
            path=f"{INTERNAL_API_BASE_PATH}/streamers",
            path_qs=f"{INTERNAL_API_BASE_PATH}/streamers",
        )
        payload = {"login": "same_streamer"}
        scope_key = server._idempotency_scope_key(request=request, key="idem-ttl-expired-2")
        fingerprint = server._request_fingerprint(request=request, payload=payload)
        stale_future = asyncio.get_running_loop().create_future()
        server._idempotency_inflight[scope_key] = SimpleNamespace(
            fingerprint=fingerprint,
            future=stale_future,
            created_at=100.0,
        )

        with patch("bot.internal_api.app.time_module.time", return_value=200.0):
            key, seen_fingerprint, replay, wait_future, is_owner = server._prepare_idempotency(
                request=request,
                payload=payload,
            )

        self.assertEqual(key, "")
        self.assertEqual(seen_fingerprint, "")
        self.assertIsNotNone(replay)
        self.assertIsNone(wait_future)
        self.assertFalse(is_owner)
        self.assertEqual(replay.status, 503)
        self.assertEqual(replay.headers.get("X-Idempotency-Replayed"), "1")
        self.assertTrue(stale_future.done())
        self.assertIn(scope_key, server._idempotency_cache)
        self.assertNotIn(scope_key, server._idempotency_inflight)

    async def test_live_active_announcements_returns_minimal_entries(self) -> None:
        async def _live_active_announcements_cb() -> list[dict[str, object]]:
            return [
                {
                    "streamer_login": "partner_one",
                    "message_id": 123456789,
                    "tracking_token": "deadbeef1234",
                    "referral_url": "https://www.twitch.tv/partner_one?ref=DE-Deadlock-Discord",
                    "button_label": "Auf Twitch ansehen",
                    "channel_id": 987654321,
                }
            ]

        app = build_internal_api_app(
            token="secret-token",
            live_active_announcements_cb=_live_active_announcements_cb,
        )
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/live/active-announcements",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertEqual(
            payload,
            [
                {
                    "streamer_login": "partner_one",
                    "message_id": 123456789,
                    "tracking_token": "deadbeef1234",
                    "referral_url": "https://www.twitch.tv/partner_one?ref=DE-Deadlock-Discord",
                    "button_label": "Auf Twitch ansehen",
                    "channel_id": 987654321,
                }
            ],
        )

    async def test_live_link_click_replays_idempotent_request(self) -> None:
        seen: list[dict[str, str | None]] = []

        async def _live_link_click_cb(**kwargs) -> dict[str, object]:
            seen.append(kwargs)
            return {"ok": True}

        app = build_internal_api_app(token="secret-token", live_link_click_cb=_live_link_click_cb)
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                headers = {
                    INTERNAL_TOKEN_HEADER: "secret-token",
                    IDEMPOTENCY_KEY_HEADER: "idem-live-click-1",
                }
                body = {
                    "streamer_login": "partner_one",
                    "tracking_token": "deadbeef1234",
                    "discord_user_id": "12345",
                    "discord_username": "Viewer One",
                    "guild_id": "111",
                    "channel_id": "222",
                    "message_id": "333",
                    "source_hint": "discord_button",
                }
                first = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/live/link-click",
                    headers=headers,
                    json=body,
                )
                first_payload = await first.json()

                second = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/live/link-click",
                    headers=headers,
                    json=body,
                )
                second_payload = await second.json()

        self.assertEqual(first.status, 200)
        self.assertEqual(second.status, 200)
        self.assertEqual(first_payload, {"ok": True})
        self.assertEqual(second_payload, {"ok": True})
        self.assertEqual(second.headers.get("X-Idempotency-Replayed"), "1")
        self.assertEqual(
            seen,
            [
                {
                    "streamer_login": "partner_one",
                    "tracking_token": "deadbeef1234",
                    "discord_user_id": "12345",
                    "discord_username": "Viewer One",
                    "guild_id": "111",
                    "channel_id": "222",
                    "message_id": "333",
                    "source_hint": "discord_button",
                }
            ],
        )

    async def test_live_link_click_rejects_missing_required_fields(self) -> None:
        app = build_internal_api_app(token="secret-token")
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/live/link-click",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                    json={
                        "streamer_login": "partner_one",
                        "discord_user_id": "12345",
                        "discord_username": "Viewer One",
                        "guild_id": "111",
                        "channel_id": "222",
                        "message_id": "333",
                        "source_hint": "discord_button",
                    },
                )
                payload = await response.json()

        self.assertEqual(response.status, 400)
        self.assertEqual(payload.get("error"), "bad_request")
        self.assertEqual(payload.get("message"), "invalid request body")

    def test_loopback_host_parser_accepts_ipv6_literals(self) -> None:
        self.assertEqual(InternalApiServer._host_without_port("::1"), "::1")
        self.assertEqual(InternalApiServer._host_without_port("0:0:0:0:0:0:0:1"), "0:0:0:0:0:0:0:1")
        self.assertTrue(InternalApiServer._is_loopback_host("::1"))
        self.assertTrue(InternalApiServer._is_loopback_host("[::1]:8777"))
        self.assertTrue(InternalApiServer._is_loopback_host("0:0:0:0:0:0:0:1"))

    async def test_healthz_allows_ipv6_loopback_host_variants(self) -> None:
        app = build_internal_api_app(token="secret-token")
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response_1 = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/healthz",
                    headers={
                        INTERNAL_TOKEN_HEADER: "secret-token",
                        "Host": "[::1]:8777",
                    },
                )
                payload_1 = await response_1.json()

                response_2 = await client.get(
                    f"{INTERNAL_API_BASE_PATH}/healthz",
                    headers={
                        INTERNAL_TOKEN_HEADER: "secret-token",
                        "Host": "0:0:0:0:0:0:0:1",
                    },
                )
                payload_2 = await response_2.json()

        self.assertEqual(response_1.status, 200)
        self.assertTrue(payload_1.get("ok"))
        self.assertEqual(response_2.status, 200)
        self.assertTrue(payload_2.get("ok"))

    async def test_streamer_add_concurrent_idempotent_requests_execute_once(self) -> None:
        seen: list[tuple[str, bool]] = []
        entered = asyncio.Event()
        release = asyncio.Event()

        async def _add_cb(login: str, require_link: bool) -> str:
            seen.append((login, require_link))
            entered.set()
            await release.wait()
            return "added"

        app = build_internal_api_app(token="secret-token", add_cb=_add_cb)
        async with TestServer(app) as server:
            async with TestClient(server) as client:
                headers = {
                    INTERNAL_TOKEN_HEADER: "secret-token",
                    IDEMPOTENCY_KEY_HEADER: "idem-concurrent-add-1",
                }
                req_payload = {"login": "parallel_streamer", "require_link": True}

                first_task = asyncio.create_task(
                    client.post(
                        f"{INTERNAL_API_BASE_PATH}/streamers",
                        headers=headers,
                        json=req_payload,
                    )
                )
                await entered.wait()
                second_task = asyncio.create_task(
                    client.post(
                        f"{INTERNAL_API_BASE_PATH}/streamers",
                        headers=headers,
                        json=req_payload,
                    )
                )
                await asyncio.sleep(0.05)
                release.set()

                first = await first_task
                second = await second_task
                first_payload = await first.json()
                second_payload = await second.json()

        self.assertEqual(first.status, 201)
        self.assertEqual(second.status, 201)
        self.assertEqual(first_payload, second_payload)
        self.assertIsNone(first.headers.get("X-Idempotency-Replayed"))
        self.assertEqual(second.headers.get("X-Idempotency-Replayed"), "1")
        self.assertEqual(seen, [("parallel_streamer", True)])

    async def test_raid_auth_url_runtime_error_response_is_sanitized(self) -> None:
        async def _raid_auth_url_cb(login: str) -> str:
            raise RuntimeError(f"database connection failed for {login}: secret=super-secret")

        app = build_internal_api_app(token="secret-token", raid_auth_url_cb=_raid_auth_url_cb)
        with self.assertLogs("TwitchStreams", level="WARNING") as captured:
            async with TestServer(app) as server:
                async with TestClient(server) as client:
                    response = await client.get(
                        f"{INTERNAL_API_BASE_PATH}/raid/auth-url?login=some_streamer",
                        headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                    )
                    payload = await response.json()

        self.assertEqual(response.status, 503)
        self.assertEqual(payload.get("error"), "upstream_unavailable")
        self.assertEqual(payload.get("message"), "upstream unavailable")
        self.assertNotIn("super-secret", payload.get("message", ""))
        self.assertTrue(any("raid auth url runtime failed (RuntimeError)" in line for line in captured.output))
        self.assertTrue(all("super-secret" not in line for line in captured.output))

    async def test_raid_auth_url_internal_type_error_is_not_silently_retried(self) -> None:
        seen_calls: list[tuple[str, str | None]] = []

        async def _raid_auth_url_cb(login: str, discord_user_id: str | None = None) -> str:
            seen_calls.append((login, discord_user_id))
            raise TypeError("broken callback internals")

        app = build_internal_api_app(token="secret-token", raid_auth_url_cb=_raid_auth_url_cb)
        with self.assertLogs("TwitchStreams", level="WARNING") as captured:
            async with TestServer(app) as server:
                async with TestClient(server) as client:
                    response = await client.get(
                        f"{INTERNAL_API_BASE_PATH}/raid/auth-url?login=partner_one&discord_user_id=123456789",
                        headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                    )
                    payload = await response.json()

        self.assertEqual(response.status, 500)
        self.assertEqual(payload.get("error"), "internal_error")
        self.assertEqual(payload.get("message"), "failed to generate raid auth url")
        self.assertEqual(seen_calls, [("partner_one", "123456789")])
        self.assertTrue(any("internal api raid auth url failed" in line for line in captured.output))

    async def test_discord_profile_scope_allowlist_rejects_unlisted_ids(self) -> None:
        seen: list[dict[str, str | bool | None]] = []

        async def _discord_profile_cb(
            login: str,
            discord_user_id: str | None = None,
            discord_display_name: str | None = None,
            mark_member: bool = True,
        ) -> str:
            seen.append(
                {
                    "login": login,
                    "discord_user_id": discord_user_id,
                    "discord_display_name": discord_display_name,
                    "mark_member": mark_member,
                }
            )
            return "updated"

        with patch.dict(
            "os.environ",
            {
                "TWITCH_INTERNAL_API_ALLOWED_GUILD_IDS": "111",
                "TWITCH_INTERNAL_API_ALLOWED_CHANNEL_IDS": "222",
                "TWITCH_INTERNAL_API_ALLOWED_ROLE_IDS": "333",
            },
            clear=False,
        ):
            app = build_internal_api_app(token="secret-token", discord_profile_cb=_discord_profile_cb)

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/streamers/some_streamer/discord-profile",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                    json={
                        "discord_user_id": "123",
                        "guild_id": 111,
                        "channel_id": 999,
                        "role_id": 333,
                    },
                )
                payload = await response.json()

        self.assertEqual(response.status, 403)
        self.assertEqual(payload.get("error"), "forbidden")
        self.assertEqual(payload.get("message"), "action outside configured scope")
        self.assertEqual(seen, [])

    async def test_discord_profile_scope_allowlist_accepts_listed_ids(self) -> None:
        seen: list[str] = []

        async def _discord_profile_cb(
            login: str,
            discord_user_id: str | None = None,
            discord_display_name: str | None = None,
            mark_member: bool = True,
        ) -> str:
            del discord_user_id, discord_display_name, mark_member
            seen.append(login)
            return "updated"

        with patch.dict(
            "os.environ",
            {
                "TWITCH_INTERNAL_API_ALLOWED_GUILD_IDS": "111",
                "TWITCH_INTERNAL_API_ALLOWED_CHANNEL_IDS": "222",
                "TWITCH_INTERNAL_API_ALLOWED_ROLE_IDS": "333",
            },
            clear=False,
        ):
            app = build_internal_api_app(token="secret-token", discord_profile_cb=_discord_profile_cb)

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/streamers/some_streamer/discord-profile",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                    json={
                        "discord_user_id": "123",
                        "guild_id": 111,
                        "channel_id": 222,
                        "role_id": 333,
                    },
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertTrue(payload.get("ok"))
        self.assertEqual(seen, ["some_streamer"])

    async def test_discord_profile_scope_allowlist_not_configured_keeps_existing_behavior(self) -> None:
        seen: list[str] = []

        async def _discord_profile_cb(
            login: str,
            discord_user_id: str | None = None,
            discord_display_name: str | None = None,
            mark_member: bool = True,
        ) -> str:
            del discord_user_id, discord_display_name, mark_member
            seen.append(login)
            return "updated"

        with patch.dict("os.environ", {}, clear=True):
            app = build_internal_api_app(token="secret-token", discord_profile_cb=_discord_profile_cb)

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/streamers/some_streamer/discord-profile",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                    json={
                        "discord_user_id": "123",
                        "guild_id": 999001,
                        "channel_id": 999002,
                        "role_id": 999003,
                    },
                )
                payload = await response.json()

        self.assertEqual(response.status, 200)
        self.assertTrue(payload.get("ok"))
        self.assertEqual(seen, ["some_streamer"])

    async def test_discord_profile_scope_allowlist_configured_invalid_values_denies_all(self) -> None:
        seen: list[str] = []

        async def _discord_profile_cb(
            login: str,
            discord_user_id: str | None = None,
            discord_display_name: str | None = None,
            mark_member: bool = True,
        ) -> str:
            del discord_user_id, discord_display_name, mark_member
            seen.append(login)
            return "updated"

        with patch.dict(
            "os.environ",
            {
                "TWITCH_INTERNAL_API_ALLOWED_GUILD_IDS": "abc, -1, ;",
                "TWITCH_INTERNAL_API_ALLOWED_CHANNEL_IDS": "NaN",
                "TWITCH_INTERNAL_API_ALLOWED_ROLE_IDS": "0, none",
            },
            clear=False,
        ):
            with self.assertLogs("TwitchStreams", level="WARNING") as captured:
                app = build_internal_api_app(token="secret-token", discord_profile_cb=_discord_profile_cb)

        async with TestServer(app) as server:
            async with TestClient(server) as client:
                response = await client.post(
                    f"{INTERNAL_API_BASE_PATH}/streamers/some_streamer/discord-profile",
                    headers={INTERNAL_TOKEN_HEADER: "secret-token"},
                    json={
                        "discord_user_id": "123",
                        "guild_id": 111,
                        "channel_id": 222,
                        "role_id": 333,
                    },
                )
                payload = await response.json()

        self.assertEqual(response.status, 403)
        self.assertEqual(payload.get("error"), "forbidden")
        self.assertEqual(payload.get("message"), "action outside configured scope")
        self.assertEqual(seen, [])
        self.assertTrue(
            any(
                "TWITCH_INTERNAL_API_ALLOWED_GUILD_IDS configured but no valid positive IDs parsed"
                in line
                for line in captured.output
            )
        )
        self.assertTrue(
            any(
                "TWITCH_INTERNAL_API_ALLOWED_CHANNEL_IDS configured but no valid positive IDs parsed"
                in line
                for line in captured.output
            )
        )
        self.assertTrue(
            any(
                "TWITCH_INTERNAL_API_ALLOWED_ROLE_IDS configured but no valid positive IDs parsed"
                in line
                for line in captured.output
            )
        )


if __name__ == "__main__":
    unittest.main()
