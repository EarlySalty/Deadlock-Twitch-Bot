from __future__ import annotations

import logging
from collections.abc import Callable
from contextlib import AbstractContextManager
from typing import Any


log = logging.getLogger("TwitchStreams.RaidManager")

ReadonlyConnectionFactory = Callable[[], AbstractContextManager[Any]]
TransactionFactory = Callable[[], AbstractContextManager[Any]]
NormalizeLoginFn = Callable[[str | None], str]
PartnerTargetLookup = Callable[..., bool]
NextFlowIdFn = Callable[..., str]


class RaidMetricsStore:
    def __init__(
        self,
        *,
        readonly_connection: ReadonlyConnectionFactory,
        transaction: TransactionFactory,
        normalize_broadcaster_login: NormalizeLoginFn,
        is_partner_target_channel: PartnerTargetLookup,
        next_raid_observability_flow_id: NextFlowIdFn,
        logger: logging.Logger | None = None,
    ) -> None:
        self._readonly_connection = readonly_connection
        self._transaction = transaction
        self._normalize_broadcaster_login = normalize_broadcaster_login
        self._is_partner_target_channel = is_partner_target_channel
        self._next_raid_observability_flow_id = next_raid_observability_flow_id
        self._logger = logger or log

    def get_received_network_raid_count(self, to_broadcaster_id: str) -> int:
        target_id = str(to_broadcaster_id or "").strip()
        if not target_id:
            return 0

        try:
            with self._readonly_connection() as conn:
                row = conn.execute(
                    """
                    SELECT COUNT(*)
                    FROM twitch_raid_history
                    WHERE to_broadcaster_id = %s
                      AND COALESCE(success, FALSE) IS TRUE
                    """,
                    (target_id,),
                ).fetchone()
            return int(row[0]) if row and row[0] is not None else 0
        except Exception:
            self._logger.debug(
                "Could not count received network raids for %s",
                target_id,
                exc_info=True,
            )
            return 0

    def get_confirmed_external_recruitment_raid_count(self, to_broadcaster_id: str) -> int:
        target_id = str(to_broadcaster_id or "").strip()
        if not target_id:
            return 0

        try:
            with self._readonly_connection() as conn:
                row = conn.execute(
                    """
                    SELECT COUNT(*)
                    FROM twitch_confirmed_external_recruitment_raids
                    WHERE to_broadcaster_id = %s
                    """,
                    (target_id,),
                ).fetchone()
            return int(row[0]) if row and row[0] is not None else 0
        except Exception:
            self._logger.debug(
                "Could not count confirmed external recruitment raids for %s",
                target_id,
                exc_info=True,
            )
            return 0

    def record_confirmed_external_recruitment_raid(
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
        target_id = str(to_broadcaster_id or "").strip()
        target_login = self._normalize_broadcaster_login(to_broadcaster_login)
        if not target_id or not target_login:
            return None

        normalized_flow_id = (
            str(raid_flow_id or "").strip()
            or self._next_raid_observability_flow_id(prefix="external-confirmed")
        )
        try:
            with self._transaction() as conn:
                conn.execute(
                    """
                    INSERT INTO twitch_confirmed_external_recruitment_raids (
                        raid_flow_id,
                        from_broadcaster_id,
                        from_broadcaster_login,
                        to_broadcaster_id,
                        to_broadcaster_login,
                        viewer_count,
                        confirmation_signal
                    )
                    VALUES (%s, %s, %s, %s, %s, %s, %s)
                    ON CONFLICT (raid_flow_id) DO NOTHING
                    """,
                    (
                        normalized_flow_id,
                        str(from_broadcaster_id or "").strip() or None,
                        self._normalize_broadcaster_login(from_broadcaster_login),
                        target_id,
                        target_login,
                        int(viewer_count or 0),
                        str(confirmation_signal or "").strip() or None,
                    ),
                )

                row = conn.execute(
                    """
                    SELECT COUNT(*)
                    FROM twitch_confirmed_external_recruitment_raids
                    WHERE to_broadcaster_id = %s
                    """,
                    (target_id,),
                ).fetchone()
            return int(row[0]) if row and row[0] is not None else 0
        except Exception:
            self._logger.exception(
                "Failed to persist confirmed external recruitment raid for %s (%s)",
                target_login,
                target_id,
            )
            try:
                return self.get_confirmed_external_recruitment_raid_count(target_id)
            except Exception:
                return None

    def is_target_currently_partner(
        self,
        *,
        target_id: str,
        target_login: str,
    ) -> bool:
        normalized_id = str(target_id or "").strip()
        normalized_login = self._normalize_broadcaster_login(target_login)
        if not normalized_id or not normalized_login:
            return False
        try:
            return self._is_partner_target_channel(
                broadcaster_id=normalized_id,
                broadcaster_login=normalized_login,
            )
        except Exception:
            self._logger.debug(
                "Partner lookup failed for %s (%s)",
                normalized_login,
                normalized_id,
                exc_info=True,
            )
            return False

    def get_recent_raid_targets(self, from_broadcaster_id: str, days: int) -> set[str]:
        normalized_id = str(from_broadcaster_id or "").strip()
        if not normalized_id or days <= 0:
            return set()
        cutoff = f"{int(days)} days"
        try:
            with self._readonly_connection() as conn:
                rows = conn.execute(
                    """
                    SELECT DISTINCT to_broadcaster_id
                    FROM twitch_raid_history
                    WHERE from_broadcaster_id = %s
                      AND COALESCE(success, FALSE) IS TRUE
                      AND executed_at >= NOW() - (%s::interval)
                    """,
                    (normalized_id, cutoff),
                ).fetchall()
            return {str(row[0]) for row in rows if row and row[0]}
        except Exception:
            self._logger.debug(
                "Failed to load recent raid targets for %s",
                normalized_id,
                exc_info=True,
            )
            return set()


__all__ = ["RaidMetricsStore"]
