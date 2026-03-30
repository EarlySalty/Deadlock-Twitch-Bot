"""Dashboard package."""

from __future__ import annotations

from importlib import import_module
from typing import TYPE_CHECKING, Any

__all__ = [
    "DashboardRuntimeConfig",
    "DashboardRuntimeServices",
    "DashboardRuntimeState",
]

if TYPE_CHECKING:
    from .runtime import DashboardRuntimeConfig, DashboardRuntimeServices, DashboardRuntimeState


def __getattr__(name: str) -> Any:
    if name not in __all__:
        raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
    module = import_module(".runtime", __name__)
    value = getattr(module, name)
    globals()[name] = value
    return value


def __dir__() -> list[str]:
    return sorted(set(globals()) | set(__all__))
