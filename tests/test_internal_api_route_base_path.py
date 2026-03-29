from __future__ import annotations

from types import SimpleNamespace

from bot.internal_api.routes.raid import build_raid_route_defs
from bot.internal_api.routes.streamers import build_streamer_route_defs


def _paths(route_defs) -> set[str]:
    return {route_def.path for route_def in route_defs}


def test_streamer_route_defs_honor_server_base_path() -> None:
    server = SimpleNamespace(_base_path="/internal/custom/v2")

    paths = _paths(build_streamer_route_defs(server))

    assert "/internal/custom/v2/streamers" in paths
    assert "/internal/custom/v2/stats" in paths
    assert "/internal/custom/v2/analytics/comparison" in paths
    assert all(path.startswith("/internal/custom/v2/") for path in paths)


def test_raid_route_defs_honor_server_base_path() -> None:
    server = SimpleNamespace(_base_path="/internal/custom/v2")

    paths = _paths(build_raid_route_defs(server))

    assert "/internal/custom/v2/raid/auth-url" in paths
    assert "/internal/custom/v2/raid/requirements" in paths
    assert "/internal/custom/v2/raid/oauth-callback" in paths
    assert all(path.startswith("/internal/custom/v2/") for path in paths)
