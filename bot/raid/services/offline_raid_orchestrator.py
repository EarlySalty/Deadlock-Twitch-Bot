from __future__ import annotations

import logging
import time
from collections.abc import Awaitable, Callable, Mapping
from dataclasses import dataclass, field
from typing import Any


log = logging.getLogger("TwitchStreams.RaidManager")

CreateTwitchApi = Callable[[], Any | None]
ResolveManualSourceState = Callable[..., Awaitable[dict[str, object]]]
EvaluateDeadlockRaidSource = Callable[..., dict[str, object]]
SafeIntFn = Callable[[object, int], int]
CalculateStreamDuration = Callable[[str | None], int]
LoadPartnerRoster = Callable[[str], list[dict[str, object]]]
FetchStreamsByLogins = Callable[..., Awaitable[dict[str, dict[str, Any]]]]
BuildOnlinePartnerCandidates = Callable[[list[dict[str, object]], dict[str, dict[str, Any]]], list[dict[str, Any]]]
FilterEligiblePartners = Callable[[list[dict[str, Any]]], tuple[list[dict[str, Any]], list[str]]]
ResolveTargetCategoryId = Callable[..., Awaitable[str | None]]
ExecuteRaidPipeline = Callable[..., Awaitable[dict[str, object]]]
IsOfflineSuppressed = Callable[[str], bool]
LoadOfflineEligibility = Callable[[str], Any]
GetTargetGameLower = Callable[[], str]
MonotonicFn = Callable[[], float]


@dataclass(slots=True, frozen=True)
class OfflineRaidContext:
    target_game_lower: str
    last_game: str
    had_deadlock_session: bool
    last_deadlock_seen_at: str | None
    source_evaluation: dict[str, object]
    online_partners: list[dict[str, Any]]
    eligible_partners: list[dict[str, Any]]
    filtered_out: list[str]


@dataclass(slots=True)
class OfflineRaidOrchestrator:
    create_twitch_api: CreateTwitchApi
    resolve_manual_raid_source_state: ResolveManualSourceState
    evaluate_deadlock_raid_source: EvaluateDeadlockRaidSource
    safe_int: SafeIntFn
    calculate_stream_duration_sec: CalculateStreamDuration
    load_partner_roster_for_raid: LoadPartnerRoster
    fetch_streams_by_logins_for_raid: FetchStreamsByLogins
    build_online_partner_candidates: BuildOnlinePartnerCandidates
    filter_deadlock_eligible_partner_candidates: FilterEligiblePartners
    resolve_target_category_id: ResolveTargetCategoryId
    execute_raid_pipeline: ExecuteRaidPipeline
    is_offline_auto_raid_suppressed: IsOfflineSuppressed
    load_offline_auto_raid_eligibility: LoadOfflineEligibility
    get_target_game_lower: GetTargetGameLower
    logger: logging.Logger = field(default_factory=lambda: log)
    monotonic: MonotonicFn = time.monotonic

    def prepare_offline_auto_raid_context(
        self,
        *,
        broadcaster_id: str,
        previous_state: Mapping[str, object],
        streams_by_login: dict[str, dict[str, Any]],
    ) -> OfflineRaidContext:
        last_game = str(previous_state.get("last_game") or "").strip()
        had_deadlock_session = bool(
            int(previous_state.get("had_deadlock_in_session", 0) or 0)
        )
        last_deadlock_seen_at = (
            str(previous_state.get("last_deadlock_seen_at") or "").strip() or None
        )
        source_evaluation = self.evaluate_deadlock_raid_source(
            current_game=last_game,
            had_deadlock_session=had_deadlock_session,
            last_deadlock_seen_at=last_deadlock_seen_at,
        )
        partner_rows = self.load_partner_roster_for_raid(broadcaster_id)
        online_partners = self.build_online_partner_candidates(partner_rows, streams_by_login)
        eligible_partners, filtered_out = self.filter_deadlock_eligible_partner_candidates(
            online_partners
        )
        return OfflineRaidContext(
            target_game_lower=str(self.get_target_game_lower() or "").strip().lower(),
            last_game=last_game,
            had_deadlock_session=had_deadlock_session,
            last_deadlock_seen_at=last_deadlock_seen_at,
            source_evaluation=source_evaluation,
            online_partners=online_partners,
            eligible_partners=eligible_partners,
            filtered_out=filtered_out,
        )

    async def start_manual_raid(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
    ) -> dict[str, object]:
        api = self.create_twitch_api()
        source_state = await self.resolve_manual_raid_source_state(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            api=api,
        )
        live_state = dict(source_state.get("live_state") or {})
        if str(source_state.get("status") or "") == "source_not_live":
            self.logger.info(
                "Manual raid skipped for %s: broadcaster is not live (source=%s)",
                broadcaster_login,
                source_state.get("state_source") or "unknown",
            )
            return {
                "status": "source_not_live",
                "reason": str(source_state.get("state_source") or ""),
            }

        last_game = str(live_state.get("last_game") or "").strip()
        had_deadlock_session = bool(live_state.get("had_deadlock_in_session", False))
        last_deadlock_seen_at = (
            str(live_state.get("last_deadlock_seen_at") or "").strip() or None
        )
        source_evaluation = self.evaluate_deadlock_raid_source(
            current_game=last_game,
            had_deadlock_session=had_deadlock_session,
            last_deadlock_seen_at=last_deadlock_seen_at,
        )
        if not bool(source_evaluation.get("eligible")):
            self.logger.info(
                "Manual raid skipped for %s: source not Deadlock-eligible (reason=%s, current_game=%s, had_deadlock_session=%s, last_deadlock_seen_at=%s, source=%s)",
                broadcaster_login,
                source_evaluation.get("reason") or "unknown",
                last_game or "unbekannt",
                had_deadlock_session,
                last_deadlock_seen_at or "none",
                source_state.get("state_source") or "unknown",
            )
            return {
                "status": "source_not_eligible",
                "reason": str(source_evaluation.get("reason") or ""),
            }

        viewer_count = self.safe_int(live_state.get("last_viewer_count"), 0)
        stream_duration_sec = self.calculate_stream_duration_sec(
            str(live_state.get("last_started_at") or "").strip() or None
        )

        partner_rows = self.load_partner_roster_for_raid(broadcaster_id)
        streams_by_login = await self.fetch_streams_by_logins_for_raid(
            [str(row.get("twitch_login") or "") for row in partner_rows],
            api=api,
        )
        online_partners = self.build_online_partner_candidates(partner_rows, streams_by_login)
        eligible_partners, filtered_out = self.filter_deadlock_eligible_partner_candidates(
            online_partners
        )

        self.logger.info(
            "Manual raid pipeline started for %s (id=%s): viewers=%d, stream_duration=%ds, online_partners=%d, eligible_partners=%d",
            broadcaster_login,
            broadcaster_id,
            viewer_count,
            stream_duration_sec,
            len(online_partners),
            len(eligible_partners),
        )
        if filtered_out:
            self.logger.debug(
                "Manual raid: Partner ausgeschlossen (Kategorie/Session): %s",
                "; ".join(filtered_out),
            )

        category_id = await self.resolve_target_category_id(api)
        return await self.execute_raid_pipeline(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            viewer_count=viewer_count,
            stream_duration_sec=stream_duration_sec,
            online_partners=eligible_partners,
            api=api,
            category_id=category_id,
            reason="manual_chat_command",
            set_manual_suppression=True,
        )

    async def handle_streamer_offline(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        viewer_count: int,
        stream_duration_sec: int,
        online_partners: list[dict[str, Any]],
        api: Any = None,
        category_id: str | None = None,
        offline_trigger_ts: float | None = None,
    ) -> str | None:
        flow_start_ts = offline_trigger_ts if offline_trigger_ts is not None else self.monotonic()
        offline_trigger_ts = flow_start_ts

        if self.is_offline_auto_raid_suppressed(broadcaster_id):
            self.logger.info(
                "Auto-raid suppressed for %s (manual raid detected recently)",
                broadcaster_login,
            )
            return None

        eligibility = self.load_offline_auto_raid_eligibility(broadcaster_id)
        if not eligibility.active_partner and not getattr(eligibility, "auth_row_found", False):
            self.logger.debug("Streamer %s not found in DB", broadcaster_login)
            return None
        if not eligibility.active_partner:
            self.logger.debug("Raid bot disabled for %s (not active partner)", broadcaster_login)
            return None
        if not eligibility.raid_bot_enabled:
            self.logger.debug("Raid bot disabled for %s (setting)", broadcaster_login)
            return None
        if not eligibility.raid_auth_enabled:
            self.logger.debug("Raid bot disabled for %s (no auth)", broadcaster_login)
            return None

        self.logger.info(
            "Auto-raid pipeline started for %s (id=%s): viewers=%d, stream_duration=%ds, online_partners=%d",
            broadcaster_login,
            broadcaster_id,
            viewer_count,
            stream_duration_sec,
            len(online_partners),
        )
        result = await self.execute_raid_pipeline(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            viewer_count=viewer_count,
            stream_duration_sec=stream_duration_sec,
            online_partners=online_partners,
            api=api,
            category_id=category_id,
            offline_trigger_ts=offline_trigger_ts,
            reason="auto_raid_on_offline",
        )
        if str(result.get("status") or "") == "started":
            return str(result.get("target_login") or "") or None
        return None


__all__ = ["OfflineRaidContext", "OfflineRaidOrchestrator"]
