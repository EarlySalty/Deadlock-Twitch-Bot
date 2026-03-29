from __future__ import annotations

from bot.runtime.contracts import (
    BotRuntimeContainer,
    DashboardRuntimeContainer,
    RUNTIME_STATE_FIELDS,
    ensure_bot_runtime_container,
)
from bot.runtime_bootstrap import BotRuntimeBootstrap


class _DummyCog:
    def _parse_language_filters(self, value: str) -> list[str]:
        return [value.lower()]


def test_bot_runtime_contract_excludes_dashboard_fields() -> None:
    assert "_dashboard_host" not in RUNTIME_STATE_FIELDS
    assert "_internal_api_runner" in RUNTIME_STATE_FIELDS
    assert isinstance(ensure_bot_runtime_container(_DummyCog()), BotRuntimeContainer)
    assert isinstance(DashboardRuntimeContainer(), DashboardRuntimeContainer)


def test_bot_bootstrap_keeps_dashboard_values_as_compat_attrs(monkeypatch) -> None:
    monkeypatch.setattr(
        "bot.runtime_bootstrap.load_secret_value",
        lambda key, **kwargs: {
            "TWITCH_CLIENT_ID": "client-id",
            "TWITCH_CLIENT_SECRET": "client-secret",
            "TWITCH_BOT_CLIENT_ID": "bot-client-id",
            "TWITCH_BOT_CLIENT_SECRET": "bot-secret",
            "TWITCH_DASHBOARD_TOKEN": "dashboard-token",
            "TWITCH_PARTNER_TOKEN": "partner-token",
            "TWITCH_INTERNAL_API_TOKEN": "internal-token",
            "TWITCH_WEBHOOK_SECRET": None,
        }.get(key),
    )
    monkeypatch.setattr("bot.runtime_bootstrap.require_noauth_loopback_guard", lambda **_: None)

    cog = _DummyCog()
    bootstrap = BotRuntimeBootstrap(cog)
    bootstrap.configure_runtime()

    runtime = ensure_bot_runtime_container(cog)
    assert not hasattr(runtime.config, "dashboard_host")
    assert not hasattr(runtime.services, "web")
    assert getattr(cog, "_dashboard_host") == "127.0.0.1"
    assert getattr(cog, "_dashboard_token") == "dashboard-token"
    assert getattr(cog, "_partner_dashboard_token") == "partner-token"
