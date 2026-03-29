"""Compatibility wrapper for the split runtime contracts.

New code should import from :mod:`bot.runtime.contracts` or the more specific
runtime modules under :mod:`bot.runtime`.
"""

from __future__ import annotations

from .runtime.contracts import *  # noqa: F401,F403
from .runtime.contracts import __all__  # noqa: F401
