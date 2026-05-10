"""Lazy compatibility helpers for legacy module paths."""

from __future__ import annotations

from importlib import import_module
from types import ModuleType
from typing import Any


def export_lazy(
    globals_dict: dict[str, Any],
    target: str,
    *,
    public: list[str] | tuple[str, ...] | None = None,
) -> None:
    """Expose a module lazily via ``__getattr__`` for legacy imports."""

    module_cache: ModuleType | None = None

    def _load() -> ModuleType:
        nonlocal module_cache
        if module_cache is None:
            module_cache = import_module(target, globals_dict["__package__"])  # nosemgrep: python.lang.security.audit.non-literal-import.non-literal-import
        return module_cache

    def __getattr__(name: str) -> Any:
        module = _load()
        value = getattr(module, name)
        globals_dict[name] = value
        return value

    def __dir__() -> list[str]:
        names = set(globals_dict)
        if public:
            names.update(public)
        return sorted(names)

    globals_dict["__getattr__"] = __getattr__
    globals_dict["__dir__"] = __dir__
    if public is not None:
        globals_dict["__all__"] = list(public)


def export_name_map(
    globals_dict: dict[str, Any],
    exports: dict[str, str],
) -> None:
    """Expose selected names lazily from one or more target modules."""

    module_cache: dict[str, ModuleType] = {}

    def _load(target: str) -> ModuleType:
        module = module_cache.get(target)
        if module is None:
            module = import_module(target, globals_dict["__package__"])  # nosemgrep: python.lang.security.audit.non-literal-import.non-literal-import
            module_cache[target] = module
        return module

    def __getattr__(name: str) -> Any:
        target = exports.get(name)
        if target is None:
            raise AttributeError(f"module {globals_dict['__name__']!r} has no attribute {name!r}")
        module = _load(target)
        value = getattr(module, name)
        globals_dict[name] = value
        return value

    def __dir__() -> list[str]:
        return sorted(set(globals_dict) | set(exports))

    globals_dict["__getattr__"] = __getattr__
    globals_dict["__dir__"] = __dir__
    globals_dict["__all__"] = sorted(exports)
