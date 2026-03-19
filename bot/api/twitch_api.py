import asyncio
import logging
import time

import aiohttp

try:
    from service.http_client import build_resilient_connector
except ModuleNotFoundError:  # pragma: no cover - split runtime fallback
    from ..compat.http_client import build_resilient_connector

from .twitch_auth import (
    TwitchClientConfigError,
    is_invalid_client_response,
    normalize_twitch_credential,
)

TWITCH_TOKEN_URL = "https://id.twitch.tv/oauth2/token"  # noqa: S105
TWITCH_API_BASE = "https://api.twitch.tv/helix"


class TwitchAPI:
    """
    Async Wrapper für Twitch Helix mit App-Access-Token.

    - Eine wiederverwendete aiohttp.ClientSession (lazy erstellt)
    - Token wird automatisch geholt/refresh't
    - Hilfsfunktionen für Users, Streams & Kategorien
    """

    def __init__(
        self,
        client_id: str,
        client_secret: str,
        session: aiohttp.ClientSession | None = None,
    ):
        self.client_id = normalize_twitch_credential(client_id)
        self.client_secret = normalize_twitch_credential(client_secret)
        self._session = session
        self._own_session = False
        self._token: str | None = None
        self._token_expiry: float = 0.0
        self._lock = asyncio.Lock()
        self._category_cache: dict[str, str] = {}  # name_lower -> id
        self._log = logging.getLogger("TwitchStreams")
        self._auth_blocked_until: float = 0.0
        self._auth_block_reason: str | None = None

    # ---- Session lifecycle -------------------------------------------------
    def _ensure_session(self) -> None:
        if self._session is not None and self._session.closed:
            self._log.warning("Detected closed TwitchAPI HTTP session; creating a new session")
            self._session = None
            self._own_session = False

        if self._session is None:
            connector = build_resilient_connector()
            timeout = aiohttp.ClientTimeout(total=20)
            self._session = aiohttp.ClientSession(
                timeout=timeout,
                connector=connector,
                trust_env=True,
            )
            self._own_session = True

    @staticmethod
    def _is_closed_session_error(exc: BaseException) -> bool:
        return isinstance(exc, RuntimeError) and "Session is closed" in str(exc)

    def get_http_session(self) -> aiohttp.ClientSession:
        """Return the internal aiohttp session, ensuring it exists."""
        self._ensure_session()
        assert self._session is not None
        return self._session

    async def aclose(self) -> None:
        if self._own_session and self._session and not self._session.closed:
            await self._session.close()
        self._session = None
        self._own_session = False

    async def __aenter__(self):
        self._ensure_session()
        return self

    async def __aexit__(self, exc_type, exc, tb):
        await self.aclose()

    def is_auth_blocked(self) -> bool:
        return time.time() < self._auth_blocked_until

    def _block_auth(self, reason: str, *, cooldown_seconds: float = 900.0) -> TwitchClientConfigError:
        self._auth_blocked_until = time.time() + float(cooldown_seconds)
        self._auth_block_reason = reason
        return TwitchClientConfigError(reason)

    def _raise_if_auth_blocked(self) -> None:
        if self.is_auth_blocked():
            raise TwitchClientConfigError(
                self._auth_block_reason
                or "Twitch OAuth requests are temporarily blocked due to invalid client configuration."
            )

    def _ensure_client_credentials(self) -> None:
        if self.client_id and self.client_secret:
            return

        reason = (
            "Twitch client credentials are missing or blank after trimming whitespace. "
            "Verify TWITCH_CLIENT_ID and TWITCH_CLIENT_SECRET, then restart the bot."
        )
        if not self.is_auth_blocked() or self._auth_block_reason != reason:
            self._log.error(reason)
        raise self._block_auth(reason)

    # ---- OAuth -------------------------------------------------------------
    async def _ensure_token(self):
        async with self._lock:
            if self._token and time.time() < self._token_expiry - 60:
                return
            self._raise_if_auth_blocked()
            self._ensure_client_credentials()
            data = {
                "client_id": self.client_id,
                "client_secret": self.client_secret,
                "grant_type": "client_credentials",
            }
            for attempt in range(3):
                self._ensure_session()
                assert self._session is not None
                try:
                    async with self._session.post(TWITCH_TOKEN_URL, data=data) as r:
                        if r.status != 200:
                            txt = await r.text()
                            if is_invalid_client_response(r.status, txt):
                                reason = (
                                    "Twitch client credentials were rejected by Twitch OAuth "
                                    "(invalid client). Verify TWITCH_CLIENT_ID/TWITCH_CLIENT_SECRET, "
                                    "rotate the secret if needed, then restart the bot. "
                                    "Suppressing additional token requests for 15 minutes."
                                )
                                if not self.is_auth_blocked() or self._auth_block_reason != reason:
                                    self._log.error(reason)
                                raise self._block_auth(reason)
                            self._log.error(
                                "twitch token exchange failed: HTTP %s: %s",
                                r.status,
                                txt[:300].replace("\n", " "),
                            )
                            r.raise_for_status()
                        js = await r.json()
                        self._token = js.get("access_token")
                        expires = js.get("expires_in", 3600)
                        self._token_expiry = time.time() + float(expires)
                        self._auth_blocked_until = 0.0
                        self._auth_block_reason = None
                        return
                except RuntimeError as exc:
                    if not self._is_closed_session_error(exc):
                        raise
                    self._log.warning(
                        "Token request retry %s/3 after closed HTTP session",
                        attempt + 1,
                    )
                    self._session = None
                    self._own_session = False
                    if attempt < 2:
                        await asyncio.sleep(0.2 * (attempt + 1))
                        continue
                    raise

    def _headers(self) -> dict[str, str]:
        return {"Client-ID": self.client_id, "Authorization": f"Bearer {self._token}"}

    @staticmethod
    def _normalize_bearer_token(token_value: str | None) -> str:
        token = str(token_value or "").strip()
        if token.lower().startswith("oauth:"):
            token = token.split(":", 1)[1].strip()
        return token

    @staticmethod
    def _sanitize_error_text(text: object, *, limit: int = 240) -> str | None:
        cleaned = str(text or "").replace("\r", " ").replace("\n", " ").strip()
        if not cleaned:
            return None
        return cleaned[:limit]

    @classmethod
    def _helix_result(
        cls,
        *,
        ok: bool,
        data: object = None,
        http_status: int | None = None,
        error_code: str | None = None,
        message: str | None = None,
        request_attempted: bool = False,
    ) -> dict[str, object]:
        return {
            "ok": bool(ok),
            "data": data,
            "http_status": int(http_status) if http_status is not None else None,
            "error_code": str(error_code or "").strip() or None,
            "message": cls._sanitize_error_text(message),
            "request_attempted": bool(request_attempted),
        }

    @staticmethod
    def _map_helix_error_code(
        *,
        http_status: int | None,
        message: str | None,
        default: str,
    ) -> str:
        text = str(message or "").strip().lower()
        if http_status == 401:
            if "must match" in text or "does not match" in text or "token" in text:
                return "helix_401_token_mismatch"
            return "helix_401_unauthorized"
        if http_status == 403:
            if "moderator" in text or "forbidden" in text or "mod" in text:
                return "helix_403_not_moderator"
            return "helix_403_forbidden"
        if http_status == 404:
            return "helix_404_not_found"
        if http_status == 410:
            return "helix_410_gone"
        if http_status == 429:
            return "helix_429_rate_limited"
        if http_status is not None and http_status >= 500:
            return "helix_5xx_upstream"
        return str(default or "helix_request_failed").strip() or "helix_request_failed"

    async def _post(
        self,
        path: str,
        json: dict | None = None,
        *,
        log_on_error: bool = True,
        oauth_token: str | None = None,
        max_attempts: int = 3,
        request_timeout_total: float | None = None,
    ) -> dict:
        # Allow caller to override the auth token (e.g. EventSub with user tokens)
        token_override = (oauth_token or "").strip()
        if token_override.lower().startswith("oauth:"):
            token_override = token_override.split(":", 1)[1]

        if not token_override:
            await self._ensure_token()
            token_override = self._token or ""

        url = f"{TWITCH_API_BASE}{path}"
        last_exc: Exception | None = None
        attempts = max(1, min(int(max_attempts or 1), 5))
        request_timeout = None
        if request_timeout_total is not None:
            request_timeout = aiohttp.ClientTimeout(total=max(0.1, float(request_timeout_total)))

        for attempt in range(attempts):
            self._ensure_session()
            assert self._session is not None
            try:
                headers = {
                    "Client-ID": self.client_id,
                    "Authorization": f"Bearer {token_override}",
                }
                request_kwargs = {"headers": headers, "json": json}
                if request_timeout is not None:
                    request_kwargs["timeout"] = request_timeout
                async with self._session.post(url, **request_kwargs) as r:
                    if r.status not in {200, 202}:
                        txt = await r.text()
                        txt_one_line = txt[:300].replace("\n", " ")
                        if log_on_error:
                            self._log.error(
                                "POST %s failed: HTTP %s: %s",
                                path,
                                r.status,
                                txt_one_line,
                            )
                        else:
                            self._log.debug(
                                "POST %s failed (quiet): HTTP %s: %s",
                                path,
                                r.status,
                                txt[:180].replace("\n", " "),
                            )
                        raise aiohttp.ClientResponseError(
                            request_info=r.request_info,
                            history=r.history,
                            status=r.status,
                            message=txt_one_line or (r.reason or ""),
                            headers=r.headers,
                        )
                    return await r.json()
            except RuntimeError as exc:
                if not self._is_closed_session_error(exc):
                    raise
                last_exc = exc
                if attempt < attempts - 1:
                    delay = 0.2 * (attempt + 1)
                    self._log.warning(
                        "POST %s retry %s/%s after closed HTTP session (%ss)",
                        path,
                        attempt + 1,
                        attempts,
                        delay,
                    )
                    self._session = None
                    self._own_session = False
                    await asyncio.sleep(delay)
                    continue
                self._log.error("POST %s failed after retries: closed HTTP session", path)
                raise last_exc
            except aiohttp.ClientResponseError:
                raise
            except (TimeoutError, aiohttp.ClientError, OSError) as exc:
                last_exc = exc
                if attempt < attempts - 1:
                    delay = 0.5 * (attempt + 1)
                    self._log.warning(
                        "POST %s retry %s/%s after %s (%s)",
                        path,
                        attempt + 1,
                        attempts,
                        delay,
                        exc.__class__.__name__,
                    )
                    await asyncio.sleep(delay)
                    continue
                self._log.error("POST %s failed after retries: %s", path, exc)
                raise last_exc
        # Defensive guard to avoid an implicit None on unexpected fallthrough
        raise last_exc or RuntimeError(f"POST {path} failed without raising")

    # ---- Core GET ----------------------------------------------------------
    async def _get(
        self,
        path: str,
        params: dict[str, str] | list[tuple[str, str]] | None = None,
        *,
        log_on_error: bool = True,
    ) -> dict:
        await self._ensure_token()
        url = f"{TWITCH_API_BASE}{path}"
        last_exc: Exception | None = None
        for attempt in range(3):
            self._ensure_session()
            assert self._session is not None
            try:
                async with self._session.get(url, headers=self._headers(), params=params) as r:
                    if r.status != 200:
                        txt = await r.text()
                        if log_on_error:
                            self._log.error(
                                "GET %s failed: HTTP %s: %s",
                                path,
                                r.status,
                                txt[:300].replace("\n", " "),
                            )
                        else:
                            self._log.debug(
                                "GET %s failed (quiet): HTTP %s: %s",
                                path,
                                r.status,
                                txt[:180].replace("\n", " "),
                            )
                        if r.status in {500, 502, 503, 504} and attempt < 2:
                            delay = 0.5 * (attempt + 1)
                            self._log.warning(
                                "GET %s retry %s/3 after HTTP %s (%ss)",
                                path,
                                attempt + 1,
                                r.status,
                                delay,
                            )
                            await asyncio.sleep(delay)
                            continue
                        r.raise_for_status()
                    return await r.json()
            except RuntimeError as exc:
                if not self._is_closed_session_error(exc):
                    raise
                last_exc = exc
                if attempt < 2:
                    delay = 0.2 * (attempt + 1)
                    self._log.warning(
                        "GET %s retry %s/3 after closed HTTP session (%ss)",
                        path,
                        attempt + 1,
                        delay,
                    )
                    self._session = None
                    self._own_session = False
                    await asyncio.sleep(delay)
                    continue
                self._log.error("GET %s failed after retries: closed HTTP session", path)
                raise last_exc
            except aiohttp.ClientResponseError:
                raise
            except (TimeoutError, aiohttp.ClientError, OSError) as exc:
                last_exc = exc
                if attempt < 2:
                    delay = 0.5 * (attempt + 1)
                    self._log.warning(
                        "GET %s retry %s/3 after %s (%s)",
                        path,
                        attempt + 1,
                        delay,
                        exc.__class__.__name__,
                    )
                    await asyncio.sleep(delay)
                    continue
                self._log.error("GET %s failed after retries: %s", path, exc)
                raise last_exc
        # Sollte nie erreicht werden; defensive Absicherung gegen implizites None
        raise last_exc or RuntimeError(f"GET {path} failed without raising")

    # ---- Categories --------------------------------------------------------
    async def search_category_id(self, query: str) -> str | None:
        if not query:
            return None
        ql = query.lower()
        if ql in self._category_cache:
            return self._category_cache[ql]
        js = await self._get("/search/categories", params={"query": query, "first": "25"})
        best: str | None = None
        for item in js.get("data", []) or []:
            name = (item.get("name") or "").strip()
            if name.lower() == ql:
                best = item.get("id")
                break
            if not best and name.lower().startswith(ql):
                best = item.get("id")
        if best:
            self._category_cache[ql] = best
        return best

    async def get_category_id(self, name: str) -> str | None:
        return await self.search_category_id(name)

    # ---- Users & Streams ---------------------------------------------------
    async def get_users(self, logins: list[str]) -> dict[str, dict]:
        out: dict[str, dict] = {}
        if not logins:
            return out
        for i in range(0, len(logins), 100):
            chunk = logins[i : i + 100]
            params: list[tuple[str, str]] = [("login", x) for x in chunk]
            js = await self._get("/users", params=params)
            for u in js.get("data", []) or []:
                login = (u.get("login") or "").lower()
                out[login] = u
        return out

    async def get_user_info(self, login: str) -> dict | None:
        """Liefert detaillierte Informationen für einen einzelnen User (inkl. Bio/Description)."""
        users = await self.get_users([login])
        return users.get(login.lower())

    async def _fetch_stream_page(
        self,
        *,
        game_id: str | None = None,
        language: str | None = None,
        first: int = 100,
        after: str | None = None,
        logins: list[str] | None = None,
    ) -> tuple[list[dict], str | None]:
        params: list[tuple[str, str]] = []
        if game_id:
            params.append(("game_id", game_id))
        if language:
            params.append(("language", language))
        if logins:
            for lg in logins:
                params.append(("user_login", lg))
        params.append(("first", str(max(1, min(first, 100)))))
        if after:
            params.append(("after", after))

        js = await self._get("/streams", params=params)
        data = js.get("data", []) or []
        cursor = (js.get("pagination") or {}).get("cursor")
        return data, cursor

    async def get_streams_for_game(
        self,
        *,
        game_id: str | None,
        game_name: str,
        language: str | None = None,
        limit: int = 500,
    ) -> list[dict]:
        """Fetch up to ``limit`` live streams for the given game.

        Falls die Game-ID unbekannt ist, wird nach ``game_name`` gefiltert.
        """
        limit = max(1, min(limit, 1200))  # hard cap to protect API limits
        out: list[dict] = []
        after: str | None = None

        if game_id:
            while len(out) < limit:
                data, after = await self._fetch_stream_page(
                    game_id=game_id,
                    language=language,
                    first=100,
                    after=after,
                )
                out.extend(data)
                if not after or not data:
                    break
        else:
            # Fallback: ohne game_id viele Streams ziehen und anschließend filtern
            scanned = 0
            after = None
            while scanned < limit:
                data, after = await self._fetch_stream_page(
                    language=language,
                    first=100,
                    after=after,
                )
                if not data:
                    break
                out.extend(data)
                if not after:
                    break
            target = (game_name or "").lower()
            out = [s for s in out if (s.get("game_name") or "").lower() == target]

        if len(out) > limit:
            out = out[:limit]
        return out

    async def get_streams_by_logins(
        self, logins: list[str], language: str | None = None
    ) -> list[dict]:
        """Return live streams for the given user logins.
        Wrapper around Helix /streams with user_login filters (batched).
        """
        if not logins:
            return []
        await self._ensure_token()
        out: list[dict] = []
        for i in range(0, len(logins), 100):
            chunk = [x for x in logins[i : i + 100] if x]
            if not chunk:
                continue
            params: list[tuple[str, str]] = []
            for lg in chunk:
                params.append(("user_login", lg))
            if language:
                params.append(("language", language))
            js = await self._get("/streams", params=params)
            out.extend(js.get("data", []) or [])
        return out

    async def get_streams_by_category(
        self, category_id: str, language: str | None = None, limit: int = 500
    ) -> list[dict]:
        """Return live streams for a given category/game id.
        Convenience wrapper that delegates to get_streams_for_game.
        """
        return await self.get_streams_for_game(
            game_id=category_id, game_name="", language=language, limit=limit
        )

    async def create_clip(
        self,
        broadcaster_id: str,
        *,
        user_token: str,
        title: str | None = None,
        duration: float | int | None = None,
        has_delay: bool = False,
    ) -> dict | None:
        """Create a clip for a broadcaster using a user OAuth token.

        Optional parameters:
        - title: clip title/description (trimmed to 60 chars for safety)
        - duration: requested clip length in seconds (clamped to Twitch limits 15–60s)

        Note: Twitch still determines the final segment from the stream buffer.
        """
        if not broadcaster_id or not user_token:
            return None

        token = (user_token or "").strip()
        if token.lower().startswith("oauth:"):
            token = token.split(":", 1)[1].strip()
        if not token:
            return None

        self._ensure_session()
        assert self._session is not None

        url = f"{TWITCH_API_BASE}/clips"

        cleaned_title = (title or "").strip()
        if cleaned_title:
            max_len = 60
            if len(cleaned_title) > max_len:
                cleaned_title = cleaned_title[: max_len - 3].rstrip() + "..."

        duration_value: float | None = None
        if duration is not None:
            try:
                duration_value = float(duration)
            except (TypeError, ValueError):
                duration_value = None
            if duration_value is not None:
                duration_value = max(15.0, min(60.0, duration_value))

        params = {
            "broadcaster_id": str(broadcaster_id),
            "has_delay": "true" if has_delay else "false",
        }
        if cleaned_title:
            params["title"] = cleaned_title
        if duration_value is not None:
            params["duration"] = f"{duration_value:.1f}".rstrip("0").rstrip(".")
        headers = {
            "Client-ID": self.client_id,
            "Authorization": f"Bearer {token}",
        }

        try:
            async with self._session.post(url, headers=headers, params=params) as r:
                if r.status not in {200, 202}:
                    txt = await r.text()
                    self._log.debug(
                        "POST /clips failed: HTTP %s: %s",
                        r.status,
                        txt[:180].replace("\n", " "),
                    )
                    return None
                js = await r.json()
        except Exception:
            self._log.debug("create_clip failed for broadcaster=%s", broadcaster_id, exc_info=True)
            return None

        data = js.get("data", []) if isinstance(js, dict) else []
        if not data or not isinstance(data[0], dict):
            return None
        return data[0]

    async def get_latest_vod_thumbnail(
        self, *, user_id: str | None = None, login: str | None = None
    ) -> str | None:
        """Best-effort: Thumbnail des neuesten VOD (type=archive) als 1280x720-URL."""
        target_user_id = (user_id or "").strip()
        login_normalized = (login or "").strip().lower()

        # Falls f�lschlich ein Login als user_id gespeichert wurde (z.B. aus DB-Fallback),
        # behandle ihn wie einen Login und ermittle die echte numerische ID.
        if target_user_id and not target_user_id.isdigit():
            if not login_normalized:
                login_normalized = target_user_id.lower()
            target_user_id = ""

        if not target_user_id and login_normalized:
            try:
                users = await self.get_users([login_normalized])
                if login_normalized in users:
                    target_user_id = str(users[login_normalized].get("id") or "").strip()
            except Exception:
                self._log.exception(
                    "get_latest_vod_thumbnail: konnte user-id nicht ermitteln (%s)",
                    login_normalized,
                )
                return None

        if not target_user_id:
            return None

        try:
            js = await self._get(
                "/videos",
                params={"user_id": target_user_id, "type": "archive", "first": "1"},
            )
        except Exception:
            self._log.exception(
                "get_latest_vod_thumbnail: API-Fehler fuer %s",
                login_normalized or target_user_id,
            )
            return None

        first = js.get("data", []) or []
        if not first:
            return None
        thumb = (first[0].get("thumbnail_url") or "").strip()
        if not thumb:
            return None
        thumb = thumb.replace("{width}", "1280").replace("{height}", "720")
        return f"{thumb}?rand={int(time.time())}"

    async def get_followers_total_result(
        self,
        user_id: str,
        user_token: str | None = None,
    ) -> dict[str, object]:
        """Structured result for /channels/followers."""
        if not user_id:
            return self._helix_result(
                ok=False,
                error_code="missing_user_id",
                message="broadcaster_id is required",
            )
        try:
            if user_token:
                token = self._normalize_bearer_token(user_token)
                if not token:
                    return self._helix_result(
                        ok=False,
                        error_code="missing_user_token",
                        message="user token is required",
                    )
                self._ensure_session()
                assert self._session is not None
                url = f"{TWITCH_API_BASE}/channels/followers"
                async with self._session.get(
                    url,
                    headers={
                        "Client-ID": self.client_id,
                        "Authorization": f"Bearer {token}",
                    },
                    params={"broadcaster_id": user_id, "first": "1"},
                ) as r:
                    if r.status != 200:
                        txt = await r.text()
                        self._log.debug(
                            "GET /channels/followers (user) failed: HTTP %s: %s",
                            r.status,
                            txt[:180].replace("\n", " "),
                        )
                        return self._helix_result(
                            ok=False,
                            http_status=r.status,
                            error_code=self._map_helix_error_code(
                                http_status=r.status,
                                message=txt,
                                default="helix_followers_failed",
                            ),
                            message=txt,
                            request_attempted=True,
                        )
                    js = await r.json()
            else:
                try:
                    js = await self._get(
                        "/channels/followers",
                        params={"broadcaster_id": user_id, "first": "1"},
                        log_on_error=False,
                    )
                except aiohttp.ClientResponseError as exc:
                    return self._helix_result(
                        ok=False,
                        http_status=exc.status,
                        error_code=self._map_helix_error_code(
                            http_status=exc.status,
                            message=exc.message,
                            default="helix_followers_failed",
                        ),
                        message=exc.message,
                        request_attempted=True,
                    )
            if not isinstance(js, dict):
                return self._helix_result(
                    ok=False,
                    error_code="invalid_response",
                    message="followers response was not a JSON object",
                    request_attempted=True,
                )
            total = js.get("total")
            parsed_total = int(total) if total is not None else None
            return self._helix_result(
                ok=parsed_total is not None,
                data=parsed_total,
                http_status=200,
                error_code=None if parsed_total is not None else "missing_total",
                message=None if parsed_total is not None else "followers response did not contain total",
                request_attempted=True,
            )
        except asyncio.TimeoutError:
            return self._helix_result(
                ok=False,
                error_code="helix_timeout",
                message="followers request timed out",
                request_attempted=True,
            )
        except Exception:
            self._log.debug("get_followers_total_result failed for %s", user_id, exc_info=True)
            return self._helix_result(
                ok=False,
                error_code="helix_exception",
                message="followers request raised an exception",
                request_attempted=True,
            )

    async def get_followers_total(self, user_id: str, user_token: str | None = None) -> int | None:
        """Liefert die Follower-Gesamtzahl für einen Broadcaster (best-effort, via /channels/followers)."""
        result = await self.get_followers_total_result(user_id, user_token=user_token)
        return int(result["data"]) if result.get("ok") and result.get("data") is not None else None

    async def get_broadcaster_subscriptions_result(
        self,
        user_id: str,
        user_token: str,
    ) -> dict[str, object]:
        """
        Structured result for /subscriptions.
        Benötigt Scope: channel:read:subscriptions
        """
        if not user_id or not user_token:
            return self._helix_result(
                ok=False,
                error_code="missing_credentials",
                message="user_id and user_token are required",
            )
        try:
            token = self._normalize_bearer_token(user_token)
            if not token:
                return self._helix_result(
                    ok=False,
                    error_code="missing_user_token",
                    message="user token is required",
                )
            self._ensure_session()
            assert self._session is not None
            url = f"{TWITCH_API_BASE}/subscriptions"
            async with self._session.get(
                url,
                headers={
                    "Client-ID": self.client_id,
                    "Authorization": f"Bearer {token}",
                },
                params={"broadcaster_id": user_id, "first": "1"},
            ) as r:
                if r.status != 200:
                    txt = await r.text()
                    self._log.debug(
                        "GET /subscriptions failed: HTTP %s: %s",
                        r.status,
                        txt[:180].replace("\n", " "),
                    )
                    return self._helix_result(
                        ok=False,
                        http_status=r.status,
                        error_code=self._map_helix_error_code(
                            http_status=r.status,
                            message=txt,
                            default="helix_subscriptions_failed",
                        ),
                        message=txt,
                        request_attempted=True,
                    )
                js = await r.json()
                if not isinstance(js, dict):
                    return self._helix_result(
                        ok=False,
                        error_code="invalid_response",
                        message="subscriptions response was not a JSON object",
                        request_attempted=True,
                    )
                return self._helix_result(
                    ok=True,
                    data=js,
                    http_status=200,
                    request_attempted=True,
                )
        except asyncio.TimeoutError:
            return self._helix_result(
                ok=False,
                error_code="helix_timeout",
                message="subscriptions request timed out",
                request_attempted=True,
            )
        except Exception:
            self._log.debug(
                "get_broadcaster_subscriptions_result failed for %s",
                user_id,
                exc_info=True,
            )
            return self._helix_result(
                ok=False,
                error_code="helix_exception",
                message="subscriptions request raised an exception",
                request_attempted=True,
            )

    async def get_broadcaster_subscriptions(self, user_id: str, user_token: str) -> dict | None:
        result = await self.get_broadcaster_subscriptions_result(user_id, user_token)
        data = result.get("data")
        return data if result.get("ok") and isinstance(data, dict) else None

    async def get_ad_schedule_result(self, user_id: str, user_token: str) -> dict[str, object]:
        """
        Structured result for /channels/ads.
        Benötigt Scope: channel:read:ads
        """
        if not user_id or not user_token:
            return self._helix_result(
                ok=False,
                error_code="missing_credentials",
                message="user_id and user_token are required",
            )
        try:
            token = self._normalize_bearer_token(user_token)
            if not token:
                return self._helix_result(
                    ok=False,
                    error_code="missing_user_token",
                    message="user token is required",
                )
            self._ensure_session()
            assert self._session is not None
            url = f"{TWITCH_API_BASE}/channels/ads"
            async with self._session.get(
                url,
                headers={
                    "Client-ID": self.client_id,
                    "Authorization": f"Bearer {token}",
                },
                params={"broadcaster_id": user_id},
            ) as r:
                if r.status != 200:
                    txt = await r.text()
                    self._log.debug(
                        "GET /channels/ads failed: HTTP %s: %s",
                        r.status,
                        txt[:180].replace("\n", " "),
                    )
                    return self._helix_result(
                        ok=False,
                        http_status=r.status,
                        error_code=self._map_helix_error_code(
                            http_status=r.status,
                            message=txt,
                            default="helix_ads_failed",
                        ),
                        message=txt,
                        request_attempted=True,
                    )
                js = await r.json()
                data = js.get("data", []) or []
                if not data:
                    return self._helix_result(
                        ok=False,
                        http_status=200,
                        error_code="empty_data",
                        message="ads response did not contain data",
                        request_attempted=True,
                    )
                first = data[0]
                if not isinstance(first, dict):
                    return self._helix_result(
                        ok=False,
                        http_status=200,
                        error_code="invalid_response",
                        message="ads response contained an invalid data item",
                        request_attempted=True,
                    )
                return self._helix_result(
                    ok=True,
                    data=first,
                    http_status=200,
                    request_attempted=True,
                )
        except asyncio.TimeoutError:
            return self._helix_result(
                ok=False,
                error_code="helix_timeout",
                message="ads request timed out",
                request_attempted=True,
            )
        except Exception:
            self._log.debug("get_ad_schedule_result failed for %s", user_id, exc_info=True)
            return self._helix_result(
                ok=False,
                error_code="helix_exception",
                message="ads request raised an exception",
                request_attempted=True,
            )

    async def get_ad_schedule(self, user_id: str, user_token: str) -> dict | None:
        result = await self.get_ad_schedule_result(user_id, user_token)
        data = result.get("data")
        return data if result.get("ok") and isinstance(data, dict) else None

    async def get_chatters_result(
        self,
        broadcaster_id: str,
        moderator_id: str,
        user_token: str,
        first: int = 1000,
    ) -> dict[str, object]:
        """
        Structured result for /chat/chatters.
        Benötigt Scope: moderator:read:chatters
        broadcaster_id == moderator_id wenn der Streamer selbst seinen Chat abfragt.
        """
        if not broadcaster_id or not moderator_id or not user_token:
            return self._helix_result(
                ok=False,
                error_code="missing_credentials",
                message="broadcaster_id, moderator_id and user_token are required",
            )
        all_chatters: list[dict] = []
        cursor: str | None = None
        try:
            token = self._normalize_bearer_token(user_token)
            if not token:
                return self._helix_result(
                    ok=False,
                    error_code="missing_user_token",
                    message="user token is required",
                )
            self._ensure_session()
            assert self._session is not None
            url = f"{TWITCH_API_BASE}/chat/chatters"
            while True:
                params: dict[str, str] = {
                    "broadcaster_id": broadcaster_id,
                    "moderator_id": moderator_id,
                    "first": str(min(first, 1000)),
                }
                if cursor:
                    params["after"] = cursor
                async with self._session.get(
                    url,
                    headers={
                        "Client-ID": self.client_id,
                        "Authorization": f"Bearer {token}",
                    },
                    params=params,
                ) as r:
                    if r.status != 200:
                        txt = await r.text()
                        self._log.debug(
                            "GET /chat/chatters failed: HTTP %s: %s",
                            r.status,
                            txt[:180].replace("\n", " "),
                        )
                        return self._helix_result(
                            ok=False,
                            http_status=r.status,
                            error_code=self._map_helix_error_code(
                                http_status=r.status,
                                message=txt,
                                default="helix_chatters_failed",
                            ),
                            message=txt,
                            request_attempted=True,
                        )
                    js = await r.json()
                    page = js.get("data") or []
                    all_chatters.extend(page)
                    cursor = (js.get("pagination") or {}).get("cursor")
                    if not cursor or not page:
                        break
        except asyncio.TimeoutError:
            return self._helix_result(
                ok=False,
                error_code="helix_timeout",
                message="chatters request timed out",
                request_attempted=True,
            )
        except Exception:
            self._log.debug("get_chatters_result failed for broadcaster %s", broadcaster_id, exc_info=True)
            return self._helix_result(
                ok=False,
                error_code="helix_exception",
                message="chatters request raised an exception",
                request_attempted=True,
            )
        return self._helix_result(
            ok=True,
            data=all_chatters,
            http_status=200,
            request_attempted=True,
        )

    async def get_chatters(
        self,
        broadcaster_id: str,
        moderator_id: str,
        user_token: str,
        first: int = 1000,
    ) -> list[dict] | None:
        result = await self.get_chatters_result(
            broadcaster_id=broadcaster_id,
            moderator_id=moderator_id,
            user_token=user_token,
            first=first,
        )
        data = result.get("data")
        return data if result.get("ok") and isinstance(data, list) else None

    async def subscribe_eventsub_websocket(
        self,
        *,
        session_id: str,
        sub_type: str,
        condition: dict[str, str],
        version: str = "1",
        oauth_token: str | None = None,
    ) -> dict:
        """Register a WebSocket EventSub subscription (e.g. stream.offline)."""
        payload = {
            "type": sub_type,
            "version": version,
            "condition": condition,
            "transport": {"method": "websocket", "session_id": session_id},
        }
        return await self._post(
            "/eventsub/subscriptions",
            json=payload,
            oauth_token=oauth_token,
            max_attempts=1,
            request_timeout_total=8.0,
        )

    async def subscribe_eventsub_webhook(
        self,
        *,
        sub_type: str,
        condition: dict[str, str],
        webhook_url: str,
        secret: str,
        version: str = "1",
        oauth_token: str | None = None,
    ) -> dict:
        """Registriert eine Webhook-basierte EventSub Subscription."""
        payload = {
            "type": sub_type,
            "version": version,
            "condition": condition,
            "transport": {
                "method": "webhook",
                "callback": webhook_url,
                "secret": secret,
            },
        }
        try:
            return await self._post(
                "/eventsub/subscriptions",
                json=payload,
                log_on_error=False,
                oauth_token=oauth_token,
                max_attempts=1,
                request_timeout_total=10.0,
            )
        except aiohttp.ClientResponseError as exc:
            if exc.status == 409:
                self._log.debug(
                    "POST /eventsub/subscriptions: subscription already exists (409), treating as success"
                )
                return {"already_exists": True}
            raise

    async def delete_eventsub_subscription(
        self, subscription_id: str, oauth_token: str | None = None
    ) -> bool:
        """Löscht eine EventSub Subscription per ID."""
        await self._ensure_token()
        self._ensure_session()
        assert self._session is not None

        token_override = (oauth_token or "").strip()
        if token_override.lower().startswith("oauth:"):
            token_override = token_override.split(":", 1)[1]
        if not token_override:
            token_override = self._token or ""

        url = f"{TWITCH_API_BASE}/eventsub/subscriptions"
        headers = {
            "Client-ID": self.client_id,
            "Authorization": f"Bearer {token_override}",
        }
        try:
            async with self._session.delete(
                url, headers=headers, params={"id": subscription_id}
            ) as r:
                if r.status == 204:
                    return True
                txt = await r.text()
                self._log.warning(
                    "DELETE /eventsub/subscriptions?id=%s: HTTP %s: %s",
                    subscription_id,
                    r.status,
                    txt[:200],
                )
                return False
        except Exception as exc:
            self._log.error(
                "delete_eventsub_subscription(%s) fehlgeschlagen: %s",
                subscription_id,
                exc,
            )
            return False

    async def list_eventsub_subscriptions(
        self,
        *,
        status: str = "enabled",
        oauth_token: str | None = None,
    ) -> list[dict]:
        """Listet aktive EventSub Subscriptions (paginiert)."""
        await self._ensure_token()
        self._ensure_session()
        assert self._session is not None

        token_override = (oauth_token or "").strip()
        if token_override.lower().startswith("oauth:"):
            token_override = token_override.split(":", 1)[1]
        if not token_override:
            token_override = self._token or ""

        headers = {
            "Client-ID": self.client_id,
            "Authorization": f"Bearer {token_override}",
        }
        url = f"{TWITCH_API_BASE}/eventsub/subscriptions"
        results: list[dict] = []
        cursor: str | None = None

        for _ in range(20):  # Max 20 Seiten (Schutz vor Endlosschleife)
            params: list[tuple[str, str]] = []
            if status:
                params.append(("status", status))
            if cursor:
                params.append(("after", cursor))
            try:
                async with self._session.get(url, headers=headers, params=params or None) as r:
                    if r.status != 200:
                        txt = await r.text()
                        self._log.warning(
                            "GET /eventsub/subscriptions: HTTP %s: %s",
                            r.status,
                            txt[:200],
                        )
                        return []
                    js = await r.json()
            except Exception as exc:
                self._log.error("list_eventsub_subscriptions fehlgeschlagen: %s", exc)
                return []

            if not isinstance(js, dict):
                self._log.warning(
                    "GET /eventsub/subscriptions: invalid response payload type %s",
                    type(js).__name__,
                )
                return []

            data = js.get("data") or []
            if not isinstance(data, list):
                self._log.warning(
                    "GET /eventsub/subscriptions: invalid data payload type %s",
                    type(data).__name__,
                )
                return []
            results.extend(data)
            pagination = js.get("pagination") or {}
            cursor = pagination.get("cursor")
            if not cursor:
                break

        return results
