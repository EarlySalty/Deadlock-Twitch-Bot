from __future__ import annotations

import json
import logging
import os
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any

import discord

from ...storage import readonly_connection, transaction
from ..clip_manager import ClipManager
from ..settings import get_auto_approve_settings

log = logging.getLogger("TwitchStreams.SocialMedia.Approval")

APPROVAL_STATE_AWAITING = "awaiting_approval"
APPROVAL_STATE_APPROVED = "approved"
APPROVAL_STATE_SKIPPED = "skipped"
APPROVAL_STATE_EDITING = "editing"

DECISION_APPROVE = "approve"
DECISION_SKIP = "skip"
DECISION_EDIT = "edit"

SUPPORTED_PLATFORMS: tuple[str, ...] = ("youtube", "tiktok", "instagram")

_DEFAULT_ADMIN_DISCORD_USER_ID = "662995601738170389"  # nosemgrep: discord-client-id


def _utcnow() -> datetime:
    return datetime.now(UTC)


def _decode_json(raw: Any, default: Any) -> Any:
    if raw is None:
        return default
    if isinstance(raw, (list, dict)):
        return raw
    if isinstance(raw, (bytes, bytearray)):
        try:
            return json.loads(raw.decode("utf-8"))
        except Exception:
            return default
    if isinstance(raw, str):
        if not raw:
            return default
        try:
            return json.loads(raw)
        except Exception:
            return default
    return default


def _normalize_platforms(platforms: list[str] | tuple[str, ...] | set[str] | None) -> list[str]:
    normalized: list[str] = []
    seen: set[str] = set()
    for raw in platforms or ():
        value = str(raw or "").strip().lower()
        if value not in SUPPORTED_PLATFORMS or value in seen:
            continue
        seen.add(value)
        normalized.append(value)
    return normalized


def _normalize_decision(decision: str) -> str:
    value = str(decision or "").strip().lower()
    if value in {DECISION_APPROVE, "approved"}:
        return DECISION_APPROVE
    if value in {DECISION_SKIP, "skipped"}:
        return DECISION_SKIP
    if value in {DECISION_EDIT, "editing"}:
        return DECISION_EDIT
    raise ValueError(f"unsupported decision: {decision}")


def _row_value(row: Any, key: str, index: int) -> Any:
    return row[key] if hasattr(row, "keys") else row[index]


@dataclass(frozen=True)
class ApprovalRecord:
    clip_db_id: int
    state: str
    approved_platforms: list[str]
    approver_user_id: str | None = None
    decided_at: str | None = None
    dm_message_id: str | None = None
    dm_channel_id: str | None = None
    last_sent_at: str | None = None


def ensure_approval_row(clip_db_id: int) -> ApprovalRecord:
    record = get_approval_record(clip_db_id)
    if record is not None:
        return record
    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO social_media_clip_approval (
                clip_db_id, state, approved_platforms
            )
            VALUES (%s, %s, '[]')
            ON CONFLICT (clip_db_id) DO NOTHING
            """,
            (clip_db_id, APPROVAL_STATE_AWAITING),
        )
    return get_approval_record(clip_db_id) or ApprovalRecord(
        clip_db_id=int(clip_db_id),
        state=APPROVAL_STATE_AWAITING,
        approved_platforms=[],
    )


def get_approval_record(clip_db_id: int) -> ApprovalRecord | None:
    with readonly_connection() as conn:
        row = conn.execute(
            """
            SELECT clip_db_id, state, approved_platforms, approver_user_id, decided_at,
                   dm_message_id, dm_channel_id, last_sent_at
              FROM social_media_clip_approval
             WHERE clip_db_id = %s
             LIMIT 1
            """,
            (clip_db_id,),
        ).fetchone()
    if not row:
        return None
    return ApprovalRecord(
        clip_db_id=int(_row_value(row, "clip_db_id", 0)),
        state=str(_row_value(row, "state", 1) or APPROVAL_STATE_AWAITING),
        approved_platforms=_normalize_platforms(_decode_json(_row_value(row, "approved_platforms", 2), [])),
        approver_user_id=_row_value(row, "approver_user_id", 3),
        decided_at=(
            str(_row_value(row, "decided_at", 4))
            if _row_value(row, "decided_at", 4) is not None
            else None
        ),
        dm_message_id=_row_value(row, "dm_message_id", 5),
        dm_channel_id=_row_value(row, "dm_channel_id", 6),
        last_sent_at=(
            str(_row_value(row, "last_sent_at", 7))
            if _row_value(row, "last_sent_at", 7) is not None
            else None
        ),
    )


def mark_clip_awaiting_approval(clip_db_id: int) -> None:
    ensure_approval_row(clip_db_id)
    with transaction() as conn:
        conn.execute(
            """
            UPDATE social_media_clip_approval
               SET state = %s,
                   approver_user_id = NULL,
                   decided_at = NULL,
                   approved_platforms = '[]',
                   dm_message_id = NULL,
                   dm_channel_id = NULL,
                   last_sent_at = NULL
             WHERE clip_db_id = %s
               AND state <> %s
            """,
            (APPROVAL_STATE_AWAITING, clip_db_id, APPROVAL_STATE_AWAITING),
        )
        conn.execute(
            """
            UPDATE twitch_clips_social_media
               SET status = %s
             WHERE id = %s
               AND COALESCE(status, '') NOT IN ('published_all', 'published_partial', 'discarded')
            """,
            (APPROVAL_STATE_AWAITING, clip_db_id),
        )


def set_dm_delivery_state(
    clip_db_id: int,
    *,
    dm_message_id: str | None,
    dm_channel_id: str | None,
    last_sent_at: datetime | None,
) -> None:
    ensure_approval_row(clip_db_id)
    with transaction() as conn:
        conn.execute(
            """
            UPDATE social_media_clip_approval
               SET dm_message_id = %s,
                   dm_channel_id = %s,
                   last_sent_at = %s
             WHERE clip_db_id = %s
            """,
            (
                str(dm_message_id).strip() if dm_message_id else None,
                str(dm_channel_id).strip() if dm_channel_id else None,
                last_sent_at.isoformat() if hasattr(last_sent_at, "isoformat") else last_sent_at,
                clip_db_id,
            ),
        )


def iter_clips_needing_approval_dm(limit: int = 10) -> list[int]:
    with readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT c.id
              FROM twitch_clips_social_media c
              JOIN social_media_clip_enrichment e
                ON e.clip_db_id = c.id
               AND e.status = 'done'
              LEFT JOIN social_media_clip_approval a
                ON a.clip_db_id = c.id
             WHERE c.discarded_at IS NULL
               AND c.status = %s
               AND (
                    a.clip_db_id IS NULL
                    OR COALESCE(a.state, %s) = %s
               )
               AND COALESCE(a.dm_message_id, '') = ''
             ORDER BY c.created_at DESC, c.id DESC
             LIMIT %s
            """,
            (
                APPROVAL_STATE_AWAITING,
                APPROVAL_STATE_AWAITING,
                APPROVAL_STATE_AWAITING,
                max(1, int(limit)),
            ),
        ).fetchall()
    return [int(_row_value(row, "id", 0)) for row in rows]


def iter_clips_with_existing_approval_dm(limit: int = 100) -> list[ApprovalRecord]:
    with readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT clip_db_id, state, approved_platforms, approver_user_id, decided_at,
                   dm_message_id, dm_channel_id, last_sent_at
              FROM social_media_clip_approval
             WHERE COALESCE(dm_message_id, '') <> ''
               AND state IN (%s, %s)
             ORDER BY clip_db_id ASC
             LIMIT %s
            """,
            (
                APPROVAL_STATE_AWAITING,
                APPROVAL_STATE_EDITING,
                max(1, int(limit)),
            ),
        ).fetchall()
    return [
        ApprovalRecord(
            clip_db_id=int(_row_value(row, "clip_db_id", 0)),
            state=str(_row_value(row, "state", 1) or APPROVAL_STATE_AWAITING),
            approved_platforms=_normalize_platforms(
                _decode_json(_row_value(row, "approved_platforms", 2), [])
            ),
            approver_user_id=_row_value(row, "approver_user_id", 3),
            decided_at=(
                str(_row_value(row, "decided_at", 4))
                if _row_value(row, "decided_at", 4) is not None
                else None
            ),
            dm_message_id=_row_value(row, "dm_message_id", 5),
            dm_channel_id=_row_value(row, "dm_channel_id", 6),
            last_sent_at=(
                str(_row_value(row, "last_sent_at", 7))
                if _row_value(row, "last_sent_at", 7) is not None
                else None
            ),
        )
        for row in rows
    ]


def iter_approved_clips_pending_queue(limit: int = 20) -> list[int]:
    with readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT clip_db_id
              FROM social_media_clip_approval
             WHERE state = %s
             ORDER BY CASE WHEN decided_at IS NULL THEN 1 ELSE 0 END,
                      decided_at DESC,
                      clip_db_id DESC
             LIMIT %s
            """,
            (
                APPROVAL_STATE_APPROVED,
                max(1, int(limit)),
            ),
        ).fetchall()
    return [int(_row_value(row, "clip_db_id", 0)) for row in rows]


def is_clip_approved_for(clip_db_id: int, platform: str) -> bool:
    normalized_platform = str(platform or "").strip().lower()
    if normalized_platform not in SUPPORTED_PLATFORMS:
        return False
    record = get_approval_record(clip_db_id)
    if record is None or record.state != APPROVAL_STATE_APPROVED:
        return False
    return normalized_platform in set(record.approved_platforms)


def serialize_approval_record(record: ApprovalRecord | None) -> dict[str, Any] | None:
    if record is None:
        return None
    return {
        "clip_db_id": record.clip_db_id,
        "state": record.state,
        "approved_platforms": list(record.approved_platforms),
        "approver_user_id": record.approver_user_id,
        "decided_at": record.decided_at,
        "dm_message_id": record.dm_message_id,
        "dm_channel_id": record.dm_channel_id,
        "last_sent_at": record.last_sent_at,
    }


class PlatformSelect(discord.ui.Select):
    def __init__(self, clip_db_id: int, default_platforms: list[str] | None = None) -> None:
        defaults = set(default_platforms or [])
        options = [
            discord.SelectOption(
                label="YouTube Shorts",
                value="youtube",
                default="youtube" in defaults,
                emoji="▶️",
            ),
            discord.SelectOption(
                label="TikTok",
                value="tiktok",
                default="tiktok" in defaults,
                emoji="🎵",
            ),
            discord.SelectOption(
                label="Instagram Reels",
                value="instagram",
                default="instagram" in defaults,
                emoji="📸",
            ),
        ]
        super().__init__(
            placeholder="Plattformen auswählen",
            min_values=0,
            max_values=len(options),
            options=options,
            custom_id=f"social-media-approval:platforms:{clip_db_id}",
        )

    async def callback(self, interaction: discord.Interaction) -> None:
        view = self.view
        if isinstance(view, ApprovalDecisionView):
            view.set_selected_platforms(list(self.values))
        await interaction.response.send_message(
            f"Plattformen gespeichert: {', '.join(self.values) if self.values else 'keine Auswahl'}",
            ephemeral=True,
        )


class ApprovalDecisionView(discord.ui.View):
    def __init__(
        self,
        service: ApprovalService,
        clip_db_id: int,
        *,
        default_platforms: list[str] | None = None,
        locked: bool = False,
    ) -> None:
        super().__init__(timeout=None)
        self.service = service
        self.clip_db_id = int(clip_db_id)
        self.selected_platforms = _normalize_platforms(default_platforms)
        self._platform_select = PlatformSelect(self.clip_db_id, self.selected_platforms)
        self.add_item(self._platform_select)
        if locked:
            self.disable_all_items()

    def set_selected_platforms(self, platforms: list[str]) -> None:
        normalized = _normalize_platforms(platforms)
        self.selected_platforms = normalized
        defaults = set(normalized)
        for option in self._platform_select.options:
            option.default = option.value in defaults

    @discord.ui.button(
        label="Posten",
        style=discord.ButtonStyle.success,
        emoji="✅",
        custom_id="social-media-approval:approve",
        row=1,
    )
    async def approve_button(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button,
    ) -> None:
        del button
        await self.service.handle_decision(
            self.clip_db_id,
            DECISION_APPROVE,
            self.selected_platforms,
            str(interaction.user.id),
        )
        await interaction.response.edit_message(
            embed=await self.service.build_embed(self.clip_db_id),
            view=self.service.build_view(self.clip_db_id, locked=True),
        )

    @discord.ui.button(
        label="Bearbeiten",
        style=discord.ButtonStyle.secondary,
        emoji="✏️",
        custom_id="social-media-approval:edit",
        row=1,
    )
    async def edit_button(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button,
    ) -> None:
        del button
        await self.service.handle_decision(
            self.clip_db_id,
            DECISION_EDIT,
            self.selected_platforms,
            str(interaction.user.id),
        )
        await interaction.response.edit_message(
            embed=await self.service.build_embed(self.clip_db_id),
            view=self.service.build_view(self.clip_db_id, locked=False),
        )

    @discord.ui.button(
        label="Skip",
        style=discord.ButtonStyle.danger,
        emoji="🗑️",
        custom_id="social-media-approval:skip",
        row=1,
    )
    async def skip_button(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button,
    ) -> None:
        del button
        await self.service.handle_decision(
            self.clip_db_id,
            DECISION_SKIP,
            self.selected_platforms,
            str(interaction.user.id),
        )
        await interaction.response.edit_message(
            embed=await self.service.build_embed(self.clip_db_id),
            view=self.service.build_view(self.clip_db_id, locked=True),
        )


class ApprovalService:
    def __init__(self, bot=None, clip_manager: ClipManager | None = None) -> None:
        self.bot = bot
        self.clip_manager = clip_manager or ClipManager()

    @staticmethod
    def default_admin_user_id() -> str:
        for env_name in (
            "SOCIAL_MEDIA_APPROVAL_ADMIN_DISCORD_USER_ID",
            "SOCIAL_MEDIA_REAUTH_ADMIN_DISCORD_USER_ID",
            "TWITCH_ADMIN_DISCORD_USER_ID",
        ):
            value = str(os.getenv(env_name) or "").strip()
            if value:
                return value
        return _DEFAULT_ADMIN_DISCORD_USER_ID

    def build_view(self, clip_db_id: int, *, locked: bool | None = None) -> ApprovalDecisionView:
        record = ensure_approval_row(clip_db_id)
        if locked is None:
            locked = record.state in {APPROVAL_STATE_APPROVED, APPROVAL_STATE_SKIPPED}
        return ApprovalDecisionView(
            self,
            clip_db_id,
            default_platforms=record.approved_platforms,
            locked=locked,
        )

    async def build_embed(self, clip_db_id: int) -> discord.Embed:
        clip = self._load_clip_context(clip_db_id)
        if clip is None:
            raise ValueError(f"clip_db_id {clip_db_id} not found")
        record = ensure_approval_row(clip_db_id)
        embed = discord.Embed(
            title=f"Clip-Freigabe #{clip_db_id}",
            description=str(clip.get("clip_title") or "Ohne Titel"),
            color=self._embed_color(record.state),
            timestamp=_utcnow(),
        )
        embed.add_field(name="Streamer", value=str(clip.get("streamer_login") or "unbekannt"), inline=True)
        embed.add_field(name="Status", value=record.state, inline=True)
        embed.add_field(name="Views", value=str(clip.get("view_count") or 0), inline=True)
        if clip.get("clip_thumbnail_url"):
            embed.set_thumbnail(url=str(clip["clip_thumbnail_url"]))
        for platform in SUPPORTED_PLATFORMS:
            title_key = f"title_{platform}"
            hashtags_key = f"hashtags_{platform}"
            title_value = str(clip.get(title_key) or clip.get("clip_title") or "—").strip() or "—"
            hashtags = _normalize_platforms([])  # keep mypy quiet for empty default
            hashtags = [
                str(tag or "").strip()
                for tag in _decode_json(clip.get(hashtags_key), []) or []
                if str(tag or "").strip()
            ]
            value = title_value
            if hashtags:
                value = f"{value}\n{' '.join(hashtags[:6])}"
            embed.add_field(name=platform.upper(), value=value[:1024], inline=False)
        if record.decided_at:
            embed.set_footer(text=f"Letzte Entscheidung: {record.state} · {record.decided_at}")
        else:
            embed.set_footer(text="Aktion auswählen: Posten, Bearbeiten oder Skip")
        return embed

    async def send_dm(self, clip_db_id: int, admin_user_id: str) -> dict[str, str] | None:
        if self.bot is None:
            raise RuntimeError("Discord bot is required for send_dm")
        record = ensure_approval_row(clip_db_id)
        if record.dm_message_id:
            return {
                "message_id": str(record.dm_message_id),
                "channel_id": str(record.dm_channel_id or ""),
            }
        admin_user = await self._resolve_admin_user(admin_user_id)
        if admin_user is None:
            raise RuntimeError("approval admin user not available")

        embed = await self.build_embed(clip_db_id)
        view = self.build_view(clip_db_id, locked=False)
        message = await admin_user.send(embed=embed, view=view)
        set_dm_delivery_state(
            clip_db_id,
            dm_message_id=str(message.id),
            dm_channel_id=str(message.channel.id),
            last_sent_at=_utcnow(),
        )
        return {
            "message_id": str(message.id),
            "channel_id": str(message.channel.id),
        }

    async def handle_decision(
        self,
        clip_db_id: int,
        decision: str,
        approved_platforms: list[str] | tuple[str, ...] | set[str] | None,
        user_id: str | None,
    ) -> ApprovalRecord:
        normalized_decision = _normalize_decision(decision)
        clip = self._load_clip_context(clip_db_id)
        if clip is None:
            raise ValueError(f"clip_db_id {clip_db_id} not found")

        selected_platforms = _normalize_platforms(approved_platforms)
        auto_settings = get_auto_approve_settings()
        auto_platforms = [platform for platform, enabled in auto_settings.items() if enabled]
        final_platforms = selected_platforms
        next_state = APPROVAL_STATE_AWAITING
        next_status = APPROVAL_STATE_AWAITING

        if normalized_decision == DECISION_APPROVE:
            final_platforms = _normalize_platforms([*selected_platforms, *auto_platforms])
            if not final_platforms:
                raise ValueError("at least one platform must be approved")
            next_state = APPROVAL_STATE_APPROVED
            next_status = APPROVAL_STATE_APPROVED
        elif normalized_decision == DECISION_SKIP:
            final_platforms = []
            next_state = APPROVAL_STATE_SKIPPED
            next_status = APPROVAL_STATE_SKIPPED
        elif normalized_decision == DECISION_EDIT:
            next_state = APPROVAL_STATE_EDITING
            next_status = APPROVAL_STATE_EDITING

        ensure_approval_row(clip_db_id)
        with transaction() as conn:
            now = _utcnow().isoformat()
            conn.execute(
                """
                UPDATE social_media_clip_approval
                   SET state = %s,
                       approved_platforms = %s,
                       approver_user_id = %s,
                       decided_at = %s
                 WHERE clip_db_id = %s
                """,
                (
                    next_state,
                    json.dumps(final_platforms),
                    str(user_id).strip() if user_id else None,
                    now,
                    clip_db_id,
                ),
            )
            conn.execute(
                "UPDATE twitch_clips_social_media SET status = %s WHERE id = %s",
                (next_status, clip_db_id),
            )

        if normalized_decision == DECISION_APPROVE:
            self.ensure_queued_uploads(clip_db_id)

        return ensure_approval_row(clip_db_id)

    def ensure_queued_uploads(self, clip_db_id: int) -> list[dict[str, Any]]:
        clip = self._load_clip_context(clip_db_id)
        if clip is None:
            raise ValueError(f"clip_db_id {clip_db_id} not found")
        record = ensure_approval_row(clip_db_id)
        if record.state != APPROVAL_STATE_APPROVED:
            return []

        # Local import avoids circular imports from enrichment -> approval.
        from ..enrichment import get_enrichment

        enrichment = get_enrichment(clip_db_id)
        queued: list[dict[str, Any]] = []

        for platform in record.approved_platforms:
            if not is_clip_approved_for(clip_db_id, platform):
                continue
            if self._upload_already_exists(clip_db_id, platform):
                continue
            queue_id = self.clip_manager.queue_upload(
                clip_db_id=clip_db_id,
                platform=platform,
                title=getattr(enrichment, f"title_{platform}", None) if enrichment else None,
                description=getattr(enrichment, f"description_{platform}", None) if enrichment else None,
                hashtags=getattr(enrichment, f"hashtags_{platform}", None) if enrichment else None,
            )
            queued.append({"platform": platform, "queue_id": queue_id})
        return queued

    def _load_clip_context(self, clip_db_id: int) -> dict[str, Any] | None:
        with readonly_connection() as conn:
            row = conn.execute(
                """
                SELECT c.id,
                       c.clip_id,
                       c.clip_title,
                       c.clip_thumbnail_url,
                       c.streamer_login,
                       c.status,
                       c.view_count,
                       e.title_youtube,
                       e.title_tiktok,
                       e.title_instagram,
                       e.hashtags_youtube,
                       e.hashtags_tiktok,
                       e.hashtags_instagram
                  FROM twitch_clips_social_media c
                  LEFT JOIN social_media_clip_enrichment e
                    ON e.clip_db_id = c.id
                 WHERE c.id = %s
                 LIMIT 1
                """,
                (clip_db_id,),
            ).fetchone()
        return dict(row) if row else None

    def _upload_already_exists(self, clip_db_id: int, platform: str) -> bool:
        column = {
            "youtube": "uploaded_youtube",
            "tiktok": "uploaded_tiktok",
            "instagram": "uploaded_instagram",
        }.get(platform)
        if not column:
            return True
        with readonly_connection() as conn:
            row = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT {column} AS uploaded,
                       EXISTS(
                           SELECT 1
                             FROM twitch_clips_upload_queue
                            WHERE clip_id = %s
                              AND platform = %s
                              AND status <> 'failed'
                       ) AS has_queue
                  FROM twitch_clips_social_media
                 WHERE id = %s
                 LIMIT 1
                """,
                (clip_db_id, platform, clip_db_id),
            ).fetchone()
        if not row:
            return True
        uploaded = row["uploaded"] if hasattr(row, "keys") else row[0]
        has_queue = row["has_queue"] if hasattr(row, "keys") else row[1]
        return bool(uploaded) or bool(has_queue)

    @staticmethod
    def _embed_color(state: str) -> discord.Color:
        if state == APPROVAL_STATE_APPROVED:
            return discord.Color.green()
        if state == APPROVAL_STATE_SKIPPED:
            return discord.Color.red()
        if state == APPROVAL_STATE_EDITING:
            return discord.Color.gold()
        return discord.Color.orange()

    async def _resolve_admin_user(self, admin_user_id: str | None) -> discord.abc.User | None:
        if self.bot is None:
            return None
        raw_user_id = str(admin_user_id or self.default_admin_user_id()).strip()
        try:
            user_id = int(raw_user_id)
        except (TypeError, ValueError):
            log.warning("Invalid approval admin Discord user id configured")
            return None

        getter = getattr(self.bot, "get_user", None)
        user = getter(user_id) if callable(getter) else None
        if user is not None:
            return user
        try:
            return await self.bot.fetch_user(user_id)
        except discord.NotFound:
            return None
        except discord.Forbidden:
            log.info("Cannot fetch approval admin Discord user %s", raw_user_id)
            return None
        except discord.HTTPException:
            log.warning("Failed to fetch approval admin Discord user %s", raw_user_id, exc_info=True)
            return None
