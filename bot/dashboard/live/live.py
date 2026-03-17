"""Live dashboard views and actions for managing Twitch streamers."""

from __future__ import annotations

import html
from datetime import UTC, datetime
from typing import Any
from urllib.parse import quote_plus

from aiohttp import web

from ... import storage as _storage
from ...core.constants import log
from ..billing.billing_plans import BILLING_PLANS as _BILLING_PLANS

# Alle Broadcaster-Scopes, die ein vollständig autorisierter Streamer haben sollte.
_REQUIRED_SCOPES: list[str] = [
    "channel:manage:raids",
    "channel:read:subscriptions",
    "channel:manage:moderators",
    "channel:bot",
    "clips:edit",
    "channel:read:ads",
    "bits:read",
    "channel:read:hype_train",
    "channel:read:redemptions",
]

_SCOPE_COLUMN_LABELS: dict[str, str] = {
    "channel:manage:raids": "Raids",
    "channel:read:subscriptions": "Subs",
    "channel:manage:moderators": "Mods",
    "channel:bot": "Bot",
    "clips:edit": "Clips",
    "channel:read:ads": "Ads",
    "bits:read": "Bits",
    "channel:read:hype_train": "Hype",
    "channel:read:redemptions": "Points",
}

# Scopes die besonders wichtig für Analytics/Lurker-Tracking sind
_CRITICAL_SCOPES: set[str] = {
    "channel:read:redemptions",  # Channel Point Redemptions
    "bits:read",  # Bits Events
    "channel:read:hype_train",  # Hype Train Events
    "channel:read:subscriptions",  # Sub Events
}

_BOT_REQUIRED_SCOPES: tuple[str, ...] = (
    "user:read:chat",
    "user:write:chat",
    "moderator:read:chatters",
    "moderator:read:followers",
    "moderator:manage:chat_messages",
    "moderator:manage:announcements",
    "moderator:manage:banned_users",
    "moderator:manage:shoutouts",
)
_BOT_OPTIONAL_SCOPES: tuple[str, ...] = ("user:bot",)

_BOT_SCOPE_LABELS: dict[str, str] = {
    "user:read:chat": "Bot Chat Read",
    "user:write:chat": "Bot Chat Write",
    "moderator:read:chatters": "Bot Chatters",
    "moderator:read:followers": "Bot Followers",
    "moderator:manage:chat_messages": "Bot Chat Mod",
    "moderator:manage:announcements": "Bot Announcements",
    "moderator:manage:banned_users": "Bot Bans",
    "moderator:manage:shoutouts": "Bot Shoutouts",
    "user:bot": "Bot App-Token",
}

_DASHBOARD_OWNER_DISCORD_ID = "662995601738170389"
_CHAT_ACTION_MODES = {"message", "action", "announcement"}
_CHAT_ANNOUNCEMENT_COLORS = {"blue", "green", "orange", "purple", "primary"}


class DashboardLiveMixin:
    async def _read_post_with_csrf(
        self, request: web.Request, *, fallback_path: str = "/twitch/admin"
    ):
        data = await request.post()
        csrf_token = str(data.get("csrf_token") or "").strip()
        if self._csrf_verify_token(request, csrf_token):
            return data
        location = self._redirect_location(
            request,
            err="Ungültiges CSRF-Token",
            default_path=fallback_path,
        )
        raise web.HTTPFound(location=location)

    def _get_discord_admin_user_id(self, request: web.Request) -> str:
        getter = getattr(self, "_get_discord_admin_session", None)
        if not callable(getter):
            return ""
        try:
            session = getter(request) or {}
        except Exception:
            log.debug("Could not resolve discord admin session for live action", exc_info=True)
            return ""
        return str(session.get("user_id") or "").strip()

    def _resolve_dashboard_chat_bot(self) -> Any | None:
        raid_bot = getattr(self, "_raid_bot", None)
        if raid_bot is None:
            return None

        chat_bot = getattr(raid_bot, "chat_bot", None)
        if chat_bot is not None:
            return chat_bot

        cog = getattr(raid_bot, "_cog", None)
        if cog is None:
            return None
        return getattr(cog, "_twitch_chat_bot", None)

    def _resolve_dashboard_bot_scope_state(self) -> dict[str, Any]:
        token_mgr = None
        chat_bot = self._resolve_dashboard_chat_bot()
        if chat_bot is not None:
            token_mgr = getattr(chat_bot, "_token_manager", None)

        if token_mgr is None:
            raid_bot = getattr(self, "_raid_bot", None)
            cog = getattr(raid_bot, "_cog", None) if raid_bot is not None else None
            if cog is not None:
                token_mgr = getattr(cog, "_bot_token_manager", None)

        if token_mgr is None:
            return {"available": False, "loaded": False, "scopes": set()}

        try:
            scopes = {
                str(scope).strip().lower()
                for scope in (getattr(token_mgr, "scopes", None) or set())
                if str(scope).strip()
            }
            loaded = bool(
                getattr(token_mgr, "bot_id", None) or getattr(token_mgr, "expires_at", None)
            )
            return {"available": True, "loaded": loaded, "scopes": scopes}
        except Exception:
            log.debug("Could not resolve dashboard bot scope state", exc_info=True)
            return {"available": True, "loaded": False, "scopes": set()}

    @staticmethod
    def _build_dashboard_chat_channel(chat_bot: Any, login: str, channel_id: str) -> Any:
        make_channel = getattr(chat_bot, "_make_promo_channel", None)
        if callable(make_channel):
            return make_channel(login, channel_id)

        class _Channel:
            __slots__ = ("name", "id")

            def __init__(self, name: str, cid: str):
                self.name = name
                self.id = cid

        return _Channel(login, channel_id)

    @staticmethod
    def _coerce_twitch_user_id(value: Any) -> str:
        if value is None:
            return ""
        if isinstance(value, dict):
            for key in ("twitch_user_id", "user_id", "id"):
                resolved = DashboardLiveMixin._coerce_twitch_user_id(value.get(key))
                if resolved:
                    return resolved
            data = value.get("data")
            return DashboardLiveMixin._coerce_twitch_user_id(data)
        if isinstance(value, (list, tuple)):
            for item in value:
                resolved = DashboardLiveMixin._coerce_twitch_user_id(item)
                if resolved:
                    return resolved
            return ""
        for attr in ("twitch_user_id", "user_id", "id"):
            resolved = str(getattr(value, attr, "") or "").strip()
            if resolved.isdigit():
                return resolved
        candidate = str(value or "").strip()
        return candidate if candidate.isdigit() else ""

    def _resolve_streamer_user_id_from_db(self, login: str) -> str:
        try:
            with _storage.get_conn() as conn:
                partner_row = _storage.load_active_partner(conn, twitch_login=login)
                if partner_row:
                    raw = (
                        partner_row.get("twitch_user_id")
                        if hasattr(partner_row, "keys")
                        else partner_row[1]
                    )
                    resolved = self._coerce_twitch_user_id(raw)
                    if resolved:
                        return resolved

                identity_row = _storage.load_streamer_identity(conn, twitch_login=login)
                if identity_row:
                    raw = (
                        identity_row.get("twitch_user_id")
                        if hasattr(identity_row, "keys")
                        else identity_row[0]
                    )
                    resolved = self._coerce_twitch_user_id(raw)
                    if resolved:
                        return resolved

                auth_row = conn.execute(
                    """
                    SELECT twitch_user_id
                    FROM twitch_raid_auth
                    WHERE LOWER(COALESCE(twitch_login, '')) = LOWER(?)
                    LIMIT 1
                    """,
                    (login,),
                ).fetchone()
            if not auth_row:
                return ""
            raw = auth_row["twitch_user_id"] if hasattr(auth_row, "keys") else auth_row[0]
            return self._coerce_twitch_user_id(raw)
        except Exception:
            safe_login = str(login or "").replace("\r", "\\r").replace("\n", "\\n")
            log.debug("Could not resolve twitch_user_id from DB for %s", safe_login, exc_info=True)
            return ""

    async def _resolve_streamer_user_id_from_twitch(self, chat_bot: Any, login: str) -> str:
        fetch_user = getattr(chat_bot, "fetch_user", None)
        if not callable(fetch_user):
            return ""

        try:
            try:
                user = await fetch_user(login=login)
            except TypeError:
                user = await fetch_user(login)
        except Exception:
            safe_login = str(login or "").replace("\r", "\\r").replace("\n", "\\n")
            log.debug("Could not resolve twitch_user_id via Twitch API for %s", safe_login, exc_info=True)
            return ""

        return self._coerce_twitch_user_id(user)

    def _persist_streamer_user_id(self, login: str, user_id: str) -> None:
        if not login or not user_id:
            return
        try:
            with _storage.get_conn() as conn:
                existing = _storage.load_streamer_identity(conn, twitch_login=login)
                canonical_login = str(
                    (
                        existing.get("twitch_login")
                        if existing and hasattr(existing, "keys")
                        else (existing[1] if existing else login)
                    )
                    or login
                ).strip().lower()
                _storage.upsert_streamer_identity(
                    conn,
                    twitch_user_id=user_id,
                    twitch_login=canonical_login,
                )
        except Exception:
            safe_login = str(login or "").replace("\r", "\\r").replace("\n", "\\n")
            log.debug("Could not persist twitch_user_id for %s", safe_login, exc_info=True)

    @staticmethod
    def _billing_admin_source_badge(source: str) -> str:
        normalized = str(source or "").strip().lower()
        if normalized == "manual_override":
            return "<span class='pill ok'>Manuell</span>"
        if normalized == "billing_subscription":
            return "<span class='pill neutral'>Stripe</span>"
        return "<span class='pill warn'>Basic Fallback</span>"

    def _render_billing_admin_plan_card(self, *, csrf_input_html: str) -> str:
        rows_getter = getattr(self, "_billing_admin_plan_rows_for_all_streamers", None)
        if not callable(rows_getter):
            return ""

        try:
            plan_rows = list(rows_getter() or [])
        except Exception:
            log.exception("billing admin plan rows render failed")
            return (
                "<div class='card scope-card'>"
                "  <div class='card-header'>"
                "    <div>"
                "      <p class='eyebrow'>Billing & Plaene</p>"
                "      <h2>Manuelle Planvergabe</h2>"
                "      <p class='lead'>Die Planverwaltung konnte nicht geladen werden.</p>"
                "    </div>"
                "  </div>"
                "</div>"
            )

        plan_labels = {
            str(plan.get("id") or "").strip(): str(plan.get("name") or "").strip()
            for plan in _BILLING_PLANS
            if str(plan.get("id") or "").strip()
        }
        plan_options = "".join(
            (
                f"<option value='{html.escape(plan_id, quote=True)}'>"
                f"{html.escape(plan_name)}"
                "</option>"
            )
            for plan_id, plan_name in plan_labels.items()
        )

        table_rows: list[str] = []
        for row in plan_rows:
            login = str(row.get("twitch_login") or "").strip()
            if not login:
                continue
            escaped_login = html.escape(login, quote=True)
            effective_plan_id = str(row.get("effective_plan_id") or "raid_free").strip() or "raid_free"
            effective_plan_label = plan_labels.get(effective_plan_id, effective_plan_id)
            effective_source = str(row.get("effective_plan_source") or "default_basic").strip()
            manual_override = row.get("manual_override") if isinstance(row.get("manual_override"), dict) else None
            billing_subscription = (
                row.get("billing_subscription")
                if isinstance(row.get("billing_subscription"), dict)
                else None
            )
            billing_status = str((billing_subscription or {}).get("status") or "").strip() or "—"
            billing_plan_id = str((billing_subscription or {}).get("plan_id") or "").strip()
            billing_plan_label = plan_labels.get(billing_plan_id, billing_plan_id or "—")
            manual_plan_id = str((manual_override or {}).get("plan_id") or "").strip()
            manual_plan_label = plan_labels.get(manual_plan_id, manual_plan_id or "—")
            manual_notes = str((manual_override or {}).get("notes") or "").strip()
            manual_expires = str((manual_override or {}).get("expires_at") or "").strip()
            manual_is_active = bool((manual_override or {}).get("is_active"))
            manual_is_expired = bool((manual_override or {}).get("is_expired"))
            manual_source_badge = (
                "<span class='pill ok'>Aktiv</span>"
                if manual_is_active
                else (
                    "<span class='pill warn'>Abgelaufen</span>"
                    if manual_is_expired
                    else "<span class='pill neutral'>Kein Override</span>"
                )
            )
            flag_chips = []
            if bool(row.get("manual_partner_opt_out")):
                flag_chips.append("<span class='chip'>Kein Partner</span>")
            if row.get("archived_at"):
                flag_chips.append("<span class='chip'>Archiviert</span>")
            if bool(row.get("is_on_discord")):
                flag_chips.append("<span class='chip'>Discord</span>")
            flags_html = "".join(flag_chips) or "<span class='status-meta'>Keine Zusatzflags</span>"
            selected_manual_id = manual_plan_id if manual_plan_id in plan_labels else effective_plan_id
            option_html = "".join(
                (
                    f"<option value='{html.escape(plan_id, quote=True)}'"
                    f"{' selected' if plan_id == selected_manual_id else ''}>"
                    f"{html.escape(plan_name)}"
                    "</option>"
                )
                for plan_id, plan_name in plan_labels.items()
            )
            expires_value = ""
            if manual_expires:
                expires_value = manual_expires[:10]
            preview_rows = [
                (
                    "<div class='discord-preview-row'>"
                    "<span class='preview-label'>Effektiv</span>"
                    f"<span>{html.escape(effective_plan_label)}</span>"
                    "</div>"
                ),
                (
                    "<div class='discord-preview-row'>"
                    "<span class='preview-label'>Quelle</span>"
                    f"<span>{self._billing_admin_source_badge(effective_source)}</span>"
                    "</div>"
                ),
                (
                    "<div class='discord-preview-row'>"
                    "<span class='preview-label'>Stripe</span>"
                    f"<span>{html.escape(billing_plan_label)} · {html.escape(billing_status)}</span>"
                    "</div>"
                ),
                (
                    "<div class='discord-preview-row'>"
                    "<span class='preview-label'>Override</span>"
                    f"<span>{manual_source_badge}</span>"
                    "</div>"
                ),
            ]
            if manual_notes:
                preview_rows.append(
                    "<div class='discord-preview-row'>"
                    "<span class='preview-label'>Notiz</span>"
                    f"<span>{html.escape(manual_notes)}</span>"
                    "</div>"
                )
            if expires_value:
                preview_rows.append(
                    "<div class='discord-preview-row'>"
                    "<span class='preview-label'>Ablauf</span>"
                    f"<span>{html.escape(expires_value)}</span>"
                    "</div>"
                )
            action_summary = "Override anpassen" if manual_override else "Plan manuell vergeben"
            clear_button_html = ""
            if manual_override:
                clear_button_html = (
                    "<button class='btn btn-small btn-secondary' type='submit' "
                    "formaction='/twitch/admin/manual-plan/clear' formmethod='post'>"
                    "Override entfernen"
                    "</button>"
                )

            table_rows.append(
                "<tr>"
                f"  <td><strong>{html.escape(login)}</strong><div class='status-meta'>{flags_html}</div></td>"
                "  <td>"
                f"    <div class='raid-cell'><span class='pill ok'>{html.escape(effective_plan_label)}</span>"
                f"    <div class='status-meta'>{self._billing_admin_source_badge(effective_source)}</div></div>"
                "  </td>"
                "  <td>"
                f"    <div class='raid-cell'>{manual_source_badge}"
                f"    <div class='status-meta'>{html.escape(manual_plan_label)}</div></div>"
                "  </td>"
                "  <td>"
                f"    <div class='raid-cell'><span class='pill neutral'>{html.escape(billing_plan_label)}</span>"
                f"    <div class='status-meta'>Status: {html.escape(billing_status)}</div></div>"
                "  </td>"
                "  <td>"
                "    <details class='advanced-details'>"
                f"      <summary>{html.escape(action_summary)}</summary>"
                "      <div class='advanced-content'>"
                f"        <div class='discord-preview'>{''.join(preview_rows)}</div>"
                "        <form method='post' action='/twitch/admin/manual-plan'>"
                f"          {csrf_input_html}"
                f"          <input type='hidden' name='login' value='{escaped_login}' />"
                "          <div class='form-row'>"
                f"            <label>Plan<select name='plan_id'>{option_html}</select></label>"
                f"            <label>Ablauf (optional)<input type='text' name='expires_at' value='{html.escape(expires_value, quote=True)}' placeholder='YYYY-MM-DD'></label>"
                "          </div>"
                f"          <label>Notiz<input type='text' name='notes' value='{html.escape(manual_notes, quote=True)}' placeholder='z.B. Testphase / Kulanz'></label>"
                "          <div class='action-stack'>"
                "            <button class='btn btn-small'>Speichern</button>"
                f"            {clear_button_html}"
                "          </div>"
                "        </form>"
                "      </div>"
                "    </details>"
                "  </td>"
                "</tr>"
            )

        if not table_rows:
            table_rows.append(
                "<tr><td colspan='5'>Keine Streamer für die Planverwaltung gefunden.</td></tr>"
            )

        return (
            "<div class='card scope-card'>"
            "  <div class='card-header'>"
            "    <div>"
            "      <p class='eyebrow'>Billing & Plaene</p>"
            "      <h2>Manuelle Planvergabe</h2>"
            "      <p class='lead'>Override fuer Partner setzen oder entfernen. Bei Entfernen greift wieder Stripe oder der Basic-Fallback.</p>"
            "    </div>"
            "    <div class='raid-metrics'>"
            f"      <div class='mini-stat'><strong>{len(plan_rows)}</strong><span>Streamer</span></div>"
            "    </div>"
            "  </div>"
            "  <div class='table-wrap'>"
            "    <table>"
            "      <thead><tr>"
            "        <th>Streamer</th>"
            "        <th>Effektiv</th>"
            "        <th>Manuell</th>"
            "        <th>Stripe</th>"
            "        <th>Aktionen</th>"
            "      </tr></thead>"
            f"      <tbody>{''.join(table_rows)}</tbody>"
            "    </table>"
            "  </div>"
            "</div>"
        )

    async def index(self, request: web.Request):
        self._require_token(request)
        items = await self._list()

        msg = request.query.get("ok", "")
        err = request.query.get("err", "")
        discord_admin_user_id = self._get_discord_admin_user_id(request)
        can_use_owner_chat_actions = discord_admin_user_id == _DASHBOARD_OWNER_DISCORD_ID
        csrf_token = self._csrf_generate_token(request)
        csrf_input_html = (
            f"<input type='hidden' name='csrf_token' value='{html.escape(csrf_token, quote=True)}'>"
        )
        admin_nav_renderer = getattr(self, "_render_admin_section_nav", None)
        admin_section_nav_html = (
            admin_nav_renderer("overview") if callable(admin_nav_renderer) else ""
        )
        announcement_card_renderer = getattr(
            self,
            "_render_admin_announcement_overview_card",
            None,
        )
        admin_announcement_card_html = (
            announcement_card_renderer() if callable(announcement_card_renderer) else ""
        )

        discord_filter = (request.query.get("discord") or "any").lower()
        if discord_filter not in {"any", "yes", "no", "linked"}:
            discord_filter = "any"

        total_count = sum(
            1
            for st in items
            if not bool(st.get("manual_partner_opt_out")) and not bool(st.get("archived_at"))
        )
        raid_bot_available = bool(getattr(self, "_raid_bot", None))
        if not raid_bot_available:
            # Split-runtime dashboard has no in-process raid bot object.
            # In this mode, a wired non-empty list callback means the bot runtime is reachable.
            list_cb = getattr(self, "_list", None)
            empty_list_cb = getattr(self, "_empty_list", None)
            list_func = getattr(list_cb, "__func__", list_cb)
            empty_list_func = getattr(empty_list_cb, "__func__", empty_list_cb)
            if callable(list_cb) and callable(empty_list_cb) and list_func is not empty_list_func:
                raid_bot_available = True

        def raid_auth_link(login: str) -> str:
            return f"/twitch/raid/auth?login={quote_plus(login)}"

        now = datetime.now(UTC)

        def _parse_dt(value: str | datetime | None) -> datetime | None:
            """Best-effort parser supporting ISO strings and datetime objects."""
            if value is None:
                return None
            if isinstance(value, datetime):
                dt = value
            else:
                try:
                    dt = datetime.fromisoformat(str(value))
                except (ValueError, TypeError):
                    return None
            if dt.tzinfo is None:
                dt = dt.replace(tzinfo=UTC)
            return dt.astimezone(UTC)

        rows: list[str] = []
        non_partner_entries: list[dict] = []
        archived_entries: list[dict] = []
        filtered_count = 0
        raid_authorized_count = 0
        raid_ready_count = 0
        raid_missing_logins: list[str] = []
        for st in items:
            login = st.get("twitch_login", "")
            login_html = html.escape(login)
            permanent = bool(st.get("manual_verified_permanent"))
            until_raw = st.get("manual_verified_until")
            until_dt = _parse_dt(until_raw)
            verified_at_dt = _parse_dt(st.get("manual_verified_at"))
            archived_at_raw = st.get("archived_at")
            archived_dt = _parse_dt(archived_at_raw)
            is_archived = archived_dt is not None
            last_deadlock_dt = _parse_dt(st.get("last_deadlock_stream_at"))
            inactive_days: int | None = None
            if last_deadlock_dt:
                inactive_days = (now.date() - last_deadlock_dt.date()).days
            partner_opt_out = bool(st.get("manual_partner_opt_out"))
            raid_auth_enabled = st.get("raid_auth_enabled")
            raid_authorized_at = st.get("raid_authorized_at")
            raid_bot_enabled = bool(st.get("raid_bot_enabled"))
            raid_is_authorized = raid_auth_enabled is not None or bool(raid_authorized_at)
            raid_auto_active = bool(raid_auth_enabled) and raid_bot_enabled

            if not partner_opt_out and not is_archived:
                if raid_is_authorized:
                    raid_authorized_count += 1
                else:
                    if login:
                        raid_missing_logins.append(login)
                if raid_auto_active:
                    raid_ready_count += 1

            is_on_discord = bool(st.get("is_on_discord"))
            discord_user_id = str(st.get("discord_user_id") or "").strip()
            discord_display_name = str(st.get("discord_display_name") or "").strip()
            has_discord_data = bool(discord_user_id or discord_display_name)

            if discord_filter == "yes" and not is_on_discord:
                continue
            if discord_filter == "no" and is_on_discord:
                continue
            if discord_filter == "linked" and not has_discord_data:
                continue

            status_badge = "<span class='badge badge-neutral'>Nicht verifiziert</span>"
            status_text = "Nicht verifiziert"
            meta_parts: list[str] = []
            countdown_label = "—"
            countdown_classes: list[str] = []

            if partner_opt_out:
                status_badge = "<span class='badge badge-neutral'>Kein Partner</span>"
                status_text = "Kein Partner"
                meta_parts.append("Nicht als Partner gelistet")
            elif permanent:
                status_badge = "<span class='badge badge-ok'>Dauerhaft verifiziert</span>"
                status_text = "Dauerhaft verifiziert"
            elif until_dt:
                day_diff = (until_dt.date() - now.date()).days
                if day_diff >= 0:
                    status_badge = "<span class='badge badge-ok'>Verifiziert (30 Tage)</span>"
                    status_text = "Verifiziert (30 Tage)"
                    countdown_label = f"{day_diff} Tage"
                    countdown_classes.append("countdown-ok")
                    meta_parts.append(f"Bis {until_dt.date().isoformat()}")
                else:
                    status_badge = "<span class='badge badge-warn'>Verifizierung überfällig</span>"
                    status_text = "Verifizierung überfällig"
                    countdown_label = f"Überfällig {abs(day_diff)} Tage"
                    countdown_classes.append("countdown-warn")
                    meta_parts.append(f"Abgelaufen am {until_dt.date().isoformat()}")

            if verified_at_dt:
                meta_parts.append(f"Bestätigt am {verified_at_dt.date().isoformat()}")

            if last_deadlock_dt:
                days_since = inactive_days or 0
                meta_parts.append(
                    f"Letzter Deadlock-Stream: {last_deadlock_dt.date().isoformat()} ({days_since} Tage her)"
                )
            else:
                meta_parts.append("Noch kein Deadlock-Stream erfasst")

            meta_html = (
                f"<div class='status-meta'>{' • '.join(meta_parts)}</div>" if meta_parts else ""
            )

            countdown_html = html.escape(countdown_label)
            if countdown_classes:
                countdown_html = (
                    f"<span class='{' '.join(countdown_classes)}'>{countdown_html}</span>"
                )

            missing_discord_id = not discord_user_id
            discord_warning = ""

            if missing_discord_id and (is_on_discord or has_discord_data):
                discord_icon = "⚠️"
                discord_label = "Discord nicht verknüpft"
                discord_warning = "Discord-ID fehlt – bitte verknüpfen."
            elif is_on_discord:
                discord_icon = "✅"
                discord_label = "Auf Discord"
            elif has_discord_data:
                discord_icon = "🟡"
                discord_label = "Discord-Daten vorhanden"
            else:
                discord_icon = "❌"
                discord_label = "Nicht verknüpft"

            discord_html_parts = [
                "<div class='discord-status'>",
                f"  <div class='discord-icon'>{discord_icon} {html.escape(discord_label)}</div>",
            ]
            if discord_warning:
                discord_html_parts.append(
                    f"  <div class='discord-warning'>{html.escape(discord_warning)}</div>"
                )
            discord_html_parts.append("</div>")
            discord_html = "".join(discord_html_parts)

            escaped_login = html.escape(login, quote=True)
            escaped_user_id = html.escape(discord_user_id, quote=True)
            escaped_display = html.escape(discord_display_name, quote=True)
            member_checked = " checked" if is_on_discord else ""
            toggle_mode = "mark" if not is_on_discord else "unmark"
            toggle_label = (
                "Als Discord-Mitglied markieren"
                if not is_on_discord
                else "Discord-Markierung entfernen"
            )
            toggle_classes = "btn btn-small" if not is_on_discord else "btn btn-small btn-secondary"

            archived_at_label = (
                archived_dt.date().isoformat() if archived_dt else (archived_at_raw or "—")
            )
            last_stream_label = last_deadlock_dt.date().isoformat() if last_deadlock_dt else "—"

            if is_archived:
                raid_status = (
                    "OAuth fehlt"
                    if not raid_is_authorized
                    else ("Auto-Raid aktiv" if raid_auto_active else "Autorisiert")
                )
                archived_entries.append(
                    {
                        "login": login,
                        "status": status_text,
                        "status_badge": status_badge,
                        "archived_at": archived_at_label,
                        "last_stream": last_stream_label,
                        "inactive_days": inactive_days,
                        "raid_status": raid_status,
                        "meta": list(meta_parts),
                        "escaped_login": escaped_login,
                    }
                )
                continue

            should_list_as_non_partner = partner_opt_out
            if should_list_as_non_partner:
                non_partner_entries.append(
                    {
                        "login": login,
                        "status": status_text,
                        "status_badge": status_badge,
                        "countdown": countdown_label,
                        "meta": list(meta_parts),
                        "discord_label": discord_label,
                        "discord_display_name": discord_display_name,
                        "discord_user_id": discord_user_id,
                        "warning": discord_warning,
                        "is_on_discord": is_on_discord,
                        "escaped_login": escaped_login,
                        "escaped_user_id": escaped_user_id,
                        "escaped_display": escaped_display,
                        "member_checked": member_checked,
                        "toggle_mode": toggle_mode,
                        "toggle_label": toggle_label,
                        "toggle_classes": toggle_classes,
                    }
                )
                continue

            discord_preview_rows: list[str] = []
            if discord_display_name:
                discord_preview_rows.append(
                    f"<span class='preview-label'>Name</span><span>{html.escape(discord_display_name)}</span>"
                )
            if discord_user_id:
                discord_preview_rows.append(
                    f"<span class='preview-label'>ID</span><span>{html.escape(discord_user_id)}</span>"
                )
            if not discord_preview_rows:
                discord_preview_rows.append(
                    "<span class='preview-empty'>Keine zusätzlichen Discord-Angaben hinterlegt.</span>"
                )

            discord_preview_html = "".join(
                f"<div class='discord-preview-row'>{row}</div>" for row in discord_preview_rows
            )

            advanced_html = (
                "  <details class='advanced-details'>"
                "    <summary>Discord verwalten</summary>"
                "    <div class='advanced-content'>"
                f"      <div class='discord-preview'>{discord_preview_html}</div>"
                "      <form method='post' action='/twitch/discord_link'>"
                f"        {csrf_input_html}"
                f"        <input type='hidden' name='login' value='{escaped_login}' />"
                "        <div class='form-row'>"
                f"          <label>Discord User ID<input type='text' name='discord_user_id' value='{escaped_user_id}' placeholder='123456789012345678'></label>"
                f"          <label>Discord Anzeigename<input type='text' name='discord_display_name' value='{escaped_display}' placeholder='Discord-Name'></label>"
                "        </div>"
                "        <div class='checkbox-label'>"
                f"          <input type='checkbox' name='member_flag' value='1'{member_checked}>"
                "          <span>Auch als Discord-Mitglied markieren</span>"
                "        </div>"
                "        <div class='hint'>Discord-Mitglieder erhalten höhere Priorität beim Posten.</div>"
                "        <div class='action-stack'>"
                "          <button class='btn btn-small'>Speichern</button>"
                "          <a class='btn btn-small btn-secondary' href='/twitch?discord=linked'>Nur verknüpfte anzeigen</a>"
                "        </div>"
                "      </form>"
                "    </div>"
                "  </details>"
            )

            raid_cell_parts: list[str] = []
            if raid_bot_available:
                if raid_is_authorized:
                    badge_class = "badge-ok" if raid_auto_active else "badge-neutral"
                    badge_label = "Bereit" if raid_auto_active else "Autorisiert"
                    raid_cell_parts.append(
                        f"<span class='badge {badge_class}'>{badge_label}</span>"
                    )
                    if raid_auto_active:
                        raid_cell_parts.append("<div class='status-meta'>Auto-Raid aktiv</div>")
                    else:
                        raid_cell_parts.append("<div class='status-meta'>Auto-Raid aus</div>")
                else:
                    raid_cell_parts.append(
                        "<span class='badge badge-warn'>Nicht autorisiert</span>"
                    )
                    raid_cell_parts.append("<div class='status-meta'>OAuth fehlt</div>")
                    raid_link = html.escape(raid_auth_link(login), quote=True)
                    requirements_link = html.escape(
                        f"/twitch/raid/requirements?login={escaped_login}",
                        quote=True,
                    )
                    raid_cell_parts.append(
                        "<a class='btn btn-small btn-secondary' href='"
                        + raid_link
                        + "'>Autorisieren</a>"
                    )
                    raid_cell_parts.append(
                        "<a class='btn btn-small' href='"
                        + requirements_link
                        + "' data-same-tab='1'>Anforderungen senden</a>"
                    )
            else:
                raid_cell_parts.append("<span class='badge badge-warn'>Bot offline</span>")
                raid_cell_parts.append("<div class='status-meta'>Raids nicht verfügbar</div>")

            raid_cell_html = "<div class='raid-cell'>" + "".join(raid_cell_parts) + "</div>"

            rows.append(
                "<tr>"
                f"  <td>{login_html}</td>"
                f"  <td>{discord_html}{advanced_html}</td>"
                f"  <td>{status_badge}{meta_html}</td>"
                f"  <td>{countdown_html}</td>"
                f"  <td>{raid_cell_html}</td>"
                "  <td>"
                "    <div class='action-stack'>"
                "      <form method='post' action='/twitch/verify' class='inline'>"
                f"        {csrf_input_html}"
                f"        <input type='hidden' name='login' value='{escaped_login}'>"
                "        <select name='mode'>"
                "          <option value='permanent'>Permanent</option>"
                "          <option value='temp'>30 Tage</option>"
                "          <option value='failed'>Verifizierung fehlgeschlagen</option>"
                "          <option value='clear'>Kein Partner</option>"
                "        </select>"
                "        <button class='btn btn-small'>Anwenden</button>"
                "      </form>"
                "      <form method='post' action='/twitch/discord_flag' class='inline'>"
                f"        {csrf_input_html}"
                f"        <input type='hidden' name='login' value='{escaped_login}'>"
                f"        <input type='hidden' name='mode' value='{toggle_mode}'>"
                f"        <button class='{toggle_classes}'>{html.escape(toggle_label)}</button>"
                "      </form>"
                "      <form method='post' action='/twitch/archive' class='inline'>"
                f"        {csrf_input_html}"
                f"        <input type='hidden' name='login' value='{escaped_login}'>"
                "        <input type='hidden' name='mode' value='archive'>"
                "        <button class='btn btn-small btn-secondary'>Archivieren</button>"
                "      </form>"
                "      <form method='post' action='/twitch/remove' class='inline'>"
                f"        {csrf_input_html}"
                f"        <input type='hidden' name='login' value='{escaped_login}'>"
                "        <button class='btn btn-small btn-danger'>Streamer entfernen</button>"
                "      </form>"
                "    </div>"
                "  </td>"
                "</tr>"
            )
            filtered_count += 1

        if not rows:
            rows.append("<tr><td colspan=6><i>Keine Streamer gefunden.</i></td></tr>")

        table_rows = "".join(rows)

        filter_options = [
            ("any", "Alle"),
            ("yes", "Nur Discord-Mitglieder"),
            ("no", "Nicht auf Discord"),
            ("linked", "Discord-Daten vorhanden"),
        ]

        filter_options_html = "".join(
            f"<option value='{html.escape(value, quote=True)}'{' selected' if discord_filter == value else ''}>{html.escape(label)}</option>"
            for value, label in filter_options
        )

        add_streamer_card_html = (
            "<div class='card add-streamer-card'>"
            "  <h2>Twitch Streamer hinzufügen</h2>"
            "  <form method='post' action='/twitch/add_streamer'>"
            f"    {csrf_input_html}"
            "    <div class='form-grid'>"
            "      <label>"
            "        Twitch Login oder URL"
            "        <input type='text' name='login' placeholder='earlysalty  |  https://twitch.tv/earlysalty' required>"
            "      </label>"
            "      <label>"
            "        Discord User ID"
            "        <input type='text' name='discord_user_id' placeholder='123456789012345678'>"
            "      </label>"
            "      <label>"
            "        Discord Anzeigename"
            "        <input type='text' name='discord_display_name' placeholder='Discord-Name'>"
            "      </label>"
            "    </div>"
            "    <div class='form-actions'>"
            "      <label class='checkbox-label'>"
            "        <input type='checkbox' name='member_flag' value='1'>"
            "        <span>Als Discord-Mitglied markieren</span>"
            "      </label>"
            "      <button class='btn'>Speichern</button>"
            "    </div>"
            "    <div class='hint'>"
            "      Akzeptiert: @login, login, twitch.tv/login, auch URL-encoded. Discord-Angaben sind optional, können aber direkt mitgespeichert werden."
            "    </div>"
            "    <div class='hint'>"
            "      Ohne Haken bleibt der Streamer ohne Partner-Markierung im Live-Panel, die Discord-Daten werden dennoch gespeichert."
            "    </div>"
            "  </form>"
            "</div>"
        )

        if non_partner_entries:
            non_partner_rows: list[str] = []
            for entry in non_partner_entries:
                countdown_badge = ""
                countdown_label = entry.get("countdown") or ""
                if countdown_label and countdown_label != "—":
                    countdown_badge = (
                        f"<span class='badge badge-neutral'>{html.escape(countdown_label)}</span>"
                    )

                discord_details: list[str] = []
                if entry.get("discord_label"):
                    discord_details.append(entry["discord_label"])
                if entry.get("discord_display_name"):
                    discord_details.append(entry["discord_display_name"])
                if entry.get("discord_user_id"):
                    discord_details.append(f"ID: {entry['discord_user_id']}")

                discord_line = ""
                if discord_details:
                    discord_line = (
                        "    <span><span class='meta-label'>Discord</span><span>"
                        + " • ".join(html.escape(part) for part in discord_details)
                        + "</span></span>"
                    )

                info_lines = "".join(
                    f"    <span><span class='meta-label'>Info</span><span>{html.escape(meta)}</span></span>"
                    for meta in entry.get("meta") or []
                )

                warning_line = ""
                if entry.get("warning"):
                    warning_line = f"    <span class='non-partner-warning'>{html.escape(entry['warning'])}</span>"

                preview_rows: list[str] = []
                if entry.get("discord_display_name"):
                    preview_rows.append(
                        f"<span class='preview-label'>Name</span><span>{html.escape(entry['discord_display_name'])}</span>"
                    )
                if entry.get("discord_user_id"):
                    preview_rows.append(
                        f"<span class='preview-label'>ID</span><span>{html.escape(entry['discord_user_id'])}</span>"
                    )
                if not preview_rows:
                    preview_rows.append(
                        "<span class='preview-empty'>Keine zusätzlichen Discord-Angaben hinterlegt.</span>"
                    )
                preview_html = "".join(
                    f"<div class='discord-preview-row'>{row}</div>" for row in preview_rows
                )

                non_partner_rows.append(
                    "<li class='non-partner-item'>"
                    "  <div class='non-partner-header'>"
                    f"    <strong>{html.escape(entry['login'])}</strong>"
                    "    <div class='non-partner-badges'>"
                    f"      {entry.get('status_badge', '')}"
                    f"      {countdown_badge}"
                    "    </div>"
                    "  </div>"
                    "  <div class='non-partner-meta'>"
                    f"    <span><span class='meta-label'>Status</span><span>{html.escape(entry['status'])}</span></span>"
                    f"{discord_line}"
                    f"{info_lines}"
                    f"{warning_line}"
                    "  </div>"
                    "  <details class='non-partner-manage'>"
                    "    <summary>Verwaltung</summary>"
                    "    <div class='manage-body'>"
                    f"      <div class='discord-preview'>{preview_html}</div>"
                    "      <form method='post' action='/twitch/verify' class='inline'>"
                    f"        {csrf_input_html}"
                    f"        <input type='hidden' name='login' value='{entry['escaped_login']}'>"
                    "        <select name='mode'>"
                    "          <option value='permanent'>Permanent</option>"
                    "          <option value='temp'>30 Tage</option>"
                    "          <option value='failed'>Verifizierung fehlgeschlagen</option>"
                    "          <option value='clear'>Kein Partner</option>"
                    "        </select>"
                    "        <button class='btn btn-small'>Anwenden</button>"
                    "      </form>"
                    "      <form method='post' action='/twitch/discord_link'>"
                    f"        {csrf_input_html}"
                    f"        <input type='hidden' name='login' value='{entry['escaped_login']}' />"
                    "        <div class='form-row'>"
                    f"          <label>Discord User ID<input type='text' name='discord_user_id' value='{entry['escaped_user_id']}' placeholder='123456789012345678'></label>"
                    f"          <label>Discord Anzeigename<input type='text' name='discord_display_name' value='{entry['escaped_display']}' placeholder='Discord-Name'></label>"
                    "        </div>"
                    "        <div class='checkbox-label'>"
                    f"          <input type='checkbox' name='member_flag' value='1'{entry['member_checked']}>"
                    "          <span>Als Discord-Mitglied markieren</span>"
                    "        </div>"
                    "        <div class='hint'>Speichern aktualisiert die Discord-Angaben.</div>"
                    "        <div class='non-partner-actions'>"
                    "          <button class='btn btn-small'>Speichern</button>"
                    "          <a class='btn btn-small btn-secondary' href='/twitch?discord=linked'>Nur verknüpfte anzeigen</a>"
                    "        </div>"
                    "      </form>"
                    "      <div class='non-partner-actions'>"
                    "        <form method='post' action='/twitch/discord_flag' class='inline'>"
                    f"          {csrf_input_html}"
                    f"          <input type='hidden' name='login' value='{entry['escaped_login']}'>"
                    f"          <input type='hidden' name='mode' value='{entry['toggle_mode']}'>"
                    f"          <button class='{entry['toggle_classes']}'>{html.escape(entry['toggle_label'])}</button>"
                    "        </form>"
                    "        <form method='post' action='/twitch/remove' class='inline'>"
                    f"          {csrf_input_html}"
                    f"          <input type='hidden' name='login' value='{entry['escaped_login']}'>"
                    "          <button class='btn btn-small btn-danger'>Streamer entfernen</button>"
                    "        </form>"
                    "      </div>"
                    "      <p class='non-partner-note'>Aktionen verschieben den Streamer bei Bedarf zurück in die Hauptliste.</p>"
                    "    </div>"
                    "  </details>"
                    "</li>"
                )
            non_partner_list_html = "".join(non_partner_rows)
        else:
            non_partner_list_html = "<li class='non-partner-item'><span class='non-partner-meta'>Keine zusätzlichen Streamer ohne Partner-Status vorhanden.</span></li>"

        if archived_entries:
            archived_rows: list[str] = []
            for entry in archived_entries:
                inactive_badge = ""
                if entry.get("inactive_days") is not None:
                    inactive_badge = f"<span class='badge badge-neutral'>{entry['inactive_days']} Tage inaktiv</span>"
                meta_line = ""
                if entry.get("meta"):
                    meta_line = (
                        "    <span><span class='meta-label'>Info</span><span>"
                        + " • ".join(html.escape(m) for m in entry["meta"])
                        + "</span></span>"
                    )
                raid_line = ""
                if entry.get("raid_status"):
                    raid_line = (
                        "    <span><span class='meta-label'>Raid</span><span>"
                        + html.escape(entry["raid_status"])
                        + "</span></span>"
                    )

                archived_rows.append(
                    "<li class='non-partner-item archived-item'>"
                    "  <div class='non-partner-header'>"
                    f"    <strong>{html.escape(entry['login'])}</strong>"
                    "    <div class='non-partner-badges'>"
                    f"      {entry.get('status_badge', '')}"
                    f"      {inactive_badge}"
                    "    </div>"
                    "  </div>"
                    "  <div class='non-partner-meta'>"
                    f"    <span><span class='meta-label'>Archiviert</span><span>{html.escape(entry.get('archived_at') or '—')}</span></span>"
                    f"    <span><span class='meta-label'>Letzter Stream</span><span>{html.escape(entry.get('last_stream') or '—')}</span></span>"
                    f"{raid_line}"
                    f"{meta_line}"
                    "  </div>"
                    "  <div class='non-partner-actions'>"
                    "    <form method='post' action='/twitch/archive' class='inline'>"
                    f"      {csrf_input_html}"
                    f"      <input type='hidden' name='login' value='{entry['escaped_login']}'>"
                    "      <input type='hidden' name='mode' value='unarchive'>"
                    "      <button class='btn btn-small'>Reaktivieren</button>"
                    "    </form>"
                    "    <form method='post' action='/twitch/remove' class='inline'>"
                    f"      {csrf_input_html}"
                    f"      <input type='hidden' name='login' value='{entry['escaped_login']}'>"
                    "      <button class='btn btn-small btn-danger'>Streamer entfernen</button>"
                    "    </form>"
                    "  </div>"
                    "</li>"
                )
            archived_list_html = "".join(archived_rows)
        else:
            archived_list_html = "<li class='non-partner-item archived-item'><span class='non-partner-meta'>Keine archivierten Streamer.</span></li>"

        non_partner_card_html = (
            "<div class='card non-partner-card'>"
            "  <h2>Keine Partner</h2>"
            "  <p>Streamer, die ausdrücklich als „Kein Partner“ markiert wurden. Sie tauchen nicht in der Hauptliste auf, können aber hier samt Discord-Verknüpfung weiterverwaltet werden.</p>"
            f"  <ul class='non-partner-list'>{non_partner_list_html}</ul>"
            "</div>"
        )

        archived_card_html = (
            "<div class='card non-partner-card archived-card'>"
            "  <h2>Archivierte Streamer</h2>"
            "  <p>Automatisch nach 10+ Tagen Inaktivität. Bei neuem Stream werden sie automatisch reaktiviert.</p>"
            f"  <ul class='non-partner-list'>{archived_list_html}</ul>"
            "</div>"
        )

        filter_card_html = (
            '<div class="card filter-card">'
            '  <form method="get" action="/twitch" class="row filter-row">'
            '    <label class="filter-label">Discord Status'
            f'      <select name="discord">{filter_options_html}</select>'
            "    </label>"
            '    <button class="btn btn-small btn-secondary">Filter anwenden</button>'
            '    <a class="btn btn-small btn-secondary" href="/twitch">Zurücksetzen</a>'
            "  </form>"
            f'  <div class="status-meta">Treffer: {filtered_count} / {total_count}</div>'
            "</div>"
        )

        unique_logins = sorted(
            {
                (st.get("twitch_login") or "").strip()
                for st in items
                if (st.get("twitch_login") or "").strip()
            }
        )
        login_options_html = "".join(
            f"<option value='{html.escape(login, quote=True)}'></option>" for login in unique_logins
        )
        missing_unique = sorted({login for login in raid_missing_logins if login})
        missing_preview = missing_unique[:6]
        missing_count = max(0, total_count - raid_authorized_count)
        if missing_preview:
            missing_chips = "".join(
                f"<span class='chip'>{html.escape(login)}</span>" for login in missing_preview
            )
            raid_missing_html = (
                "<div class='raid-meta'>"
                f"<span class='pill warn'>Fehlende Autorisierung: {missing_count}</span>"
                f"{missing_chips}"
                "</div>"
            )
        else:
            raid_missing_html = (
                "<div class='raid-meta'><span class='pill ok'>Alle Partner autorisiert</span></div>"
            )

        raid_bot_state_label = "Verfügbar" if raid_bot_available else "Nicht aktiv"
        raid_bot_state_class = "ok" if raid_bot_available else "warn"
        redirect_label = html.escape(self._redirect_uri or "—")
        raid_form_disabled_attr = " disabled" if not raid_bot_available else ""
        raid_form_note = ""
        if not raid_bot_available:
            raid_form_note = "<div class='status-meta'>Raid Bot ist nicht aktiv. Autorisierung ist derzeit nicht möglich.</div>"

        raid_auth_card_html = (
            "<div class='card raid-auth-card'>"
            "  <div class='card-header'>"
            "    <div>"
            "      <p class='eyebrow'>Raid Bot</p>"
            "      <h2>Twitch Bot Autorisierung</h2>"
            "      <p class='lead'>Einmal autorisieren: Auto-Raid, Chat-Schutz und Discord Auto-Post (mit Cooldowns).</p>"
            "    </div>"
            "    <div class='raid-metrics'>"
            f"      <div class='mini-stat'><strong>{raid_authorized_count}</strong><span>Autorisiert</span></div>"
            f"      <div class='mini-stat'><strong>{missing_count}</strong><span>Fehlt</span></div>"
            f"      <div class='mini-stat'><strong>{raid_ready_count}</strong><span>Auto-Raid aktiv</span></div>"
            "    </div>"
            "  </div>"
            "  <form class='raid-form' method='get' action='/twitch/raid/auth' target='_blank'>"
            "    <label>Streamer Login"
            f"      <input type='text' name='login' list='raid-login-list' placeholder='earlysalty' required{raid_form_disabled_attr}>"
            "    </label>"
            f"    <button class='btn'{raid_form_disabled_attr}>OAuth Link erzeugen</button>"
            "  </form>"
            f"  {raid_form_note}"
            f"  <datalist id='raid-login-list'>{login_options_html}</datalist>"
            "  <div class='raid-meta'>"
            f"    <span class='pill'>Redirect: {redirect_label}</span>"
            f"    <span class='pill {raid_bot_state_class}'>Raid Bot: {raid_bot_state_label}</span>"
            "    <span class='pill'>Aktivieren im Chat: !raid_enable</span>"
            "  </div>"
            f"  {raid_missing_html}"
            "</div>"
        )

        chat_action_disabled_attr = "" if can_use_owner_chat_actions else " disabled"
        chat_action_notice = (
            "<div class='status-meta'>Nur der freigeschaltete Discord-Owner kann diese Aktion nutzen.</div>"
            if not can_use_owner_chat_actions
            else "<div class='status-meta'>Nur Partner-Streamer sind erlaubt (nicht archiviert, nicht als Kein Partner markiert).</div>"
        )
        chat_action_card_html = (
            "<div class='card raid-auth-card'>"
            "  <div class='card-header'>"
            "    <div>"
            "      <p class='eyebrow'>Owner Feature</p>"
            "      <h2>Partner Chat Aktion</h2>"
            "      <p class='lead'>Manuelle Nachricht, /me Action oder Announcement direkt aus dem Admin-Dashboard.</p>"
            "    </div>"
            "  </div>"
            "  <form class='raid-form' method='post' action='/twitch/admin/chat_action'>"
            f"    {csrf_input_html}"
            "    <label>Partner Login"
            f"      <input type='text' name='login' list='chat-action-login-list' placeholder='earlysalty' required{chat_action_disabled_attr}>"
            "    </label>"
            "    <label>Modus"
            f"      <select name='mode'{chat_action_disabled_attr}>"
            "        <option value='message'>Nachricht</option>"
            "        <option value='action'>/me Action</option>"
            "        <option value='announcement'>Announcement</option>"
            "      </select>"
            "    </label>"
            "    <label>Announcement Farbe"
            f"      <select name='color'{chat_action_disabled_attr}>"
            "        <option value='purple'>Purple</option>"
            "        <option value='blue'>Blue</option>"
            "        <option value='green'>Green</option>"
            "        <option value='orange'>Orange</option>"
            "        <option value='primary'>Primary</option>"
            "      </select>"
            "    </label>"
            "    <label>Text"
            f"      <textarea name='message' rows='3' maxlength='450' placeholder='Nachricht für den Twitch-Chat' required{chat_action_disabled_attr}></textarea>"
            "    </label>"
            f"    <button class='btn'{chat_action_disabled_attr}>Senden</button>"
            "  </form>"
            f"  <datalist id='chat-action-login-list'>{login_options_html}</datalist>"
            f"  {chat_action_notice}"
            "</div>"
        )

        # --- Scope Status Card ---
        scope_rows: list[str] = []
        scope_headers_html = "".join(
            (
                f"<th class='scope-header' title='{html.escape(scope, quote=True)}'>"
                f"{html.escape(_SCOPE_COLUMN_LABELS.get(scope, scope))}"
                "</th>"
            )
            for scope in _REQUIRED_SCOPES
        )
        scope_table_colspan = 2 + len(_REQUIRED_SCOPES)
        total_authorized = 0
        full_scope_count = 0
        try:
            with _storage.get_conn() as _sc:
                auth_rows = _sc.execute(
                    "SELECT twitch_login, scopes, needs_reauth FROM twitch_raid_auth ORDER BY twitch_login"
                ).fetchall()
            for auth_row in auth_rows:
                total_authorized += 1
                _login = str(
                    auth_row[0] if not hasattr(auth_row, "keys") else auth_row["twitch_login"]
                )
                _scopes_raw = str(
                    auth_row[1] if not hasattr(auth_row, "keys") else auth_row["scopes"] or ""
                )
                _needs_reauth = bool(
                    auth_row[2] if not hasattr(auth_row, "keys") else auth_row["needs_reauth"]
                )
                _token_scopes = set(_scopes_raw.split()) if _scopes_raw else set()
                _missing = [s for s in _REQUIRED_SCOPES if s not in _token_scopes]
                _missing_critical = [s for s in _missing if s in _CRITICAL_SCOPES]

                if _needs_reauth:
                    row_class = "scope-row scope-reauth"
                    status_pill = "<span class='pill err'>Re-Auth nötig</span>"
                elif _missing_critical:
                    row_class = "scope-row scope-critical"
                    status_pill = "<span class='pill warn'>Kritisch unvollständig</span>"
                elif _missing:
                    row_class = "scope-row scope-partial"
                    status_pill = "<span class='pill neutral'>Unvollständig</span>"
                else:
                    row_class = "scope-row scope-full"
                    status_pill = "<span class='pill ok'>Vollständig</span>"
                    full_scope_count += 1

                scope_cells_html = "".join(
                    (
                        f"<td class='scope-check {'yes' if scope in _token_scopes else 'no'}' "
                        f"title='{html.escape(scope, quote=True)}'>"
                        f"{'☑' if scope in _token_scopes else 'X'}"
                        "</td>"
                    )
                    for scope in _REQUIRED_SCOPES
                )

                scope_rows.append(
                    f"<tr class='{row_class}'>"
                    f"  <td><strong>{html.escape(_login)}</strong></td>"
                    f"  <td>{status_pill}</td>"
                    f"  {scope_cells_html}"
                    f"</tr>"
                )
        except Exception:
            log.debug("Scope-Status Card: DB-Fehler", exc_info=True)
            scope_rows = [
                f"<tr><td colspan='{scope_table_colspan}'>Fehler beim Laden der Scope-Daten</td></tr>"
            ]

        missing_scope_count = total_authorized - full_scope_count
        scope_summary_pills = (
            f"<span class='pill ok'>{full_scope_count} vollständig</span>"
            f"<span class='pill warn'>{missing_scope_count} unvollständig</span>"
        )
        bot_scope_state = self._resolve_dashboard_bot_scope_state()
        bot_scope_pills_html = ""
        if not bool(bot_scope_state.get("available")):
            bot_scope_pills_html = (
                "<span class='pill err'>Zentraler Bot-TokenManager fehlt</span>"
            )
        elif not bool(bot_scope_state.get("loaded")):
            bot_scope_pills_html = (
                "<span class='pill neutral'>Zentrale Bot-Scopes noch nicht geladen</span>"
            )
        else:
            bot_scope_set = {
                str(scope).strip().lower()
                for scope in (bot_scope_state.get("scopes") or set())
                if str(scope).strip()
            }
            bot_scope_pills_html = "".join(
                (
                    f"<span class='pill {'ok' if scope in bot_scope_set else 'warn'}'>"
                    f"{html.escape(_BOT_SCOPE_LABELS.get(scope, scope))}"
                    "</span>"
                )
                for scope in _BOT_REQUIRED_SCOPES
            )
        bot_optional_scope_pills_html = ""
        if bool(bot_scope_state.get("loaded")):
            bot_scope_set = {
                str(scope).strip().lower()
                for scope in (bot_scope_state.get("scopes") or set())
                if str(scope).strip()
            }
            bot_optional_scope_pills_html = "".join(
                (
                    f"<span class='pill {'ok' if scope in bot_scope_set else 'neutral'}'>"
                    f"{html.escape(_BOT_SCOPE_LABELS.get(scope, scope))}"
                    "</span>"
                )
                for scope in _BOT_OPTIONAL_SCOPES
            )
        scope_empty_row_html = (
            f"<tr><td colspan='{scope_table_colspan}'>Kein Streamer mit OAuth autorisiert</td></tr>"
        )
        scope_body_html = "".join(scope_rows) if scope_rows else scope_empty_row_html

        scope_card_html = (
            "<div class='card scope-card'>"
            "  <div class='card-header'>"
            "    <div>"
            "      <p class='eyebrow'>OAuth Token Scopes</p>"
            "      <h2>Scope-Status pro Streamer</h2>"
            "      <p class='lead'>Die Tabelle zeigt nur Broadcaster-/Streamer-Scopes wie <code>channel:bot</code> oder <code>channel:manage:raids</code>. Bot-User-Scopes wie <code>user:read:chat</code> werden darunter getrennt als globaler Bot-Zustand angezeigt.</p>"
            "    </div>"
            "    <div class='raid-metrics'>"
            f"      <div class='mini-stat'><strong>{total_authorized}</strong><span>Mit OAuth</span></div>"
            f"      <div class='mini-stat'><strong>{full_scope_count}</strong><span>Vollständig</span></div>"
            f"      <div class='mini-stat'><strong>{missing_scope_count}</strong><span>Unvollständig</span></div>"
            "    </div>"
            "  </div>"
            "  <div class='status-meta' style='margin-bottom:.8rem;'>"
            f"    {scope_summary_pills}"
            "    <span class='pill neutral'>Re-Auth Link → Raid Bot Autorisierung</span>"
            "  </div>"
            "  <div class='status-meta' style='margin-bottom:.8rem;'>"
            "    <span class='pill neutral'>Zentraler Bot</span>"
            f"    {bot_scope_pills_html}"
            "  </div>"
            "  <div class='status-meta' style='margin-bottom:.8rem;'>"
            "    <span class='pill neutral'>Optional / Zukunft</span>"
            f"    {bot_optional_scope_pills_html or '<span class=\"pill neutral\">user:bot nur für App-Token-Chat-EventSub nötig</span>'}"
            "  </div>"
            "  <div class='table-wrap'>"
            "  <table>"
            "    <thead><tr>"
            "      <th>Streamer</th>"
            "      <th>Status</th>"
            f"      {scope_headers_html}"
            "    </tr></thead>"
            f"    <tbody>{scope_body_html}</tbody>"
            "  </table>"
            "  </div>"
            "</div>"
        )

        raid_arrival_card_html = ""
        try:
            with _storage.get_conn() as _sc:
                arrival_rows = _sc.execute(
                    """
                    SELECT detected_at,
                           from_broadcaster_login,
                           to_broadcaster_login,
                           viewer_count,
                           classification,
                           confirmation_signals,
                           correlation_status
                    FROM twitch_raid_arrival_tracking
                    ORDER BY detected_at DESC
                    LIMIT 12
                    """
                ).fetchall()
        except Exception:
            arrival_rows = []

        if arrival_rows:
            arrival_table_rows = []
            for row in arrival_rows:
                detected_at = str(
                    row[0] if not hasattr(row, "keys") else row["detected_at"] or ""
                )[:19]
                from_login = str(
                    row[1] if not hasattr(row, "keys") else row["from_broadcaster_login"] or ""
                )
                to_login = str(
                    row[2] if not hasattr(row, "keys") else row["to_broadcaster_login"] or ""
                )
                viewer_count = str(row[3] if not hasattr(row, "keys") else row["viewer_count"] or 0)
                classification = str(
                    row[4] if not hasattr(row, "keys") else row["classification"] or ""
                )
                confirmation_signals = str(
                    row[5] if not hasattr(row, "keys") else row["confirmation_signals"] or ""
                ).replace(",", " + ")
                correlation_status = str(
                    row[6] if not hasattr(row, "keys") else row["correlation_status"] or ""
                )
                arrival_table_rows.append(
                    "<tr>"
                    f"<td>{html.escape(detected_at, quote=True)}</td>"
                    f"<td><strong>{html.escape(from_login, quote=True)}</strong></td>"
                    f"<td><strong>{html.escape(to_login, quote=True)}</strong></td>"
                    f"<td>{html.escape(viewer_count, quote=True)}</td>"
                    f"<td>{html.escape(classification, quote=True)}</td>"
                    f"<td>{html.escape(confirmation_signals, quote=True)}</td>"
                    f"<td>{html.escape(correlation_status, quote=True)}</td>"
                    "</tr>"
                )

            raid_arrival_card_html = (
                "<div class='card scope-card' style='margin-top:1.2rem;'>"
                "  <div class='card-header'>"
                "    <div>"
                "      <p class='eyebrow'>Partner Raid Arrivals</p>"
                "      <h2>Bestätigungssignale und Klassifizierung</h2>"
                "      <p class='lead'>Zeigt zuletzt erkannte Partner-Raid-Ankünfte mit Bestätigungspfad (<code>channel.raid</code> und/oder <code>channel.chat.notification</code>) sowie der fachlichen Klassifizierung.</p>"
                "    </div>"
                "  </div>"
                "  <div class='table-wrap'>"
                "    <table>"
                "      <thead><tr><th>Zeitpunkt</th><th>Von</th><th>Nach</th><th>Viewer</th><th>Klassifizierung</th><th>Signale</th><th>Korrelation</th></tr></thead>"
                f"      <tbody>{''.join(arrival_table_rows)}</tbody>"
                "    </table>"
                "  </div>"
                "</div>"
            )
        else:
            raid_arrival_card_html = (
                "<div class='card scope-card' style='margin-top:1.2rem;'>"
                "  <div class='card-header'>"
                "    <div>"
                "      <p class='eyebrow'>Partner Raid Arrivals</p>"
                "      <h2>Bestätigungssignale und Klassifizierung</h2>"
                "      <p class='lead'>Noch keine gespeicherten Partner-Raid-Ankünfte mit Zielseiten-Bestätigung.</p>"
                "    </div>"
                "  </div>"
                "</div>"
            )

        raid_history_link = ""
        if raid_bot_available:
            history_href = "/twitch/raid/history"
            raid_history_link = (
                "<a class='btn btn-secondary' href='"
                + html.escape(history_href, quote=True)
                + "' target='_blank' rel='noopener'>Raid History</a>"
            )

        hero_actions = (
            (raid_history_link + " " if raid_history_link else "")
            + "<form method='post' action='/twitch/reload'>"
            + csrf_input_html
            + "<button class='btn btn-warn'>Reload Twitch Cog</button>"
            + "</form>"
        )

        hero_html = (
            "<header class='hero'>"
            "  <div>"
            "    <p class='eyebrow'>Twitch Admin</p>"
            "    <h1>Deadlock Twitch Ops</h1>"
            "    <p class='lead'>Live-Partner verwalten, Discord-Verknüpfungen prüfen und Raid-Bot autorisieren.</p>"
            "  </div>"
            f"  <div class='hero-actions'>{hero_actions}</div>"
            "</header>"
        )

        table_html = f"""
<div class="card table-card" style="margin-top: 1.4rem;">
  <div class="table-wrap">
    <table>
      <thead>
        <tr><th>Login</th><th>Discord</th><th>Verifizierung</th><th>Countdown</th><th>Raid Bot</th><th>Aktionen</th></tr>
      </thead>
      <tbody>
        {table_rows}
      </tbody>
    </table>
  </div>
</div>
"""
        plan_management_card_html = self._render_billing_admin_plan_card(
            csrf_input_html=csrf_input_html
        )

        body = f"""
{admin_section_nav_html}
{hero_html}

<div class="panel-grid">
  {add_streamer_card_html}
  {raid_auth_card_html}
  {chat_action_card_html}
  {admin_announcement_card_html}
  {filter_card_html}
</div>

{table_html}

{scope_card_html}
{raid_arrival_card_html}
{plan_management_card_html}

{archived_card_html}
{non_partner_card_html}
"""

        return web.Response(
            text=self._html(body, active="live", msg=msg, err=err),
            content_type="text/html",
        )

    async def admin_manual_plan_save(self, request: web.Request):
        self._require_token(request)
        data = await self._read_post_with_csrf(request, fallback_path="/twitch/admin")
        billing_setter = getattr(self, "_billing_admin_set_manual_plan", None)
        if not callable(billing_setter):
            location = self._redirect_location(
                request,
                err="Manuelle Planvergabe ist aktuell nicht verfügbar",
                default_path="/twitch/admin",
            )
            raise web.HTTPFound(location=location)

        login = str(data.get("login") or "").strip()
        plan_id = str(data.get("plan_id") or "").strip()
        expires_at = str(data.get("expires_at") or "").strip()
        notes = str(data.get("notes") or "").strip()

        try:
            result = billing_setter(
                twitch_login=login,
                plan_id=plan_id,
                expires_at=expires_at or None,
                notes=notes,
            )
            effective_plan_id = str(result.get("effective_plan_id") or plan_id or "raid_free").strip()
            location = self._redirect_location(
                request,
                ok=f"Manueller Plan für {login} gesetzt ({effective_plan_id})",
                default_path="/twitch/admin",
            )
        except ValueError as exc:
            error_map = {
                "twitch_login_required": "Bitte einen Twitch-Login angeben",
                "unknown_plan_id": "Unbekannte Plan-ID",
                "unknown_streamer": f"Streamer {login or '—'} nicht gefunden",
                "streamer_user_id_missing": (
                    f"Für {login or 'den Streamer'} fehlt die Twitch User-ID"
                ),
                "manual_plan_save_failed": "Manueller Plan konnte nicht gespeichert werden",
            }
            location = self._redirect_location(
                request,
                err=error_map.get(str(exc), str(exc)),
                default_path="/twitch/admin",
            )
        except Exception:
            safe_login = str(login or "").replace("\r", "\\r").replace("\n", "\\n")
            log.exception("dashboard manual plan save failed for %s", safe_login)
            location = self._redirect_location(
                request,
                err="Manueller Plan konnte nicht gespeichert werden",
                default_path="/twitch/admin",
            )
        raise web.HTTPFound(location=location)

    async def admin_manual_plan_clear(self, request: web.Request):
        self._require_token(request)
        data = await self._read_post_with_csrf(request, fallback_path="/twitch/admin")
        billing_clearer = getattr(self, "_billing_admin_clear_manual_plan", None)
        if not callable(billing_clearer):
            location = self._redirect_location(
                request,
                err="Plan-Override kann aktuell nicht entfernt werden",
                default_path="/twitch/admin",
            )
            raise web.HTTPFound(location=location)

        login = str(data.get("login") or "").strip()
        try:
            result = billing_clearer(twitch_login=login)
            effective_plan_id = str(result.get("effective_plan_id") or "raid_free").strip() or "raid_free"
            location = self._redirect_location(
                request,
                ok=f"Manueller Override für {login} entfernt ({effective_plan_id})",
                default_path="/twitch/admin",
            )
        except ValueError as exc:
            error_map = {
                "twitch_login_required": "Bitte einen Twitch-Login angeben",
                "unknown_streamer": f"Streamer {login or '—'} nicht gefunden",
                "streamer_user_id_missing": (
                    f"Für {login or 'den Streamer'} fehlt die Twitch User-ID"
                ),
                "manual_plan_clear_failed": "Manueller Override konnte nicht entfernt werden",
            }
            location = self._redirect_location(
                request,
                err=error_map.get(str(exc), str(exc)),
                default_path="/twitch/admin",
            )
        except Exception:
            safe_login = str(login or "").replace("\r", "\\r").replace("\n", "\\n")
            log.exception("dashboard manual plan clear failed for %s", safe_login)
            location = self._redirect_location(
                request,
                err="Manueller Override konnte nicht entfernt werden",
                default_path="/twitch/admin",
            )
        raise web.HTTPFound(location=location)

    async def add_any(self, request: web.Request):
        """Flexible Variante: nimmt POST-Felder `q`, `login` oder `url`."""
        self._require_token(request)
        data = await self._read_post_with_csrf(request)
        raw = str(data.get("q") or data.get("login") or data.get("url") or "").strip()
        try:
            msg = await self._do_add(raw)
            raise web.HTTPFound(location="/twitch?ok=" + quote_plus(msg))
        except web.HTTPException:
            raise
        except Exception as e:
            log.exception("dashboard add_any failed: %s", e)
            raise web.HTTPFound(location="/twitch?err=" + quote_plus("could not add (twitch api)"))

    async def add_url(self, request: web.Request):
        """Backward-compatible: nimmt POST-Feld `url` (kann auch Login enthalten)."""
        self._require_token(request)
        data = await self._read_post_with_csrf(request)
        raw = str(data.get("url") or "").strip()
        try:
            msg = await self._do_add(raw)
            raise web.HTTPFound(location="/twitch?ok=" + quote_plus(msg))
        except web.HTTPException:
            raise
        except Exception as e:
            log.exception("dashboard add_url failed: %s", e)
            raise web.HTTPFound(location="/twitch?err=" + quote_plus("could not add (twitch api)"))

    async def add_login(self, request: web.Request):
        """Pfad-Shortcut: /twitch/add_login/<login>"""
        self._require_token(request)
        data = await self._read_post_with_csrf(request)
        raw = str(request.match_info.get("login") or data.get("login") or "").strip()
        try:
            msg = await self._do_add(raw)
            raise web.HTTPFound(location="/twitch?ok=" + quote_plus(msg))
        except web.HTTPException:
            raise
        except Exception as e:
            log.exception("dashboard add_login failed: %s", e)
            raise web.HTTPFound(location="/twitch?err=" + quote_plus("could not add (twitch api)"))

    async def add_streamer(self, request: web.Request):
        self._require_token(request)
        data = await self._read_post_with_csrf(request)
        raw_login = (data.get("login") or "").strip()
        discord_user_id = (data.get("discord_user_id") or "").strip()
        discord_display_name = (data.get("discord_display_name") or "").strip()
        member_raw = (data.get("member_flag") or "").strip().lower()
        mark_member = member_raw in {"1", "true", "on", "yes"}

        if not raw_login:
            location = self._redirect_location(request, err="Bitte einen Twitch-Login angeben")
            raise web.HTTPFound(location=location)

        try:
            add_message = await self._do_add(raw_login)
        except web.HTTPBadRequest as exc:
            err_text = exc.text or "Ungültiger Twitch-Login"
            location = self._redirect_location(request, err=err_text)
            raise web.HTTPFound(location=location)
        except Exception as exc:
            log.exception("dashboard add_streamer failed: %s", exc)
            location = self._redirect_location(
                request, err="Twitch-Streamer konnte nicht hinzugefügt werden"
            )
            raise web.HTTPFound(location=location)

        profile_message = ""
        should_update_discord = bool(discord_user_id or discord_display_name or mark_member)
        if should_update_discord:
            try:
                profile_message = await self._discord_profile(
                    raw_login,
                    discord_user_id=discord_user_id or None,
                    discord_display_name=discord_display_name or None,
                    mark_member=mark_member,
                )
            except ValueError as exc:
                location = self._redirect_location(request, err=str(exc))
                raise web.HTTPFound(location=location)
            except Exception as exc:
                log.exception("dashboard add_streamer discord save failed: %s", exc)
                location = self._redirect_location(
                    request, err="Discord-Daten konnten nicht gespeichert werden"
                )
                raise web.HTTPFound(location=location)

        messages = [m for m in (add_message, profile_message) if m]
        ok_message = " – ".join(dict.fromkeys(messages)) if messages else "Gespeichert"
        location = self._redirect_location(request, ok=ok_message)
        raise web.HTTPFound(location=location)

    async def admin_partner_chat_action(self, request: web.Request):
        self._require_token(request)
        data = await self._read_post_with_csrf(request, fallback_path="/twitch/admin")

        discord_user_id = self._get_discord_admin_user_id(request)
        if discord_user_id != _DASHBOARD_OWNER_DISCORD_ID:
            safe_path = str(request.path or "").replace("\r", "\\r").replace("\n", "\\n")
            log.warning(
                "AUDIT dashboard chat action denied: discord_user_id=%s path=%s",
                "present" if discord_user_id else "none",
                safe_path,
            )
            location = self._redirect_location(
                request,
                err="Nur der freigeschaltete Discord-Owner darf diese Chat-Aktion nutzen",
                default_path="/twitch/admin",
            )
            raise web.HTTPFound(location=location)

        raw_login = str(data.get("login") or data.get("streamer") or "").strip()
        normalize_login = getattr(self, "_normalize_login", None)
        if callable(normalize_login):
            login = str(normalize_login(raw_login) or "").strip().lower()
        else:
            login = raw_login.strip().lstrip("@#").lower()
        if not login:
            location = self._redirect_location(
                request,
                err="Bitte einen Partner-Login angeben",
                default_path="/twitch/admin",
            )
            raise web.HTTPFound(location=location)

        mode = str(data.get("mode") or "message").strip().lower()
        if mode not in _CHAT_ACTION_MODES:
            mode = "message"
        color = str(data.get("color") or "purple").strip().lower()
        if color not in _CHAT_ANNOUNCEMENT_COLORS:
            color = "purple"

        message = str(data.get("message") or "").strip()
        if not message:
            location = self._redirect_location(
                request,
                err="Bitte eine Nachricht eingeben",
                default_path="/twitch/admin",
            )
            raise web.HTTPFound(location=location)
        if len(message) > 450:
            location = self._redirect_location(
                request,
                err="Nachricht ist zu lang (max. 450 Zeichen)",
                default_path="/twitch/admin",
            )
            raise web.HTTPFound(location=location)

        items = await self._list()
        target = next(
            (
                row
                for row in items
                if str(row.get("twitch_login") or "").strip().lower() == login
            ),
            None,
        )
        if target is None:
            location = self._redirect_location(
                request,
                err=f"Streamer {login} nicht gefunden",
                default_path="/twitch/admin",
            )
            raise web.HTTPFound(location=location)

        if bool(target.get("manual_partner_opt_out")) or bool(target.get("archived_at")):
            location = self._redirect_location(
                request,
                err="Chat-Aktion ist nur für aktive Partner-Streamer erlaubt",
                default_path="/twitch/admin",
            )
            raise web.HTTPFound(location=location)

        chat_bot = self._resolve_dashboard_chat_bot()
        if chat_bot is None:
            location = self._redirect_location(
                request,
                err="Twitch Chat Bot ist aktuell nicht verfügbar",
                default_path="/twitch/admin",
            )
            raise web.HTTPFound(location=location)

        channel_id = self._coerce_twitch_user_id(target.get("twitch_user_id"))
        if not channel_id:
            channel_id = self._resolve_streamer_user_id_from_db(login)
        if not channel_id:
            channel_id = await self._resolve_streamer_user_id_from_twitch(chat_bot, login)
        if channel_id:
            target["twitch_user_id"] = channel_id
            self._persist_streamer_user_id(login, channel_id)
        else:
            location = self._redirect_location(
                request,
                err=f"Für {login} fehlt die Twitch User-ID",
                default_path="/twitch/admin",
            )
            raise web.HTTPFound(location=location)

        channel = self._build_dashboard_chat_channel(chat_bot, login, channel_id)
        send_text = message if mode != "action" else f"/me {message}"

        ok = False
        try:
            if mode == "announcement":
                send_announcement = getattr(chat_bot, "_send_announcement", None)
                if callable(send_announcement):
                    ok = bool(
                        await send_announcement(
                            channel,
                            send_text,
                            color=color,
                            source="admin_dashboard_manual",
                        )
                    )
                else:
                    send_chat = getattr(chat_bot, "_send_chat_message", None)
                    if callable(send_chat):
                        ok = bool(
                            await send_chat(channel, send_text, source="admin_dashboard_manual")
                        )
            else:
                send_chat = getattr(chat_bot, "_send_chat_message", None)
                if callable(send_chat):
                    ok = bool(await send_chat(channel, send_text, source="admin_dashboard_manual"))
        except Exception:
            safe_login = str(login or "").replace("\r", "\\r").replace("\n", "\\n")
            log.exception("dashboard chat action failed for %s", safe_login)
            ok = False

        if not ok:
            location = self._redirect_location(
                request,
                err=f"Chat-Aktion für {login} konnte nicht gesendet werden",
                default_path="/twitch/admin",
            )
            raise web.HTTPFound(location=location)

        action_label = (
            "Announcement" if mode == "announcement" else ("Action" if mode == "action" else "Nachricht")
        )
        location = self._redirect_location(
            request,
            ok=f"{action_label} an {login} gesendet",
            default_path="/twitch/admin",
        )
        raise web.HTTPFound(location=location)

    async def discord_flag(self, request: web.Request):
        self._require_token(request)
        data = await self._read_post_with_csrf(request)
        login = (data.get("login") or "").strip()
        mode = (data.get("mode") or "").strip().lower()
        desired: bool | None
        if mode in {"mark", "on", "enable", "1"}:
            desired = True
        elif mode in {"unmark", "off", "disable", "0"}:
            desired = False
        else:
            desired = None

        try:
            if desired is None:
                raise ValueError("Ungültiger Modus für Discord-Markierung")
            message = await self._discord_flag(login, desired)
            location = self._redirect_location(request, ok=message)
        except ValueError as exc:
            location = self._redirect_location(request, err=str(exc))
        except Exception as exc:
            log.exception("dashboard discord_flag failed: %s", exc)
            location = self._redirect_location(
                request, err="Discord-Markierung konnte nicht aktualisiert werden"
            )
        raise web.HTTPFound(location=location)

    async def discord_link(self, request: web.Request):
        self._require_token(request)
        data = await self._read_post_with_csrf(request)
        login = (data.get("login") or "").strip()
        discord_user_id = (data.get("discord_user_id") or "").strip()
        discord_display_name = (data.get("discord_display_name") or "").strip()
        member_raw = (data.get("member_flag") or "").strip().lower()
        mark_member = member_raw in {"1", "true", "on", "yes"}

        try:
            message = await self._discord_profile(
                login,
                discord_user_id=discord_user_id or None,
                discord_display_name=discord_display_name or None,
                mark_member=mark_member,
            )
            location = self._redirect_location(request, ok=message)
        except ValueError as exc:
            location = self._redirect_location(request, err=str(exc))
        except Exception as exc:
            log.exception("dashboard discord_link failed: %s", exc)
            location = self._redirect_location(
                request, err="Discord-Daten konnten nicht gespeichert werden"
            )
        raise web.HTTPFound(location=location)

    async def remove(self, request: web.Request):
        self._require_token(request)
        data = await self._read_post_with_csrf(request)
        login = (data.get("login") or "").strip()
        try:
            msg = await self._remove(login)
            message = msg or f"{login} removed"
            location = self._redirect_location(request, ok=message)
            raise web.HTTPFound(location=location)
        except web.HTTPException:
            raise
        except Exception as e:
            log.exception("dashboard remove failed: %s", e)
            location = self._redirect_location(request, err="could not remove")
            raise web.HTTPFound(location=location)

    async def verify(self, request: web.Request):
        self._require_token(request)
        data = await self._read_post_with_csrf(request)
        login = (data.get("login") or "").strip()
        mode = (data.get("mode") or "").strip().lower()
        try:
            msg = await self._verify(login, mode)
            message = msg or f"verify {mode} for {login}"
            location = self._redirect_location(request, ok=message)
            raise web.HTTPFound(location=location)
        except web.HTTPException:
            raise
        except Exception as e:
            log.exception("dashboard verify failed: %s", e)
            location = self._redirect_location(request, err="Verifizierung fehlgeschlagen")
            raise web.HTTPFound(location=location)

    async def archive(self, request: web.Request):
        self._require_token(request)
        data = await self._read_post_with_csrf(request)
        login = (data.get("login") or "").strip()
        mode = (data.get("mode") or "").strip().lower() or "toggle"
        try:
            msg = await self._archive(login, mode)
            location = self._redirect_location(request, ok=msg)
        except ValueError as exc:
            location = self._redirect_location(request, err=str(exc))
        except Exception as exc:
            log.exception("dashboard archive failed: %s", exc)
            location = self._redirect_location(request, err="Archivierung fehlgeschlagen")
        raise web.HTTPFound(location=location)


__all__ = ["DashboardLiveMixin"]
