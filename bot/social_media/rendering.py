from __future__ import annotations

from functools import lru_cache
from pathlib import Path
from typing import Any


_PACKAGE_DIR = Path(__file__).resolve().parent
_TEMPLATE_DIR = _PACKAGE_DIR / "templates"


@lru_cache(maxsize=32)
def _read_template(path: str) -> str:
    return Path(path).read_text(encoding="utf-8")


def _apply_substitutions(template: str, substitutions: dict[str, Any]) -> str:
    rendered = template
    for key, value in substitutions.items():
        rendered = rendered.replace(f"__{key.upper()}__", str(value))
    return rendered


def render_social_media_template(name: str, **substitutions: Any) -> str:
    template_path = _TEMPLATE_DIR / name
    template = _read_template(str(template_path)).lstrip("\ufeff\n")
    return _apply_substitutions(template, substitutions)


def render_social_media_dashboard(*, safe_streamer_label: str, safe_streamer_data: str) -> str:
    return render_social_media_template(
        "dashboard.html",
        safe_streamer_label=safe_streamer_label,
        safe_streamer_data=safe_streamer_data,
    )


def render_social_media_terms() -> str:
    return render_social_media_template("terms.html")


def render_social_media_privacy() -> str:
    return render_social_media_template("privacy.html")
