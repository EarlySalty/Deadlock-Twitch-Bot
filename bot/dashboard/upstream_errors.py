"""Shared helpers for dashboard upstream error handling."""

from __future__ import annotations


def is_upstream_service_error(exc: Exception) -> bool:
    """Return True when an exception represents an upstream/service outage."""

    status = int(getattr(exc, "status", 0) or 0)
    code = str(getattr(exc, "code", "") or "").strip().lower()
    return status >= 500 or code.startswith("upstream_")
