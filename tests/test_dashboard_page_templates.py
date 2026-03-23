from __future__ import annotations

from bot.dashboard.pages import (
    build_market_research_page,
    build_roadmap_body,
    build_scope_panel,
    build_stats_entry_page,
)
from bot.dashboard.raids.pages import build_raid_analytics_page, build_raid_history_page


def test_roadmap_body_contains_kanban_and_actions() -> None:
    body = build_roadmap_body()
    assert "kanban-board" in body
    assert "loadRoadmap" in body
    assert "addItem()" in body


def test_market_page_contains_data_route_and_chart() -> None:
    page = build_market_research_page()
    assert "Deadlock Market Research (Internal)" in page
    assert "/twitch/api/market_data" in page
    assert "marketChart" in page
    assert "integrity=" in page


def test_scope_panel_renders_success_and_missing_states() -> None:
    success = build_scope_panel(
        twitch_login="tester",
        missing_scopes=[],
        missing_critical=[],
        required_scopes=("a", "b"),
        critical_scopes=("a",),
        scope_column_labels={},
    )
    assert "Alle OAuth-Scopes vorhanden" in success

    missing = build_scope_panel(
        twitch_login="tester",
        missing_scopes=("a", "b"),
        missing_critical=("a",),
        required_scopes=("a", "b"),
        critical_scopes=("a",),
        scope_column_labels={"a": "Critical scope"},
    )
    assert "Fehlende OAuth-Scopes" in missing
    assert "Critical scope" in missing


def test_stats_entry_page_contains_expected_navigation() -> None:
    page = build_stats_entry_page(
        twitch_login="tester",
        logout_url="/logout",
        legacy_url="/legacy",
        beta_url="/beta",
        scope_panel="<div>scope</div>",
    )
    assert "Willkommen, tester!" in page
    assert "/logout" in page
    assert "/legacy" in page
    assert "/beta" in page
    assert "insights-panel" in page


def test_raid_page_helpers_render_expected_markers() -> None:
    history_page = build_raid_history_page("<tr><td>row</td></tr>")
    assert "Raid History" in history_page
    assert "Zurueck zum Dashboard" in history_page

    analytics_page = build_raid_analytics_page(
        partner_stats=[{"login": "alpha", "sent": 1, "received": 2, "balance": -1, "viewers_sent": 3, "viewers_recv": 4}],
        leechers=[],
        manual_list=[],
        date_min="2026-03-01",
        date_max="2026-03-23",
        total=1,
    )
    assert "Raid Analytics" in analytics_page
    assert "barChart" in analytics_page
    assert "integrity=" in analytics_page
