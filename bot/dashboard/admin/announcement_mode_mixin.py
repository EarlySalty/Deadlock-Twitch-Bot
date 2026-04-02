"""Admin-only legacy dashboard page for global chat announcement overrides."""

from __future__ import annotations

import html
import os
from typing import Any
from urllib.parse import urlsplit

from aiohttp import web

from ... import storage as _storage
from ...core.constants import log
from ...promo_mode import (
    PROMO_MODE_CUSTOM_EVENT,
    PROMO_MODE_STANDARD,
    evaluate_global_promo_mode,
    format_datetime_local_utc,
    load_global_promo_mode,
    save_global_promo_mode,
    validate_global_promo_mode_config,
)


class DashboardAdminAnnouncementMixin:
    """Legacy admin page for the global Twitch chat announcement mode."""

    @staticmethod
    def _admin_announcement_origin_from_value(raw_value: str | None) -> str | None:
        candidate = str(raw_value or "").strip()
        if not candidate:
            return None
        if "://" not in candidate:
            candidate = f"https://{candidate}"
        try:
            parsed = urlsplit(candidate)
        except Exception:
            return None
        scheme = str(parsed.scheme or "").strip().lower()
        host = str(parsed.hostname or "").strip().lower()
        if scheme not in {"http", "https"} or not host or not parsed.netloc:
            return None
        if scheme == "http" and host not in {"127.0.0.1", "localhost", "::1"}:
            return None
        return f"{scheme}://{parsed.netloc}".rstrip("/")

    def _admin_announcement_public_origin(self) -> str:
        candidates = (
            os.getenv("TWITCH_ADMIN_PUBLIC_URL"),
            os.getenv("MASTER_DASHBOARD_PUBLIC_URL"),
            getattr(self, "_discord_admin_redirect_uri", ""),
            "https://admin.earlysalty.de",
        )
        for candidate in candidates:
            origin = self._admin_announcement_origin_from_value(candidate)
            if origin:
                return origin
        return "https://admin.earlysalty.de"

    def _admin_announcement_enforce_admin_host(self, request: web.Request) -> None:
        local_checker = getattr(self, "_is_local_request", None)
        if callable(local_checker):
            try:
                if bool(local_checker(request)):
                    return
            except Exception:
                pass

        admin_host_checker = getattr(self, "_is_admin_dashboard_host_request", None)
        is_admin_host = False
        if callable(admin_host_checker):
            try:
                is_admin_host = bool(admin_host_checker(request))
            except Exception:
                is_admin_host = False

        if is_admin_host:
            return

        if request.method in {"GET", "HEAD"}:
            path_qs = request.rel_url.path_qs if request.rel_url else request.path
            safe_path = str(path_qs or "").strip()
            if not safe_path.startswith("/"):
                safe_path = "/twitch/admin/announcements"
            raise web.HTTPFound(f"{self._admin_announcement_public_origin()}{safe_path}")

        raise web.HTTPForbidden(
            text="This admin page is only available on the admin dashboard host."
        )

    def _admin_announcement_actor_label(self, request: web.Request) -> str:
        getter = getattr(self, "_get_discord_admin_session", None)
        if callable(getter):
            try:
                session = getter(request) or {}
            except Exception:
                session = {}
            user_id = str(session.get("user_id") or "").strip()
            if user_id.isdigit():
                return f"discord:{user_id}"
        return "admin"

    def _admin_announcement_redirect(
        self,
        request: web.Request,
        *,
        ok: str | None = None,
        err: str | None = None,
    ) -> web.HTTPFound:
        redirect_builder = getattr(self, "_redirect_location", None)
        if callable(redirect_builder):
            location = redirect_builder(
                request,
                ok=ok,
                err=err,
                default_path="/twitch/admin/announcements",
            )
        else:
            params = []
            if ok:
                params.append(f"ok={ok}")
            if err:
                params.append(f"err={err}")
            suffix = f"?{'&'.join(params)}" if params else ""
            location = f"/twitch/admin/announcements{suffix}"
        safe_location = (
            self._safe_internal_redirect(location, fallback="/twitch/admin/announcements")
            if hasattr(self, "_safe_internal_redirect")
            else "/twitch/admin/announcements"
        )
        return web.HTTPFound(location=safe_location)

    @staticmethod
    def _admin_announcement_status_parts(evaluation: dict[str, Any]) -> tuple[str, str, str]:
        status = str(evaluation.get("status") or "").strip().lower()
        if status == "active":
            return "Aktiv", "ok", "Globaler Event-Text überschreibt aktuell alle Partner-Overrides."
        if status == "scheduled":
            return "Geplant", "neutral", "Der Event-Text ist gespeichert und wird zum Startzeitpunkt aktiv."
        if status == "expired":
            return "Abgelaufen", "warn", "Das Endzeitfenster ist vorbei. Der Bot fällt automatisch auf Standard zurück."
        if status == "disabled":
            return "Deaktiviert", "warn", "Der Custom-Event-Modus ist gespeichert, aber aktuell ausgeschaltet."
        if status == "invalid":
            return "Ungültig", "err", "Die gespeicherte Nachricht ist ungültig und wird nicht als Override verwendet."
        return "Standard", "neutral", "Es gilt das normale Verhalten mit streamer-spezifischen Overrides und Fallback-Texten."

    @staticmethod
    def _admin_announcement_window_summary(evaluation: dict[str, Any]) -> str:
        starts_at = str(evaluation.get("starts_at") or "").strip()
        ends_at = str(evaluation.get("ends_at") or "").strip()
        parts: list[str] = []
        if starts_at:
            parts.append(f"Start: {starts_at}")
        else:
            parts.append("Start: sofort")
        if ends_at:
            parts.append(f"Ende: {ends_at}")
        else:
            parts.append("Ende: manuell")
        return " | ".join(parts)

    def _render_admin_section_nav(self, active: str) -> str:
        sections = (
            ("/twitch/admin", "Übersicht", "overview"),
            ("/twitch/admin/announcements", "Announcement-Modus", "announcements"),
        )

        def _anchor(href: str, label: str, key: str) -> str:
            cls = "tab active" if key == active else "tab"
            return f"<a class='{cls}' href='{href}'>{html.escape(label)}</a>"

        return "<nav class='tabs admin-subtabs'>" + "".join(
            _anchor(href, label, key) for href, label, key in sections
        ) + "</nav>"

    def _render_admin_announcement_overview_card(self) -> str:
        try:
            with _storage.readonly_connection() as conn:
                config = load_global_promo_mode(conn)
        except Exception:
            return (
                "<div class='card raid-auth-card'>"
                "  <div class='card-header'>"
                "    <div>"
                "      <p class='eyebrow'>Global Chat Promos</p>"
                "      <h2>Announcement-Modus</h2>"
                "      <p class='lead'>Die globale Event-Konfiguration konnte nicht geladen werden.</p>"
                "    </div>"
                "  </div>"
                "  <a class='btn' href='/twitch/admin/announcements'>Öffnen</a>"
                "</div>"
            )

        evaluation = evaluate_global_promo_mode(config)
        status_label, status_class, description = self._admin_announcement_status_parts(evaluation)
        window_summary = self._admin_announcement_window_summary(evaluation)
        updated_at = str(config.get("updated_at") or "").strip() or "—"
        updated_by = str(config.get("updated_by") or "").strip() or "—"

        return (
            "<div class='card raid-auth-card'>"
            "  <div class='card-header'>"
            "    <div>"
            "      <p class='eyebrow'>Global Chat Promos</p>"
            "      <h2>Announcement-Modus</h2>"
            "      <p class='lead'>Ein globaler Event-Text kann alle streamer-spezifischen Promo-Nachrichten temporär überschreiben.</p>"
            "    </div>"
            "    <div class='raid-metrics'>"
            f"      <div class='mini-stat'><strong>{html.escape(status_label)}</strong><span>Status</span></div>"
            "    </div>"
            "  </div>"
            "  <div class='raid-meta'>"
            f"    <span class='pill {status_class}'>{html.escape(status_label)}</span>"
            f"    <span class='pill'>{html.escape(window_summary)}</span>"
            "  </div>"
            f"  <div class='status-meta'>{html.escape(description)}</div>"
            "  <div class='status-meta'>"
            f"Letzte Änderung: {html.escape(updated_at)} | von {html.escape(updated_by)}"
            "  </div>"
            "  <div class='hero-actions' style='margin-top:1rem;'>"
            "    <a class='btn' href='/twitch/admin/announcements'>Verwalten</a>"
            "  </div>"
            "</div>"
        )

    async def admin_announcements_page(self, request: web.Request) -> web.StreamResponse:
        self._admin_announcement_enforce_admin_host(request)
        self._require_token(request)

        flash_ok = str(request.query.get("ok") or "").strip()
        flash_err = str(request.query.get("err") or "").strip()
        csrf_token = self._csrf_generate_token(request)

        with _storage.readonly_connection() as conn:
            config = load_global_promo_mode(conn)
        evaluation = evaluate_global_promo_mode(config)
        status_label, status_class, description = self._admin_announcement_status_parts(evaluation)
        window_summary = self._admin_announcement_window_summary(evaluation)

        mode = str(config.get("mode") or PROMO_MODE_STANDARD).strip().lower()
        custom_message = str(config.get("custom_message") or "")
        starts_at_value = format_datetime_local_utc(config.get("starts_at"))
        ends_at_value = format_datetime_local_utc(config.get("ends_at"))
        is_enabled = bool(config.get("is_enabled"))
        updated_at = str(config.get("updated_at") or "").strip() or "—"
        updated_by = str(config.get("updated_by") or "").strip() or "—"

        status_card_html = (
            "<div class='card scope-card'>"
            "  <div class='card-header'>"
            "    <div>"
            "      <p class='eyebrow'>Status</p>"
            "      <h2>Aktueller Announcement-Modus</h2>"
            f"      <p class='lead'>{html.escape(description)}</p>"
            "    </div>"
            "    <div class='raid-metrics'>"
            f"      <div class='mini-stat'><strong>{html.escape(status_label)}</strong><span>Status</span></div>"
            "    </div>"
            "  </div>"
            "  <div class='raid-meta'>"
            f"    <span class='pill {status_class}'>{html.escape(status_label)}</span>"
            f"    <span class='pill'>{html.escape(window_summary)}</span>"
            "    <span class='pill'>Priorität: global vor streamer_plans.promo_message</span>"
            "  </div>"
            "  <div class='status-meta'>"
            f"Letzte Änderung: {html.escape(updated_at)} | von {html.escape(updated_by)}"
            "  </div>"
            "</div>"
        )

        preview_message_html = (
            "<div class='discord-preview'>"
            "  <div class='discord-preview-row'>"
            "    <span class='preview-label'>Text</span>"
            f"    <span>{html.escape(custom_message or 'Kein Event-Text hinterlegt.')}</span>"
            "  </div>"
            "</div>"
        )

        form_html = (
            "<div class='card scope-card'>"
            "  <div class='card-header'>"
            "    <div>"
            "      <p class='eyebrow'>Konfiguration</p>"
            "      <h2>Globalen Chat-Override verwalten</h2>"
            "      <p class='lead'>Der Override greift nur im Modus <code>custom_event</code>, wenn er aktiviert ist und das UTC-Zeitfenster passt.</p>"
            "    </div>"
            "  </div>"
            "  <form method='post' action='/twitch/admin/announcements'>"
            f"    <input type='hidden' name='csrf_token' value='{html.escape(csrf_token, quote=True)}'>"
            "    <div class='advanced-content'>"
            "      <div class='form-row'>"
            "        <label>Modus"
            "          <select name='mode'>"
            f"            <option value='{PROMO_MODE_STANDARD}'{' selected' if mode == PROMO_MODE_STANDARD else ''}>Standard</option>"
            f"            <option value='{PROMO_MODE_CUSTOM_EVENT}'{' selected' if mode == PROMO_MODE_CUSTOM_EVENT else ''}>Custom Event</option>"
            "          </select>"
            "        </label>"
            "        <label>Start (UTC)"
            f"          <input type='datetime-local' name='starts_at' value='{html.escape(starts_at_value, quote=True)}' style='background:var(--bg-alt);border:1px solid var(--bd);color:var(--text);padding:.4rem .6rem;border-radius:.5rem;'>"
            "        </label>"
            "        <label>Ende (UTC)"
            f"          <input type='datetime-local' name='ends_at' value='{html.escape(ends_at_value, quote=True)}' style='background:var(--bg-alt);border:1px solid var(--bd);color:var(--text);padding:.4rem .6rem;border-radius:.5rem;'>"
            "        </label>"
            "      </div>"
            "      <label>Event-Text"
            f"        <textarea name='custom_message' rows='7' placeholder='Zum Event kommt ihr hier rein: {{invite}}' style='width:100%;min-height:10rem;background:var(--bg-alt);border:1px solid var(--bd);color:var(--text);padding:.65rem .75rem;border-radius:.6rem;'>{html.escape(custom_message)}</textarea>"
            "      </label>"
            "      <label class='checkbox-label'>"
            f"        <input type='checkbox' name='is_enabled' value='1'{' checked' if is_enabled else ''}>"
            "        <span>Custom/Event-Modus aktivieren</span>"
            "      </label>"
            "      <div class='hint'>Optionaler Platzhalter: <code>{invite}</code>. Ein fester Text ohne Invite ist hier ebenfalls erlaubt. Ohne Startzeit gilt der Modus sofort, ohne Endzeit bis zur manuellen Deaktivierung.</div>"
            "      <div class='hint'>Das Zeitfenster wird in UTC interpretiert. Nach Ablauf fällt der Bot automatisch auf das Standardverhalten zurück.</div>"
            f"      {preview_message_html}"
            "      <div class='action-stack'>"
            "        <button class='btn'>Speichern</button>"
            "        <a class='btn btn-secondary' href='/twitch/admin'>Zurück zur Übersicht</a>"
            "      </div>"
            "    </div>"
            "  </form>"
            "</div>"
        )

        body = (
            f"{self._render_admin_section_nav('announcements')}"
            "<header class='hero'>"
            "  <div>"
            "    <p class='eyebrow'>Twitch Admin</p>"
            "    <h1>Globaler Announcement-Modus</h1>"
            "    <p class='lead'>Steuert globale Event-Texte für Twitch-Chat-Promos im Legacy-Admin.</p>"
            "  </div>"
            "</header>"
            f"{status_card_html}"
            f"{form_html}"
        )

        return web.Response(
            text=self._html(body, active="live", msg=flash_ok, err=flash_err),
            content_type="text/html",
        )

    async def admin_announcements_save(self, request: web.Request) -> web.StreamResponse:
        self._admin_announcement_enforce_admin_host(request)
        self._require_token(request)

        data = await request.post()
        csrf_token = str(data.get("csrf_token") or "").strip()
        if not self._csrf_verify_token(request, csrf_token):
            raise self._admin_announcement_redirect(
                request,
                err="Ungültiges CSRF-Token",
            )

        raw_config = {
            "mode": data.get("mode"),
            "custom_message": data.get("custom_message"),
            "starts_at": data.get("starts_at"),
            "ends_at": data.get("ends_at"),
            "is_enabled": data.get("is_enabled"),
        }
        normalized_config, issues = validate_global_promo_mode_config(raw_config)
        if issues:
            raise self._admin_announcement_redirect(request, err=issues[0]["message"])

        try:
            with _storage.transaction() as conn:
                save_global_promo_mode(
                    conn,
                    config=normalized_config,
                    updated_by=self._admin_announcement_actor_label(request),
                )
        except ValueError as exc:
            raise self._admin_announcement_redirect(request, err=str(exc)) from exc
        except Exception as exc:
            log.exception("Failed to save global promo mode: %s", exc)
            raise self._admin_announcement_redirect(
                request,
                err="Speichern fehlgeschlagen",
            ) from exc

        raise self._admin_announcement_redirect(
            request,
            ok="Announcement-Modus gespeichert",
        )
