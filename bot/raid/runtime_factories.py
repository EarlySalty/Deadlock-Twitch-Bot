from __future__ import annotations

import asyncio
import logging
from datetime import UTC, datetime
from typing import Any

from ..storage import (
    insert_observability_event,
    load_active_partner,
    load_streamer_identity,
    readonly_connection,
    transaction,
)
from .arrival_confirmation import ArrivalConfirmationService
from .services.candidate_selection import CandidateSelectionService
from .services.external_recruitment import ExternalRecruitmentService
from .services.followers import CandidateFollowersDependencies, CandidateFollowersService
from .services.manual_raid_suppression import (
    ManualRaidSuppressionDependencies,
    ManualRaidSuppressionService,
)
from .services.offline_raid_orchestrator import OfflineRaidOrchestrator
from .observability import RaidObservabilityEvent, RaidObservabilityService
from .services.partner_arrival_tracking import (
    PartnerArrivalTrackingDependencies,
    PartnerArrivalTrackingService,
)
from .services.partner_raid_delivery import (
    PartnerRaidDeliveryConfig,
    PartnerRaidDeliveryDependencies,
    PartnerRaidDeliveryPlanner,
    PartnerRaidDeliveryService,
)
from .services.partner_setup_service import PartnerSetupService
from .raid_arrival_runtime import RaidArrivalRuntime, RaidArrivalRuntimeDependencies
from .services.raid_blacklist import (
    RaidBlacklistConfig,
    RaidBlacklistService,
    build_runtime_raid_blacklist_service,
)
from .services.raid_data_sources import RaidDataSourceService
from .raid_metrics_store import RaidMetricsStore
from .raid_pipeline import RaidPipelineDependencies, RaidPipelineService
from .raid_state_store import RaidStateStore, RaidStateStoreConfig
from .raid_tracking_runtime import (
    RaidTrackingRuntimeDependencies,
    RaidTrackingRuntimeService,
    RaidTrackingRuntimeState,
)
from .services.outreach_boost_targets import (
    load_outreach_boost_logins,
    mark_outreach_boost_used,
)
from .services.recruitment_messaging import (
    RecruitmentMessagingService,
    build_runtime_recruitment_messaging_service,
)
from .signal_correlation import RaidSignalCorrelationService

try:
    from .partner_scores import (
        load_partner_raid_score_map,
        refresh_partner_raid_score_async,
    )
except Exception:  # pragma: no cover - best effort if helper is unavailable during partial deploys
    load_partner_raid_score_map = None  # type: ignore[assignment]
    refresh_partner_raid_score_async = None  # type: ignore[assignment]

try:
    from .partner_raid_score_tracking import track_confirmed_partner_raid
except Exception:  # pragma: no cover - best effort if helper is unavailable during partial deploys
    track_confirmed_partner_raid = None  # type: ignore[assignment]


log = logging.getLogger("TwitchStreams.RaidManager")


def make_raid_observability_service(
    bot: Any,
    *,
    insert_observability_event_fn=insert_observability_event,
) -> RaidObservabilityService:
    def _sink(event: RaidObservabilityEvent) -> None:
        storage_payload = event.as_storage_payload()
        insert_observability_event_fn(
            flow_type=str(storage_payload.get("flow_type") or "raid"),
            flow_id=str(storage_payload.get("flow_id") or ""),
            entity_login=str(storage_payload.get("entity_login") or ""),
            entity_id=str(storage_payload.get("entity_id") or ""),
            step=str(storage_payload.get("step") or "event"),
            decision=str(storage_payload.get("decision") or "unknown"),
            details=event.as_log_fields(),
        )

    service = RaidObservabilityService(
        event_sink=_sink,
        counter_store=bot._raid_observability_counters(),
    )
    service.sequence = int(getattr(bot, "_raid_observability_sequence", 0) or 0)
    return service


def make_partner_raid_delivery_planner() -> PartnerRaidDeliveryPlanner:
    return PartnerRaidDeliveryPlanner(
        PartnerRaidDeliveryConfig(
            delay_seconds=5.0,
        )
    )


def make_partner_raid_delivery_service(
    bot: Any,
    *,
    planner: PartnerRaidDeliveryPlanner | None = None,
) -> PartnerRaidDeliveryService:
    return PartnerRaidDeliveryService(
        PartnerRaidDeliveryDependencies(
            get_chat_bot=lambda: bot.chat_bot,
            count_received_network_raids=bot._get_received_network_raid_count,
            lookup_outbound_chat_suppression=lambda **kwargs: bot._lookup_outbound_chat_suppression(
                str(kwargs.get("target_login") or ""),
                str(kwargs.get("target_id") or "") or None,
                source=str(kwargs.get("source") or ""),
            ),
            join_chat_channel=lambda chat_bot, channel_login, channel_id: chat_bot.join(
                channel_login,
                channel_id=channel_id,
            ),
            send_chat_message=lambda chat_bot, channel, message, source: (
                chat_bot._send_chat_message(
                    channel,
                    message,
                    source=source,
                )
                if hasattr(chat_bot, "_send_chat_message")
                else None
            ),
            logger=log,
        ),
        planner=planner or make_partner_raid_delivery_planner(),
    )


def make_external_recruitment_service(bot: Any) -> ExternalRecruitmentService:
    return ExternalRecruitmentService(
        persist_confirmed_raid=bot._record_confirmed_external_recruitment_raid,
        count_confirmed_raids=bot._get_confirmed_external_recruitment_raid_count,
        schedule_pending_blacklist=bot._schedule_external_recruitment_blacklist_pending,
        delete_pending_blacklist=bot._delete_external_recruitment_blacklist_pending,
        is_target_partner=bot._is_target_currently_partner,
    )


def make_arrival_confirmation_service(bot: Any) -> ArrivalConfirmationService:
    return ArrivalConfirmationService(
        partner_lookup=lambda **lookup_kwargs: bot._lookup_partner_target_channel(
            broadcaster_id=str(lookup_kwargs.get("twitch_user_id") or ""),
            broadcaster_login=str(lookup_kwargs.get("twitch_login") or ""),
        ),
        known_streamer_lookup=lambda **lookup_kwargs: bot._resolve_known_streamer_identity(
            broadcaster_login=str(lookup_kwargs.get("broadcaster_login") or ""),
            broadcaster_id=str(lookup_kwargs.get("broadcaster_id") or "") or None,
        ),
    )


def make_raid_state_store_config(
    *,
    recent_raid_arrival_ttl_seconds: float,
    orphan_chat_notification_grace_seconds: float,
    orphan_chat_notification_retention_seconds: float,
    raid_readiness_ttl_seconds: float,
    raid_readiness_max_entries: int,
) -> RaidStateStoreConfig:
    return RaidStateStoreConfig(
        recent_raid_arrival_ttl_seconds=recent_raid_arrival_ttl_seconds,
        orphan_chat_notification_grace_seconds=orphan_chat_notification_grace_seconds,
        orphan_chat_notification_retention_seconds=orphan_chat_notification_retention_seconds,
        raid_readiness_ttl_seconds=raid_readiness_ttl_seconds,
        raid_readiness_max_entries=raid_readiness_max_entries,
    )


def make_raid_state_store(bot: Any, *, config: RaidStateStoreConfig) -> RaidStateStore:
    return RaidStateStore(
        bot,
        config=config,
        logger=log,
    )


def make_manual_raid_suppression_service(
    bot: Any,
    *,
    readonly_connection_factory=readonly_connection,
    load_active_partner_fn=load_active_partner,
) -> ManualRaidSuppressionService:
    return ManualRaidSuppressionService(
        bot,
        ManualRaidSuppressionDependencies(
            readonly_connection=readonly_connection_factory,
            load_active_partner=load_active_partner_fn,
            logger=log,
        ),
    )


def make_partner_arrival_tracking_service(
    bot: Any,
    *,
    readonly_connection_factory=readonly_connection,
    transaction_factory=transaction,
    load_active_partner_fn=load_active_partner,
    load_streamer_identity_fn=load_streamer_identity,
) -> PartnerArrivalTrackingService:
    manual_suppression = bot._manual_raid_suppression_service()
    state_store = bot._raid_state_store()
    return PartnerArrivalTrackingService(
        PartnerArrivalTrackingDependencies(
            readonly_connection=readonly_connection_factory,
            transaction=transaction_factory,
            load_active_partner=load_active_partner_fn,
            load_streamer_identity=load_streamer_identity_fn,
            resolve_streamer_id_by_login=manual_suppression.resolve_streamer_id_by_login,
            mark_manual_raid_started=manual_suppression.mark_manual_raid_started,
            remember_recent_raid_arrival=state_store.remember_recent_raid_arrival,
            logger=log,
        )
    )


def make_raid_data_source_service(
    bot: Any,
    *,
    readonly_connection_factory=readonly_connection,
    utcnow=lambda: datetime.now(UTC),
) -> RaidDataSourceService:
    return RaidDataSourceService(
        client_id=bot.auth_manager.client_id,
        client_secret=bot.auth_manager.client_secret,
        session_getter=lambda: bot.session,
        target_game_lower_getter=lambda: (
            str(getattr(bot._cog, "_get_target_game_lower")() or "").strip().lower()
            if getattr(bot, "_cog", None) is not None
            and callable(getattr(bot._cog, "_get_target_game_lower", None))
            else None
        ),
        language_filter_getter=lambda: (
            list(getattr(bot._cog, "_language_filter_values")())
            if getattr(bot, "_cog", None) is not None
            and callable(getattr(bot._cog, "_language_filter_values", None))
            else []
        ),
        shared_stream_fetch=(
            getattr(bot._cog, "_fetch_streams_by_logins_quick")
            if getattr(bot, "_cog", None) is not None
            and callable(getattr(bot._cog, "_fetch_streams_by_logins_quick", None))
            else None
        ),
        cached_category_id_getter=lambda: getattr(bot._cog, "_category_id", None)
        if getattr(bot, "_cog", None) is not None
        else None,
        readonly_connection_factory=readonly_connection_factory,
        utcnow=utcnow,
        logger=log,
    )


def make_partner_setup_service(
    bot: Any,
    *,
    moderator_url_base: str,
    mask_log_identifier: Any,
    readonly_connection_factory=readonly_connection,
    transaction_factory=transaction,
) -> PartnerSetupService:
    return PartnerSetupService(
        auth_manager=bot.auth_manager,
        session_getter=lambda: bot.session,
        chat_bot_getter=lambda: bot.chat_bot,
        bot_id_getter=bot._resolve_bot_id_for_setup,
        readonly_connection_factory=readonly_connection_factory,
        transaction_factory=transaction_factory,
        moderator_url_base=moderator_url_base,
        mask_log_identifier=mask_log_identifier,
        logger=log,
    )


def make_offline_raid_orchestrator(bot: Any) -> OfflineRaidOrchestrator:
    return OfflineRaidOrchestrator(
        create_twitch_api=bot._create_twitch_api,
        resolve_manual_raid_source_state=bot._resolve_manual_raid_source_state,
        evaluate_deadlock_raid_source=bot._evaluate_deadlock_raid_source,
        safe_int=bot._safe_int,
        calculate_stream_duration_sec=bot._calculate_stream_duration_sec,
        load_partner_roster_for_raid=bot._load_partner_roster_for_raid,
        fetch_streams_by_logins_for_raid=bot._fetch_streams_by_logins_for_raid,
        build_online_partner_candidates=bot._build_online_partner_candidates,
        filter_deadlock_eligible_partner_candidates=bot._filter_deadlock_eligible_partner_candidates,
        resolve_target_category_id=bot._resolve_target_category_id,
        execute_raid_pipeline=bot._execute_raid_pipeline,
        is_offline_auto_raid_suppressed=bot.is_offline_auto_raid_suppressed,
        load_offline_auto_raid_eligibility=bot._load_offline_auto_raid_eligibility,
        get_target_game_lower=bot._get_target_game_lower,
        logger=log,
    )


def make_raid_metrics_store(
    bot: Any,
    *,
    readonly_connection_factory=readonly_connection,
    transaction_factory=transaction,
) -> RaidMetricsStore:
    return RaidMetricsStore(
        readonly_connection=readonly_connection_factory,
        transaction=transaction_factory,
        normalize_broadcaster_login=bot._normalize_broadcaster_login,
        is_partner_target_channel=bot._is_partner_target_channel,
        next_raid_observability_flow_id=bot._next_raid_observability_flow_id,
        logger=log,
    )


def make_candidate_followers_service(bot: Any) -> CandidateFollowersService:
    return CandidateFollowersService(
        CandidateFollowersDependencies(
            create_twitch_api=lambda session: bot._create_twitch_api(session=session),
            resolve_bot_oauth_context=bot._resolve_bot_oauth_context,
            get_followers_total_result=lambda api, user_id, user_token: bot._get_followers_total_result_with_legacy_fallback(
                api,
                user_id,
                user_token=user_token,
            ),
            resolve_valid_token=lambda user_id, session: bot.auth_manager.get_valid_token(
                user_id,
                session,
            ),
            increment_counter=bot._increment_raid_observability_counter,
            warn_user_scope_fallback_once=bot._warn_user_scope_fallback_once,
            clear_user_scope_fallback_warning=bot._clear_user_scope_fallback_warning,
            logger=log,
        ),
        max_concurrency=8,
    )


def make_candidate_selection_service(
    bot: Any,
    *,
    recent_raid_cooldown_days: int,
    load_partner_raid_score_map_fn=load_partner_raid_score_map,
    refresh_partner_raid_score_async_fn=refresh_partner_raid_score_async,
    readonly_connection_factory=readonly_connection,
) -> CandidateSelectionService:
    return CandidateSelectionService(
        load_partner_raid_score_map=load_partner_raid_score_map_fn,
        refresh_partner_raid_score_async=refresh_partner_raid_score_async_fn,
        recent_raid_targets_loader=bot._get_recent_raid_targets,
        attach_followers_totals=bot._attach_followers_totals,
        readonly_connection_factory=readonly_connection_factory,
        logger=log,
        recent_raid_cooldown_days=recent_raid_cooldown_days,
    )


def make_raid_blacklist_service(
    bot: Any,
    *,
    external_recruitment_raid_limit: int,
    external_recruitment_blacklist_grace_seconds: int,
    external_target_ban_check_delay_seconds: int,
    readonly_connection_factory=readonly_connection,
    transaction_factory=transaction,
) -> RaidBlacklistService:
    return build_runtime_raid_blacklist_service(
        readonly_connection_factory=readonly_connection_factory,
        transaction_factory=transaction_factory,
        is_target_partner=bot._is_target_currently_partner,
        get_chat_bot=lambda: bot.chat_bot,
        config=RaidBlacklistConfig(
            external_recruitment_raid_limit=external_recruitment_raid_limit,
            external_recruitment_blacklist_grace_seconds=external_recruitment_blacklist_grace_seconds,
            external_target_ban_check_delay_seconds=external_target_ban_check_delay_seconds,
        ),
    )


def make_raid_pipeline_service(bot: Any) -> RaidPipelineService:
    def _log_event(**payload: object) -> None:
        level = payload.get("level", logging.INFO)
        if isinstance(level, str):
            level = getattr(logging, level.upper(), logging.INFO)
        bot._log_raid_observability_event(
            raid_flow_id=str(payload.get("flow_id") or ""),
            step=str(payload.get("step") or "event"),
            decision=str(payload.get("decision") or "unknown"),
            level=level,
            from_broadcaster_login=payload.get("from_broadcaster_login"),
            from_broadcaster_id=payload.get("from_broadcaster_id"),
            to_broadcaster_login=payload.get("to_broadcaster_login"),
            to_broadcaster_id=payload.get("to_broadcaster_id"),
            details=payload.get("details"),
        )

    async def _open_voice_reaction_conversation(target_login: str, target_id: str | None) -> Any:
        chat_bot = getattr(bot, "chat_bot", None)
        if chat_bot is None or not hasattr(chat_bot, "_open_conversation"):
            return None
        try:
            return await chat_bot._open_conversation(
                target_login,
                target_id,
                source="raid_boost",
            )
        except Exception:
            log.debug(
                "RaidPipeline: Voice-Reaction-Open via runtime_factories warf für %s",
                target_login,
                exc_info=True,
            )
            return None

    return RaidPipelineService(
        RaidPipelineDependencies(
            load_raid_blacklist=bot._load_raid_blacklist,
            add_to_blacklist=bot._add_to_blacklist,
            increment_raid_disabled_strikes=bot._increment_raid_disabled_strikes,
            select_partner_candidate_by_score=bot._select_partner_candidate_by_score,
            select_fairest_candidate=bot._select_fairest_candidate,
            ensure_raid_arrival_subscription_ready=lambda target_id, target_login, raid_flow_id: bot._ensure_raid_arrival_subscription_ready(
                to_broadcaster_id=target_id,
                to_broadcaster_login=target_login,
                raid_flow_id=raid_flow_id,
            ),
            start_raid=bot.raid_executor.start_raid,
            register_pending_raid=bot._register_pending_raid,
            mark_manual_raid_started=lambda broadcaster_id, ttl_seconds: bot.mark_manual_raid_started(
                broadcaster_id=broadcaster_id,
                ttl_seconds=ttl_seconds,
            ),
            logger=log,
            next_raid_observability_flow_id=lambda prefix: bot._next_raid_observability_flow_id(
                prefix=prefix
            ),
            increment_raid_observability_counter=bot._increment_raid_observability_counter,
            log_raid_observability_event=_log_event,
            to_thread=asyncio.to_thread,
            load_outreach_boost_logins=load_outreach_boost_logins,
            mark_outreach_boost_used=mark_outreach_boost_used,
            open_voice_reaction_conversation=_open_voice_reaction_conversation,
        )
    )


def make_raid_tracking_runtime_service(bot: Any) -> RaidTrackingRuntimeService:
    state_store = bot._raid_state_store()
    state_store.ensure_runtime_raid_tracking_state()

    def _eventsub_has_sub(cog: Any, sub_type: str, broadcaster_user_id: str) -> bool:
        checker = getattr(cog, "_eventsub_has_sub", None)
        return bool(checker(sub_type, broadcaster_user_id)) if callable(checker) else False

    async def _ensure_raid_target_dynamic_ready(
        cog: Any,
        broadcaster_user_id: str,
        broadcaster_login: str,
        raid_flow_id: str | None,
    ) -> Any:
        ensure_ready = getattr(cog, "ensure_raid_target_dynamic_ready", None)
        if not callable(ensure_ready):
            return False, None
        return await ensure_ready(
            broadcaster_user_id,
            broadcaster_login,
            raid_flow_id=raid_flow_id,
        )

    async def _subscribe_raid_target_dynamic(
        cog: Any,
        broadcaster_user_id: str,
        broadcaster_login: str,
    ) -> bool:
        subscribe = getattr(cog, "subscribe_raid_target_dynamic", None)
        if not callable(subscribe):
            return False
        return bool(await subscribe(broadcaster_user_id, broadcaster_login))

    return RaidTrackingRuntimeService(
        RaidTrackingRuntimeDependencies(
            state=RaidTrackingRuntimeState(
                pending_store=state_store.pending_raid_store(),
                recent_raid_arrivals=getattr(bot, "_recent_raid_arrivals", {}),
                orphan_chat_raid_notifications=getattr(bot, "_orphan_chat_raid_notifications", {}),
                readiness_states=getattr(bot, "_raid_readiness_by_flow_id", {}),
            ),
            snapshot_chat_notification_subscription=bot._snapshot_chat_notification_subscription,
            get_cog=lambda: bot._cog,
            eventsub_has_sub=_eventsub_has_sub,
            ensure_raid_target_dynamic_ready=_ensure_raid_target_dynamic_ready,
            subscribe_raid_target_dynamic=_subscribe_raid_target_dynamic,
            orphan_chat_raid_notification_handler=lambda payload: bot.on_chat_raid_notification(
                to_broadcaster_id=str(payload.get("to_broadcaster_id") or ""),
                to_broadcaster_login=str(payload.get("to_broadcaster_login") or ""),
                from_broadcaster_login=str(payload.get("from_broadcaster_login") or ""),
                viewer_count=int(payload.get("viewer_count") or 0),
                from_broadcaster_id=str(payload.get("from_broadcaster_id") or "") or None,
                message_id=str(payload.get("message_id") or "") or None,
                event_timestamp=str(payload.get("event_timestamp") or "") or None,
            ),
            next_raid_observability_flow_id=lambda prefix: bot._next_raid_observability_flow_id(
                prefix=prefix
            ),
            increment_raid_observability_counter=bot._increment_raid_observability_counter,
            log_raid_observability_event=bot._log_raid_observability_event,
        )
    )


def make_raid_arrival_runtime(
    bot: Any,
    *,
    track_confirmed_partner_raid_fn=track_confirmed_partner_raid,
) -> RaidArrivalRuntime:
    external_recruitment = bot._external_recruitment_service()
    arrival_confirmation = bot._arrival_confirmation_service()

    def _confirm_pending_raid_arrival_with_overrides(
        *,
        pending_raid: Any,
        signal_type: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        viewer_count: int,
        from_broadcaster_id: str | None = None,
    ) -> Any:
        classification, source_resolution = bot._classify_partner_raid_arrival(
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            expected_partner=bool(pending_raid.is_partner_raid),
        )
        return arrival_confirmation.confirm_pending_raid_arrival(
            pending_raid=pending_raid,
            signal_type=signal_type,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            viewer_count=viewer_count,
            classification_override=classification,
            source_resolution_override=source_resolution,
            target_is_partner_override=classification is not None,
        )

    return RaidArrivalRuntime(
        RaidArrivalRuntimeDependencies(
            arrival_confirmation_service=type(
                "_ArrivalConfirmationProxy",
                (),
                {"confirm_pending_raid_arrival": staticmethod(_confirm_pending_raid_arrival_with_overrides)},
            )(),
            signal_correlation_service=bot._signal_correlation_service(),
            get_pending_raid=lambda to_broadcaster_id, from_broadcaster_login: bot._get_pending_raid(
                to_broadcaster_id=to_broadcaster_id,
                from_broadcaster_login=from_broadcaster_login,
            ),
            store_pending_raid=bot._store_pending_raid,
            pop_pending_raid=lambda to_broadcaster_id, from_broadcaster_login: bot._pop_pending_raid(
                to_broadcaster_id=to_broadcaster_id,
                from_broadcaster_login=from_broadcaster_login,
            ),
            record_pending_signal_observation=lambda pending, signal_type, status, reason, detail: bot._record_pending_signal_observation(
                pending,
                signal_type=signal_type,
                status=status,
                reason=reason,
                detail=detail,
            ),
            store_orphan_chat_raid_notification=bot._store_orphan_chat_raid_notification,
            lookup_recent_raid_arrival=lambda to_broadcaster_id, from_broadcaster_login: bot._lookup_recent_raid_arrival(
                to_broadcaster_id=to_broadcaster_id,
                from_broadcaster_login=from_broadcaster_login,
            ),
            remember_recent_raid_arrival=bot._remember_recent_raid_arrival,
            update_partner_raid_arrival=lambda arrival_tracking_id, confirmation_signals, unraid_seen: bot._update_partner_raid_arrival(
                arrival_tracking_id=arrival_tracking_id,
                confirmation_signals=confirmation_signals,
                unraid_seen=unraid_seen,
            ),
            store_partner_raid_arrival=bot._store_partner_raid_arrival,
            load_recent_raid_history_reference=lambda from_broadcaster_login, to_broadcaster_id: bot._load_recent_raid_history_reference(
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
            ),
            process_independent_partner_raid_arrival=bot._process_independent_partner_raid_arrival,
            cancel_pending_raids_for_source_unraid=bot._cancel_pending_raids_for_source_unraid,
            resolve_streamer_id_by_login=bot._resolve_streamer_id_by_login,
            mark_manual_raid_started=lambda broadcaster_id, ttl_seconds: bot.mark_manual_raid_started(
                broadcaster_id=broadcaster_id,
                ttl_seconds=ttl_seconds,
            ),
            lookup_silent_raid_enabled=bot._lookup_silent_raid_enabled,
            refresh_partner_score_cache_if_available=lambda twitch_user_id, reason: bot._refresh_partner_score_cache_if_available(
                twitch_user_id,
                reason=reason,
            ),
            track_confirmed_partner_raid=track_confirmed_partner_raid_fn,
            delete_external_recruitment_blacklist_pending=bot._delete_external_recruitment_blacklist_pending,
            record_confirmed_external_recruitment_raid=lambda **kwargs: external_recruitment.record_confirmed_raid(
                **kwargs
            ).persisted_count,
            maybe_schedule_external_recruitment_blacklist_pending=external_recruitment.maybe_schedule_blacklist,
            send_partner_raid_message=bot._send_partner_raid_message,
            send_recruitment_message=bot._send_recruitment_message_now,
            increment_raid_observability_counter=bot._increment_raid_observability_counter,
            log_raid_observability_event=bot._log_raid_observability_event,
            next_raid_observability_flow_id=lambda prefix: bot._next_raid_observability_flow_id(
                prefix=prefix
            ),
        )
    )


def make_recruitment_messaging_service(
    bot: Any,
    *,
    readonly_connection_factory=readonly_connection,
) -> RecruitmentMessagingService:
    return build_runtime_recruitment_messaging_service(
        create_twitch_api=lambda session: bot._create_twitch_api(session=session),
        readonly_connection_factory=readonly_connection_factory,
        resolve_bot_oauth_context=bot._resolve_bot_oauth_context,
        resolve_valid_token=lambda twitch_user_id, session: bot.auth_manager.get_valid_token(
            twitch_user_id,
            session,
        ),
        get_followers_total_result=lambda api, twitch_user_id, user_token: bot._get_followers_total_result_with_legacy_fallback(
            api,
            twitch_user_id,
            user_token=user_token,
        ),
        build_followers_runtime_state=bot._build_analytics_followers_runtime_state,
        increment_counter=bot._increment_raid_observability_counter,
        log_followers_decision=bot._log_analytics_followers_decision,
        next_flow_id=lambda prefix: bot._next_raid_observability_flow_id(prefix=prefix),
        warn_user_scope_fallback_once=bot._warn_user_scope_fallback_once,
        clear_user_scope_fallback_warning=bot._clear_user_scope_fallback_warning,
        get_chat_bot=lambda: bot.chat_bot,
        count_confirmed_external_recruitment_raids=bot._get_confirmed_external_recruitment_raid_count,
        schedule_external_target_ban_check=bot._schedule_external_target_ban_check,
        sleep=asyncio.sleep,
    )


def make_signal_correlation_service() -> RaidSignalCorrelationService:
    return RaidSignalCorrelationService()
