"""Resolve effective plan and entitlements from persisted billing state."""

from __future__ import annotations

from typing import Any

from ..storage import pg as storage
from . import repository


def _is_missing_current_period_end_error(exc: Exception) -> bool:
    return repository.is_missing_current_period_end_error(exc)


def _load_billing_subscription(conn: Any, refs: list[str]) -> dict[str, Any] | None:
    return repository.load_billing_subscription(conn, refs)


def resolve_plan_snapshot_for_refs(
    refs: list[str] | tuple[str, ...],
    *,
    conn: Any | None = None,
    fallback_ref: str = "",
) -> dict[str, Any]:
    if conn is not None:
        return repository.resolve_plan_snapshot(conn, refs, fallback_ref=fallback_ref)

    with storage.readonly_connection() as local_conn:
        return repository.resolve_plan_snapshot(local_conn, refs, fallback_ref=fallback_ref)


def resolve_plan_snapshot_for_login(login: str, *, conn: Any | None = None) -> dict[str, Any]:
    normalized_login = str(login or "").strip().lower()
    if not normalized_login:
        return resolve_plan_snapshot_for_refs((), conn=conn, fallback_ref="")
    return resolve_plan_snapshot_for_refs((normalized_login,), conn=conn, fallback_ref=normalized_login)
