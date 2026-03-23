"""Typed dependency containers for dashboard route groups."""

from __future__ import annotations

from dataclasses import dataclass
from collections.abc import Callable, Mapping
from typing import Any


@dataclass(frozen=True, slots=True)
class EntryRouteDeps:
    """Dependencies used by entry, auth, and utility dashboard routes."""

    critical_scopes: tuple[str, ...]
    dashboard_v2_login_url: str
    dashboards_discord_login_url: str
    dashboards_login_url: str
    html: Any
    json: Any
    log: Any
    required_scopes: tuple[str, ...]
    scope_column_labels: Mapping[str, str]
    storage: Any


@dataclass(frozen=True, slots=True)
class BillingRouteDeps:
    """Dependencies used by billing and legal dashboard routes."""

    asyncio: Any
    billing_cycle_discounts: Mapping[int, Any]
    billing_is_paid_plan: Callable[[Any], bool]
    billing_public_error_message: Callable[..., str]
    billing_stripe_quickstart_url: str
    build_billing_catalog: Callable[[Any], dict[str, Any]]
    format_eur_cents: Callable[[int], str]
    json: Any
    log: Any
    normalize_billing_cycle: Callable[[Any], int]
    storage: Any


@dataclass(frozen=True, slots=True)
class MarketRouteDeps:
    """Dependencies used by the market research dashboard route."""

    json: Any
    log: Any
    storage: Any
    uuid4: Callable[[], Any]
