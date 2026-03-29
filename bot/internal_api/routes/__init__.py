"""Route group helpers for the internal API."""

from .raid import attach_raid_routes, build_raid_route_defs
from .streamers import attach_streamer_routes, build_streamer_route_defs

__all__ = [
    "attach_raid_routes",
    "attach_streamer_routes",
    "build_raid_route_defs",
    "build_streamer_route_defs",
]
