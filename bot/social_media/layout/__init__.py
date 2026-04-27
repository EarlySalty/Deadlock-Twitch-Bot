from __future__ import annotations

"""
Layout model helpers for Social Media Phase 1.

Stored streamer layout JSON schema:
{
  "version": 1,
  "source": { "width": 1920, "height": 1080 },
  "game_crop": { "x": 0, "y": 0, "w": 1080, "h": 1080 },
  "cam_crop": { "x": 1500, "y": 50, "w": 380, "h": 380 },
  "cam_position": { "x": 0, "y": 0, "w": 1080, "h": 540 }
}

Validation rules:
- version must be 1
- 0 <= x/y
- w/h > 0
- x + w <= source.width
- y + h <= source.height
"""

from dataclasses import dataclass
from typing import Any, Mapping


class LayoutValidationError(ValueError):
    """Raised when layout payload validation fails."""


@dataclass(frozen=True)
class LayoutBox:
    x: int
    y: int
    w: int
    h: int

    @classmethod
    def from_mapping(cls, name: str, payload: Mapping[str, Any] | None) -> "LayoutBox":
        if not isinstance(payload, Mapping):
            raise LayoutValidationError(f"{name} must be an object")
        x = _require_int(payload.get("x"), f"{name}.x")
        y = _require_int(payload.get("y"), f"{name}.y")
        w = _require_int(payload.get("w"), f"{name}.w")
        h = _require_int(payload.get("h"), f"{name}.h")
        if x < 0:
            raise LayoutValidationError(f"{name}.x must be >= 0")
        if y < 0:
            raise LayoutValidationError(f"{name}.y must be >= 0")
        if w <= 0:
            raise LayoutValidationError(f"{name}.w must be > 0")
        if h <= 0:
            raise LayoutValidationError(f"{name}.h must be > 0")
        return cls(x=x, y=y, w=w, h=h)

    def validate_within(self, source: "LayoutSource", name: str) -> None:
        if self.x + self.w > source.width:
            raise LayoutValidationError(
                f"{name}.x + {name}.w must be <= source.width ({source.width})"
            )
        if self.y + self.h > source.height:
            raise LayoutValidationError(
                f"{name}.y + {name}.h must be <= source.height ({source.height})"
            )

    def to_dict(self) -> dict[str, int]:
        return {"x": self.x, "y": self.y, "w": self.w, "h": self.h}


@dataclass(frozen=True)
class LayoutSource:
    width: int
    height: int

    @classmethod
    def from_mapping(cls, payload: Mapping[str, Any] | None) -> "LayoutSource":
        if not isinstance(payload, Mapping):
            raise LayoutValidationError("source must be an object")
        width = _require_int(payload.get("width"), "source.width")
        height = _require_int(payload.get("height"), "source.height")
        if width <= 0:
            raise LayoutValidationError("source.width must be > 0")
        if height <= 0:
            raise LayoutValidationError("source.height must be > 0")
        return cls(width=width, height=height)

    def to_dict(self) -> dict[str, int]:
        return {"width": self.width, "height": self.height}


@dataclass(frozen=True)
class StreamerLayout:
    version: int
    source: LayoutSource
    game_crop: LayoutBox
    cam_crop: LayoutBox
    cam_position: LayoutBox
    cam_enabled: bool = True
    mode: str = "pip"

    @classmethod
    def from_mapping(
        cls,
        payload: Mapping[str, Any] | None,
        *,
        cam_enabled: bool | None = None,
        mode: str | None = None,
    ) -> "StreamerLayout":
        if not isinstance(payload, Mapping):
            raise LayoutValidationError("layout must be an object")
        version = _require_int(payload.get("version", 1), "version")
        if version != 1:
            raise LayoutValidationError("version must be 1")
        source = LayoutSource.from_mapping(payload.get("source"))
        game_crop = LayoutBox.from_mapping("game_crop", payload.get("game_crop"))
        cam_crop = LayoutBox.from_mapping("cam_crop", payload.get("cam_crop"))
        cam_position = LayoutBox.from_mapping("cam_position", payload.get("cam_position"))
        resolved_mode = str(mode if mode is not None else payload.get("mode", "pip")).strip().lower()
        if resolved_mode not in {"pip", "stacked"}:
            raise LayoutValidationError("mode must be one of: pip, stacked")
        resolved_cam_enabled = (
            bool(cam_enabled) if cam_enabled is not None else bool(payload.get("cam_enabled", True))
        )
        layout = cls(
            version=version,
            source=source,
            game_crop=game_crop,
            cam_crop=cam_crop,
            cam_position=cam_position,
            cam_enabled=resolved_cam_enabled,
            mode=resolved_mode,
        )
        layout.validate()
        return layout

    def validate(self) -> None:
        self.game_crop.validate_within(self.source, "game_crop")
        self.cam_crop.validate_within(self.source, "cam_crop")
        self.cam_position.validate_within(self.source, "cam_position")

    def to_layout_json(self) -> dict[str, Any]:
        return {
            "version": self.version,
            "source": self.source.to_dict(),
            "game_crop": self.game_crop.to_dict(),
            "cam_crop": self.cam_crop.to_dict(),
            "cam_position": self.cam_position.to_dict(),
        }

    def to_override_json(self) -> dict[str, Any]:
        payload = self.to_layout_json()
        payload["cam_enabled"] = self.cam_enabled
        payload["mode"] = self.mode
        return payload


def _require_int(value: Any, field_name: str) -> int:
    if isinstance(value, bool):
        raise LayoutValidationError(f"{field_name} must be an integer")
    try:
        return int(value)
    except (TypeError, ValueError) as exc:
        raise LayoutValidationError(f"{field_name} must be an integer") from exc


DEFAULT_STREAMER_LAYOUT = StreamerLayout.from_mapping(
    {
        "version": 1,
        "source": {"width": 1920, "height": 1080},
        "game_crop": {"x": 0, "y": 0, "w": 1080, "h": 1080},
        "cam_crop": {"x": 1500, "y": 50, "w": 380, "h": 380},
        "cam_position": {"x": 0, "y": 0, "w": 1080, "h": 540},
    }
)


from .storage import (  # noqa: E402
    apply_default_layout,
    get_clip_effective_layout,
    get_streamer_layout,
    set_clip_layout_override,
    upsert_streamer_layout,
)

__all__ = [
    "DEFAULT_STREAMER_LAYOUT",
    "LayoutBox",
    "LayoutSource",
    "LayoutValidationError",
    "StreamerLayout",
    "apply_default_layout",
    "get_clip_effective_layout",
    "get_streamer_layout",
    "set_clip_layout_override",
    "upsert_streamer_layout",
]
