import asyncio
import unittest

from bot.raid.services.followers import (
    CandidateFollowersDependencies,
    CandidateFollowersService,
    FollowerAuthContext,
    FollowerTotalEnricher,
)


class RaidFollowerEnricherTests(unittest.IsolatedAsyncioTestCase):
    async def test_db_hit_short_circuits_api_fallback(self) -> None:
        candidates = [
            {"user_id": "1001", "user_login": "Alpha", "followers_total": None},
            {"user_id": "2002", "user_login": "Bravo", "followers_total": None},
        ]
        db_calls: list[tuple[str, ...]] = []

        async def load_cached_totals(logins):
            db_calls.append(tuple(logins))
            return {"alpha": 111, "bravo": 222}

        async def fetch_followers_total(user_id: str, user_token: str | None = None):
            raise AssertionError("API fallback should not be called when DB has totals")

        await FollowerTotalEnricher(max_concurrency=2).enrich_candidates(
            candidates,
            load_cached_totals=load_cached_totals,
            fetch_followers_total=fetch_followers_total,
            auth_context=FollowerAuthContext(bot_token="bot-token", bot_scopes={"moderator:read:followers"}),
        )

        self.assertEqual(candidates[0]["followers_total"], 111)
        self.assertEqual(candidates[1]["followers_total"], 222)
        self.assertEqual(db_calls, [("alpha", "bravo")])

    async def test_api_fallback_is_bounded_by_semaphore(self) -> None:
        candidates = [
            {"user_id": "1001", "user_login": "alpha", "followers_total": None},
            {"user_id": "2002", "user_login": "bravo", "followers_total": None},
            {"user_id": "3003", "user_login": "charlie", "followers_total": None},
            {"user_id": "4004", "user_login": "delta", "followers_total": None},
        ]
        active = 0
        max_active = 0
        started = 0
        release = asyncio.Event()

        async def fetch_followers_total(user_id: str, user_token: str | None = None):
            nonlocal active, max_active, started
            started += 1
            active += 1
            max_active = max(max_active, active)
            await release.wait()
            active -= 1
            return int(user_id)

        task = asyncio.create_task(
            FollowerTotalEnricher(max_concurrency=2).enrich_candidates(
                candidates,
                fetch_followers_total=fetch_followers_total,
                auth_context=FollowerAuthContext(bot_token="bot-token", bot_scopes={"moderator:read:followers"}),
            )
        )

        deadline = asyncio.get_running_loop().time() + 1.0
        while started < 2 and asyncio.get_running_loop().time() < deadline:
            await asyncio.sleep(0)
        self.assertGreaterEqual(started, 2)
        self.assertLessEqual(max_active, 2)

        release.set()
        await task

        self.assertEqual([candidate["followers_total"] for candidate in candidates], [1001, 2002, 3003, 4004])

    async def test_duplicate_candidate_login_and_id_reuse(self) -> None:
        candidates = [
            {"user_id": "2002", "user_login": "alpha", "followers_total": None},
            {"user_id": "2002", "user_login": "alpha", "followers_total": None},
            {"user_id": None, "user_login": "alpha", "followers_total": None},
        ]
        calls: list[tuple[str, str | None]] = []

        async def fetch_followers_total(user_id: str, user_token: str | None = None):
            calls.append((user_id, user_token))
            return 987

        await FollowerTotalEnricher(max_concurrency=4).enrich_candidates(
            candidates,
            fetch_followers_total=fetch_followers_total,
            auth_context=FollowerAuthContext(bot_token="bot-token", bot_scopes={"moderator:read:followers"}),
        )

        self.assertEqual(calls, [("2002", "bot-token")])
        self.assertEqual([candidate["followers_total"] for candidate in candidates], [987, 987, 987])


class CandidateFollowersServiceTests(unittest.IsolatedAsyncioTestCase):
    class _CustomAwaitable:
        def __init__(self, value) -> None:
            self._value = value

        def __await__(self):
            async def _inner():
                return self._value

            return _inner().__await__()

    async def test_resolve_bot_oauth_context_is_called_without_session_argument(self) -> None:
        calls: list[str] = []

        async def resolve_bot_oauth_context():
            calls.append("called")
            return "bot-token", "9999", {"moderator:read:followers"}

        async def get_followers_total_result(_api, user_id: str, user_token: str | None):
            self.assertEqual(user_id, "1001")
            self.assertEqual(user_token, "bot-token")
            return {"ok": True, "data": 123}

        service = CandidateFollowersService(
            CandidateFollowersDependencies(
                create_twitch_api=lambda _session: object(),
                resolve_bot_oauth_context=resolve_bot_oauth_context,
                get_followers_total_result=get_followers_total_result,
                resolve_valid_token=lambda _user_id, _session: None,
                increment_counter=lambda _name, _delta: 0,
                warn_user_scope_fallback_once=lambda **_kwargs: None,
                clear_user_scope_fallback_warning=lambda **_kwargs: None,
                logger=type("Logger", (), {"debug": lambda *args, **kwargs: None})(),
            )
        )

        candidates = [{"user_id": "1001", "user_login": "alpha", "followers_total": None}]
        await service.attach_followers_totals(candidates, session=object())

        self.assertEqual(calls, ["called"])
        self.assertEqual(candidates[0]["followers_total"], 123)

    async def test_accepts_custom_awaitable_from_resolve_bot_oauth_context(self) -> None:
        service = CandidateFollowersService(
            CandidateFollowersDependencies(
                create_twitch_api=lambda _session: object(),
                resolve_bot_oauth_context=lambda: self._CustomAwaitable(
                    ("bot-token", "9999", {"moderator:read:followers"})
                ),
                get_followers_total_result=lambda _api, _user_id, _user_token: self._CustomAwaitable(
                    {"ok": True, "data": 456}
                ),
                resolve_valid_token=lambda _user_id, _session: None,
                increment_counter=lambda _name, _delta: 0,
                warn_user_scope_fallback_once=lambda **_kwargs: None,
                clear_user_scope_fallback_warning=lambda **_kwargs: None,
                logger=type("Logger", (), {"debug": lambda *args, **kwargs: None})(),
            )
        )

        candidates = [{"user_id": "1001", "user_login": "alpha", "followers_total": None}]
        await service.attach_followers_totals(candidates, session=object())

        self.assertEqual(candidates[0]["followers_total"], 456)


if __name__ == "__main__":
    unittest.main()
