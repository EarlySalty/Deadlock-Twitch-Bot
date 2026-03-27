from __future__ import annotations

import logging
from dataclasses import dataclass, field
from datetime import UTC, datetime
from typing import Any, Callable

from ..partner_resolution import classify_partner_raid_arrival
from ..pending_raids import normalize_broadcaster_login


log = logging.getLogger("TwitchStreams.RaidManager")


@dataclass(slots=True, frozen=True)
class PartnerArrivalTrackingDependencies:
    readonly_connection: Callable[[], Any]
    transaction: Callable[[], Any]
    load_active_partner: Callable[..., Any]
    load_streamer_identity: Callable[..., Any]
    resolve_streamer_id_by_login: Callable[[str], str | None]
    mark_manual_raid_started: Callable[[str, float], Any]
    remember_recent_raid_arrival: Callable[..., Any]
    logger: logging.Logger = field(default_factory=lambda: log)
    utcnow: Callable[[], datetime] = lambda: datetime.now(UTC)


@dataclass(slots=True, frozen=True)
class IndependentPartnerArrivalResult:
    processed: bool
    classification: str | None = None
    source_resolution: str | None = None
    arrival_tracking_id: int | None = None


class PartnerArrivalTrackingService:
    def __init__(self, dependencies: PartnerArrivalTrackingDependencies) -> None:
        self._deps = dependencies

    def resolve_known_streamer_identity(
        self,
        *,
        broadcaster_login: str,
        broadcaster_id: str | None = None,
    ) -> dict[str, str] | None:
        login_key = normalize_broadcaster_login(broadcaster_login)
        broadcaster_key = str(broadcaster_id or "").strip()
        if not login_key and not broadcaster_key:
            return None
        try:
            with self._deps.readonly_connection() as conn:
                row = self._deps.load_streamer_identity(
                    conn,
                    twitch_user_id=broadcaster_key or None,
                    twitch_login=login_key or None,
                )
        except Exception:
            self._deps.logger.debug(
                "Konnte Streamer-Identity nicht auflösen: %s/%s",
                broadcaster_key,
                login_key,
                exc_info=True,
            )
            return None
        if not row:
            return None
        return {
            "twitch_user_id": str(
                row[0] if not hasattr(row, "keys") else row["twitch_user_id"] or ""
            ).strip(),
            "twitch_login": normalize_broadcaster_login(
                row[1] if not hasattr(row, "keys") else row["twitch_login"]
            ),
        }

    def is_partner_target_channel(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        expected_partner: bool = False,
    ) -> bool:
        if expected_partner:
            return True
        return bool(
            self.lookup_partner_target_channel(
                broadcaster_id=broadcaster_id,
                broadcaster_login=broadcaster_login,
            )
        )

    def lookup_partner_target_channel(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
    ) -> Any:
        broadcaster_key = str(broadcaster_id or "").strip()
        login_key = normalize_broadcaster_login(broadcaster_login)
        try:
            with self._deps.readonly_connection() as conn:
                return self._deps.load_active_partner(
                    conn,
                    twitch_user_id=broadcaster_key or None,
                    twitch_login=login_key or None,
                )
        except Exception:
            self._deps.logger.debug(
                "Partner target lookup failed for %s (%s)",
                login_key,
                broadcaster_key,
                exc_info=True,
            )
            return None

    def classify_partner_raid_arrival(
        self,
        *,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        expected_partner: bool = False,
    ) -> tuple[str | None, str]:
        partner_row = self.lookup_partner_target_channel(
            broadcaster_id=to_broadcaster_id,
            broadcaster_login=to_broadcaster_login,
        )
        result = classify_partner_raid_arrival(
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            partner_lookup=lambda **_kwargs: partner_row,
            known_streamer_lookup=lambda **lookup_kwargs: self.resolve_known_streamer_identity(
                broadcaster_login=str(lookup_kwargs.get("broadcaster_login") or ""),
                broadcaster_id=str(lookup_kwargs.get("broadcaster_id") or "") or None,
            ),
        )
        if result.classification is None and expected_partner:
            result = classify_partner_raid_arrival(
                from_broadcaster_login=from_broadcaster_login,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_id=to_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                partner_lookup=lambda **_kwargs: {"source": "pending_partner_override"},
                known_streamer_lookup=lambda **lookup_kwargs: self.resolve_known_streamer_identity(
                    broadcaster_login=str(lookup_kwargs.get("broadcaster_login") or ""),
                    broadcaster_id=str(lookup_kwargs.get("broadcaster_id") or "") or None,
                ),
            )
        return result.as_tuple()

    def load_recent_raid_history_reference(
        self,
        *,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
    ) -> tuple[int | None, str | None]:
        try:
            with self._deps.readonly_connection() as conn:
                row = conn.execute(
                    """
                    SELECT id, executed_at
                    FROM twitch_raid_history
                    WHERE LOWER(from_broadcaster_login) = %s
                      AND to_broadcaster_id = %s
                      AND COALESCE(success, FALSE) IS TRUE
                    ORDER BY executed_at DESC
                    LIMIT 1
                    """,
                    (
                        normalize_broadcaster_login(from_broadcaster_login),
                        str(to_broadcaster_id or "").strip(),
                    ),
                ).fetchone()
        except Exception:
            self._deps.logger.debug(
                "Could not load raid history reference for %s -> %s",
                from_broadcaster_login,
                to_broadcaster_id,
                exc_info=True,
            )
            return None, None
        if not row:
            return None, None
        raid_history_id = int(row[0] if not hasattr(row, "keys") else row["id"])
        executed_at = str(
            row[1] if not hasattr(row, "keys") else row["executed_at"] or ""
        ).strip() or None
        return raid_history_id, executed_at

    def store_partner_raid_arrival(
        self,
        *,
        from_broadcaster_id: str | None,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        viewer_count: int,
        classification: str,
        confirmation_signals: set[str],
        primary_signal: str,
        correlation_status: str,
        correlation_detail: str | None = None,
        source_resolution: str,
        raid_history_id: int | None = None,
        raid_history_executed_at: str | None = None,
        unraid_seen: bool = False,
    ) -> int | None:
        confirmation_signal_text = self.serialize_confirmation_signals(confirmation_signals)
        try:
            with self._deps.transaction() as conn:
                row = conn.execute(
                    """
                    INSERT INTO twitch_raid_arrival_tracking (
                        from_broadcaster_id,
                        from_broadcaster_login,
                        to_broadcaster_id,
                        to_broadcaster_login,
                        viewer_count,
                        classification,
                        confirmation_signals,
                        primary_signal,
                        correlation_status,
                        correlation_detail,
                        source_resolution,
                        raid_history_id,
                        raid_history_executed_at,
                        unraid_seen,
                        last_unraid_at
                    ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
                    RETURNING id
                    """,
                    (
                        str(from_broadcaster_id or "").strip() or None,
                        normalize_broadcaster_login(from_broadcaster_login),
                        str(to_broadcaster_id or "").strip(),
                        normalize_broadcaster_login(to_broadcaster_login),
                        int(viewer_count or 0),
                        str(classification or "").strip(),
                        confirmation_signal_text,
                        str(primary_signal or "").strip(),
                        str(correlation_status or "").strip(),
                        str(correlation_detail or "").strip() or None,
                        str(source_resolution or "").strip(),
                        raid_history_id,
                        raid_history_executed_at,
                        bool(unraid_seen),
                        self._deps.utcnow().isoformat() if unraid_seen else None,
                    ),
                ).fetchone()
            if not row:
                return None
            return int(row[0] if not hasattr(row, "keys") else row["id"])
        except Exception:
            self._deps.logger.exception(
                "Failed to store partner raid arrival: %s -> %s (%s)",
                from_broadcaster_login,
                to_broadcaster_login,
                correlation_status,
            )
            return None

    def update_partner_raid_arrival(
        self,
        *,
        arrival_tracking_id: int,
        confirmation_signals: set[str],
        unraid_seen: bool = False,
    ) -> None:
        if not arrival_tracking_id:
            return
        try:
            with self._deps.transaction() as conn:
                conn.execute(
                    """
                    UPDATE twitch_raid_arrival_tracking
                    SET confirmation_signals = %s,
                        last_signal_at = CURRENT_TIMESTAMP,
                        unraid_seen = CASE WHEN %s THEN TRUE ELSE unraid_seen END,
                        last_unraid_at = CASE WHEN %s THEN %s ELSE last_unraid_at END
                    WHERE id = %s
                    """,
                    (
                        self.serialize_confirmation_signals(confirmation_signals),
                        bool(unraid_seen),
                        bool(unraid_seen),
                        self._deps.utcnow().isoformat() if unraid_seen else None,
                        int(arrival_tracking_id),
                    ),
                )
        except Exception:
            self._deps.logger.debug(
                "Could not update partner raid arrival tracking row %s",
                arrival_tracking_id,
                exc_info=True,
            )

    def process_independent_partner_raid_arrival(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        viewer_count: int,
        signal_type: str,
        correlation_status: str,
        correlation_detail: str | None = None,
    ) -> bool:
        return self.process_independent_partner_raid_arrival_result(
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            viewer_count=viewer_count,
            signal_type=signal_type,
            correlation_status=correlation_status,
            correlation_detail=correlation_detail,
        ).processed

    def process_independent_partner_raid_arrival_result(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        viewer_count: int,
        signal_type: str,
        correlation_status: str,
        correlation_detail: str | None = None,
    ) -> IndependentPartnerArrivalResult:
        classification, source_resolution = self.classify_partner_raid_arrival(
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
        )
        if classification is None:
            return IndependentPartnerArrivalResult(processed=False)

        arrival_tracking_id = self.store_partner_raid_arrival(
            from_broadcaster_id=from_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            viewer_count=viewer_count,
            classification=classification,
            confirmation_signals={signal_type},
            primary_signal=signal_type,
            correlation_status=correlation_status,
            correlation_detail=correlation_detail,
            source_resolution=source_resolution,
        )
        if arrival_tracking_id is None:
            return IndependentPartnerArrivalResult(
                processed=False,
                classification=classification,
                source_resolution=source_resolution,
            )

        from_broadcaster_key = str(from_broadcaster_id or "").strip()
        if not from_broadcaster_key:
            from_broadcaster_key = (
                self._deps.resolve_streamer_id_by_login(from_broadcaster_login) or ""
            )
        if from_broadcaster_key:
            self._deps.mark_manual_raid_started(from_broadcaster_key, 180.0)

        self._deps.remember_recent_raid_arrival(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            viewer_count=viewer_count,
            classification=classification,
            confirmation_signals={signal_type},
            arrival_tracking_id=arrival_tracking_id,
        )
        return IndependentPartnerArrivalResult(
            processed=True,
            classification=classification,
            source_resolution=source_resolution,
            arrival_tracking_id=arrival_tracking_id,
        )

    @staticmethod
    def serialize_confirmation_signals(signals: set[str] | list[str] | tuple[str, ...]) -> str:
        return ",".join(
            sorted({str(signal).strip() for signal in signals if str(signal).strip()})
        )


__all__ = [
    "IndependentPartnerArrivalResult",
    "PartnerArrivalTrackingDependencies",
    "PartnerArrivalTrackingService",
]
