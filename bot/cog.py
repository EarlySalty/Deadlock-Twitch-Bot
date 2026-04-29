"""Composable Twitch cog assembled from dedicated mixins."""

from __future__ import annotations

from .analytics.legacy_token import LegacyTokenAnalyticsMixin
from .analytics.mixin import TwitchAnalyticsMixin
from .base import TwitchBaseCog
from .community.admin import TwitchAdminMixin
from .community.leaderboard import (
    LeaderboardOptions,
    TwitchLeaderboardMixin,
    TwitchLeaderboardView,
)
from .community.partner_recruit import TwitchPartnerRecruitMixin
from .dashboard.mixin import TwitchDashboardMixin
from .highlight_clipper.mixin import HighlightClipperMixin
from .monitoring.monitoring import TwitchMonitoringMixin
from .raid.commands import RaidCommandsMixin
from .raid.mixin import TwitchRaidMixin
from .reload_mixin import TwitchReloadMixin

__all__ = [
    "TwitchStreamCog",
    "LeaderboardOptions",
    "TwitchLeaderboardView",
]


class TwitchStreamCog(
    HighlightClipperMixin,
    LegacyTokenAnalyticsMixin,
    TwitchAnalyticsMixin,
    TwitchRaidMixin,
    RaidCommandsMixin,
    TwitchPartnerRecruitMixin,
    TwitchDashboardMixin,
    TwitchLeaderboardMixin,
    TwitchAdminMixin,
    TwitchMonitoringMixin,
    TwitchReloadMixin,
    TwitchBaseCog,
):
    """Monitor Twitch-Streamer (Deadlock), poste Go-Live, sammle Stats, Dashboard, Auto-Raids."""

    # The mixins and base class provide the full implementation.
    pass
