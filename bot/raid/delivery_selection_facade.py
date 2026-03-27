from __future__ import annotations

from .chat_targets import lookup_outbound_chat_suppression, make_chat_target
from .raid_pipeline import RaidPipelineRequest, is_retryable_raid_error
from .recruitment_messaging import RecruitmentMessagingService


class RaidDeliverySelectionFacadeMixin:
    async def _send_partner_raid_message(
        self,
        from_broadcaster_login: str,
        to_broadcaster_login: str,
        to_broadcaster_id: str,
        viewer_count: int,
    ):
        await self._partner_raid_delivery_service().send_partner_raid_message(
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            viewer_count=viewer_count,
        )

    def _get_received_network_raid_count(self, to_broadcaster_id: str) -> int:
        return self._raid_metrics_store().get_received_network_raid_count(to_broadcaster_id)

    def _get_confirmed_external_recruitment_raid_count(self, to_broadcaster_id: str) -> int:
        return self._raid_metrics_store().get_confirmed_external_recruitment_raid_count(
            to_broadcaster_id
        )

    def _record_confirmed_external_recruitment_raid(
        self,
        *,
        raid_flow_id: str | None,
        from_broadcaster_id: str | None,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        viewer_count: int,
        confirmation_signal: str,
    ) -> int | None:
        return self._raid_metrics_store().record_confirmed_external_recruitment_raid(
            raid_flow_id=raid_flow_id,
            from_broadcaster_id=from_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            viewer_count=viewer_count,
            confirmation_signal=confirmation_signal,
        )

    def _is_target_currently_partner(
        self,
        *,
        target_id: str,
        target_login: str,
    ) -> bool:
        return self._raid_metrics_store().is_target_currently_partner(
            target_id=target_id,
            target_login=target_login,
        )

    def _schedule_external_recruitment_blacklist_pending(
        self,
        *,
        target_id: str,
        target_login: str,
        confirmed_raid_count: int,
        raid_flow_id: str | None,
    ) -> None:
        self._raid_blacklist_service().schedule_external_recruitment_blacklist_pending(
            target_id=target_id,
            target_login=target_login,
            confirmed_raid_count=confirmed_raid_count,
            raid_flow_id=raid_flow_id,
        )

    def _delete_external_recruitment_blacklist_pending(self, target_id: str) -> None:
        self._raid_blacklist_service().delete_external_recruitment_blacklist_pending(
            target_id
        )

    def _process_due_external_recruitment_blacklist_pending(self) -> None:
        self._raid_blacklist_service().process_due_external_recruitment_blacklist_pending()

    def _schedule_external_target_ban_check(
        self,
        *,
        target_id: str | None,
        target_login: str,
        source: str,
    ) -> None:
        self._raid_blacklist_service().schedule_external_target_ban_check(
            target_id=target_id,
            target_login=target_login,
            source=source,
        )

    def _delete_external_target_ban_check_pending(self, target_id: str) -> None:
        self._raid_blacklist_service().delete_external_target_ban_check_pending(target_id)

    def _reschedule_external_target_ban_check_pending(
        self,
        target_id: str,
        delay_seconds: int = 900,
    ) -> None:
        self._raid_blacklist_service().reschedule_external_target_ban_check_pending(
            target_id,
            delay_seconds=delay_seconds,
        )

    async def _process_due_external_target_ban_checks(self) -> None:
        await self._raid_blacklist_service().process_due_external_target_ban_checks()

    @staticmethod
    def _parse_nonnegative_int(value: object) -> int | None:
        return RecruitmentMessagingService.parse_nonnegative_int(value)

    async def _resolve_recruitment_followers_total(
        self,
        *,
        login: str,
        target_id: str | None,
        target_stream_data: dict | None,
    ) -> int | None:
        return await self._recruitment_messaging_service().resolve_recruitment_followers_total(
            login=login,
            target_id=target_id,
            target_stream_data=target_stream_data,
            session=self.session,
        )

    async def _send_recruitment_message_now(
        self,
        from_broadcaster_login: str,
        to_broadcaster_login: str,
        target_stream_data: dict | None = None,
        confirmed_external_raid_count: int | None = None,
    ):
        await self._recruitment_messaging_service().send_recruitment_message_now(
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_login=to_broadcaster_login,
            target_stream_data=target_stream_data,
            confirmed_external_raid_count=confirmed_external_raid_count,
            session=getattr(self, "_session", None),
            chat_bot=self.chat_bot,
        )

    @staticmethod
    def _make_chat_target(login: str, user_id: str):
        return make_chat_target(login, user_id)

    def _lookup_outbound_chat_suppression(
        self,
        target_login: str,
        target_id: str | None,
        *,
        source: str,
    ) -> dict | None:
        return lookup_outbound_chat_suppression(
            self.chat_bot,
            target_login=target_login,
            target_id=target_id,
            source=source,
        )

    def _get_recent_raid_targets(self, from_broadcaster_id: str, days: int) -> set[str]:
        return self._raid_metrics_store().get_recent_raid_targets(
            from_broadcaster_id,
            days,
        )

    async def _attach_followers_totals(self, candidates: list[dict]) -> None:
        session = self.session
        if not candidates or session is None:
            return
        await self._candidate_followers_service().attach_followers_totals(
            candidates,
            session=session,
        )

    def _load_prepared_partner_scores(
        self,
        twitch_user_ids: list[str],
    ) -> dict[str, dict[str, object]]:
        return self._candidate_selection_service().load_prepared_partner_scores(
            twitch_user_ids
        )

    async def _refresh_partner_score_cache_if_available(
        self,
        twitch_user_id: str,
        *,
        reason: str,
    ) -> None:
        await self._candidate_selection_service().refresh_partner_score_cache_if_available(
            twitch_user_id,
            reason=reason,
        )

    async def _select_partner_candidate_by_score(
        self,
        candidates: list[dict],
        from_broadcaster_id: str,
    ) -> dict | None:
        return await self._candidate_selection_service().select_partner_candidate_by_score(
            candidates,
            from_broadcaster_id,
        )

    async def _select_fairest_candidate(
        self, candidates: list[dict], from_broadcaster_id: str
    ) -> dict | None:
        return await self._candidate_selection_service().select_fairest_candidate(
            candidates,
            from_broadcaster_id,
        )

    def _is_blacklisted(self, target_id: str, target_login: str) -> bool:
        return self._raid_blacklist_service().is_blacklisted(target_id, target_login)

    def _load_raid_blacklist(self) -> tuple[set[str], set[str]]:
        return self._raid_blacklist_service().load_raid_blacklist()

    def _add_to_blacklist(self, target_id: str, target_login: str, reason: str):
        self._raid_blacklist_service().add_to_blacklist(target_id, target_login, reason)

    def _is_retryable_raid_error(self, error: str | None) -> bool:
        return is_retryable_raid_error(error)

    async def _execute_raid_pipeline(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        viewer_count: int,
        stream_duration_sec: int,
        online_partners: list[dict],
        api=None,
        category_id: str | None = None,
        offline_trigger_ts: float | None = None,
        reason: str,
        set_manual_suppression: bool = False,
    ) -> dict[str, object]:
        return await self._raid_pipeline_service().execute(
            RaidPipelineRequest(
                broadcaster_id=broadcaster_id,
                broadcaster_login=broadcaster_login,
                viewer_count=viewer_count,
                stream_duration_sec=stream_duration_sec,
                online_partners=online_partners,
                session=self.session,
                api=api,
                category_id=category_id,
                offline_trigger_ts=offline_trigger_ts,
                reason=reason,
                set_manual_suppression=set_manual_suppression,
            )
        )
