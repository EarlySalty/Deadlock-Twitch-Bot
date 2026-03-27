from __future__ import annotations

from datetime import datetime
from ..core.constants import TWITCH_TARGET_GAME_NAME
from .partner_setup_service import PartnerSetupService
from .runtime_support import create_twitch_api


class RaidDataSetupFacadeMixin:
    def _get_target_game_lower(self) -> str:
        return str(TWITCH_TARGET_GAME_NAME or "").strip().lower()

    def _is_recent_deadlock(
        self,
        last_deadlock_seen_at: str | None,
        *,
        now_utc: datetime | None = None,
        recency_cap_seconds: int = 360,
    ) -> bool:
        return self._raid_data_source_service().is_recent_deadlock(
            last_deadlock_seen_at,
            now_utc=now_utc,
            recency_cap_seconds=recency_cap_seconds,
        )

    def _evaluate_deadlock_raid_source(
        self,
        *,
        current_game: str,
        had_deadlock_session: bool,
        last_deadlock_seen_at: str | None,
    ) -> dict[str, object]:
        return self._raid_data_source_service().evaluate_deadlock_raid_source(
            current_game=current_game,
            had_deadlock_session=had_deadlock_session,
            last_deadlock_seen_at=last_deadlock_seen_at,
        )

    def _is_deadlock_raid_source_eligible(
        self,
        *,
        last_game: str,
        had_deadlock_session: bool,
        last_deadlock_seen_at: str | None,
    ) -> bool:
        return self._raid_data_source_service().is_deadlock_raid_source_eligible(
            last_game=last_game,
            had_deadlock_session=had_deadlock_session,
            last_deadlock_seen_at=last_deadlock_seen_at,
        )

    def _is_deadlock_partner_candidate_eligible(
        self,
        *,
        game_name: str,
        had_deadlock_session: bool,
        last_deadlock_seen_at: str | None,
    ) -> bool:
        return self._raid_data_source_service().is_deadlock_partner_candidate_eligible(
            game_name=game_name,
            had_deadlock_session=had_deadlock_session,
            last_deadlock_seen_at=last_deadlock_seen_at,
        )

    def _load_partner_roster_for_raid(self, source_user_id: str) -> list[dict[str, object]]:
        return self._raid_data_source_service().load_partner_roster_for_raid(source_user_id)

    def _build_online_partner_candidates(
        self,
        partner_rows: list[dict[str, object]],
        streams_by_login: dict[str, dict],
    ) -> list[dict]:
        return self._raid_data_source_service().build_online_partner_candidates(
            partner_rows,
            streams_by_login,
        )

    def _load_partner_live_state_map(
        self,
        partner_logins_lower: list[str],
    ) -> dict[str, dict[str, object]]:
        return self._raid_data_source_service().load_partner_live_state_map(
            partner_logins_lower
        )

    def _filter_deadlock_eligible_partner_candidates(
        self,
        online_partners: list[dict],
    ) -> tuple[list[dict], list[str]]:
        return self._raid_data_source_service().filter_deadlock_eligible_partner_candidates(
            online_partners
        )

    def _load_broadcaster_live_state(self, broadcaster_id: str) -> dict[str, object]:
        return self._raid_data_source_service().load_broadcaster_live_state(broadcaster_id)

    def _calculate_stream_duration_sec(self, started_at: str | None) -> int:
        return self._raid_data_source_service().calculate_stream_duration_sec(started_at)

    def _raid_language_filters(self) -> list[str | None]:
        return self._raid_data_source_service().raid_language_filters()

    def _create_twitch_api(self, *, session=None):
        return create_twitch_api(self, session=session)

    async def _fetch_streams_by_logins_for_raid(
        self,
        logins: list[str],
        *,
        api=None,
    ) -> dict[str, dict]:
        return await self._raid_data_source_service().fetch_streams_by_logins_for_raid(
            logins,
            api=api,
        )

    def _overlay_broadcaster_live_state_from_stream(
        self,
        live_state: dict[str, object],
        stream_data: dict[str, object],
    ) -> dict[str, object]:
        return self._raid_data_source_service().overlay_broadcaster_live_state_from_stream(
            live_state,
            stream_data,
        )

    async def _resolve_manual_raid_source_state(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        api=None,
    ) -> dict[str, object]:
        return await self._raid_data_source_service().resolve_manual_raid_source_state(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            api=api,
        )

    async def _resolve_target_category_id(self, api=None) -> str | None:
        return await self._raid_data_source_service().resolve_target_category_id(api)

    def mark_manual_raid_started(self, broadcaster_id: str, ttl_seconds: float = 300.0) -> None:
        self._manual_raid_suppression_service().mark_manual_raid_started(
            broadcaster_id=broadcaster_id,
            ttl_seconds=ttl_seconds,
        )

    def is_offline_auto_raid_suppressed(self, broadcaster_id: str) -> bool:
        return self._manual_raid_suppression_service().is_offline_auto_raid_suppressed(
            broadcaster_id
        )

    def _resolve_streamer_id_by_login(self, broadcaster_login: str) -> str | None:
        return self._manual_raid_suppression_service().resolve_streamer_id_by_login(
            broadcaster_login
        )

    def _cleanup_expired_manual_raid_suppressions(self) -> None:
        self._manual_raid_suppression_service().cleanup_expired_manual_raid_suppressions()

    @staticmethod
    def _normalize_discord_user_id(raw: str | None) -> str | None:
        return PartnerSetupService.normalize_discord_user_id(raw)

    async def _resolve_discord_display_name(self, discord_user_id: str | None) -> str | None:
        return await self._partner_setup_service().resolve_discord_display_name(
            discord_user_id
        )

    async def _apply_streamer_role(
        self,
        discord_user_id: str | None,
        *,
        should_have_role: bool,
        reason: str,
    ) -> None:
        await self._partner_setup_service().apply_streamer_role(
            discord_user_id,
            should_have_role=should_have_role,
            reason=reason,
        )

    async def _sync_partner_state_after_auth(
        self,
        twitch_user_id: str,
        twitch_login: str,
        *,
        state_discord_user_id: str | None = None,
        activate_partner_features: bool = True,
    ) -> str | None:
        return await self._partner_setup_service().sync_partner_state_after_auth(
            twitch_user_id,
            twitch_login,
            state_discord_user_id=state_discord_user_id,
            activate_partner_features=activate_partner_features,
        )

    async def complete_setup_for_streamer(
        self,
        twitch_user_id: str,
        twitch_login: str,
        state_discord_user_id: str | None = None,
        activate_partner_features: bool = True,
    ):
        await self._partner_setup_service().complete_setup_for_streamer(
            twitch_user_id,
            twitch_login,
            state_discord_user_id=state_discord_user_id,
            activate_partner_features=activate_partner_features,
        )

    async def start_manual_raid(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
    ) -> dict[str, object]:
        return await self._offline_raid_orchestrator().start_manual_raid(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
        )

    async def handle_streamer_offline(
        self,
        broadcaster_id: str,
        broadcaster_login: str,
        viewer_count: int,
        stream_duration_sec: int,
        online_partners: list[dict],
        api=None,
        category_id: str | None = None,
        offline_trigger_ts: float | None = None,
    ) -> str | None:
        return await self._offline_raid_orchestrator().handle_streamer_offline(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            viewer_count=viewer_count,
            stream_duration_sec=stream_duration_sec,
            online_partners=online_partners,
            api=api,
            category_id=category_id,
            offline_trigger_ts=offline_trigger_ts,
        )
