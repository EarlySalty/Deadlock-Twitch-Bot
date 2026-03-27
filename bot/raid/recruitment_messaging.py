from __future__ import annotations

import logging
from collections.abc import Callable
from contextlib import AbstractContextManager
from dataclasses import dataclass
from typing import Any, Awaitable, Literal, Protocol

from .chat_targets import lookup_outbound_chat_suppression, make_chat_target
from .recruitment_delivery import (
    RecruitmentDeliveryConfig,
    RecruitmentDeliveryPlan,
    RecruitmentDeliveryPlanner,
    RecruitmentDeliveryRequest,
    RecruitmentInviteVariant,
    RecruitmentMessageVariant,
)

log = logging.getLogger("TwitchStreams.RaidManager")

ReadonlyConnectionFactory = Callable[[], AbstractContextManager[Any]]


class CreateTwitchApi(Protocol):
    def __call__(self, session: Any) -> Any: ...


class ResolveBotOauthContext(Protocol):
    def __call__(self, session: Any) -> Awaitable[tuple[str | None, str | None, set[str]]] | tuple[str | None, str | None, set[str]]: ...


class ResolveValidToken(Protocol):
    def __call__(self, twitch_user_id: str, session: Any) -> Awaitable[str | None] | str | None: ...


class GetFollowersTotalResult(Protocol):
    def __call__(self, api: Any, twitch_user_id: str, user_token: str | None) -> Awaitable[dict[str, Any]] | dict[str, Any]: ...


class BuildFollowersRuntimeState(Protocol):
    def __call__(self) -> dict[str, object]: ...


class IncrementCounter(Protocol):
    def __call__(self, name: str, amount: int = 1) -> int: ...


class LogFollowersDecision(Protocol):
    def __call__(self, **kwargs: Any) -> None: ...


class NextFlowId(Protocol):
    def __call__(self, prefix: str) -> str: ...


class ScopeFallbackWarning(Protocol):
    def __call__(self, *, area: str, subject: str) -> None: ...


class GetChatBot(Protocol):
    def __call__(self) -> Any | None: ...


class FetchUsers(Protocol):
    def __call__(self, chat_bot: Any, logins: list[str]) -> Awaitable[list[Any]] | list[Any]: ...


class LookupOutboundChatSuppression(Protocol):
    def __call__(self, chat_bot: Any, *, target_login: str, target_id: str | None, source: str) -> dict[str, Any] | None: ...


class JoinChatChannel(Protocol):
    def __call__(self, chat_bot: Any, channel_login: str, channel_id: str | None) -> Awaitable[bool] | Awaitable[None] | bool | None: ...


class FollowChannel(Protocol):
    def __call__(self, chat_bot: Any, target_id: str) -> Awaitable[None] | None: ...


class SendChatMessage(Protocol):
    def __call__(self, chat_bot: Any, channel: Any, message: str, source: str) -> Awaitable[bool] | bool: ...


class CountRecentRaids(Protocol):
    def __call__(self, to_broadcaster_id: str) -> int: ...


class CountConfirmedExternalRecruitmentRaids(Protocol):
    def __call__(self, to_broadcaster_id: str) -> int: ...


class ScheduleExternalTargetBanCheck(Protocol):
    def __call__(self, *, target_id: str | None, target_login: str, source: str) -> None: ...


class LoadDeadlockStats(Protocol):
    def __call__(self, to_broadcaster_login: str) -> Any: ...


class SleepFn(Protocol):
    def __call__(self, seconds: float) -> Awaitable[None]: ...


@dataclass(slots=True, frozen=True)
class RecruitmentMessagingDependencies:
    create_twitch_api: CreateTwitchApi | None = None
    resolve_bot_oauth_context: ResolveBotOauthContext | None = None
    resolve_valid_token: ResolveValidToken | None = None
    get_followers_total_result: GetFollowersTotalResult | None = None
    build_followers_runtime_state: BuildFollowersRuntimeState | None = None
    increment_counter: IncrementCounter | None = None
    log_followers_decision: LogFollowersDecision | None = None
    next_flow_id: NextFlowId | None = None
    warn_user_scope_fallback_once: ScopeFallbackWarning | None = None
    clear_user_scope_fallback_warning: ScopeFallbackWarning | None = None
    get_chat_bot: GetChatBot | None = None
    fetch_users: FetchUsers | None = None
    lookup_outbound_chat_suppression: LookupOutboundChatSuppression | None = None
    join_chat_channel: JoinChatChannel | None = None
    follow_channel: FollowChannel | None = None
    send_chat_message: SendChatMessage | None = None
    count_recent_raids: CountRecentRaids | None = None
    count_confirmed_external_recruitment_raids: CountConfirmedExternalRecruitmentRaids | None = None
    schedule_external_target_ban_check: ScheduleExternalTargetBanCheck | None = None
    load_deadlock_stats: LoadDeadlockStats | None = None
    sleep: SleepFn | None = None
    logger: logging.Logger = log
    planner: "RecruitmentMessagingPlanner" | None = None


@dataclass(slots=True, frozen=True)
class RecruitmentMessageDraft:
    delivery_plan: RecruitmentDeliveryPlan
    message: str | None
    discord_invite: str | None
    stats_teaser: str

    @property
    def should_deliver(self) -> bool:
        return self.delivery_plan.should_deliver and bool(self.message)


RecruitmentOutcomeStatus = Literal["sent", "blocked", "unavailable", "failed"]


@dataclass(slots=True, frozen=True)
class RecruitmentMessageResult:
    status: RecruitmentOutcomeStatus
    reason: str | None
    target_id: str | None
    target_login: str
    recent_raid_count: int | None
    total_recruitment_raid_count: int | None
    followers_total: int | None
    message_variant: RecruitmentMessageVariant | None
    invite_variant: RecruitmentInviteVariant | None
    message: str | None


class RecruitmentMessagingPlanner:
    def __init__(self, config: RecruitmentDeliveryConfig | None = None) -> None:
        self._delivery_planner = RecruitmentDeliveryPlanner(config)

    @property
    def delivery_planner(self) -> RecruitmentDeliveryPlanner:
        return self._delivery_planner

    def plan(
        self,
        *,
        from_broadcaster_login: str,
        to_broadcaster_login: str,
        target_id: str | None,
        recent_raid_count: int,
        total_recruitment_raid_count: int | None,
        followers_total: int | None,
        chat_bot_available: bool,
        outbound_chat_suppressed: bool,
        stats_teaser: str = "",
    ) -> RecruitmentMessageDraft:
        delivery_plan = self._delivery_planner.plan(
            RecruitmentDeliveryRequest(
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_login=to_broadcaster_login,
                target_id=target_id,
                recent_raid_count=recent_raid_count,
                total_recruitment_raid_count=total_recruitment_raid_count,
                followers_total=followers_total,
                chat_bot_available=chat_bot_available,
                outbound_chat_suppressed=outbound_chat_suppressed,
            )
        )

        message: str | None = None
        discord_invite: str | None = None
        if delivery_plan.should_deliver and delivery_plan.message_variant is not None:
            discord_invite = (
                "https://discord.gg/z5TfVHuQq2"
                if delivery_plan.invite_variant == "direct"
                else "Discord: Server hinzufuegen & Code eingeben: z5TfVHuQq2"
            )
            message = self._build_message(
                delivery_plan.message_variant,
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_login=to_broadcaster_login,
                total_recruitment_raid_count=int(total_recruitment_raid_count or 0),
                stats_teaser=stats_teaser,
            )

        return RecruitmentMessageDraft(
            delivery_plan=delivery_plan,
            message=message,
            discord_invite=discord_invite,
            stats_teaser=stats_teaser,
        )

    def _build_message(
        self,
        variant: RecruitmentMessageVariant,
        *,
        from_broadcaster_login: str,
        to_broadcaster_login: str,
        total_recruitment_raid_count: int,
        stats_teaser: str,
    ) -> str:
        del stats_teaser
        if variant == "intro":
            return (
                f"Was fuer ein Match! 🔥 @{from_broadcaster_login} bringt dir gerade Unterstuetzung aus unserem "
                f"Deadlock-Streamer-Netzwerk vorbei. Wir suchen gerade Streamer fuer unsere deutsche Deadlock Community noch aktive Streamer. "
                f"Und wenn du in der Kategorie nicht untergehen willst, sondern endlich Impact haben moechtest, Check mal die Bio ab! ❤️"
            )
        if variant == "second":
            return (
                "Schon der 2. Raid von UNS fuer DICH! ❤️ Das ist kein Zufall mehr. "
                "Wir vernetzen die Deutschen Deadlock-Streamer mit der Community, damit du nicht mehr einer von vielen bleibst. "
                "Mehr infos in der Bio! 🚀"
            )
        if variant == "hattrick":
            return (
                f"Hattrick! 🎯 Aller guten Dinge sind 3, @{to_broadcaster_login}. Wir liefern dir die Viewer und "
                f"die Positionierung, die du alleine niemals schaffst. Willst du weiter unsichtbar bleiben oder Teil der Elite werden? "
                f"Grow or Fade Away - Dein Platz wartet in der Bio! 🕯️"
            )
        if variant == "support":
            return (
                f"Dauersupport fuer @{to_broadcaster_login}! 💎 {total_recruitment_raid_count}. Raid von uns. "
                f"Werde Teil des Deadlock-Partner-Netzwerks und dominiere die Kategorie mit uns. Wir regeln die Raids, du den Content. "
                f"Zum Onboarding gehts in der Bio! 🔥"
            )
        return ""


class RecruitmentMessagingService:
    def __init__(
        self,
        dependencies: RecruitmentMessagingDependencies,
        planner: RecruitmentMessagingPlanner | None = None,
    ) -> None:
        self._deps = dependencies
        self._planner = planner or dependencies.planner or RecruitmentMessagingPlanner()

    @staticmethod
    def parse_nonnegative_int(value: object) -> int | None:
        try:
            if value is None:
                return None
            parsed = int(value)
            return parsed if parsed >= 0 else None
        except (TypeError, ValueError):
            return None

    async def resolve_recruitment_followers_total(
        self,
        *,
        login: str,
        target_id: str | None,
        target_stream_data: dict | None,
        session: Any,
    ) -> int | None:
        cached_total = self.parse_nonnegative_int((target_stream_data or {}).get("followers_total"))
        if cached_total is not None:
            return cached_total

        flow_id = self._next_flow_id("followers-recruitment")
        resolved_target_id = str(target_id or "").strip()
        runtime_state = self._build_runtime_state()
        if not resolved_target_id or session is None:
            self._increment_counter("followers_recruitment_reason_missing_target_id_total")
            self._log_followers_decision(
                flow_id=flow_id,
                flow="followers_recruitment",
                login=login,
                target_id=resolved_target_id,
                decision="failed",
                reason="missing_target_id" if not resolved_target_id else "raid_session_unavailable",
                request_attempted="none",
                request_result="not_attempted",
                http_status=None,
                scope_state={"bot": "unknown", "streamer": "absent"},
                runtime_state=runtime_state,
            )
            return None

        create_api = self._deps.create_twitch_api
        resolve_oauth = self._deps.resolve_bot_oauth_context
        resolve_valid_token = self._deps.resolve_valid_token
        get_followers_total_result = self._deps.get_followers_total_result
        if not all(
            callable(cb)
            for cb in (create_api, resolve_oauth, resolve_valid_token, get_followers_total_result)
        ):
            return None

        try:
            api = create_api(session)
            followers_total = None
            bot_token, _bot_id, bot_scopes = await _maybe_await(resolve_oauth(session))
            bot_scope_state = (
                "present"
                if bot_token and "moderator:read:followers" in bot_scopes
                else ("unknown" if bot_token and not bot_scopes else ("absent" if not bot_token else "missing"))
            )
            bot_http_status: int | None = None
            streamer_http_status: int | None = None
            if bot_token and (not bot_scopes or "moderator:read:followers" in bot_scopes):
                self._increment_counter("followers_recruitment_bot_path_attempt_total")
                bot_result = await _maybe_await(
                    get_followers_total_result(api, resolved_target_id, bot_token)
                )
                bot_http_status = (
                    int(bot_result.get("http_status"))
                    if bot_result.get("http_status") is not None
                    else None
                )
                if bot_result.get("ok") and bot_result.get("data") is not None:
                    followers_total = bot_result.get("data")
                    self._clear_user_scope_fallback_warning(
                        area="recruitment follower lookup",
                        subject=login or resolved_target_id,
                    )
                    self._increment_counter("followers_recruitment_bot_path_success_total")
                    self._log_followers_decision(
                        flow_id=flow_id,
                        flow="followers_recruitment",
                        login=login,
                        target_id=resolved_target_id,
                        decision="success",
                        reason="bot_path_success",
                        request_attempted="bot",
                        request_result="success",
                        http_status=bot_http_status or 200,
                        scope_state={"bot": bot_scope_state, "streamer": "absent"},
                        runtime_state=runtime_state,
                        bot_request_attempted=True,
                        bot_request_success=True,
                        bot_http_status=bot_http_status,
                    )
                else:
                    self._increment_counter("followers_recruitment_bot_path_failure_total")
            if followers_total is None:
                user_token: str | None = None
                try:
                    user_token = await _maybe_await(
                        resolve_valid_token(resolved_target_id, session)
                    )
                except Exception:
                    user_token = None
                streamer_scope_state = "absent" if not user_token else "unknown"
                if user_token:
                    self._warn_user_scope_fallback_once(
                        area="recruitment follower lookup",
                        subject=login or resolved_target_id,
                    )
                    streamer_result = await _maybe_await(
                        get_followers_total_result(api, resolved_target_id, user_token)
                    )
                    streamer_http_status = (
                        int(streamer_result.get("http_status"))
                        if streamer_result.get("http_status") is not None
                        else None
                    )
                    if streamer_result.get("ok") and streamer_result.get("data") is not None:
                        followers_total = streamer_result.get("data")
                        self._increment_counter(
                            "followers_recruitment_reason_fallback_to_streamer_token_total"
                        )
                        self._log_followers_decision(
                            flow_id=flow_id,
                            flow="followers_recruitment",
                            login=login,
                            target_id=resolved_target_id,
                            decision="success",
                            reason="fallback_to_streamer_token",
                            request_attempted="bot,streamer" if bot_token else "streamer",
                            request_result="success",
                            http_status=streamer_http_status or 200,
                            scope_state={"bot": bot_scope_state, "streamer": streamer_scope_state},
                            runtime_state=runtime_state,
                            bot_request_attempted=bool(bot_token),
                            bot_request_success=False,
                            bot_http_status=bot_http_status,
                            streamer_http_status=streamer_http_status,
                        )
                    else:
                        final_reason = str(
                            streamer_result.get("error_code") or "helix_followers_failed"
                        )
                        self._increment_counter(
                            f"followers_recruitment_reason_{final_reason}_total"
                        )
                        self._log_followers_decision(
                            flow_id=flow_id,
                            flow="followers_recruitment",
                            login=login,
                            target_id=resolved_target_id,
                            decision="failed",
                            reason=final_reason,
                            request_attempted="bot,streamer" if bot_token else "streamer",
                            request_result="failed",
                            http_status=streamer_http_status,
                            scope_state={"bot": bot_scope_state, "streamer": streamer_scope_state},
                            runtime_state=runtime_state,
                            bot_request_attempted=bool(bot_token),
                            bot_request_success=False,
                            bot_http_status=bot_http_status,
                            streamer_http_status=streamer_http_status,
                        )
                else:
                    final_reason = (
                        "bot_scope_missing"
                        if bot_scope_state == "missing"
                        else ("bot_token_missing" if not bot_token else "bot_path_unavailable")
                    )
                    self._increment_counter(
                        f"followers_recruitment_reason_{final_reason}_total"
                    )
                    self._log_followers_decision(
                        flow_id=flow_id,
                        flow="followers_recruitment",
                        login=login,
                        target_id=resolved_target_id,
                        decision="failed",
                        reason=final_reason,
                        request_attempted="bot" if bot_token else "none",
                        request_result="failed" if bot_token else "not_attempted",
                        http_status=bot_http_status,
                        scope_state={"bot": bot_scope_state, "streamer": "absent"},
                        runtime_state=runtime_state,
                        bot_request_attempted=bool(bot_token),
                        bot_request_success=False,
                        bot_http_status=bot_http_status,
                    )
        except Exception:
            self._deps.logger.debug("Follower-Check fehlgeschlagen fuer %s", login, exc_info=True)
            return None

        parsed_total = self.parse_nonnegative_int(followers_total)
        if parsed_total is not None and isinstance(target_stream_data, dict):
            target_stream_data["followers_total"] = parsed_total
        return parsed_total

    async def send_recruitment_message_now(
        self,
        *,
        from_broadcaster_login: str,
        to_broadcaster_login: str,
        target_stream_data: dict | None = None,
        confirmed_external_raid_count: int | None = None,
        session: Any | None = None,
        chat_bot: Any | None = None,
    ) -> RecruitmentMessageResult:
        chat_bot = chat_bot if chat_bot is not None else (
            self._deps.get_chat_bot() if callable(self._deps.get_chat_bot) else None
        )
        if not chat_bot:
            self._deps.logger.debug("Chat bot not available for recruitment message")
            return RecruitmentMessageResult(
                status="unavailable",
                reason="chat_bot_unavailable",
                target_id=None,
                target_login=to_broadcaster_login,
                recent_raid_count=None,
                total_recruitment_raid_count=None,
                followers_total=None,
                message_variant=None,
                invite_variant=None,
                message=None,
            )

        target_id = None
        if target_stream_data:
            target_id = target_stream_data.get("user_id")

        if not target_id and callable(self._deps.fetch_users):
            try:
                users = await _maybe_await(
                    self._deps.fetch_users(chat_bot, [to_broadcaster_login])
                )
                if users:
                    target_id = str(users[0].id)
            except Exception:
                target_id = None

        if not target_id:
            self._deps.logger.warning(
                "Could not resolve user ID for recruitment message to %s",
                to_broadcaster_login,
            )
            return RecruitmentMessageResult(
                status="blocked",
                reason="target_id_unresolved",
                target_id=None,
                target_login=to_broadcaster_login,
                recent_raid_count=None,
                total_recruitment_raid_count=None,
                followers_total=None,
                message_variant=None,
                invite_variant=None,
                message=None,
            )

        target_channel = make_chat_target(to_broadcaster_login, str(target_id))
        suppression = None
        if callable(self._deps.lookup_outbound_chat_suppression):
            try:
                suppression = self._deps.lookup_outbound_chat_suppression(
                    chat_bot,
                    target_login=to_broadcaster_login,
                    target_id=str(target_id),
                    source="recruitment",
                )
            except Exception:
                suppression = None
        if suppression is not None:
            self._deps.logger.info(
                "Skipping recruitment message to %s due stored chat suppression (code=%s, until=%s)",
                to_broadcaster_login,
                suppression.get("reason_code") or "unknown",
                suppression.get("suppressed_until") or "-",
            )
            return RecruitmentMessageResult(
                status="blocked",
                reason="outbound_chat_suppressed",
                target_id=str(target_id),
                target_login=to_broadcaster_login,
                recent_raid_count=None,
                total_recruitment_raid_count=None,
                followers_total=None,
                message_variant=None,
                invite_variant=None,
                message=None,
            )

        try:
            if callable(self._deps.join_chat_channel):
                await _maybe_await(
                    self._deps.join_chat_channel(chat_bot, to_broadcaster_login, str(target_id))
                )
        except Exception:
            self._deps.logger.debug("Konnte Channel %s nicht vorab beitreten", to_broadcaster_login)
            target_channel = make_chat_target(to_broadcaster_login, str(target_id))

        if target_id and callable(self._deps.follow_channel):
            try:
                await _maybe_await(self._deps.follow_channel(chat_bot, str(target_id)))
            except Exception:
                pass

        recent_raids = 0
        if callable(self._deps.count_recent_raids):
            try:
                recent_raids = int(self._deps.count_recent_raids(str(target_id)) or 0)
            except Exception:
                recent_raids = 0

        total_raids = (
            int(confirmed_external_raid_count)
            if confirmed_external_raid_count is not None
            else (
                int(self._deps.count_confirmed_external_recruitment_raids(str(target_id)))
                if callable(self._deps.count_confirmed_external_recruitment_raids)
                else 0
            )
        )

        followers_total = await self.resolve_recruitment_followers_total(
            login=to_broadcaster_login,
            target_id=str(target_id),
            target_stream_data=target_stream_data,
            session=session,
        )

        stats_teaser = ""
        if callable(self._deps.load_deadlock_stats):
            try:
                stats = self._deps.load_deadlock_stats(to_broadcaster_login.lower())
                if stats and stats[0]:
                    avg_viewers = int(stats[0])
                    peak_viewers = int(stats[1]) if stats[1] else 0
                    if peak_viewers > 0:
                        stats_teaser = (
                            f"Uebrigens: Du hattest im Schnitt {avg_viewers} Viewer bei Deadlock, dein Peak war {peak_viewers}. "
                        )
            except Exception:
                self._deps.logger.debug(
                    "Could not fetch stats for %s", to_broadcaster_login, exc_info=True
                )

        draft = self._planner.plan(
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_login=to_broadcaster_login,
            target_id=str(target_id or "").strip() or None,
            recent_raid_count=int(recent_raids or 0),
            total_recruitment_raid_count=int(total_raids or 0),
            followers_total=followers_total,
            chat_bot_available=bool(chat_bot),
            outbound_chat_suppressed=False,
            stats_teaser=stats_teaser,
        )
        if not draft.should_deliver or not draft.message:
            self._deps.logger.info(
                "Skipping recruitment message to %s (%s)",
                to_broadcaster_login,
                draft.delivery_plan.reason or "blocked",
            )
            return RecruitmentMessageResult(
                status="blocked",
                reason=draft.delivery_plan.reason,
                target_id=str(target_id),
                target_login=to_broadcaster_login,
                recent_raid_count=int(recent_raids or 0),
                total_recruitment_raid_count=int(total_raids or 0),
                followers_total=followers_total,
                message_variant=draft.delivery_plan.message_variant,
                invite_variant=draft.delivery_plan.invite_variant,
                message=None,
            )

        self._deps.logger.info(
            "Warte %.0fs vor Senden der Recruitment-Message an %s...",
            draft.delivery_plan.delay_seconds,
            to_broadcaster_login,
        )
        if callable(self._deps.sleep):
            await self._deps.sleep(float(draft.delivery_plan.delay_seconds))

        if callable(self._deps.send_chat_message):
            try:
                success = await _maybe_await(
                    self._deps.send_chat_message(chat_bot, target_channel, draft.message, "recruitment")
                )
                if success:
                    self._deps.logger.info(
                        "Sent recruitment message in %s's chat (raided by %s)",
                        to_broadcaster_login,
                        from_broadcaster_login,
                    )
                    if callable(self._deps.schedule_external_target_ban_check):
                        self._deps.schedule_external_target_ban_check(
                            target_id=str(target_id),
                            target_login=to_broadcaster_login,
                            source="recruitment",
                        )
                    return RecruitmentMessageResult(
                        status="sent",
                        reason=None,
                        target_id=str(target_id),
                        target_login=to_broadcaster_login,
                        recent_raid_count=int(recent_raids or 0),
                        total_recruitment_raid_count=int(total_raids or 0),
                        followers_total=followers_total,
                        message_variant=draft.delivery_plan.message_variant,
                        invite_variant=draft.delivery_plan.invite_variant,
                        message=draft.message,
                    )
                self._deps.logger.warning(
                    "Failed to send recruitment message to %s (returned False)",
                    to_broadcaster_login,
                )
                return RecruitmentMessageResult(
                    status="failed",
                    reason="send_chat_message_returned_false",
                    target_id=str(target_id),
                    target_login=to_broadcaster_login,
                    recent_raid_count=int(recent_raids or 0),
                    total_recruitment_raid_count=int(total_raids or 0),
                    followers_total=followers_total,
                    message_variant=draft.delivery_plan.message_variant,
                    invite_variant=draft.delivery_plan.invite_variant,
                    message=draft.message,
                )
            except Exception:
                self._deps.logger.exception(
                    "Failed to send recruitment message to %s (raided by %s)",
                    to_broadcaster_login,
                    from_broadcaster_login,
                )

        return RecruitmentMessageResult(
            status="failed",
            reason="send_chat_message_unavailable",
            target_id=str(target_id),
            target_login=to_broadcaster_login,
            recent_raid_count=int(recent_raids or 0),
            total_recruitment_raid_count=int(total_raids or 0),
            followers_total=followers_total,
            message_variant=draft.delivery_plan.message_variant,
            invite_variant=draft.delivery_plan.invite_variant,
            message=draft.message,
        )

    def _next_flow_id(self, prefix: str) -> str:
        if callable(self._deps.next_flow_id):
            try:
                flow_id = self._deps.next_flow_id(prefix)
                if str(flow_id or "").strip():
                    return str(flow_id)
            except Exception:
                pass
        return f"{prefix}-fallback"

    def _increment_counter(self, name: str, amount: int = 1) -> None:
        if callable(self._deps.increment_counter):
            try:
                self._deps.increment_counter(name, amount)
            except Exception:
                pass

    def _log_followers_decision(self, **kwargs: Any) -> None:
        if callable(self._deps.log_followers_decision):
            try:
                self._deps.log_followers_decision(**kwargs)
            except Exception:
                pass

    def _build_runtime_state(self) -> dict[str, object]:
        if callable(self._deps.build_followers_runtime_state):
            try:
                return dict(self._deps.build_followers_runtime_state())
            except Exception:
                return {}
        return {}

    def _warn_user_scope_fallback_once(self, *, area: str, subject: str) -> None:
        if callable(self._deps.warn_user_scope_fallback_once):
            try:
                self._deps.warn_user_scope_fallback_once(area=area, subject=subject)
            except Exception:
                pass

    def _clear_user_scope_fallback_warning(self, *, area: str, subject: str) -> None:
        if callable(self._deps.clear_user_scope_fallback_warning):
            try:
                self._deps.clear_user_scope_fallback_warning(area=area, subject=subject)
            except Exception:
                pass


async def _maybe_await(value: object) -> object:
    if hasattr(value, "__await__"):
        return await value  # type: ignore[no-any-return]
    return value


def build_runtime_recruitment_messaging_service(
    *,
    create_twitch_api: CreateTwitchApi,
    readonly_connection_factory: ReadonlyConnectionFactory,
    resolve_bot_oauth_context: Callable[[], Awaitable[tuple[str | None, str | None, set[str]]] | tuple[str | None, str | None, set[str]]],
    resolve_valid_token: ResolveValidToken,
    get_followers_total_result: GetFollowersTotalResult,
    build_followers_runtime_state: BuildFollowersRuntimeState,
    increment_counter: IncrementCounter,
    log_followers_decision: LogFollowersDecision,
    next_flow_id: NextFlowId,
    warn_user_scope_fallback_once: ScopeFallbackWarning,
    clear_user_scope_fallback_warning: ScopeFallbackWarning,
    get_chat_bot: GetChatBot,
    count_confirmed_external_recruitment_raids: CountConfirmedExternalRecruitmentRaids,
    schedule_external_target_ban_check: ScheduleExternalTargetBanCheck,
    sleep: SleepFn,
) -> RecruitmentMessagingService:
    def _count_recent_raids(to_broadcaster_id: str) -> int:
        with readonly_connection_factory() as conn:
            row = conn.execute(
                """
                SELECT COUNT(*) FROM twitch_raid_history
                WHERE to_broadcaster_id = %s
                  AND COALESCE(success, FALSE) IS TRUE
                  AND executed_at > NOW() - INTERVAL '1 day'
                """,
                (to_broadcaster_id,),
            ).fetchone()
        return int(row[0]) if row and row[0] is not None else 0

    def _load_deadlock_stats(to_broadcaster_login: str):
        with readonly_connection_factory() as conn:
            return conn.execute(
                """
                SELECT
                    ROUND(AVG(viewer_count)) as avg_viewers,
                    MAX(viewer_count) as peak_viewers
                FROM twitch_stats_category
                WHERE streamer = %s
                  AND viewer_count > 0
                """,
                (to_broadcaster_login,),
            ).fetchone()

    return RecruitmentMessagingService(
        RecruitmentMessagingDependencies(
            create_twitch_api=create_twitch_api,
            resolve_bot_oauth_context=lambda _session: resolve_bot_oauth_context(),
            resolve_valid_token=resolve_valid_token,
            get_followers_total_result=get_followers_total_result,
            build_followers_runtime_state=build_followers_runtime_state,
            increment_counter=increment_counter,
            log_followers_decision=log_followers_decision,
            next_flow_id=next_flow_id,
            warn_user_scope_fallback_once=warn_user_scope_fallback_once,
            clear_user_scope_fallback_warning=clear_user_scope_fallback_warning,
            get_chat_bot=get_chat_bot,
            fetch_users=lambda chat_bot, logins: chat_bot.fetch_users(logins=logins),
            lookup_outbound_chat_suppression=lookup_outbound_chat_suppression,
            join_chat_channel=lambda chat_bot, channel_login, channel_id: chat_bot.join(
                channel_login,
                channel_id=channel_id,
            ),
            follow_channel=lambda chat_bot, target_id: chat_bot.follow_channel(target_id),
            send_chat_message=lambda chat_bot, channel, message, source: chat_bot._send_chat_message(
                channel,
                message,
                source=source,
            )
            if hasattr(chat_bot, "_send_chat_message")
            else False,
            count_recent_raids=_count_recent_raids,
            count_confirmed_external_recruitment_raids=count_confirmed_external_recruitment_raids,
            schedule_external_target_ban_check=schedule_external_target_ban_check,
            load_deadlock_stats=_load_deadlock_stats,
            sleep=sleep,
        )
    )


__all__ = [
    "BuildFollowersRuntimeState",
    "CountConfirmedExternalRecruitmentRaids",
    "CountRecentRaids",
    "CreateTwitchApi",
    "FetchUsers",
    "FollowChannel",
    "GetChatBot",
    "GetFollowersTotalResult",
    "IncrementCounter",
    "JoinChatChannel",
    "LoadDeadlockStats",
    "LogFollowersDecision",
    "LookupOutboundChatSuppression",
    "NextFlowId",
    "RecruitmentMessageDraft",
    "RecruitmentMessageResult",
    "RecruitmentMessagingDependencies",
    "RecruitmentMessagingPlanner",
    "RecruitmentMessagingService",
    "ResolveBotOauthContext",
    "ResolveValidToken",
    "ReadonlyConnectionFactory",
    "ScheduleExternalTargetBanCheck",
    "ScopeFallbackWarning",
    "SendChatMessage",
    "SleepFn",
    "build_runtime_recruitment_messaging_service",
]
