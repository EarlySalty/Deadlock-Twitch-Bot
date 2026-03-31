"""Standalone dashboard service app that forwards bot operations via internal API."""

from __future__ import annotations

import asyncio
import os
from typing import Any

from aiohttp import web

from ..app_keys import (
    ANALYTICS_DB_FINGERPRINT_DETAILS_KEY,
    ANALYTICS_DB_FINGERPRINT_ERROR_KEY,
    ANALYTICS_DB_FINGERPRINT_KEY,
    ANALYTICS_DB_FINGERPRINT_MISMATCH_KEY,
    BOT_API_CLIENT_KEY,
    INTERNAL_API_ANALYTICS_DB_FINGERPRINT_KEY,
)
from ..core.constants import (
    TWITCH_DASHBOARD_HOST,
    TWITCH_DASHBOARD_NOAUTH,
    TWITCH_DASHBOARD_PORT,
    TWITCH_INTERNAL_API_HOST,
    TWITCH_INTERNAL_API_PORT,
    log,
)
from ..dashboard.server_v2 import build_v2_app
from ..runtime.dashboard_runtime import DashboardRuntimeServices
from ..runtime_lock import runtime_pid_lock
from ..runtime_mode import (
    INTERNAL_API_PORT as RUNTIME_INTERNAL_API_PORT,
    enforce_dashboard_service_runtime,
)
from ..runtime_security import require_noauth_loopback_guard
from ..secret_store import load_secret_value
from ..storage import analytics_db_fingerprint_details
from .client import BotApiClient, BotApiClientError
from .eventsub_bridge import DashboardEventSubBridgeRuntime

DASHBOARD_EVENTSUB_BRIDGE_KEY = web.AppKey(
    "dashboard_eventsub_bridge",
    DashboardEventSubBridgeRuntime,
)


def _parse_env_bool(name: str, default: bool) -> bool:
    raw = (os.getenv(name) or "").strip().lower()
    if not raw:
        return default
    if raw in {"1", "true", "yes", "on"}:
        return True
    if raw in {"0", "false", "no", "off"}:
        return False
    return default


def _require_noauth_opt_in_if_enabled(*, enabled: bool) -> None:
    if not enabled:
        return
    if _parse_env_bool("TWITCH_ALLOW_DASHBOARD_NOAUTH", False):
        return
    raise RuntimeError(
        "Refusing to start dashboard with no-auth enabled. "
        "Set TWITCH_ALLOW_DASHBOARD_NOAUTH=1 only for controlled local debugging."
    )


def _parse_env_int(name: str, default: int) -> int:
    raw = (os.getenv(name) or "").strip()
    if not raw:
        return default
    try:
        return int(raw)
    except ValueError:
        return default


def _parse_env_float(name: str, default: float) -> float:
    raw = (os.getenv(name) or "").strip()
    if not raw:
        return default
    try:
        return float(raw)
    except ValueError:
        return default


def _dashboard_host_setting() -> str:
    return (
        os.getenv("TWITCH_DASHBOARD_HOST") or TWITCH_DASHBOARD_HOST or "127.0.0.1"
    ).strip()

def _default_internal_api_base_url() -> str:
    explicit = (os.getenv("TWITCH_INTERNAL_API_BASE_URL") or "").strip()
    if explicit:
        return explicit
    host = (
        os.getenv("TWITCH_INTERNAL_API_HOST") or TWITCH_INTERNAL_API_HOST or "127.0.0.1"
    ).strip()
    port = _parse_env_int(
        "TWITCH_INTERNAL_API_PORT",
        int(TWITCH_INTERNAL_API_PORT or RUNTIME_INTERNAL_API_PORT),
    )
    return f"http://{host}:{port}"


def build_dashboard_service_app(
    *,
    internal_api_base_url: str | None = None,
    internal_api_token: str | None = None,
    internal_api_allow_non_loopback: bool | None = None,
    internal_api_timeout_seconds: float | None = None,
    dashboard_token: str | None = None,
    partner_token: str | None = None,
    noauth: bool | None = None,
    oauth_client_id: str | None = None,
    oauth_client_secret: str | None = None,
    oauth_redirect_uri: str | None = None,
    session_ttl_seconds: int | None = None,
    legacy_stats_url: str | None = None,
) -> web.Application:
    """Build the standalone dashboard app and wire callbacks through `BotApiClient`."""

    resolved_noauth = (
        bool(noauth)
        if noauth is not None
        else _parse_env_bool("TWITCH_DASHBOARD_NOAUTH", bool(TWITCH_DASHBOARD_NOAUTH))
    )
    _require_noauth_opt_in_if_enabled(enabled=resolved_noauth)
    require_noauth_loopback_guard(enabled=resolved_noauth, host=_dashboard_host_setting())

    resolved_internal_base = (
        internal_api_base_url or _default_internal_api_base_url()
    ).strip()
    resolved_internal_token = (
        internal_api_token
        if internal_api_token is not None
        else load_secret_value(
            "TWITCH_INTERNAL_API_TOKEN",
            prefer_env=True,
            allow_empty_env_override=True,
        )
        or None
    )
    timeout_seconds = (
        float(internal_api_timeout_seconds)
        if internal_api_timeout_seconds is not None
        else _parse_env_float("TWITCH_INTERNAL_API_TIMEOUT_SEC", 10.0)
    )
    allow_non_loopback = (
        bool(internal_api_allow_non_loopback)
        if internal_api_allow_non_loopback is not None
        else _parse_env_bool("TWITCH_INTERNAL_API_ALLOW_NON_LOOPBACK", False)
    )
    resolved_dashboard_token = (
        dashboard_token
        if dashboard_token is not None
        else load_secret_value("TWITCH_DASHBOARD_TOKEN") or None
    )
    resolved_partner_token = (
        partner_token
        if partner_token is not None
        else load_secret_value("TWITCH_PARTNER_TOKEN") or None
    )
    resolved_oauth_client_id = (
        oauth_client_id
        if oauth_client_id is not None
        else load_secret_value("TWITCH_CLIENT_ID") or None
    )
    resolved_oauth_client_secret = (
        oauth_client_secret
        if oauth_client_secret is not None
        else load_secret_value("TWITCH_CLIENT_SECRET") or None
    )
    resolved_oauth_redirect_uri = (
        oauth_redirect_uri
        if oauth_redirect_uri is not None
        else (os.getenv("TWITCH_DASHBOARD_AUTH_REDIRECT_URI") or "").strip()
        or "https://twitch.earlysalty.com/twitch/auth/callback"
    )
    resolved_session_ttl = (
        int(session_ttl_seconds)
        if session_ttl_seconds is not None
        else max(6 * 3600, _parse_env_int("TWITCH_DASHBOARD_SESSION_TTL_SEC", 6 * 3600))
    )
    resolved_legacy_stats_url = (
        legacy_stats_url
        if legacy_stats_url is not None
        else (os.getenv("TWITCH_LEGACY_STATS_URL") or "").strip() or None
    )
    eventsub_webhook_handler = None
    webhook_secret = load_secret_value("TWITCH_WEBHOOK_SECRET") or None
    if webhook_secret:
        try:
            from ..monitoring.eventsub_webhook import EventSubWebhookHandler
            from ..monitoring.eventsub_state_store import EventSubStateStore

            eventsub_webhook_handler = EventSubWebhookHandler(
                secret=webhook_secret,
                logger=log,
                synchronous_notifications=True,
                state_store=EventSubStateStore(logger=log),
            )
        except Exception as exc:
            log.warning(
                "Dashboard service could not initialize EventSub webhook handler: %s",
                exc,
            )
    else:
        log.warning(
            "Dashboard service degraded startup: TWITCH_WEBHOOK_SECRET missing; "
            "EventSub webhook callback will be unavailable."
        )
    local_analytics_db = analytics_db_fingerprint_details()
    local_analytics_fingerprint = str(local_analytics_db.get("fingerprint") or "").strip() or None
    log.info(
        "Dashboard service analytics DB fingerprint=%s host_hash=%s db_hash=%s port_hash=%s",
        local_analytics_db.get("fingerprint"),
        local_analytics_db.get("hostHash"),
        local_analytics_db.get("databaseHash"),
        local_analytics_db.get("portHash"),
    )
    degraded_startup_reasons: list[str] = []
    if not resolved_internal_token:
        degraded_startup_reasons.append(
            "TWITCH_INTERNAL_API_TOKEN missing; dashboard will run in degraded upstream mode."
        )
    if not resolved_noauth and (
        not resolved_oauth_client_id or not resolved_oauth_client_secret
    ):
        degraded_startup_reasons.append(
            "TWITCH_CLIENT_ID/TWITCH_CLIENT_SECRET missing; Twitch OAuth login will return 503."
        )
    for reason in degraded_startup_reasons:
        log.warning("Dashboard service degraded startup: %s", reason)

    client: BotApiClient | None = None
    if resolved_internal_token:
        try:
            client = BotApiClient(
                base_url=resolved_internal_base,
                token=resolved_internal_token,
                allow_non_loopback=allow_non_loopback,
                timeout_seconds=timeout_seconds,
            )
        except ValueError as exc:
            log.warning(
                "Dashboard service degraded startup: invalid internal API config (%s). "
                "Dependent actions will report upstream_unavailable.",
                exc,
            )

    upstream_warning_emitted = False

    def _warn_upstream_once(context: str, exc: Exception) -> None:
        nonlocal upstream_warning_emitted
        if upstream_warning_emitted:
            return
        upstream_warning_emitted = True
        log.warning(
            "Dashboard internal API unavailable (degraded mode). First failure in %s: %s",
            context,
            exc,
        )

    def _upstream_unavailable_error(
        context: str,
        exc: BotApiClientError | None = None,
    ) -> BotApiClientError:
        if exc is not None:
            _warn_upstream_once(context, exc)
        elif client is None:
            _warn_upstream_once(
                context,
                BotApiClientError(
                    status=503,
                    code="upstream_unavailable",
                    message="Bot internal API is unavailable.",
                ),
            )
        return BotApiClientError(
            status=503,
            code="upstream_unavailable",
            message="Bot internal API is unavailable.",
        )

    def _is_upstream_failure(exc: BotApiClientError) -> bool:
        status = int(getattr(exc, "status", 0) or 0)
        code = str(getattr(exc, "code", "") or "").strip().lower()
        return status >= 500 or code.startswith("upstream_")

    def _upstream_service_unavailable(
        context: str,
        exc: Exception | None = None,
    ) -> web.HTTPServiceUnavailable:
        if exc is not None:
            _warn_upstream_once(context, exc)
        elif client is None:
            _warn_upstream_once(
                context,
                BotApiClientError(
                    status=503,
                    code="upstream_unavailable",
                    message="Bot internal API is unavailable.",
                ),
            )
        return web.HTTPServiceUnavailable(text="Bot internal API is unavailable.")

    async def _add_cb(login: str, require_link: bool) -> str:
        if client is None:
            raise _upstream_unavailable_error("streamer_add")
        try:
            return await client.add_streamer(login, require_link=require_link)
        except BotApiClientError as exc:
            if not _is_upstream_failure(exc):
                raise
            raise _upstream_unavailable_error("streamer_add", exc) from exc

    async def _remove_cb(login: str) -> str:
        if client is None:
            raise _upstream_unavailable_error("streamer_remove")
        try:
            return await client.remove_streamer(login)
        except BotApiClientError as exc:
            if not _is_upstream_failure(exc):
                raise
            raise _upstream_unavailable_error("streamer_remove", exc) from exc

    async def _list_cb() -> list[dict[str, Any]]:
        if client is None:
            raise _upstream_service_unavailable("streamers_list")
        try:
            return await client.get_streamers()
        except BotApiClientError as exc:
            raise _upstream_service_unavailable("streamers_list", exc) from exc

    async def _stats_cb(**kwargs: Any) -> dict[str, Any]:
        if client is None:
            raise _upstream_service_unavailable("stats")
        try:
            return await client.get_stats(
                hour_from=kwargs.get("hour_from"),
                hour_to=kwargs.get("hour_to"),
                streamer=kwargs.get("streamer"),
            )
        except BotApiClientError as exc:
            raise _upstream_service_unavailable("stats", exc) from exc

    async def _verify_cb(login: str, mode: str) -> str:
        if client is None:
            raise _upstream_unavailable_error("streamer_verify")
        try:
            return await client.verify_streamer(login, mode=mode)
        except BotApiClientError as exc:
            if not _is_upstream_failure(exc):
                raise
            raise _upstream_unavailable_error("streamer_verify", exc) from exc

    async def _archive_cb(login: str, mode: str) -> str:
        if client is None:
            raise _upstream_unavailable_error("streamer_archive")
        try:
            return await client.archive_streamer(login, mode=mode)
        except BotApiClientError as exc:
            if not _is_upstream_failure(exc):
                raise
            raise _upstream_unavailable_error("streamer_archive", exc) from exc

    async def _discord_flag_cb(login: str, is_on_discord: bool) -> str:
        if client is None:
            raise _upstream_unavailable_error("discord_flag")
        try:
            return await client.set_discord_flag(login, is_on_discord=is_on_discord)
        except BotApiClientError as exc:
            if not _is_upstream_failure(exc):
                raise
            raise _upstream_unavailable_error("discord_flag", exc) from exc

    async def _discord_profile_cb(
        login: str,
        discord_user_id: str | None,
        discord_display_name: str | None,
        mark_member: bool,
    ) -> str:
        if client is None:
            raise _upstream_unavailable_error("discord_profile")
        try:
            return await client.save_discord_profile(
                login,
                discord_user_id=discord_user_id,
                discord_display_name=discord_display_name,
                mark_member=mark_member,
            )
        except BotApiClientError as exc:
            if not _is_upstream_failure(exc):
                raise
            raise _upstream_unavailable_error("discord_profile", exc) from exc

    async def _raid_auth_url_cb(
        login: str,
        discord_user_id: str | None = None,
        scope_profile: str | None = None,
    ) -> str:
        if client is None:
            return ""
        try:
            return await client.get_raid_auth_url(
                login,
                discord_user_id=discord_user_id,
                scope_profile=scope_profile,
            )
        except BotApiClientError as exc:
            _warn_upstream_once("raid_auth_url", exc)
            return ""

    async def _raid_go_url_cb(state: str) -> str | None:
        if client is None:
            raise _upstream_unavailable_error("raid_go_url")
        try:
            return await client.get_raid_go_url(state)
        except BotApiClientError as exc:
            _warn_upstream_once("raid_go_url", exc)
            raise _upstream_unavailable_error("raid_go_url", exc) from exc

    async def _raid_requirements_cb(login: str) -> str:
        if client is None:
            raise _upstream_unavailable_error("raid_requirements")
        try:
            return await client.send_raid_requirements(login)
        except BotApiClientError as exc:
            _warn_upstream_once("raid_requirements", exc)
            raise _upstream_unavailable_error("raid_requirements", exc) from exc

    async def _raid_oauth_callback_cb(
        *, code: str, state: str, error: str
    ) -> dict[str, Any]:
        if client is None:
            return {
                "status": 503,
                "title": "Twitch OAuth nicht verfügbar",
                "body_html": "<p>Der interne Bot-Service ist aktuell nicht verfügbar.</p>",
            }
        try:
            return await client.process_raid_oauth_callback(
                code=code, state=state, error=error
            )
        except BotApiClientError as exc:
            _warn_upstream_once("raid_oauth_callback", exc)
            return {
                "status": 503,
                "title": "Twitch OAuth nicht verfügbar",
                "body_html": "<p>Der interne Bot-Service ist aktuell nicht verfügbar.</p>",
            }

    eventsub_bridge: DashboardEventSubBridgeRuntime | None = None
    if eventsub_webhook_handler is not None:
        if client is not None:
            eventsub_bridge = DashboardEventSubBridgeRuntime(
                client=client,
                logger=log,
            )
        bridged_eventsub_types = (
            "stream.online",
            "stream.offline",
            "channel.follow",
            "channel.raid",
            "channel.update",
            "channel.subscribe",
            "channel.subscription.gift",
            "channel.subscription.message",
            "channel.ad_break.begin",
            "channel.cheer",
            "channel.hype_train.begin",
            "channel.hype_train.end",
            "channel.hype_train.progress",
            "channel.subscription.end",
            "channel.ban",
            "channel.unban",
            "channel.bits.use",
            "channel.shoutout.create",
            "channel.shoutout.receive",
            "channel.channel_points_automatic_reward_redemption.add",
            "channel.channel_points_custom_reward_redemption.add",
        )

        def _build_forwarded_eventsub_condition(
            sub_type: str,
            *,
            subscription: dict | None,
            event: dict,
            broadcaster_id: str,
        ) -> dict[str, str]:
            subscription_map = subscription if isinstance(subscription, dict) else {}
            raw_condition = (
                subscription_map.get("condition")
                if isinstance(subscription_map.get("condition"), dict)
                else {}
            )
            normalized_condition = {
                str(key): str(value)
                for key, value in raw_condition.items()
                if str(key).strip() and str(value).strip()
            }
            if normalized_condition:
                return normalized_condition

            if sub_type == "channel.raid":
                to_broadcaster_id = str(
                    event.get("to_broadcaster_user_id") or broadcaster_id or ""
                ).strip()
                if to_broadcaster_id:
                    return {"to_broadcaster_user_id": to_broadcaster_id}

            fallback_keys = (
                "broadcaster_user_id",
                "to_broadcaster_user_id",
                "moderator_user_id",
                "user_id",
            )
            for key in fallback_keys:
                value = str(event.get(key) or "").strip()
                if value:
                    normalized_condition[key] = value

            if not normalized_condition and broadcaster_id:
                normalized_condition["broadcaster_user_id"] = broadcaster_id
            return normalized_condition

        async def _forward_eventsub_notification(
            sub_type: str,
            broadcaster_id: str,
            broadcaster_login: str,
            event: dict,
            *,
            message_id: str | None = None,
            subscription: dict | None = None,
        ) -> None:
            if client is None or eventsub_bridge is None:
                error_message = (
                    "Dashboard service EventSub bridge unavailable for "
                    f"{sub_type} broadcaster={broadcaster_login or broadcaster_id}"
                )
                log.warning("%s", error_message)
                raise RuntimeError(error_message)

            normalized_event = dict(event or {})
            normalized_broadcaster_id = str(broadcaster_id or "").strip()
            normalized_broadcaster_login = str(broadcaster_login or "").strip().lower()
            normalized_message_id = str(message_id or "").strip() or None
            if normalized_broadcaster_id and not any(
                str(normalized_event.get(key) or "").strip()
                for key in (
                    "broadcaster_user_id",
                    "to_broadcaster_user_id",
                    "user_id",
                )
            ):
                normalized_event["broadcaster_user_id"] = normalized_broadcaster_id
            if normalized_broadcaster_login and not any(
                str(normalized_event.get(key) or "").strip()
                for key in (
                    "broadcaster_user_login",
                    "to_broadcaster_user_login",
                    "user_login",
                )
            ):
                normalized_event["broadcaster_user_login"] = normalized_broadcaster_login

            envelope = {
                "subscription": {
                    "type": sub_type,
                    "condition": _build_forwarded_eventsub_condition(
                        sub_type,
                        subscription=subscription,
                        event=normalized_event,
                        broadcaster_id=normalized_broadcaster_id,
                    ),
                },
                "event": normalized_event,
            }
            try:
                await eventsub_bridge.dispatch_or_enqueue(
                    sub_type=sub_type,
                    payload=envelope,
                    message_id=normalized_message_id,
                )
            except Exception:
                log.exception(
                    "Dashboard service EventSub bridge failed for %s broadcaster=%s msg_id=%s",
                    sub_type,
                    normalized_broadcaster_login or normalized_broadcaster_id,
                    normalized_message_id or "n/a",
                )
                raise

        for bridged_sub_type in bridged_eventsub_types:
            async def _bridge_callback(
                bid: str,
                login: str,
                event: dict,
                *,
                _sub_type: str = bridged_sub_type,
                message_id: str | None = None,
                subscription: dict | None = None,
            ) -> None:
                await _forward_eventsub_notification(
                    _sub_type,
                    bid,
                    login,
                    event,
                    message_id=message_id,
                    subscription=subscription,
                )

            eventsub_webhook_handler.set_callback(bridged_sub_type, _bridge_callback)

        activate_dispatch = getattr(eventsub_webhook_handler, "activate_notification_dispatch", None)
        if callable(activate_dispatch):
            activate_dispatch()

        log.info(
            "Dashboard service EventSub bridge enabled for %d subscription types",
            len(bridged_eventsub_types),
        )

    dashboard_services = DashboardRuntimeServices(
        add_cb=_add_cb,
        remove_cb=_remove_cb,
        list_cb=_list_cb,
        stats_cb=_stats_cb,
        verify_cb=_verify_cb,
        archive_cb=_archive_cb,
        discord_flag_cb=_discord_flag_cb,
        discord_profile_cb=_discord_profile_cb,
        raid_auth_url_cb=_raid_auth_url_cb,
        raid_go_url_cb=_raid_go_url_cb,
        raid_requirements_cb=_raid_requirements_cb,
        raid_oauth_callback_cb=_raid_oauth_callback_cb,
        eventsub_webhook_handler=eventsub_webhook_handler,
    )

    app = build_v2_app(
        noauth=resolved_noauth,
        token=resolved_dashboard_token,
        partner_token=resolved_partner_token,
        oauth_client_id=resolved_oauth_client_id,
        oauth_client_secret=resolved_oauth_client_secret,
        oauth_redirect_uri=resolved_oauth_redirect_uri,
        session_ttl_seconds=resolved_session_ttl,
        legacy_stats_url=resolved_legacy_stats_url,
        dashboard_services=dashboard_services,
        add_cb=_add_cb,
        remove_cb=_remove_cb,
        list_cb=_list_cb,
        stats_cb=_stats_cb,
        verify_cb=_verify_cb,
        archive_cb=_archive_cb,
        discord_flag_cb=_discord_flag_cb,
        discord_profile_cb=_discord_profile_cb,
        raid_history_cb=None,
        raid_auth_url_cb=_raid_auth_url_cb,
        raid_go_url_cb=_raid_go_url_cb,
        raid_requirements_cb=_raid_requirements_cb,
        raid_oauth_callback_cb=_raid_oauth_callback_cb,
        reload_cb=None,
        eventsub_webhook_handler=eventsub_webhook_handler,
        social_media_clip_manager=None,
        social_media_twitch_api=None,
    )

    async def _close_client(_: web.Application) -> None:
        if eventsub_bridge is not None:
            await eventsub_bridge.stop()
        if client is None:
            return
        await client.close()

    async def _verify_internal_analytics_fingerprint(app: web.Application) -> None:
        app[INTERNAL_API_ANALYTICS_DB_FINGERPRINT_KEY] = None
        app[ANALYTICS_DB_FINGERPRINT_MISMATCH_KEY] = False
        app[ANALYTICS_DB_FINGERPRINT_ERROR_KEY] = None
        if client is None:
            return
        try:
            payload = await client.healthz()
        except BotApiClientError as exc:
            app[ANALYTICS_DB_FINGERPRINT_ERROR_KEY] = str(exc)
            log.warning("Dashboard fingerprint check against internal API failed: %s", exc)
            return

        upstream_fingerprint = str(payload.get("analyticsDbFingerprint") or "").strip() or None
        app[INTERNAL_API_ANALYTICS_DB_FINGERPRINT_KEY] = upstream_fingerprint
        if (
            local_analytics_fingerprint
            and upstream_fingerprint
            and local_analytics_fingerprint != upstream_fingerprint
        ):
            app[ANALYTICS_DB_FINGERPRINT_MISMATCH_KEY] = True
            app[ANALYTICS_DB_FINGERPRINT_ERROR_KEY] = (
                "Dashboard and internal API use different analytics databases."
            )
            log.error(
                "Analytics DB fingerprint mismatch dashboard=%s internal_api=%s",
                local_analytics_fingerprint,
                upstream_fingerprint,
            )

    async def _start_eventsub_bridge(_: web.Application) -> None:
        if eventsub_bridge is None:
            return
        await eventsub_bridge.start()

    app[BOT_API_CLIENT_KEY] = client
    app[ANALYTICS_DB_FINGERPRINT_KEY] = local_analytics_fingerprint
    app[ANALYTICS_DB_FINGERPRINT_DETAILS_KEY] = local_analytics_db
    app[INTERNAL_API_ANALYTICS_DB_FINGERPRINT_KEY] = None
    app[ANALYTICS_DB_FINGERPRINT_MISMATCH_KEY] = False
    app[ANALYTICS_DB_FINGERPRINT_ERROR_KEY] = None
    if eventsub_bridge is not None:
        app[DASHBOARD_EVENTSUB_BRIDGE_KEY] = eventsub_bridge
        app.on_startup.append(_start_eventsub_bridge)
    app.on_startup.append(_verify_internal_analytics_fingerprint)
    app.on_cleanup.append(_close_client)
    return app


async def run_dashboard_service(
    *,
    host: str | None = None,
    port: int | None = None,
    app: web.Application | None = None,
) -> None:
    """Run standalone dashboard service until cancelled."""

    resolved_host = (
        host
        or os.getenv("TWITCH_DASHBOARD_HOST")
        or TWITCH_DASHBOARD_HOST
        or "127.0.0.1"
    ).strip()
    resolved_noauth = _parse_env_bool("TWITCH_DASHBOARD_NOAUTH", bool(TWITCH_DASHBOARD_NOAUTH))
    _require_noauth_opt_in_if_enabled(enabled=resolved_noauth)
    require_noauth_loopback_guard(enabled=resolved_noauth, host=resolved_host)
    resolved_port = int(
        port
        if port is not None
        else _parse_env_int("TWITCH_DASHBOARD_PORT", int(TWITCH_DASHBOARD_PORT or 8765))
    )
    enforce_dashboard_service_runtime(port=resolved_port)
    with runtime_pid_lock("dashboard_service", port=resolved_port):
        dashboard_app = app or build_dashboard_service_app(noauth=resolved_noauth)
        runner = web.AppRunner(dashboard_app)
        await runner.setup()
        site = web.TCPSite(runner, host=resolved_host, port=resolved_port)
        await site.start()

        log.info(
            "Standalone dashboard service running on http://%s:%s/twitch",
            resolved_host,
            resolved_port,
        )
        try:
            await asyncio.Event().wait()
        except asyncio.CancelledError:
            log.info("Standalone dashboard service shutdown requested")
        finally:
            await runner.cleanup()


__all__ = ["build_dashboard_service_app", "run_dashboard_service"]
