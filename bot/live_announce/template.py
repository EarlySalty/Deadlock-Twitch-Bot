"""Template engine for configurable Discord live announcements."""

from __future__ import annotations

from dataclasses import asdict, dataclass, field
from datetime import UTC, datetime
import hashlib
import json
from typing import Any, Mapping
from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit
import re

TWITCH_BRAND_COLOR = 0x9146FF
TWITCH_ICON_URL = "https://static.twitchcdn.net/assets/favicon-32-e29e246c157142c94346.png"
_PLACEHOLDER_RE = re.compile(r"\{([a-zA-Z0-9_]+)\}")
_MAX_TITLE = 256
_MAX_DESCRIPTION = 4096
_MAX_FIELDS = 25
_MAX_FIELD_NAME = 256
_MAX_FIELD_VALUE = 1024


def _coerce_str(value: Any, default: str = "") -> str:
    if value is None:
        return default
    text = str(value).strip()
    return text if text else default


def _coerce_bool(value: Any, default: bool = False) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        lowered = value.strip().lower()
        if lowered in {"1", "true", "yes", "on"}:
            return True
        if lowered in {"0", "false", "no", "off"}:
            return False
    if isinstance(value, (int, float)):
        return bool(value)
    return default


def _coerce_int(value: Any, default: int = 0) -> int:
    if isinstance(value, bool):
        return default
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


@dataclass(slots=True)
class AnnouncementAuthor:
    name_template: str = "LIVE: {channel}"
    icon_mode: str = "twitch"  # twitch|channel_avatar|none
    link_to_stream: bool = True

    @classmethod
    def from_dict(cls, data: Mapping[str, Any] | None) -> "AnnouncementAuthor":
        src = data or {}
        return cls(
            name_template=_coerce_str(src.get("name_template"), "LIVE: {channel}"),
            icon_mode=_coerce_str(src.get("icon_mode"), "twitch").lower(),
            link_to_stream=_coerce_bool(src.get("link_to_stream"), True),
        )


@dataclass(slots=True)
class AnnouncementField:
    name_template: str
    value_template: str
    inline: bool = True

    @classmethod
    def from_dict(cls, data: Mapping[str, Any] | None) -> "AnnouncementField":
        src = data or {}
        return cls(
            name_template=_coerce_str(src.get("name_template"), "Info"),
            value_template=_coerce_str(src.get("value_template"), "-"),
            inline=_coerce_bool(src.get("inline"), True),
        )


@dataclass(slots=True)
class AnnouncementFooter:
    text_template: str = "Auf Twitch ansehen fuer mehr Action!"
    icon_mode: str = "twitch"  # twitch|none
    timestamp_mode: str = "started_at"  # started_at|now|none

    @classmethod
    def from_dict(cls, data: Mapping[str, Any] | None) -> "AnnouncementFooter":
        src = data or {}
        return cls(
            text_template=_coerce_str(
                src.get("text_template"), "Auf Twitch ansehen fuer mehr Action!"
            ),
            icon_mode=_coerce_str(src.get("icon_mode"), "twitch").lower(),
            timestamp_mode=_coerce_str(src.get("timestamp_mode"), "started_at").lower(),
        )


@dataclass(slots=True)
class AnnouncementButton:
    label_template: str = "Auf Twitch ansehen"
    url_template: str = "{url}"
    force_stream_url: bool = True

    @classmethod
    def from_dict(cls, data: Mapping[str, Any] | None) -> "AnnouncementButton":
        src = data or {}
        return cls(
            label_template=_coerce_str(src.get("label_template"), "Auf Twitch ansehen"),
            url_template=_coerce_str(src.get("url_template"), "{url}"),
            force_stream_url=_coerce_bool(src.get("force_stream_url"), True),
        )


@dataclass(slots=True)
class AnnouncementImages:
    thumbnail_mode: str = "channel_avatar"  # channel_avatar|custom|none
    thumbnail_url_template: str = ""
    image_mode: str = "stream_thumbnail"  # stream_thumbnail|custom|none
    image_url_template: str = ""
    image_ratio: str = "16:9"  # 16:9|4:3
    cache_buster: bool = True

    @classmethod
    def from_dict(cls, data: Mapping[str, Any] | None) -> "AnnouncementImages":
        src = data or {}
        return cls(
            thumbnail_mode=_coerce_str(src.get("thumbnail_mode"), "channel_avatar").lower(),
            thumbnail_url_template=_coerce_str(src.get("thumbnail_url_template")),
            image_mode=_coerce_str(src.get("image_mode"), "stream_thumbnail").lower(),
            image_url_template=_coerce_str(src.get("image_url_template")),
            image_ratio=_coerce_str(src.get("image_ratio"), "16:9"),
            cache_buster=_coerce_bool(src.get("cache_buster"), True),
        )


@dataclass(slots=True)
class AnnouncementMentions:
    use_streamer_ping_role: bool = True
    streamer_ping_role_name_template: str = "{channel} LIVE PING"
    allowed_editor_role_ids: list[int] = field(default_factory=list)
    static_ping_role_ids: list[int] = field(default_factory=list)
    allow_everyone: bool = False

    @classmethod
    def from_dict(cls, data: Mapping[str, Any] | None) -> "AnnouncementMentions":
        src = data or {}
        editor_roles = src.get("allowed_editor_role_ids") or []
        ping_roles = src.get("static_ping_role_ids") or []
        return cls(
            use_streamer_ping_role=_coerce_bool(src.get("use_streamer_ping_role"), True),
            streamer_ping_role_name_template=_coerce_str(
                src.get("streamer_ping_role_name_template"),
                "{channel} LIVE PING",
            ),
            allowed_editor_role_ids=[
                _coerce_int(item, -1)
                for item in editor_roles
                if _coerce_int(item, -1) > 0
            ],
            static_ping_role_ids=[
                _coerce_int(item, -1)
                for item in ping_roles
                if _coerce_int(item, -1) > 0
            ],
            allow_everyone=False,
        )


@dataclass(slots=True)
class LiveAnnouncementConfig:
    content_template: str = "{mention_role}"
    color: str | int = TWITCH_BRAND_COLOR
    author: AnnouncementAuthor = field(default_factory=AnnouncementAuthor)
    title_template: str = "{channel} ist LIVE in {game}!"
    title_link_to_stream: bool = True
    description_mode: str = "stream_title"  # stream_title|custom|custom_plus_title
    description_template: str = "{title}"
    short_description: bool = False
    fields: list[AnnouncementField] = field(
        default_factory=lambda: [
            AnnouncementField(name_template="Viewer", value_template="{viewer_count}", inline=True),
            AnnouncementField(name_template="Kategorie", value_template="{game}", inline=True),
        ]
    )
    images: AnnouncementImages = field(default_factory=AnnouncementImages)
    footer: AnnouncementFooter = field(default_factory=AnnouncementFooter)
    button: AnnouncementButton = field(default_factory=AnnouncementButton)
    mentions: AnnouncementMentions = field(default_factory=AnnouncementMentions)

    @classmethod
    def from_dict(cls, data: Mapping[str, Any] | None) -> "LiveAnnouncementConfig":
        src = data or {}
        fields_raw = src.get("fields") if isinstance(src.get("fields"), list) else []
        parsed_fields = [AnnouncementField.from_dict(item) for item in fields_raw]
        if not parsed_fields:
            parsed_fields = list(cls().fields)
        return cls(
            content_template=_coerce_str(
                src.get("content_template"), "{mention_role}"
            ),
            color=src.get("color", TWITCH_BRAND_COLOR),
            author=AnnouncementAuthor.from_dict(src.get("author")),
            title_template=_coerce_str(src.get("title_template"), "{channel} ist LIVE in {game}!"),
            title_link_to_stream=_coerce_bool(src.get("title_link_to_stream"), True),
            description_mode=_coerce_str(src.get("description_mode"), "stream_title").lower(),
            description_template=_coerce_str(src.get("description_template"), "{title}"),
            short_description=_coerce_bool(src.get("short_description"), False),
            fields=parsed_fields,
            images=AnnouncementImages.from_dict(src.get("images")),
            footer=AnnouncementFooter.from_dict(src.get("footer")),
            button=AnnouncementButton.from_dict(src.get("button")),
            mentions=AnnouncementMentions.from_dict(src.get("mentions")),
        )

    def to_dict(self) -> dict[str, Any]:
        data = asdict(self)
        data["color"] = self.color
        return data


def shorten_text(text: str, max_length: int, *, suffix: str = "...") -> str:
    if max_length <= 0:
        return ""
    if len(text) <= max_length:
        return text
    if len(suffix) >= max_length:
        return text[:max_length]
    return f"{text[: max_length - len(suffix)]}{suffix}"


def render_placeholders(template: str, context: Mapping[str, Any]) -> str:
    text = str(template or "")

    def repl(match: re.Match[str]) -> str:
        key = match.group(1)
        if key not in context:
            return match.group(0)
        return str(context.get(key, ""))

    return _PLACEHOLDER_RE.sub(repl, text)


def parse_embed_color(value: Any, *, fallback: int = TWITCH_BRAND_COLOR) -> int:
    if isinstance(value, int) and 0 <= value <= 0xFFFFFF:
        return value
    if isinstance(value, str):
        raw = value.strip().lower()
        base = 10
        if raw.startswith("#"):
            raw = raw[1:]
            base = 16
        elif raw.startswith("0x"):
            raw = raw[2:]
            base = 16
        elif re.fullmatch(r"[0-9]+", raw or ""):
            base = 10
        elif re.fullmatch(r"[0-9a-f]+", raw or ""):
            base = 16
        else:
            return fallback
        try:
            parsed = int(raw, base)
        except (TypeError, ValueError):
            return fallback
        if 0 <= parsed <= 0xFFFFFF:
            return parsed
    return fallback


def is_valid_http_url(url: str) -> bool:
    candidate = str(url or "").strip()
    if not candidate:
        return False
    try:
        parts = urlsplit(candidate)
    except Exception:
        return False
    if parts.scheme.lower() not in {"http", "https"}:
        return False
    if not parts.netloc:
        return False
    if any(ch.isspace() for ch in candidate):
        return False
    return True


def _parse_started_at(value: Any) -> datetime | None:
    if isinstance(value, datetime):
        return value.astimezone(UTC) if value.tzinfo else value.replace(tzinfo=UTC)
    raw = str(value or "").strip()
    if not raw:
        return None
    try:
        dt = datetime.fromisoformat(raw.replace("Z", "+00:00"))
    except ValueError:
        return None
    if dt.tzinfo is None:
        return dt.replace(tzinfo=UTC)
    return dt.astimezone(UTC)


def _fmt_uptime(started_at: datetime | None, now: datetime) -> str:
    if started_at is None:
        return "0m"
    delta_seconds = max(0, int((now - started_at).total_seconds()))
    hours = delta_seconds // 3600
    minutes = (delta_seconds % 3600) // 60
    if hours > 0:
        return f"{hours}h {minutes:02d}m"
    return f"{minutes}m"


def _stable_cache_buster_value(seed: str | None, *, now: datetime) -> str:
    normalized_seed = _coerce_str(seed)
    if not normalized_seed:
        return str(int(now.timestamp()))
    return hashlib.sha256(normalized_seed.encode("utf-8")).hexdigest()[:16]


def _stream_thumbnail_url(
    raw_url: str,
    *,
    ratio: str,
    cache_buster: bool,
    now: datetime,
    cache_buster_seed: str | None = None,
) -> str:
    if not raw_url:
        return ""
    width, height = (1280, 720) if ratio != "4:3" else (960, 720)
    resolved = raw_url.replace("{width}", str(width)).replace("{height}", str(height))
    if not cache_buster:
        return resolved
    parts = urlsplit(resolved)
    query = dict(parse_qsl(parts.query, keep_blank_values=True))
    query["cb"] = _stable_cache_buster_value(cache_buster_seed, now=now)
    return urlunsplit((parts.scheme, parts.netloc, parts.path, urlencode(query), parts.fragment))


def build_template_context(
    streamer_login: str,
    stream: Mapping[str, Any] | None,
    *,
    now: datetime | None = None,
) -> dict[str, str]:
    now_utc = now.astimezone(UTC) if isinstance(now, datetime) else datetime.now(tz=UTC)
    data = stream or {}
    channel = _coerce_str(data.get("user_name"), _coerce_str(streamer_login))
    login = _coerce_str(data.get("user_login"), _coerce_str(streamer_login).lower())
    started_at = _parse_started_at(data.get("started_at") or data.get("start_time"))
    tags_raw = data.get("tags")
    if isinstance(tags_raw, list):
        tags = ", ".join(str(tag).strip() for tag in tags_raw if str(tag).strip())
    else:
        tags = _coerce_str(tags_raw)
    url = _coerce_str(data.get("url"), f"https://www.twitch.tv/{login or streamer_login}")
    return {
        "channel": channel or streamer_login,
        "login": login or streamer_login.lower(),
        "url": url,
        "title": _coerce_str(data.get("title"), "Live!"),
        "viewer_count": str(_coerce_int(data.get("viewer_count"), 0)),
        "started_at": started_at.isoformat() if started_at else "",
        "language": _coerce_str(data.get("language"), "de"),
        "tags": tags,
        "uptime": _fmt_uptime(started_at, now_utc),
        "game": _coerce_str(data.get("game_name"), "Deadlock"),
        "stream_thumbnail_url": _coerce_str(data.get("thumbnail_url")),
        "channel_avatar_url": _coerce_str(
            data.get("profile_image_url") or data.get("avatar_url") or data.get("user_avatar_url")
        ),
        "now": now_utc.isoformat(),
    }


def render_announcement_payload(
    config: LiveAnnouncementConfig,
    context: Mapping[str, Any],
    *,
    now: datetime | None = None,
    cache_buster_seed: str | None = None,
) -> dict[str, Any]:
    now_utc = now.astimezone(UTC) if isinstance(now, datetime) else datetime.now(tz=UTC)
    ctx = {k: str(v) if not isinstance(v, str) else v for k, v in context.items()}

    title = render_placeholders(config.title_template, ctx)
    stream_title = render_placeholders("{title}", ctx)
    description_custom = render_placeholders(config.description_template, ctx)
    if config.description_mode == "custom":
        description = description_custom
    elif config.description_mode == "custom_plus_title":
        description = (
            f"{description_custom}\n\n{stream_title}" if description_custom and stream_title else description_custom
        )
    else:
        description = stream_title or description_custom
    if config.short_description:
        description = shorten_text(description, _MAX_DESCRIPTION)

    author_name = render_placeholders(config.author.name_template, ctx)
    author_icon_url = ""
    if config.author.icon_mode == "twitch":
        author_icon_url = TWITCH_ICON_URL
    elif config.author.icon_mode == "channel_avatar":
        author_icon_url = ctx.get("channel_avatar_url", "")

    thumbnail_url = ""
    if config.images.thumbnail_mode == "custom":
        thumbnail_url = render_placeholders(config.images.thumbnail_url_template, ctx)
    elif config.images.thumbnail_mode == "channel_avatar":
        thumbnail_url = ctx.get("channel_avatar_url", "")

    image_url = ""
    if config.images.image_mode == "custom":
        image_url = render_placeholders(config.images.image_url_template, ctx)
    elif config.images.image_mode == "stream_thumbnail":
        image_url = _stream_thumbnail_url(
            ctx.get("stream_thumbnail_url", ""),
            ratio=config.images.image_ratio,
            cache_buster=config.images.cache_buster,
            now=now_utc,
            cache_buster_seed=cache_buster_seed,
        )

    button_url = (
        ctx.get("url", "")
        if config.button.force_stream_url
        else render_placeholders(config.button.url_template, ctx)
    )

    fields: list[dict[str, Any]] = []
    for field_cfg in config.fields[:_MAX_FIELDS]:
        fields.append(
            {
                "name": render_placeholders(field_cfg.name_template, ctx),
                "value": render_placeholders(field_cfg.value_template, ctx),
                "inline": bool(field_cfg.inline),
            }
        )

    timestamp_value: str | None = None
    if config.footer.timestamp_mode == "now":
        timestamp_value = now_utc.isoformat()
    elif config.footer.timestamp_mode == "started_at":
        started = _parse_started_at(ctx.get("started_at"))
        timestamp_value = started.isoformat() if started else now_utc.isoformat()

    footer_icon_url = TWITCH_ICON_URL if config.footer.icon_mode == "twitch" else ""
    embed_url = ctx.get("url", "") if config.title_link_to_stream else ""
    author_url = ctx.get("url", "") if config.author.link_to_stream else ""
    content = render_placeholders(config.content_template, ctx).strip()

    return {
        "content": content,
        "embed": {
            "title": title,
            "url": embed_url,
            "description": description,
            "color": parse_embed_color(config.color),
            "author": {
                "name": author_name,
                "url": author_url,
                "icon_url": author_icon_url,
            },
            "fields": fields,
            "thumbnail": {"url": thumbnail_url} if thumbnail_url else None,
            "image": {"url": image_url} if image_url else None,
            "footer": {
                "text": render_placeholders(config.footer.text_template, ctx),
                "icon_url": footer_icon_url,
            },
            "timestamp": timestamp_value,
        },
        "button": {
            "label": render_placeholders(config.button.label_template, ctx),
            "url": button_url,
        },
        "allowed_mentions": {
            "allow_everyone": bool(config.mentions.allow_everyone),
            "role_ids": list(config.mentions.static_ping_role_ids),
            "use_streamer_ping_role": bool(config.mentions.use_streamer_ping_role),
            "streamer_role_name": render_placeholders(
                config.mentions.streamer_ping_role_name_template,
                ctx,
            ),
        },
    }


def validate_live_announcement_config(
    config: LiveAnnouncementConfig,
    *,
    context: Mapping[str, Any] | None = None,
) -> list[str]:
    sample_context = context or {
        "channel": "EarlySalty",
        "url": "https://www.twitch.tv/earlysalty",
        "title": "Ranked Grind",
        "viewer_count": "123",
        "started_at": "2026-03-03T12:00:00+00:00",
        "language": "de",
        "tags": "deadlock, ranked",
        "uptime": "1h 20m",
        "game": "Deadlock",
        "stream_thumbnail_url": "https://static-cdn.jtvnw.net/previews-ttv/live_user_earlysalty-{width}x{height}.jpg",
        "channel_avatar_url": "https://example.com/avatar.png",
    }
    payload = render_announcement_payload(config, sample_context)
    errors: list[str] = []

    title = str(payload["embed"].get("title") or "")
    if len(title) > _MAX_TITLE:
        errors.append(f"embed.title exceeds {_MAX_TITLE} chars")

    description = str(payload["embed"].get("description") or "")
    if len(description) > _MAX_DESCRIPTION:
        errors.append(f"embed.description exceeds {_MAX_DESCRIPTION} chars")

    if len(config.fields) > _MAX_FIELDS:
        errors.append(f"embed.fields exceeds {_MAX_FIELDS} entries")

    fields = payload["embed"].get("fields") or []
    for index, field_value in enumerate(fields, start=1):
        name_text = str(field_value.get("name") or "")
        value_text = str(field_value.get("value") or "")
        if len(name_text) > _MAX_FIELD_NAME:
            errors.append(f"embed.fields[{index}].name exceeds {_MAX_FIELD_NAME} chars")
        if len(value_text) > _MAX_FIELD_VALUE:
            errors.append(f"embed.fields[{index}].value exceeds {_MAX_FIELD_VALUE} chars")

    button_url = str((payload.get("button") or {}).get("url") or "")
    if not is_valid_http_url(button_url):
        errors.append("button.url must be a valid http/https URL")

    thumbnail = payload["embed"].get("thumbnail") or {}
    thumbnail_url = str(thumbnail.get("url") or "")
    if thumbnail_url and not is_valid_http_url(thumbnail_url):
        errors.append("embed.thumbnail.url must be a valid http/https URL")

    image = payload["embed"].get("image") or {}
    image_url = str(image.get("url") or "")
    if image_url and not is_valid_http_url(image_url):
        errors.append("embed.image.url must be a valid http/https URL")

    return errors


# ---------------------------------------------------------------------------
# Backward-compatible helper aliases used by dashboard/monitoring modules.
# ---------------------------------------------------------------------------
SUPPORTED_PLACEHOLDERS: tuple[str, ...] = (
    "channel",
    "url",
    "title",
    "viewer_count",
    "started_at",
    "language",
    "tags",
    "uptime",
    "game",
    "mention_role",
)


def default_live_announcement_config() -> dict[str, Any]:
    return LiveAnnouncementConfig().to_dict()


def deep_merge_config(base: dict[str, Any], override: dict[str, Any] | None) -> dict[str, Any]:
    merged = json.loads(json.dumps(base))
    if not isinstance(override, dict):
        return merged
    for key, value in override.items():
        if isinstance(value, dict) and isinstance(merged.get(key), dict):
            merged[key] = deep_merge_config(merged.get(key) or {}, value)
        else:
            merged[key] = value
    return merged


def parse_config_json(raw: str | None) -> dict[str, Any]:
    text = str(raw or "").strip()
    if not text:
        return default_live_announcement_config()
    try:
        parsed = json.loads(text)
    except Exception:
        return default_live_announcement_config()
    if not isinstance(parsed, dict):
        return default_live_announcement_config()
    return deep_merge_config(default_live_announcement_config(), parsed)


def build_stream_context(
    *,
    login: str,
    stream: Mapping[str, Any] | None,
    mention_role: str = "",
    now: datetime | None = None,
) -> dict[str, str]:
    context = build_template_context(login, stream, now=now)
    context["mention_role"] = str(mention_role or "")
    return context


class _ValidationIssue:
    __slots__ = ("path", "message")

    def __init__(self, path: str, message: str) -> None:
        self.path = path
        self.message = message


def validate_config(config: dict[str, Any]) -> list[_ValidationIssue]:
    cfg_obj = LiveAnnouncementConfig.from_dict(_to_template_compatible_dict(config))
    errors = validate_live_announcement_config(cfg_obj)
    return [_ValidationIssue("config", err) for err in errors]


def _to_template_compatible_dict(config: dict[str, Any]) -> dict[str, Any]:
    if "embed" not in config:
        return config
    embed = config.get("embed") if isinstance(config.get("embed"), dict) else {}
    author = embed.get("author") if isinstance(embed.get("author"), dict) else {}
    footer = embed.get("footer") if isinstance(embed.get("footer"), dict) else {}
    image = embed.get("image") if isinstance(embed.get("image"), dict) else {}
    thumbnail = embed.get("thumbnail") if isinstance(embed.get("thumbnail"), dict) else {}
    fields = embed.get("fields") if isinstance(embed.get("fields"), list) else []
    button = config.get("button") if isinstance(config.get("button"), dict) else {}
    mentions = config.get("mentions") if isinstance(config.get("mentions"), dict) else {}
    mention_role = str(mentions.get("role_id") or "").strip()
    return {
        "content_template": str(config.get("content") or ""),
        "color": embed.get("color", TWITCH_BRAND_COLOR),
        "author": {
            "name_template": str(author.get("name") or "LIVE: {channel}"),
            "icon_mode": (
                "twitch"
                if str(author.get("icon_mode") or "").strip().lower() in {"twitch_logo", "twitch"}
                else str(author.get("icon_mode") or "none").strip().lower()
            ),
            "link_to_stream": bool(author.get("link_to_channel", True)),
        },
        "title_template": str(embed.get("title") or "{channel} ist LIVE in {game}!"),
        "title_link_to_stream": bool(embed.get("title_link_enabled", True)),
        "description_mode": str(embed.get("description_mode") or "stream_title"),
        "description_template": str(embed.get("description") or "{title}"),
        "short_description": bool(embed.get("shorten", False)),
        "fields": [
            {
                "name_template": str(field.get("name") or ""),
                "value_template": str(field.get("value") or ""),
                "inline": bool(field.get("inline", True)),
            }
            for field in fields
            if isinstance(field, dict)
        ],
        "images": {
            "thumbnail_mode": (
                "custom"
                if str(thumbnail.get("mode") or "").strip().lower() == "custom_url"
                else str(thumbnail.get("mode") or "none").strip().lower()
            ),
            "thumbnail_url_template": str(thumbnail.get("custom_url") or ""),
            "image_mode": (
                "stream_thumbnail"
                if bool(image.get("use_stream_thumbnail", True))
                else ("custom" if str(image.get("custom_url") or "").strip() else "none")
            ),
            "image_url_template": str(image.get("custom_url") or ""),
            "image_ratio": str(image.get("format") or "16:9"),
            "cache_buster": bool(image.get("cache_buster", True)),
        },
        "footer": {
            "text_template": str(footer.get("text") or ""),
            "icon_mode": (
                "twitch"
                if str(footer.get("icon_mode") or "").strip().lower() in {"twitch_logo", "twitch"}
                else "none"
            ),
            "timestamp_mode": str(footer.get("timestamp_mode") or "started_at"),
        },
        "button": {
            "label_template": str(button.get("label") or "Auf Twitch ansehen"),
            "url_template": str(button.get("url_template") or "{url}"),
            "force_stream_url": True,
        },
        "mentions": {
            "use_streamer_ping_role": bool(mentions.get("enabled", True)),
            "streamer_ping_role_name_template": "{channel} LIVE PING",
            "allowed_editor_role_ids": [
                int(role_id)
                for role_id in (config.get("allowed_editor_role_ids") or [])
                if str(role_id).isdigit()
            ],
            "static_ping_role_ids": [int(mention_role)] if mention_role.isdigit() else [],
            "allow_everyone": False,
        },
    }


__all__ = [
    "SUPPORTED_PLACEHOLDERS",
    "AnnouncementAuthor",
    "AnnouncementButton",
    "AnnouncementField",
    "AnnouncementFooter",
    "AnnouncementImages",
    "AnnouncementMentions",
    "LiveAnnouncementConfig",
    "build_template_context",
    "build_stream_context",
    "default_live_announcement_config",
    "deep_merge_config",
    "parse_config_json",
    "parse_embed_color",
    "render_announcement_payload",
    "render_placeholders",
    "shorten_text",
    "validate_config",
    "validate_live_announcement_config",
]
