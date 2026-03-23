"""_EmbedsMixin – Discord embeds and UI components for live announcements."""
from __future__ import annotations

import asyncio
import hashlib
import json
import re
import secrets
from datetime import UTC, datetime
from urllib.parse import parse_qsl, urlencode, urlparse, urlunparse

import discord

from .. import storage
from ..core.constants import (
    TWITCH_BRAND_COLOR_HEX,
    TWITCH_BUTTON_LABEL,
    TWITCH_DISCORD_REF_CODE,
    TWITCH_TARGET_GAME_NAME,
    TWITCH_VOD_BUTTON_LABEL,
    log,
)
try:
    from ..live_announce.template import (
        LiveAnnouncementConfig as _LiveAnnouncementConfig,
        build_stream_context as _build_live_announce_stream_context,
        default_live_announcement_config as _default_live_announce_config,
        deep_merge_config as _merge_live_announce_config,
        parse_config_json as _parse_live_announce_config_json,
        render_announcement_payload as _render_live_announce_payload,
    )
except Exception:  # pragma: no cover - optional module during staged rollout
    _LiveAnnouncementConfig = None
    _default_live_announce_config = None
    _merge_live_announce_config = None
    _parse_live_announce_config_json = None
    _build_live_announce_stream_context = None
    _render_live_announce_payload = None


_TWITCH_ICON_URL = "https://static.twitchcdn.net/assets/favicon-32-e29e246c157142c94346.png"
_ROLE_NAME_SAFE_RE = re.compile(r"[^A-Za-z0-9 _-]+")


class _EmbedsMixin:
    @staticmethod
    def _stable_cache_buster_value(seed: str | None) -> str:
        normalized_seed = str(seed or "").strip()
        if not normalized_seed:
            return str(int(datetime.now(tz=UTC).timestamp()))
        return hashlib.sha256(normalized_seed.encode("utf-8")).hexdigest()[:16]

    @staticmethod
    def _default_live_announcement_config() -> dict:
        if callable(_default_live_announce_config):
            try:
                cfg = _default_live_announce_config()
                if isinstance(cfg, dict):
                    return cfg
            except Exception:
                log.debug("Could not load live-announcement defaults from template module", exc_info=True)
        return {
            "content": "{rolle} **{channel}** ist live! Schau ueber den Button unten rein.",
            "mentions": {"enabled": True, "role_id": ""},
            "button": {"enabled": True, "label": TWITCH_BUTTON_LABEL, "url_template": "{url}"},
        }

    @staticmethod
    def _coerce_role_id(value) -> int | None:
        text = str(value or "").strip()
        if not text.isdigit():
            return None
        try:
            parsed = int(text)
        except Exception:
            return None
        return parsed if parsed > 0 else None

    @staticmethod
    def _sanitize_live_ping_role_name(login: str) -> str:
        cleaned = _ROLE_NAME_SAFE_RE.sub("", str(login or "").strip())
        cleaned = " ".join(cleaned.split())
        if not cleaned:
            cleaned = "STREAMER"
        name = f"{cleaned.upper()} LIVE PING"
        return name[:100]

    @staticmethod
    def _sanitize_live_content(content: str) -> str:
        sanitized = str(content or "")
        sanitized = re.sub(r"@everyone", "@\u200beveryone", sanitized, flags=re.IGNORECASE)
        sanitized = re.sub(r"@here", "@\u200bhere", sanitized, flags=re.IGNORECASE)
        return sanitized

    @staticmethod
    def _extract_role_id_from_mention(value: str) -> int | None:
        text = str(value or "").strip()
        match = re.fullmatch(r"<@&(\d+)>", text)
        if not match:
            return None
        try:
            role_id = int(match.group(1))
        except Exception:
            return None
        return role_id if role_id > 0 else None

    @staticmethod
    def _allowed_role_ids_from_allowed_mentions(
        allowed_mentions: discord.AllowedMentions | None,
    ) -> list[int]:
        if allowed_mentions is None:
            return []
        resolved = getattr(allowed_mentions, "roles", False)
        if resolved is False or resolved is None:
            return []

        role_ids: list[int] = []
        if isinstance(resolved, (list, tuple, set)):
            for role_obj in resolved:
                role_id = getattr(role_obj, "id", None)
                if role_id is None:
                    try:
                        role_id = int(role_obj)
                    except (TypeError, ValueError):
                        role_id = None
                if role_id is None:
                    continue
                parsed = int(role_id)
                if parsed > 0 and parsed not in role_ids:
                    role_ids.append(parsed)
        return role_ids

    def _build_twitch_live_tracking_view_spec(
        self,
        *,
        login: str,
        referral_url: str,
        tracking_token: str,
        button_label: str,
    ) -> dict[str, str] | None:
        normalized_login = str(login or "").strip().lower()
        normalized_referral_url = str(referral_url or "").strip()
        normalized_tracking_token = str(tracking_token or "").strip()
        if not normalized_login or not normalized_referral_url or not normalized_tracking_token:
            return None
        normalized_label = str(button_label or TWITCH_BUTTON_LABEL).strip() or TWITCH_BUTTON_LABEL
        return {
            "type": "twitch_live_tracking",
            "streamer_login": normalized_login,
            "tracking_token": normalized_tracking_token,
            "referral_url": normalized_referral_url,
            "button_label": normalized_label[:80],
        }

    @staticmethod
    def _build_link_button_view_spec(url: str, *, label: str) -> dict[str, str] | None:
        normalized_url = str(url or "").strip()
        if not normalized_url:
            return None
        normalized_label = str(label or TWITCH_VOD_BUTTON_LABEL).strip() or TWITCH_VOD_BUTTON_LABEL
        return {
            "type": "link_button",
            "label": normalized_label[:80],
            "url": normalized_url,
        }

    def _load_live_announcement_config(self, login: str) -> dict:
        defaults = self._default_live_announcement_config()
        normalized_login = str(login or "").strip().lower()
        if not normalized_login:
            return defaults
        try:
            with storage.readonly_connection() as c:
                row = c.execute(
                    """
                    SELECT config_json
                      FROM twitch_live_announcement_configs
                     WHERE LOWER(streamer_login) = LOWER(%s)
                     LIMIT 1
                    """,
                    (normalized_login,),
                ).fetchone()
        except Exception:
            log.debug("Could not load live announcement config for %s", normalized_login, exc_info=True)
            return defaults
        if not row:
            return defaults
        raw_json = row[0] if not hasattr(row, "keys") else row["config_json"]
        text = str(raw_json or "").strip()
        if not text:
            return defaults
        if callable(_parse_live_announce_config_json):
            try:
                parsed_cfg = _parse_live_announce_config_json(text)
                if isinstance(parsed_cfg, dict):
                    return parsed_cfg
            except Exception:
                log.debug(
                    "Invalid live announcement config JSON for %s",
                    normalized_login,
                    exc_info=True,
                )
        try:
            parsed = json.loads(text)
        except Exception:
            return defaults
        if not isinstance(parsed, dict):
            return defaults
        if callable(_merge_live_announce_config):
            return _merge_live_announce_config(defaults, parsed)
        merged = defaults.copy()
        merged.update(parsed)
        return merged

    @staticmethod
    def _normalize_live_announcement_config(config: dict) -> dict:
        if not isinstance(config, dict):
            return {}
        if "embed" not in config:
            return config

        embed = config.get("embed") if isinstance(config.get("embed"), dict) else {}
        author = embed.get("author") if isinstance(embed.get("author"), dict) else {}
        footer = embed.get("footer") if isinstance(embed.get("footer"), dict) else {}
        fields = embed.get("fields") if isinstance(embed.get("fields"), list) else []
        image = embed.get("image") if isinstance(embed.get("image"), dict) else {}
        thumbnail = embed.get("thumbnail") if isinstance(embed.get("thumbnail"), dict) else {}
        mentions = config.get("mentions") if isinstance(config.get("mentions"), dict) else {}
        button = config.get("button") if isinstance(config.get("button"), dict) else {}

        mention_role_id = str(mentions.get("role_id") or "").strip()
        static_role_ids: list[int] = []
        if mention_role_id.isdigit():
            static_role_ids.append(int(mention_role_id))

        normalized_fields: list[dict] = []
        for field in fields:
            if not isinstance(field, dict):
                continue
            normalized_fields.append(
                {
                    "name_template": str(field.get("name") or ""),
                    "value_template": str(field.get("value") or ""),
                    "inline": bool(field.get("inline", True)),
                }
            )

        image_mode = "stream_thumbnail"
        if not bool(image.get("use_stream_thumbnail", True)):
            image_mode = "custom" if str(image.get("custom_url") or "").strip() else "none"

        normalized = {
            "content_template": str(config.get("content") or "").replace("{rolle}", "{mention_role}"),
            "color": embed.get("color", TWITCH_BRAND_COLOR_HEX),
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
            "fields": normalized_fields,
            "images": {
                "thumbnail_mode": (
                    "custom"
                    if str(thumbnail.get("mode") or "").strip().lower() == "custom_url"
                    else str(thumbnail.get("mode") or "none").strip().lower()
                ),
                "thumbnail_url_template": str(thumbnail.get("custom_url") or ""),
                "image_mode": image_mode,
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
                "enabled": bool(button.get("enabled", True)),
                "label_template": str(button.get("label") or TWITCH_BUTTON_LABEL),
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
                "static_ping_role_ids": static_role_ids,
                "allow_everyone": False,
            },
        }
        return normalized

    def _build_live_announce_context(
        self,
        *,
        login: str,
        stream: dict,
        mention_text: str,
        render_now: datetime | None = None,
    ) -> dict:
        if callable(_build_live_announce_stream_context):
            try:
                context = _build_live_announce_stream_context(
                    login=login,
                    stream=stream,
                    mention_role=mention_text,
                    now=render_now,
                )
                if isinstance(context, dict):
                    context["mention_role"] = mention_text
                    context["rolle"] = mention_text
                    return context
            except Exception:
                log.debug("Could not build context via live_announce.template", exc_info=True)
        display_name = stream.get("user_name") or login
        now_value = render_now.astimezone(UTC) if isinstance(render_now, datetime) else datetime.now(tz=UTC)
        started_at = str(stream.get("started_at") or now_value.isoformat(timespec="seconds"))
        stream_login = str(stream.get("user_login") or login).strip()
        return {
            "channel": display_name,
            "url": self._build_referral_url(stream_login),
            "title": stream.get("title") or "Live!",
            "viewer_count": int(stream.get("viewer_count") or 0),
            "started_at": started_at,
            "language": str(stream.get("language") or "de"),
            "tags": ", ".join(stream.get("tags") or []) if isinstance(stream.get("tags"), list) else "",
            "uptime": "",
            "game": stream.get("game_name") or TWITCH_TARGET_GAME_NAME,
            "mention_role": mention_text,
            "rolle": mention_text,
        }

    def _render_live_announcement_payload(
        self,
        *,
        login: str,
        stream: dict,
        mention_text: str = "",
        cache_buster_seed: str | None = None,
        render_now: datetime | None = None,
    ) -> dict | None:
        config_data = self._normalize_live_announcement_config(
            self._load_live_announcement_config(login)
        )
        context = self._build_live_announce_context(
            login=login,
            stream=stream,
            mention_text=mention_text,
            render_now=render_now,
        )
        context["url"] = self._build_referral_url(login)
        if callable(_render_live_announce_payload):
            try:
                render_config = config_data
                if (
                    isinstance(render_config, dict)
                    and _LiveAnnouncementConfig is not None
                    and callable(getattr(_LiveAnnouncementConfig, "from_dict", None))
                ):
                    render_config = _LiveAnnouncementConfig.from_dict(render_config)
                rendered = _render_live_announce_payload(
                    config=render_config,
                    context=context,
                    now=render_now,
                    cache_buster_seed=cache_buster_seed,
                )
                if isinstance(rendered, dict):
                    button = rendered.get("button")
                    if isinstance(button, dict):
                        button_cfg = (
                            config_data.get("button")
                            if isinstance(config_data.get("button"), dict)
                            else {}
                        )
                        button["enabled"] = bool(button_cfg.get("enabled", True))
                        button["url"] = self._build_referral_url(login)
                    return rendered
            except Exception:
                log.debug("Could not render payload via live_announce.template", exc_info=True)

        if not isinstance(config_data, dict):
            return None

        def _render_text(template: str) -> str:
            rendered = str(template or "")
            for key, value in context.items():
                rendered = rendered.replace(f"{{{key}}}", str(value))
            return rendered

        content_template = str(
            config_data.get("content_template")
            or config_data.get("content")
            or "{rolle} **{channel}** ist live! Schau ueber den Button unten rein."
        )
        title_template = str(config_data.get("title_template") or "{channel} ist LIVE in {game}!")
        description_mode = str(config_data.get("description_mode") or "stream_title").strip().lower()
        description_template = str(config_data.get("description_template") or "{title}")
        description = _render_text("{title}") if description_mode == "stream_title" else _render_text(description_template)
        if description_mode == "custom_plus_title":
            custom = _render_text(description_template).strip()
            stream_title = _render_text("{title}").strip()
            description = f"{custom}\n\n{stream_title}".strip() if custom and stream_title else (custom or stream_title)

        fields_payload: list[dict] = []
        raw_fields = config_data.get("fields")
        if isinstance(raw_fields, list):
            for field in raw_fields[:25]:
                if not isinstance(field, dict):
                    continue
                name = _render_text(str(field.get("name_template") or field.get("name") or ""))
                value = _render_text(str(field.get("value_template") or field.get("value") or ""))
                if not name or not value:
                    continue
                fields_payload.append(
                    {
                        "name": name[:256],
                        "value": value[:1024],
                        "inline": bool(field.get("inline", True)),
                    }
                )
        if not fields_payload:
            fields_payload = [
                {"name": "Viewer", "value": str(stream.get("viewer_count") or 0), "inline": True},
                {
                    "name": "Kategorie",
                    "value": str(stream.get("game_name") or TWITCH_TARGET_GAME_NAME),
                    "inline": True,
                },
            ]

        author_cfg = config_data.get("author") if isinstance(config_data.get("author"), dict) else {}
        footer_cfg = config_data.get("footer") if isinstance(config_data.get("footer"), dict) else {}
        images_cfg = config_data.get("images") if isinstance(config_data.get("images"), dict) else {}
        button_cfg = config_data.get("button") if isinstance(config_data.get("button"), dict) else {}

        thumbnail_mode = str(images_cfg.get("thumbnail_mode") or "").strip().lower()
        thumbnail_url = ""
        if thumbnail_mode == "channel_avatar":
            thumbnail_url = str(stream.get("profile_image_url") or "")
        elif thumbnail_mode == "custom":
            thumbnail_url = _render_text(str(images_cfg.get("thumbnail_url_template") or ""))

        image_mode = str(images_cfg.get("image_mode") or "").strip().lower()
        image_url = ""
        if image_mode == "stream_thumbnail":
            image_url = str(stream.get("thumbnail_url") or "")
            ratio = str(images_cfg.get("image_ratio") or "16:9")
            width, height = ("1024", "768") if ratio == "4:3" else ("1280", "720")
            image_url = image_url.replace("{width}", width).replace("{height}", height)
            if image_url and bool(images_cfg.get("cache_buster", False)):
                separator = "&" if "?" in image_url else "?"
                image_url = (
                    f"{image_url}{separator}rand="
                    f"{self._stable_cache_buster_value(cache_buster_seed)}"
                )
        elif image_mode == "custom":
            image_url = _render_text(str(images_cfg.get("image_url_template") or ""))

        return {
            "content": _render_text(content_template).strip(),
            "embed": {
                "title": _render_text(title_template).strip()[:256],
                "description": description[:4096],
                "color": config_data.get("color", TWITCH_BRAND_COLOR_HEX),
                "author": {
                    "name": _render_text(str(author_cfg.get("name_template") or "LIVE: {channel}"))[:256],
                    "icon_mode": str(author_cfg.get("icon_mode") or "none"),
                    "link_enabled": bool(author_cfg.get("link_to_stream", True)),
                },
                "fields": fields_payload,
                "thumbnail": {"url": thumbnail_url} if thumbnail_url else {"mode": "none"},
                "image": {"url": image_url} if image_url else {"use_stream_thumbnail": False, "custom_url": ""},
                "footer": {
                    "text": _render_text(str(footer_cfg.get("text_template") or ""))[:2048],
                    "icon_mode": str(footer_cfg.get("icon_mode") or "none"),
                },
            },
            "button": {
                "enabled": bool(button_cfg.get("enabled", True)),
                "label": _render_text(
                    str(button_cfg.get("label_template") or button_cfg.get("label") or TWITCH_BUTTON_LABEL)
                )[:80],
                "url": self._build_referral_url(login),
            },
        }

    async def _ensure_live_ping_role(
        self,
        *,
        login: str,
        streamer_entry: dict | None = None,
        notify_channel: discord.TextChannel | None = None,
    ) -> tuple[str, int | None]:
        entry = streamer_entry or {}
        raw_enabled = entry.get("live_ping_enabled", 1)
        if isinstance(raw_enabled, str):
            live_ping_enabled = raw_enabled.strip().lower() not in {"0", "false", "no", "off"}
        else:
            live_ping_enabled = bool(raw_enabled)
        if not live_ping_enabled:
            return "", None

        role_id = self._coerce_role_id(entry.get("live_ping_role_id"))
        discord_user_id = self._coerce_role_id(entry.get("discord_user_id"))

        guild: discord.Guild | None = None
        if notify_channel is not None:
            guild = getattr(notify_channel, "guild", None)
        if guild is None and getattr(self.bot, "guilds", None):
            guilds = list(getattr(self.bot, "guilds", []) or [])
            if guilds:
                guild = guilds[0]
        if guild is None:
            return "", role_id

        role = guild.get_role(role_id) if role_id else None
        role_name = self._sanitize_live_ping_role_name(login)
        if role is None:
            role = discord.utils.get(guild.roles, name=role_name)
            if role is None:
                try:
                    role = await guild.create_role(
                        name=role_name,
                        reason=f"Auto-created Twitch live ping role for {login}",
                        mentionable=True,
                    )
                except Exception:
                    log.debug("Could not create live ping role for %s", login, exc_info=True)
                    role = None

        role_id = int(role.id) if role is not None else role_id
        if role_id:
            try:
                with storage.transaction() as c:
                    storage.set_partner_live_ping_settings(
                        c,
                        twitch_login=login,
                        live_ping_role_id=role_id,
                    )
            except Exception:
                log.debug("Could not persist live ping role_id for %s", login, exc_info=True)

        if role is not None and discord_user_id:
            member = guild.get_member(discord_user_id)
            if member is None:
                try:
                    member = await guild.fetch_member(discord_user_id)
                except Exception:
                    member = None
            if member is not None and role not in member.roles:
                try:
                    await member.add_roles(role, reason=f"Live ping role mapping for {login}")
                except Exception:
                    log.debug("Could not assign live ping role to member=%s", discord_user_id, exc_info=True)

        return (f"<@&{role_id}>", role_id) if role_id else ("", None)

    def _build_live_embed(
        self,
        login: str,
        stream: dict,
        *,
        rendered_payload: dict | None = None,
        cache_buster_seed: str | None = None,
        render_now: datetime | None = None,
    ) -> discord.Embed:
        """Erzeuge ein Discord-Embed für das Go-Live-Posting mit Stream-Vorschau."""

        display_name = stream.get("user_name") or login
        game = stream.get("game_name") or TWITCH_TARGET_GAME_NAME
        title = stream.get("title") or "Live!"
        viewer_count = int(stream.get("viewer_count") or 0)

        timestamp = (
            render_now.astimezone(UTC)
            if isinstance(render_now, datetime)
            else datetime.now(tz=UTC)
        )
        started_at_raw = stream.get("started_at")
        if isinstance(started_at_raw, str) and started_at_raw:
            try:
                timestamp = datetime.fromisoformat(started_at_raw.replace("Z", "+00:00"))
            except ValueError as exc:
                log.debug("Ungültiger started_at-Wert '%s': %s", started_at_raw, exc)

        embed_title = f"{display_name} ist LIVE in {game}!"
        embed_description = title
        embed_color = TWITCH_BRAND_COLOR_HEX
        embed_url = ""
        embed_fields: list[dict] = [
            {"name": "Viewer", "value": str(viewer_count), "inline": True},
            {"name": "Kategorie", "value": game, "inline": True},
        ]
        author_name = f"LIVE: {display_name}"
        author_icon_url = None
        author_url = None
        footer_text = "Auf Twitch ansehen fuer mehr Deadlock-Action!"
        footer_icon_url = None
        thumbnail_cfg: dict = {}
        image_cfg: dict = {}

        if isinstance(rendered_payload, dict):
            embed_payload = rendered_payload.get("embed")
            if isinstance(embed_payload, dict):
                embed_title = str(embed_payload.get("title") or embed_title)
                embed_description = str(embed_payload.get("description") or embed_description)
                raw_color = embed_payload.get("color")
                try:
                    embed_color = int(raw_color or embed_color)
                except Exception:
                    color_text = str(raw_color or "").strip().lower().lstrip("#")
                    if color_text.startswith("0x"):
                        color_text = color_text[2:]
                    if len(color_text) == 6:
                        try:
                            embed_color = int(color_text, 16)
                        except Exception:
                            pass
                embed_url = str(embed_payload.get("url") or "").strip()
                raw_fields = embed_payload.get("fields")
                if isinstance(raw_fields, list):
                    embed_fields = [field for field in raw_fields if isinstance(field, dict)]
                author_cfg = embed_payload.get("author")
                if isinstance(author_cfg, dict):
                    author_name = str(author_cfg.get("name") or author_name)
                    if str(author_cfg.get("url") or "").strip():
                        author_url = str(author_cfg.get("url") or "").strip()
                    elif bool(author_cfg.get("link_enabled")):
                        author_url = self._build_referral_url(login)
                    if str(author_cfg.get("icon_url") or "").strip():
                        author_icon_url = str(author_cfg.get("icon_url") or "").strip()
                    icon_mode = str(author_cfg.get("icon_mode") or "").strip().lower()
                    if icon_mode == "twitch_logo":
                        author_icon_url = _TWITCH_ICON_URL
                    elif icon_mode == "channel_avatar":
                        author_icon_url = (stream.get("profile_image_url") or "").strip() or None
                footer_cfg = embed_payload.get("footer")
                if isinstance(footer_cfg, dict):
                    footer_text = str(footer_cfg.get("text") or footer_text)
                    if str(footer_cfg.get("icon_url") or "").strip():
                        footer_icon_url = str(footer_cfg.get("icon_url") or "").strip()
                    elif str(footer_cfg.get("icon_mode") or "").strip().lower() == "twitch_logo":
                        footer_icon_url = _TWITCH_ICON_URL
                thumbnail_cfg = embed_payload.get("thumbnail") if isinstance(embed_payload.get("thumbnail"), dict) else {}
                image_cfg = embed_payload.get("image") if isinstance(embed_payload.get("image"), dict) else {}

                timestamp_raw = str(embed_payload.get("timestamp") or "").strip()
                if timestamp_raw:
                    try:
                        timestamp = datetime.fromisoformat(timestamp_raw.replace("Z", "+00:00"))
                    except ValueError:
                        pass

        embed = discord.Embed(
            title=embed_title,
            description=embed_description,
            colour=discord.Color(embed_color),
            timestamp=timestamp,
            url=embed_url or None,
        )

        for field in embed_fields[:25]:
            name = str(field.get("name") or "").strip()
            value = str(field.get("value") or "").strip()
            if not name or not value:
                continue
            embed.add_field(name=name[:256], value=value[:1024], inline=bool(field.get("inline", True)))

        thumbnail_url = ""
        thumb_mode = str(thumbnail_cfg.get("mode") or "").strip().lower()
        if str(thumbnail_cfg.get("url") or "").strip():
            thumbnail_url = str(thumbnail_cfg.get("url") or "").strip()
        elif thumb_mode == "custom_url":
            thumbnail_url = str(thumbnail_cfg.get("custom_url") or "").strip()
        elif thumb_mode == "channel_avatar":
            thumbnail_url = (stream.get("profile_image_url") or "").strip()
        if thumbnail_url:
            embed.set_thumbnail(url=thumbnail_url)

        image_url = ""
        if image_cfg:
            if str(image_cfg.get("url") or "").strip():
                image_url = str(image_cfg.get("url") or "").strip()
            else:
                use_stream_thumbnail = bool(image_cfg.get("use_stream_thumbnail", True))
                if use_stream_thumbnail:
                    image_url = (stream.get("thumbnail_url") or "").strip()
                    ratio = str(image_cfg.get("format") or "16:9").strip()
                    width, height = ("1024", "768") if ratio == "4:3" else ("1280", "720")
                    image_url = image_url.replace("{width}", width).replace("{height}", height)
                else:
                    image_url = str(image_cfg.get("custom_url") or "").strip()
            if image_url and bool(image_cfg.get("cache_buster", False)):
                separator = "&" if "?" in image_url else "?"
                image_url = (
                    f"{image_url}{separator}rand="
                    f"{self._stable_cache_buster_value(cache_buster_seed)}"
                )
        else:
            image_url = (stream.get("thumbnail_url") or "").strip()
            if image_url:
                image_url = image_url.replace("{width}", "1280").replace("{height}", "720")
                image_url = (
                    f"{image_url}?rand={self._stable_cache_buster_value(cache_buster_seed)}"
                )
        if image_url:
            embed.set_image(url=image_url)

        if footer_icon_url:
            embed.set_footer(text=footer_text[:2048], icon_url=footer_icon_url)
        else:
            embed.set_footer(text=footer_text[:2048])
        if author_name:
            embed.set_author(name=author_name[:256], icon_url=author_icon_url, url=author_url)

        return embed

    def _build_offline_embed(
        self,
        *,
        login: str,
        display_name: str,
        last_title: str | None,
        last_game: str | None,
        preview_image_url: str | None,
    ) -> discord.Embed:
        """Offline-Overlay: gleicher Stil wie live, aber klar als VOD markiert."""

        game = last_game or TWITCH_TARGET_GAME_NAME or "Twitch"
        description = last_title or "Letzten Stream als VOD ansehen."

        embed = discord.Embed(
            title=f"{display_name} ist OFFLINE",
            description=description,
            colour=discord.Color(TWITCH_BRAND_COLOR_HEX),
            timestamp=datetime.now(tz=UTC),
        )

        embed.add_field(name="Status", value="OFFLINE", inline=True)
        embed.add_field(name="Kategorie", value=game, inline=True)
        embed.add_field(name="Hinweis", value="VOD ueber den Button abrufen.", inline=False)

        if preview_image_url:
            embed.set_image(url=preview_image_url)

        embed.set_footer(text="Letzten Stream auf Twitch ansehen.")
        embed.set_author(name=f"OFFLINE: {display_name}")

        return embed

    def _build_offline_link_view(
        self, referral_url: str, *, label: str | None = None
    ) -> discord.ui.View:
        """Offline-Ansicht: einfacher Link-Button ohne Tracking."""
        view = discord.ui.View(timeout=None)
        view.add_item(
            discord.ui.Button(
                label=label or TWITCH_BUTTON_LABEL,
                style=discord.ButtonStyle.link,
                url=referral_url,
            )
        )
        return view

    async def cog_load(self) -> None:
        await super().cog_load()
        spawner = getattr(self, "_spawn_bg_task", None)
        if callable(spawner):
            spawner(self._register_persistent_live_views(), "twitch.register_live_views")
        else:
            asyncio.create_task(
                self._register_persistent_live_views(),
                name="twitch.register_live_views",
            )

    def _build_live_view(
        self,
        streamer_login: str,
        referral_url: str,
        tracking_token: str,
        *,
        button_label: str | None = None,
    ) -> _TwitchLiveAnnouncementView | None:
        """Create a persistent view that tracks button clicks before redirecting."""
        if not tracking_token:
            return None
        return _TwitchLiveAnnouncementView(
            cog=self,
            streamer_login=streamer_login,
            referral_url=referral_url,
            tracking_token=tracking_token,
            button_label=button_label or TWITCH_BUTTON_LABEL,
        )

    @staticmethod
    def _generate_tracking_token() -> str:
        return secrets.token_hex(8)

    def _build_referral_url(self, login: str) -> str:
        """Append the configured referral parameter to the Twitch URL."""
        normalized_login = (login or "").strip()
        base_url = (
            f"https://www.twitch.tv/{normalized_login}"
            if normalized_login
            else "https://www.twitch.tv/"
        )
        ref_code = (TWITCH_DISCORD_REF_CODE or "").strip()
        if not ref_code:
            return base_url
        parsed = urlparse(base_url)
        query = dict(parse_qsl(parsed.query, keep_blank_values=True))
        query["ref"] = ref_code
        encoded = urlencode(query)
        return urlunparse(parsed._replace(query=encoded))

    def _resolve_live_button_label(self, login: str) -> str:
        try:
            config = self._load_live_announcement_config(login)
        except Exception:
            return TWITCH_BUTTON_LABEL
        button_cfg = config.get("button") if isinstance(config.get("button"), dict) else {}
        label = str(button_cfg.get("label") or button_cfg.get("label_template") or "").strip()
        return label[:80] if label else TWITCH_BUTTON_LABEL

    async def _build_live_announcement_message(
        self,
        *,
        login: str,
        stream: dict,
        streamer_entry: dict | None,
        notify_channel: discord.TextChannel | None,
        tracking_token: str | None = None,
        render_now: datetime | None = None,
    ) -> tuple[str, discord.Embed, _TwitchLiveAnnouncementView | None, discord.AllowedMentions, str]:
        tracking_token_value = str(tracking_token or "").strip() or self._generate_tracking_token()
        mention_text = ""
        streamer_role_id: int | None = None
        allowed_role_ids: list[int] = []
        try:
            mention_text, streamer_role_id = await self._ensure_live_ping_role(
                login=login,
                streamer_entry=streamer_entry,
                notify_channel=notify_channel,
            )
        except Exception:
            log.debug("Could not resolve live ping role for %s", login, exc_info=True)

        rendered_payload = self._render_live_announcement_payload(
            login=login,
            stream=stream,
            mention_text=mention_text,
            cache_buster_seed=tracking_token_value,
            render_now=render_now,
        )
        mention_cfg = (
            rendered_payload.get("allowed_mentions")
            if isinstance(rendered_payload, dict)
            and isinstance(rendered_payload.get("allowed_mentions"), dict)
            else {}
        )
        use_streamer_ping = bool(mention_cfg.get("use_streamer_ping_role", True))
        static_role_ids = mention_cfg.get("role_ids")
        if isinstance(static_role_ids, list):
            for role_id_raw in static_role_ids:
                role_id = self._coerce_role_id(role_id_raw)
                if role_id and role_id not in allowed_role_ids:
                    allowed_role_ids.append(role_id)
        if use_streamer_ping and streamer_role_id and streamer_role_id not in allowed_role_ids:
            allowed_role_ids.append(streamer_role_id)
        if use_streamer_ping and streamer_role_id and not mention_text:
            mention_text = f"<@&{streamer_role_id}>"

        display_name = stream.get("user_name") or login
        stream_title = (stream.get("title") or "").strip()
        fallback_message = f"{mention_text} **{display_name}** ist live! Schau ueber den Button unten rein."
        if stream_title:
            fallback_message = f"{fallback_message} - {stream_title}"

        content = fallback_message
        button_label = TWITCH_BUTTON_LABEL
        button_enabled = True
        if isinstance(rendered_payload, dict):
            content = str(rendered_payload.get("content") or "").strip() or fallback_message
            button_cfg = rendered_payload.get("button")
            if isinstance(button_cfg, dict):
                button_enabled = bool(button_cfg.get("enabled", True))
                if button_enabled:
                    maybe_label = str(button_cfg.get("label") or "").strip()
                    if maybe_label:
                        button_label = maybe_label[:80]
        if not use_streamer_ping and mention_text:
            content = content.replace(mention_text, "").strip()
        content = self._sanitize_live_content(content)

        embed = self._build_live_embed(
            login,
            stream,
            rendered_payload=rendered_payload if isinstance(rendered_payload, dict) else None,
            cache_buster_seed=tracking_token_value,
            render_now=render_now,
        )
        referral_url = self._build_referral_url(login)
        view = (
            self._build_live_view(
                login,
                referral_url,
                tracking_token_value,
                button_label=button_label,
            )
            if button_enabled
            else None
        )
        allowed_mentions = discord.AllowedMentions(
            everyone=False,
            users=False,
            roles=[discord.Object(id=role_id) for role_id in allowed_role_ids] if allowed_role_ids else False,
            replied_user=False,
        )
        return content, embed, view, allowed_mentions, tracking_token_value

    async def _register_persistent_live_views(self) -> None:
        """Re-register live announcement views after a restart."""
        prefers_master_broker = getattr(
            self,
            "_announcement_transport_prefers_master_broker",
            None,
        )
        if callable(prefers_master_broker):
            try:
                if prefers_master_broker():
                    log.debug(
                        "Skipping local Twitch live view rehydration because master broker owns announcement views."
                    )
                    return
            except Exception:
                log.debug(
                    "Could not determine live-view ownership before rehydration",
                    exc_info=True,
                )
        if not self._notify_channel_id:
            return
        try:
            await self.bot.wait_until_ready()
        except Exception:
            log.exception("wait_until_ready für Twitch-Views fehlgeschlagen")
            return

        try:
            with storage.readonly_connection() as c:
                rows = c.execute(
                    "SELECT streamer_login, last_discord_message_id, last_tracking_token "
                    "FROM twitch_live_state "
                    "WHERE last_discord_message_id IS NOT NULL AND last_tracking_token IS NOT NULL"
                ).fetchall()
        except Exception:
            log.exception("Konnte persistente Twitch-Views nicht registrieren")
            return

        for row in rows:
            login = (row["streamer_login"] or "").strip()
            token = (row["last_tracking_token"] or "").strip()
            message_id_raw = row["last_discord_message_id"]
            if not login or not token or not message_id_raw:
                continue
            try:
                message_id = int(message_id_raw)
            except (TypeError, ValueError):
                continue
            referral_url = self._build_referral_url(login)
            button_label = self._resolve_live_button_label(login)
            view = self._build_live_view(
                login,
                referral_url,
                token,
                button_label=button_label,
            )
            if view is None:
                continue
            view.bind_to_message(channel_id=self._notify_channel_id, message_id=message_id)
            self._register_live_view(tracking_token=token, view=view, message_id=message_id)

    def _get_live_view_registry(self) -> dict[str, _TwitchLiveAnnouncementView]:
        registry = getattr(self, "_live_view_registry", None)
        if registry is None:
            registry = {}
            self._live_view_registry = registry
        return registry

    def _register_live_view(
        self,
        *,
        tracking_token: str,
        view: _TwitchLiveAnnouncementView,
        message_id: int,
    ) -> None:
        if not tracking_token:
            return
        registry = self._get_live_view_registry()
        registry[tracking_token] = view
        try:
            self.bot.add_view(view, message_id=message_id)
        except Exception:
            log.exception("Konnte View für Twitch-Posting %s nicht registrieren", message_id)

    def _drop_live_view(self, tracking_token: str | None) -> None:
        if not tracking_token:
            return
        registry = self._get_live_view_registry()
        view = registry.pop(tracking_token, None)
        if view is None:
            return

        # discord.py hat kein natives remove_view am Bot-Objekt.
        # view.stop() reicht aus, um die Interaktionen zu beenden.
        view.stop()
        log.debug("Live-View gestoppt und aus Registry entfernt: %s", tracking_token)

    def _log_link_click(
        self,
        *,
        interaction: discord.Interaction,
        view: _TwitchLiveAnnouncementView,
    ) -> None:
        clicked_at = datetime.now(tz=UTC).isoformat(timespec="seconds")
        user = interaction.user
        user_id = str(getattr(user, "id", "") or "") or None
        username = str(user) if user else None
        guild_id = str(interaction.guild_id) if interaction.guild_id else None
        channel_source = interaction.channel_id or view.channel_id
        channel_id = str(channel_source) if channel_source else None
        if interaction.message and interaction.message.id:
            message_id = str(interaction.message.id)
        elif view.message_id:
            message_id = str(view.message_id)
        else:
            message_id = None
        ref_code = (TWITCH_DISCORD_REF_CODE or "").strip() or None

        try:
            with storage.transaction() as c:
                c.execute(
                    """
                    INSERT INTO twitch_link_clicks (
                        clicked_at,
                        streamer_login,
                        tracking_token,
                        discord_user_id,
                        discord_username,
                        guild_id,
                        channel_id,
                        message_id,
                        ref_code,
                        source_hint
                    ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
                    """,
                    (
                        clicked_at,
                        view.streamer_login.lower(),
                        view.tracking_token,
                        user_id,
                        username,
                        guild_id,
                        channel_id,
                        message_id,
                        ref_code,
                        "live_button",
                    ),
                )
        except Exception:
            log.exception("Konnte Twitch-Link-Klick nicht speichern")

    async def _handle_tracked_button_click(
        self,
        interaction: discord.Interaction,
        view: _TwitchLiveAnnouncementView,
    ) -> None:
        try:
            self._log_link_click(interaction=interaction, view=view)
        except Exception:
            log.exception("Konnte Klick nicht loggen")

        content = f"Hier ist dein Twitch-Link für **{view.streamer_login}**."
        response_view = _TwitchReferralLinkView(view.referral_url)
        try:
            if interaction.response.is_done():
                await interaction.followup.send(content, view=response_view, ephemeral=True)
            else:
                await interaction.response.send_message(content, view=response_view, ephemeral=True)
        except Exception:
            log.exception("Antwort mit Referral-Link fehlgeschlagen")


class _TwitchReferralLinkView(discord.ui.View):
    """Ephemeral view with a direct Twitch hyperlink."""

    def __init__(self, referral_url: str):
        super().__init__(timeout=60)
        self.add_item(
            discord.ui.Button(
                label=TWITCH_BUTTON_LABEL,
                style=discord.ButtonStyle.link,
                url=referral_url,
            )
        )


class _TrackedTwitchButton(discord.ui.Button):
    def __init__(self, parent: _TwitchLiveAnnouncementView, *, custom_id: str, label: str):
        super().__init__(
            label=(label or TWITCH_BUTTON_LABEL)[:80],
            style=discord.ButtonStyle.primary,
            custom_id=custom_id,
        )
        self._view_ref = parent  # Renamed from _parent to avoid discord.py conflict

    async def callback(self, interaction: discord.Interaction) -> None:  # type: ignore[override]
        await self._view_ref.handle_click(interaction)


class _TwitchLiveAnnouncementView(discord.ui.View):
    """Persistent live announcement view that tracks clicks before redirecting."""

    def __init__(
        self,
        *,
        cog: _EmbedsMixin,
        streamer_login: str,
        referral_url: str,
        tracking_token: str,
        button_label: str,
    ):
        super().__init__(timeout=None)
        self.cog = cog
        self.streamer_login = streamer_login
        self.referral_url = referral_url
        self.tracking_token = tracking_token
        self.button_label = (button_label or TWITCH_BUTTON_LABEL)[:80]
        self.message_id: int | None = None
        self.channel_id: int | None = None

        custom_id = self._build_custom_id(streamer_login, tracking_token)
        self.add_item(_TrackedTwitchButton(self, custom_id=custom_id, label=self.button_label))

    @staticmethod
    def _build_custom_id(streamer_login: str, tracking_token: str) -> str:
        login_part = "".join(ch for ch in streamer_login.lower() if ch.isalnum())[:24] or "stream"
        token_part = (tracking_token or "")[:32] or secrets.token_hex(4)
        return f"twitch-live:{login_part}:{token_part}"

    def bind_to_message(self, *, channel_id: int | None, message_id: int | None) -> None:
        self.channel_id = channel_id
        self.message_id = message_id

    async def handle_click(self, interaction: discord.Interaction) -> None:
        await self.cog._handle_tracked_button_click(interaction, self)
