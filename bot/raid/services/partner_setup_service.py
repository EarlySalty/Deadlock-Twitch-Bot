from __future__ import annotations

import asyncio
import logging
import os
from collections.abc import Callable
from contextlib import AbstractContextManager
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta
from typing import Any

import discord

from ...discord_role_sync import normalize_discord_user_id, sync_streamer_role
from ...entitlements.catalog import ANALYTICS_TRIAL_PLAN_ID, TRIAL_DURATION_DAYS
from ...storage import (
    backfill_tracked_stats_from_category,
    load_streamer_identity,
    promote_streamer_to_partner,
    readonly_connection,
    transaction,
)


log = logging.getLogger("TwitchStreams.RaidManager")

ReadonlyConnectionFactory = Callable[[], AbstractContextManager[Any]]
TransactionFactory = Callable[[], AbstractContextManager[Any]]
ChatBotGetter = Callable[[], Any | None]
SessionGetter = Callable[[], Any | None]
BotIdGetter = Callable[[], str | None]
MaskIdentifierFn = Callable[[object], str]


def _parse_datetime_value(raw_value: Any) -> datetime | None:
    """Parse a datetime value from DB (isoformat string or datetime)."""
    if raw_value is None:
        return None
    if isinstance(raw_value, datetime):
        parsed = raw_value
    else:
        text = str(raw_value).strip()
        if not text:
            return None
        if len(text) == 10 and text[4] == "-" and text[7] == "-":
            text = f"{text}T23:59:59+00:00"
        normalized = text.replace("Z", "+00:00")
        try:
            parsed = datetime.fromisoformat(normalized)
        except ValueError:
            return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return parsed.astimezone(UTC)


@dataclass(slots=True)
class PartnerSetupService:
    auth_manager: Any
    session_getter: SessionGetter | None = None
    chat_bot_getter: ChatBotGetter | None = None
    bot_id_getter: BotIdGetter | None = None
    readonly_connection_factory: ReadonlyConnectionFactory | None = None
    transaction_factory: TransactionFactory | None = None
    moderator_url_base: str = "https://api.twitch.tv/helix"
    mask_log_identifier: MaskIdentifierFn | None = None
    logger: logging.Logger = field(default_factory=lambda: log)

    @staticmethod
    def normalize_discord_user_id(raw: str | None) -> str | None:
        return normalize_discord_user_id(raw)

    def _session(self) -> Any | None:
        return self.session_getter() if callable(self.session_getter) else None

    def _chat_bot(self) -> Any | None:
        return self.chat_bot_getter() if callable(self.chat_bot_getter) else None

    def _bot_id(self) -> str | None:
        if callable(self.bot_id_getter):
            resolved = str(self.bot_id_getter() or "").strip()
            if resolved:
                return resolved
        return None

    async def resolve_discord_display_name(
        self,
        discord_user_id: str | None,
    ) -> str | None:
        normalized_id = self.normalize_discord_user_id(discord_user_id)
        if not normalized_id:
            return None

        discord_bot = getattr(self.auth_manager, "_discord_bot", None)
        if discord_bot is None:
            return None

        user_id_int = int(normalized_id)
        user = discord_bot.get_user(user_id_int)
        if user is None:
            try:
                user = await discord_bot.fetch_user(user_id_int)
            except (discord.NotFound, discord.Forbidden, discord.HTTPException):
                return None

        if user is None:
            return None
        return (
            str(
                getattr(user, "global_name", None)
                or getattr(user, "display_name", None)
                or getattr(user, "name", None)
                or ""
            ).strip()
            or None
        )

    async def apply_streamer_role(
        self,
        discord_user_id: str | None,
        *,
        should_have_role: bool,
        reason: str,
    ) -> None:
        await sync_streamer_role(
            getattr(self.auth_manager, "_discord_bot", None),
            discord_user_id,
            should_have_role=should_have_role,
            reason=reason,
            logger=self.logger,
        )

    async def sync_partner_state_after_auth(
        self,
        twitch_user_id: str,
        twitch_login: str,
        *,
        state_discord_user_id: str | None = None,
        activate_partner_features: bool = True,
    ) -> str | None:
        provided_discord_id = self.normalize_discord_user_id(state_discord_user_id)
        existing_discord_id: str | None = None
        existing_display_name: str | None = None

        connection_factory = self.readonly_connection_factory or readonly_connection
        with connection_factory() as conn:
            row = load_streamer_identity(
                conn,
                twitch_user_id=twitch_user_id,
                twitch_login=twitch_login,
            )
            if row:
                existing_discord_id = self.normalize_discord_user_id(
                    row[2] if not hasattr(row, "keys") else row["discord_user_id"]
                )
                existing_display_name = (
                    str(
                        row[3] if not hasattr(row, "keys") else row["discord_display_name"] or ""
                    ).strip()
                    or None
                )

        final_discord_id = provided_discord_id or existing_discord_id
        final_display_name = existing_display_name or await self.resolve_discord_display_name(
            final_discord_id
        )

        is_on_discord_value = 1 if final_discord_id else 0
        txn_factory = self.transaction_factory or transaction
        with txn_factory() as conn:
            partner_kwargs: dict[str, object] = {
                "discord_user_id": final_discord_id,
                "discord_display_name": final_display_name,
                "is_on_discord": is_on_discord_value,
                "manual_verified_permanent": 1,
                "manual_verified_until": None,
                "manual_verified_at": datetime.now(UTC).isoformat(),
            }
            if activate_partner_features:
                partner_kwargs.update(
                    {
                        "manual_partner_opt_out": 0,
                        "raid_bot_enabled": 1,
                    }
                )
            promote_streamer_to_partner(
                conn,
                twitch_login=twitch_login,
                twitch_user_id=twitch_user_id,
                **partner_kwargs,
            )
            copied = backfill_tracked_stats_from_category(conn, twitch_login)
            if copied:
                self.logger.info(
                    "Backfilled %d category samples into tracked for %s during partner sync",
                    copied,
                    twitch_login,
                )

        if final_discord_id:
            await self.apply_streamer_role(
                final_discord_id,
                should_have_role=True,
                reason="Twitch-Bot erfolgreich autorisiert",
            )
        return final_discord_id

    async def _record_first_login(
        self,
        twitch_user_id: str,
        twitch_login: str,
    ) -> None:
        """Record first_login_at timestamp if not already set. Called on OAuth completion."""
        def _do_record(conn: Any) -> None:
            now = datetime.now(tz=UTC).isoformat()
            conn.execute(
                """
                INSERT INTO streamer_plans (twitch_user_id, twitch_login, first_login_at)
                VALUES (%s, %s, %s)
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                    first_login_at = COALESCE(streamer_plans.first_login_at, EXCLUDED.first_login_at)
                """,
                (twitch_user_id, twitch_login, now),
            )

        connection_factory = self.transaction_factory or transaction
        try:
            with connection_factory() as conn:
                _do_record(conn)
                self.logger.info("Recorded first_login_at for %s (%s)", twitch_login, twitch_user_id)
        except Exception:
            self.logger.exception("Failed to record first_login_at for %s (%s)", twitch_login, twitch_user_id)

    async def check_and_grant_trial_eligibility(
        self,
        twitch_user_id: str,
        twitch_login: str,
    ) -> bool:
        """Check if user is eligible for 45-day Analytics Trial and grant if eligible.

        Eligibility requires:
        - first_login_at was more than 24 hours ago
        - trial_ever_granted is not already set
        - User has no paid plan (billing or manual)

        Returns True if trial was granted, False otherwise.
        """
        paid_plan_ids = {"raid_boost", "analysis_dashboard", "bundle_analysis_raid_boost"}
        grace_period_hours = 24

        def _check_and_grant(conn: Any) -> bool:
            # Check if trial was already granted (immutable flag)
            trial_row = conn.execute(
                """
                SELECT trial_ever_granted FROM streamer_plans
                WHERE TRIM(COALESCE(twitch_user_id, '')) = %s
                   OR LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                LIMIT 1
                """,
                (twitch_user_id, twitch_login),
            ).fetchone()

            if trial_row and int(trial_row[0] or 0) == 1:
                self.logger.info(
                    "User %s (%s) already has trial_ever_granted=1, skipping",
                    twitch_login,
                    twitch_user_id,
                )
                return False

            # Check if first_login_at is more than 24 hours ago
            first_login_row = conn.execute(
                """
                SELECT first_login_at FROM streamer_plans
                WHERE TRIM(COALESCE(twitch_user_id, '')) = %s
                   OR LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                LIMIT 1
                """,
                (twitch_user_id, twitch_login),
            ).fetchone()

            if not first_login_row or not first_login_row[0]:
                self.logger.info(
                    "User %s (%s) has no first_login_at yet, cannot grant trial",
                    twitch_login,
                    twitch_user_id,
                )
                return False

            first_login_at = _parse_datetime_value(first_login_row[0])
            if first_login_at is None:
                self.logger.warning(
                    "User %s (%s) has invalid first_login_at, cannot grant trial",
                    twitch_login,
                    twitch_user_id,
                )
                return False

            hours_since_login = (datetime.now(tz=UTC) - first_login_at).total_seconds() / 3600
            if hours_since_login < grace_period_hours:
                self.logger.info(
                    "User %s (%s) first_login_at is only %.1f hours ago (need %d hours), cannot grant trial yet",
                    twitch_login,
                    twitch_user_id,
                    hours_since_login,
                    grace_period_hours,
                )
                return False

            # Check if user already has a paid billing subscription
            billing_row = conn.execute(
                """
                SELECT plan_id FROM twitch_billing_subscriptions
                WHERE LOWER(customer_reference) = LOWER(%s)
                  AND status IN ('active', 'trialing')
                LIMIT 1
                """,
                (twitch_user_id,),
            ).fetchone()

            if billing_row:
                existing_plan = str(billing_row[0] or "").strip()
                if existing_plan in paid_plan_ids:
                    self.logger.info(
                        "User %s (%s) already has paid plan %s, skipping trial",
                        twitch_login,
                        twitch_user_id,
                        existing_plan,
                    )
                    return False

            # Check if user already has a manual paid plan set
            manual_row = conn.execute(
                """
                SELECT manual_plan_id FROM streamer_plans
                WHERE TRIM(COALESCE(twitch_user_id, '')) = %s
                   OR LOWER(COALESCE(twitch_login, '')) = LOWER(%s)
                LIMIT 1
                """,
                (twitch_user_id, twitch_login),
            ).fetchone()

            if manual_row:
                existing_manual_plan = str(manual_row[0] or "").strip()
                if existing_manual_plan and existing_manual_plan != "raid_free":
                    self.logger.info(
                        "User %s (%s) already has manual plan %s, skipping trial",
                        twitch_login,
                        twitch_user_id,
                        existing_manual_plan,
                    )
                    return False

            # Grant the trial and set immutable flag
            trial_expires_at = (datetime.now(tz=UTC) + timedelta(days=TRIAL_DURATION_DAYS)).isoformat()
            now = datetime.now(tz=UTC).isoformat()
            conn.execute(
                """
                INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at, trial_ever_granted, manual_plan_notes, manual_plan_updated_at)
                VALUES (%s, %s, %s, %s, 1, 'Trial granted after 24h grace period', %s)
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                    manual_plan_id = EXCLUDED.manual_plan_id,
                    manual_plan_expires_at = EXCLUDED.manual_plan_expires_at,
                    trial_ever_granted = 1,
                    manual_plan_notes = EXCLUDED.manual_plan_notes,
                    manual_plan_updated_at = EXCLUDED.manual_plan_updated_at
                """,
                (twitch_user_id, twitch_login, ANALYTICS_TRIAL_PLAN_ID, trial_expires_at, now),
            )
            self.logger.info(
                "Granted %d-day Analytics Trial to %s (%s) after %d-hour grace period, expires at %s",
                TRIAL_DURATION_DAYS,
                twitch_login,
                twitch_user_id,
                grace_period_hours,
                trial_expires_at,
            )
            return True

        connection_factory = self.transaction_factory or transaction
        try:
            with connection_factory() as conn:
                return _check_and_grant(conn)
        except Exception:
            self.logger.exception("Failed to check/grant trial eligibility for %s (%s)", twitch_login, twitch_user_id)
            return False

    async def complete_setup_for_streamer(
        self,
        twitch_user_id: str,
        twitch_login: str,
        state_discord_user_id: str | None = None,
        activate_partner_features: bool = True,
    ) -> None:
        self.logger.info("Completing setup for streamer %s (%s)", twitch_login, twitch_user_id)

        try:
            await self.sync_partner_state_after_auth(
                twitch_user_id,
                twitch_login,
                state_discord_user_id=state_discord_user_id,
                activate_partner_features=activate_partner_features,
            )
        except Exception:
            self.logger.exception(
                "Failed to sync partner state after auth for %s (%s)",
                twitch_login,
                twitch_user_id,
            )

        # Record first login timestamp for delayed trial grant (24h grace period)
        await self._record_first_login(twitch_user_id, twitch_login)

        session = self._session()
        if session is None:
            self.logger.warning("No HTTP session available to complete setup for %s", twitch_login)
            return

        tokens = await self.auth_manager.get_tokens_for_user(twitch_user_id, session)
        if not tokens:
            self.logger.warning("Could not load OAuth grant for %s to complete setup", twitch_login)
            return

        access_token, _ = tokens
        chat_bot = self._chat_bot()
        bot_id = self._bot_id()
        if not bot_id:
            bot_id = os.getenv("TWITCH_BOT_USER_ID", "").strip() or None
        if not bot_id:
            self.logger.warning(
                "complete_setup: Keine Bot-ID verfügbar für %s (chat_bot=%s). Setze TWITCH_BOT_USER_ID ENV.",
                twitch_login,
                "None" if not chat_bot else "set",
            )
            return

        try:
            url = f"{self.moderator_url_base}/moderation/moderators"
            headers = {
                "Client-ID": self.auth_manager.client_id,
                "Authorization": f"Bearer {access_token}",
            }
            params = {"broadcaster_id": twitch_user_id, "user_id": bot_id}
            async with session.post(url, headers=headers, params=params) as response:
                if response.status in {200, 204}:
                    self.logger.info(
                        "Bot (ID: %s) is now moderator in %s's channel (ID: %s)",
                        bot_id,
                        twitch_login,
                        twitch_user_id,
                    )
                elif response.status == 422:
                    self.logger.info(
                        "Bot (ID: %s) is already moderator in %s's channel",
                        bot_id,
                        twitch_login,
                    )
                else:
                    body = await response.text()
                    if response.status == 400 and "already a mod" in body.lower():
                        self.logger.info(
                            "Bot (ID: %s) is already moderator in %s's channel (HTTP 400 variant)",
                            bot_id,
                            twitch_login,
                        )
                    else:
                        subject = twitch_login
                        if callable(self.mask_log_identifier):
                            subject = self.mask_log_identifier(twitch_login)
                        self.logger.warning(
                            "Failed to add bot as moderator in %s: HTTP %s (used broadcaster grant)",
                            subject,
                            response.status,
                        )
        except Exception:
            self.logger.exception("Error adding bot as moderator for %s", twitch_login)

        if chat_bot is None:
            return

        try:
            await chat_bot.join(twitch_login, channel_id=twitch_user_id)
            await asyncio.sleep(2)

            message = "Deadlock Chatbot Guard verbunden! 🎮"
            commands_public = (
                "Commands für alle: "
                "!ping (Bot-Status) | "
                "!clip [beschreibung] (Clip erstellen) | "
                "!raid_history (letzte Raids)"
            )
            commands_mod = (
                "Mod-Commands: "
                "!raid / !traid (Raid starten) | "
                "!raid_status (Bot-Status) | "
                "!uban / !unban (letzten Auto-Ban aufheben) | "
                "!silentban / !silentraid (Benachrichtigungen an/aus)"
            )

            if hasattr(chat_bot, "_send_chat_message"):
                class MockChannel:
                    def __init__(self, login: str, uid: str) -> None:
                        self.name = login
                        self.id = uid

                mock_channel = MockChannel(twitch_login, twitch_user_id)
                await chat_bot._send_chat_message(mock_channel, message)
                await asyncio.sleep(1)
                await chat_bot._send_chat_message(mock_channel, commands_public)
                await asyncio.sleep(1)
                await chat_bot._send_chat_message(mock_channel, commands_mod)
            elif hasattr(chat_bot, "send_message") and bot_id:
                await chat_bot.send_message(str(twitch_user_id), str(bot_id), message)
                await asyncio.sleep(1)
                await chat_bot.send_message(str(twitch_user_id), str(bot_id), commands_public)
                await asyncio.sleep(1)
                await chat_bot.send_message(str(twitch_user_id), str(bot_id), commands_mod)

            self.logger.info("Sent auth success message to %s", twitch_login)
        except Exception:
            self.logger.exception("Error sending auth success message to %s", twitch_login)


__all__ = ["PartnerSetupService"]
