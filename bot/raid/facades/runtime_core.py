from __future__ import annotations

import logging
import os
from datetime import datetime
from typing import Any

from ..arrival_confirmation import ArrivalConfirmationService
from ..services.candidate_selection import CandidateSelectionService
from ..services.external_recruitment import ExternalRecruitmentService
from ..services.followers import CandidateFollowersService
from ..services.manual_raid_suppression import ManualRaidSuppressionService
from ..observability import RaidObservabilityService
from ..services.offline_raid_orchestrator import OfflineRaidOrchestrator
from ..services.partner_arrival_tracking import PartnerArrivalTrackingService
from ..services.partner_raid_delivery import PartnerRaidDeliveryPlanner, PartnerRaidDeliveryService
from ..services.partner_setup_service import PartnerSetupService
from ..raid_arrival_runtime import RaidArrivalRuntime
from ..services.raid_blacklist import RaidBlacklistService
from ..services.raid_data_sources import RaidDataSourceService
from ..raid_metrics_store import RaidMetricsStore
from ..raid_pipeline import RaidPipelineService
from ..raid_state_store import RaidStateStore, RaidStateStoreConfig
from ..raid_tracking_runtime import RaidTrackingRuntimeService
from ..services.recruitment_messaging import RecruitmentMessagingService
from ..runtime_factories import (
    make_arrival_confirmation_service,
    make_candidate_followers_service,
    make_candidate_selection_service,
    make_external_recruitment_service,
    make_manual_raid_suppression_service,
    make_offline_raid_orchestrator,
    make_partner_arrival_tracking_service,
    make_partner_raid_delivery_planner,
    make_partner_raid_delivery_service,
    make_partner_setup_service,
    make_raid_arrival_runtime,
    make_raid_blacklist_service,
    make_raid_data_source_service,
    make_raid_metrics_store,
    make_raid_observability_service,
    make_raid_pipeline_service,
    make_raid_state_store,
    make_raid_state_store_config,
    make_raid_tracking_runtime_service,
    make_recruitment_messaging_service,
    make_signal_correlation_service,
)
from ..runtime_support import (
    build_analytics_followers_runtime_state,
    clear_user_scope_fallback_warning,
    get_followers_total_result_with_legacy_fallback,
    log_analytics_followers_decision,
    parse_datetime,
    resolve_bot_oauth_context,
    row_value,
    safe_int,
    warn_user_scope_fallback_once,
)
from ..signal_correlation import RaidSignalCorrelationService
from ..runtime.dependencies import build_default_raid_runtime_deps

log = logging.getLogger("TwitchStreams.RaidManager")


class RaidRuntimeCoreFacadeMixin:
    def _runtime_deps(self):
        deps = getattr(self, "_deps", None)
        if deps is None:
            deps = build_default_raid_runtime_deps()
            self._deps = deps
        return deps

    def _next_raid_observability_flow_id(self, *, prefix: str) -> str:
        service = self._make_raid_observability_service()
        flow_id = service.next_flow_id(prefix=prefix)
        self._raid_observability_sequence = service.sequence
        self._raid_observability_counter_store = service.counter_store
        return flow_id

    def _raid_observability_counters(self) -> dict[str, int]:
        counters = getattr(self, "_raid_observability_counter_store", None)
        if not isinstance(counters, dict):
            counters = {}
            self._raid_observability_counter_store = counters
        return counters

    def _make_raid_observability_service(self) -> RaidObservabilityService:
        deps = self._runtime_deps()
        return make_raid_observability_service(
            self,
            insert_observability_event_fn=deps.insert_observability_event_fn,
        )

    @staticmethod
    def _partner_raid_delivery_planner() -> PartnerRaidDeliveryPlanner:
        return make_partner_raid_delivery_planner()

    def _partner_raid_delivery_service(self) -> PartnerRaidDeliveryService:
        return make_partner_raid_delivery_service(
            self,
            planner=self._partner_raid_delivery_planner(),
        )

    def _external_recruitment_service(self) -> ExternalRecruitmentService:
        return make_external_recruitment_service(self)

    def _arrival_confirmation_service(self) -> ArrivalConfirmationService:
        return make_arrival_confirmation_service(self)

    def _raid_state_store_config(self) -> RaidStateStoreConfig:
        deps = self._runtime_deps()
        return make_raid_state_store_config(
            recent_raid_arrival_ttl_seconds=deps.recent_raid_arrival_ttl_seconds,
            orphan_chat_notification_grace_seconds=deps.pending_chat_notification_grace_seconds,
            orphan_chat_notification_retention_seconds=deps.orphan_chat_notification_retention_seconds,
            raid_readiness_ttl_seconds=deps.raid_readiness_ttl_seconds,
            raid_readiness_max_entries=deps.raid_readiness_max_entries,
        )

    def _raid_state_store(self) -> RaidStateStore:
        return make_raid_state_store(self, config=self._raid_state_store_config())

    def _manual_raid_suppression_service(self) -> ManualRaidSuppressionService:
        deps = self._runtime_deps()
        return make_manual_raid_suppression_service(
            self,
            readonly_connection_factory=deps.readonly_connection_factory,
            load_active_partner_fn=deps.load_active_partner_fn,
        )

    def _partner_arrival_tracking_service(self) -> PartnerArrivalTrackingService:
        deps = self._runtime_deps()
        return make_partner_arrival_tracking_service(
            self,
            readonly_connection_factory=deps.readonly_connection_factory,
            transaction_factory=deps.transaction_factory,
            load_active_partner_fn=deps.load_active_partner_fn,
            load_streamer_identity_fn=deps.load_streamer_identity_fn,
        )

    def _raid_data_source_service(self) -> RaidDataSourceService:
        deps = self._runtime_deps()
        return make_raid_data_source_service(
            self,
            readonly_connection_factory=deps.readonly_connection_factory,
            utcnow=deps.utcnow,
        )

    def _partner_setup_service(self) -> PartnerSetupService:
        deps = self._runtime_deps()
        return make_partner_setup_service(
            self,
            moderator_url_base=deps.twitch_api_base,
            mask_log_identifier=deps.mask_log_identifier,
            readonly_connection_factory=deps.readonly_connection_factory,
            transaction_factory=deps.transaction_factory,
        )

    def _offline_raid_orchestrator(self) -> OfflineRaidOrchestrator:
        return make_offline_raid_orchestrator(self)

    def _raid_metrics_store(self) -> RaidMetricsStore:
        deps = self._runtime_deps()
        return make_raid_metrics_store(
            self,
            readonly_connection_factory=deps.readonly_connection_factory,
            transaction_factory=deps.transaction_factory,
        )

    def _candidate_followers_service(self) -> CandidateFollowersService:
        return make_candidate_followers_service(self)

    def _resolve_bot_id_for_setup(self) -> str | None:
        chat_bot = self.chat_bot
        if chat_bot is not None:
            bot_id = getattr(chat_bot, "bot_id_safe", None)
            if bot_id is None:
                bot_id_raw = getattr(chat_bot, "bot_id", None)
                bot_id = (
                    str(bot_id_raw).strip()
                    if bot_id_raw and str(bot_id_raw).strip()
                    else None
                )
            if bot_id:
                return str(bot_id)

        fallback_bot_id = getattr(self, "_bot_id", None)
        if fallback_bot_id:
            return str(fallback_bot_id).strip() or None

        return os.getenv("TWITCH_BOT_USER_ID", "").strip() or None

    def has_enabled_auth(self, twitch_user_id: str) -> bool:
        auth_manager = getattr(self, "auth_manager", None)
        checker = getattr(auth_manager, "has_enabled_auth", None)
        if not callable(checker):
            return False
        return bool(checker(twitch_user_id))

    def _load_offline_auto_raid_eligibility(self, broadcaster_id: str) -> Any:
        deps = self._runtime_deps()
        with deps.readonly_connection_factory() as conn:
            return deps.load_offline_auto_raid_eligibility_fn(
                conn,
                twitch_user_id=broadcaster_id,
            )

    def _candidate_selection_service(self) -> CandidateSelectionService:
        deps = self._runtime_deps()
        return make_candidate_selection_service(
            self,
            recent_raid_cooldown_days=deps.raid_target_cooldown_days,
            load_partner_raid_score_map_fn=deps.load_partner_raid_score_map_fn,
            refresh_partner_raid_score_async_fn=deps.refresh_partner_raid_score_async_fn,
            readonly_connection_factory=deps.readonly_connection_factory,
        )

    def _raid_blacklist_service(self) -> RaidBlacklistService:
        deps = self._runtime_deps()
        return make_raid_blacklist_service(
            self,
            external_recruitment_raid_limit=deps.external_recruitment_raid_limit,
            external_recruitment_blacklist_grace_seconds=deps.external_recruitment_blacklist_grace_seconds,
            external_target_ban_check_delay_seconds=deps.external_target_ban_check_delay_seconds,
            readonly_connection_factory=deps.readonly_connection_factory,
            transaction_factory=deps.transaction_factory,
        )

    def _raid_pipeline_service(self) -> RaidPipelineService:
        return make_raid_pipeline_service(self)

    def _raid_tracking_runtime_service(self) -> RaidTrackingRuntimeService:
        return make_raid_tracking_runtime_service(self)

    def _raid_arrival_runtime(self) -> RaidArrivalRuntime:
        deps = self._runtime_deps()
        return make_raid_arrival_runtime(
            self,
            track_confirmed_partner_raid_fn=deps.track_confirmed_partner_raid_fn,
        )

    def _recruitment_messaging_service(self) -> RecruitmentMessagingService:
        deps = self._runtime_deps()
        return make_recruitment_messaging_service(
            self,
            readonly_connection_factory=deps.readonly_connection_factory,
        )

    @staticmethod
    def _signal_correlation_service() -> RaidSignalCorrelationService:
        return make_signal_correlation_service()

    def _increment_raid_observability_counter(self, name: str, amount: int = 1) -> int:
        service = self._make_raid_observability_service()
        value = service.increment_counter(name, amount)
        self._raid_observability_sequence = service.sequence
        self._raid_observability_counter_store = service.counter_store
        return value

    @staticmethod
    def _raid_observability_value(value: object, *, limit: int = 240) -> str:
        return RaidObservabilityService.normalize_value(value, limit=limit)

    def _format_raid_observability_fields(self, **fields: object) -> str:
        return self._make_raid_observability_service().format_fields(**fields)

    def _log_raid_observability_event(
        self,
        *,
        raid_flow_id: str,
        step: str,
        decision: str,
        level: int = logging.INFO,
        from_broadcaster_login: str | None = None,
        from_broadcaster_id: str | None = None,
        to_broadcaster_login: str | None = None,
        to_broadcaster_id: str | None = None,
        details: dict[str, object] | None = None,
    ) -> None:
        service = self._make_raid_observability_service()
        event = service.emit_event(
            flow_type="raid",
            flow_id=str(raid_flow_id or "").strip(),
            step=step,
            decision=decision,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            details=details or {},
        )
        payload = event.as_log_fields()
        self._last_raid_observability_event = payload
        self._raid_observability_sequence = service.sequence
        self._raid_observability_counter_store = service.counter_store
        log.log(level, "raid_flow %s", service.format_fields(**payload))

    def get_observability_snapshot(self) -> dict[str, Any]:
        state_store = self._raid_state_store()
        state_store.ensure_runtime_raid_tracking_state()
        state_store.cleanup_stale_raid_readiness_states()
        pending_raids = getattr(self, "_pending_raids", {}) or {}
        recent_arrivals = getattr(self, "_recent_raid_arrivals", {}) or {}
        orphan_notifications = getattr(self, "_orphan_chat_raid_notifications", {}) or {}
        readiness_by_flow = getattr(self, "_raid_readiness_by_flow_id", {}) or {}
        return {
            "pendingCount": len(pending_raids),
            "pendingTargets": sorted(
                self._format_pending_raid_key_for_log(key)
                for key in list(pending_raids.keys())[:10]
            ),
            "recentArrivalCount": len(recent_arrivals),
            "orphanChatNotificationCount": len(orphan_notifications),
            "readinessFlowCount": len(readiness_by_flow),
            "counters": dict(self._raid_observability_counters()),
            "lastEvent": getattr(self, "_last_raid_observability_event", None),
            "lastAnalyticsFollowersDiagnostic": getattr(
                self,
                "_last_analytics_followers_diagnostic",
                None,
            ),
        }

    def _build_analytics_followers_runtime_state(self) -> dict[str, object]:
        return build_analytics_followers_runtime_state(self)

    def _log_analytics_followers_decision(
        self,
        *,
        flow_id: str,
        flow: str,
        login: str,
        target_id: str | None,
        decision: str,
        reason: str,
        request_attempted: object,
        request_result: str,
        http_status: int | None,
        scope_state: dict[str, object],
        runtime_state: dict[str, object],
        level: int = logging.INFO,
        **extra_fields: object,
    ) -> None:
        deps = self._runtime_deps()
        log_analytics_followers_decision(
            self,
            flow_id=flow_id,
            flow=flow,
            login=login,
            target_id=target_id,
            decision=decision,
            reason=reason,
            request_attempted=request_attempted,
            request_result=request_result,
            http_status=http_status,
            scope_state=scope_state,
            runtime_state=runtime_state,
            level=level,
            insert_observability_event_fn=deps.insert_observability_event_fn,
            **extra_fields,
        )

    async def _resolve_bot_oauth_context(self) -> tuple[str | None, str | None, set[str]]:
        return await resolve_bot_oauth_context(self)

    def _warn_user_scope_fallback_once(
        self,
        *,
        area: str,
        subject: str,
    ) -> None:
        warn_user_scope_fallback_once(self, area=area, subject=subject)

    def _clear_user_scope_fallback_warning(
        self,
        *,
        area: str,
        subject: str,
    ) -> None:
        clear_user_scope_fallback_warning(self, area=area, subject=subject)

    @staticmethod
    async def _get_followers_total_result_with_legacy_fallback(
        api,
        user_id: str,
        *,
        user_token: str | None = None,
    ) -> dict[str, object]:
        return await get_followers_total_result_with_legacy_fallback(
            api,
            user_id,
            user_token=user_token,
        )

    @staticmethod
    def _row_value(row, key: str, index: int, default=None):
        return row_value(row, key, index, default)

    @staticmethod
    def _safe_int(value: object, default: int = 0) -> int:
        return safe_int(value, default)

    @staticmethod
    def _parse_datetime(value: object) -> datetime | None:
        return parse_datetime(value)

    def _lookup_silent_raid_enabled(self, broadcaster_login: str) -> bool:
        try:
            deps = self._runtime_deps()
            with deps.readonly_connection_factory() as conn:
                partner_row = deps.load_active_partner_fn(
                    conn,
                    twitch_login=self._normalize_broadcaster_login(broadcaster_login),
                )
                return bool(
                    int(
                        (
                            partner_row["silent_raid"]
                            if partner_row and hasattr(partner_row, "keys")
                            else (partner_row[15] if partner_row else 0)
                        )
                        or 0
                    )
                )
        except Exception:
            log.debug(
                "Raid arrival: silent_raid lookup failed for %s",
                broadcaster_login,
                exc_info=True,
            )
            return False
