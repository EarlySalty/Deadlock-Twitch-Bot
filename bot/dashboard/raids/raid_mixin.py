"""Raid mixin for DashboardV2Server — raid OAuth, history and analytics routes."""

from __future__ import annotations

import asyncio
import html
import os
from typing import Any
from urllib.parse import urlsplit, urlunsplit

import aiohttp
import discord
from aiohttp import web

from ... import storage
from ...core.constants import log
from ...raid.scope_profiles import scopes_for_profile
from ...raid.views import RaidAuthGenerateView, build_raid_requirements_embed

TWITCH_HELIX_USERS_URL = "https://api.twitch.tv/helix/users"
DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL = "https://twitch.earlysalty.com/twitch/dashboard"
PUBLIC_STREAMER_ONBOARDING_URL = "https://twitch.earlysalty.com/twitch/onboarding"


class _DashboardRaidMixin:
    """Raid authorization, history, analytics and OAuth callback routes."""

    # ------------------------------------------------------------------ #
    # HTML builders                                                        #
    # ------------------------------------------------------------------ #

    @staticmethod
    def _build_raid_auth_start_html(login: str, auth_url: str) -> str:
        safe_login = html.escape(login, quote=True)
        safe_auth_url = html.escape(auth_url, quote=True)
        return "".join(
            [
                "<html><head><title>Raid Bot Autorisierung</title></head>",
                "<body style='font-family: sans-serif; max-width: 680px; margin: 48px auto;'>",
                "<h1>Raid Bot Autorisierung</h1>",
                "<p>Streamer: <strong>",
                safe_login,
                "</strong></p>",
                "<p>Klicke auf den Link unten, um den Raid Bot zu autorisieren:</p>",
                "<p><a href='",
                safe_auth_url,
                "' style='padding: 10px 20px; background: #9146FF; color: white; text-decoration: none; border-radius: 5px;'>",
                "Auf Twitch autorisieren</a></p>",
                "<p style='color: #666; font-size: 0.9em;'>",
                "Der Raid Bot kann dann automatisch in deinem Namen raiden, wenn du offline gehst.",
                "</p></body></html>",
            ]
        )

    @staticmethod
    def _build_raid_history_rows(history: list[dict]) -> str:
        rows: list[str] = []
        for entry in history:
            success_icon = "OK" if entry.get("success") else "X"
            executed_at = str(entry.get("executed_at") or "")[:19]
            try:
                stream_duration_min = int(entry.get("stream_duration_sec") or 0) // 60
            except (TypeError, ValueError):
                stream_duration_min = 0

            rows.append(
                "".join(
                    [
                        "<tr>",
                        "<td>",
                        html.escape(success_icon, quote=True),
                        "</td>",
                        "<td>",
                        html.escape(executed_at, quote=True),
                        "</td>",
                        "<td><strong>",
                        html.escape(str(entry.get("from_broadcaster_login") or ""), quote=True),
                        "</strong></td>",
                        "<td><strong>",
                        html.escape(str(entry.get("to_broadcaster_login") or ""), quote=True),
                        "</strong></td>",
                        "<td>",
                        html.escape(str(entry.get("viewer_count") or 0), quote=True),
                        "</td>",
                        "<td>",
                        html.escape(str(stream_duration_min), quote=True),
                        " min</td>",
                        "<td>",
                        html.escape(str(entry.get("candidates_count") or 0), quote=True),
                        "</td>",
                        "<td style='color: red; font-size: 0.85em;'>",
                        html.escape(str(entry.get("error_message") or ""), quote=True),
                        "</td>",
                        "</tr>",
                    ]
                )
            )

        if rows:
            return "".join(rows)
        return "<tr><td colspan='8'>Keine Raids gefunden</td></tr>"

    @staticmethod
    def _build_raid_history_page(rows_html: str) -> str:
        return "".join(
            [
                "<html><head><title>Raid History</title><style>",
                "body { font-family: sans-serif; margin: 32px; }",
                "table { border-collapse: collapse; width: 100%; }",
                "th, td { border: 1px solid #ddd; padding: 12px 10px; text-align: left; }",
                "th { background-color: #9146FF; color: white; }",
                "tr:nth-child(even) { background-color: #f2f2f2; }",
                "</style></head><body>",
                "<h1>Raid History</h1>",
                "<p><a href='/twitch/admin'>Zurueck zum Dashboard</a></p>",
                "<table><thead><tr>",
                "<th>Status</th><th>Zeitpunkt</th><th>Von</th><th>Nach</th>",
                "<th>Viewer</th><th>Stream-Dauer</th><th>Kandidaten</th><th>Fehler</th>",
                "</tr></thead><tbody>",
                rows_html,
                "</tbody></table></body></html>",
            ]
        )

    @staticmethod
    def _build_raid_analytics_page(
        *,
        partner_stats: list,
        leechers: list,
        manual_list: list,
        date_min: str,
        date_max: str,
        total: int,
    ) -> str:
        import json as _json

        labels = _json.dumps([p["login"] for p in partner_stats])
        sent_data = _json.dumps([p["sent"] for p in partner_stats])
        recv_data = _json.dumps([p["received"] for p in partner_stats])

        # Balance table rows
        balance_rows = []
        for p in partner_stats:
            b = p["balance"]
            if b > 0:
                badge = f"<span class='badge badge-ok'>+{b}</span>"
            elif b < 0:
                badge = f"<span class='badge badge-err'>{b}</span>"
            else:
                badge = "<span class='badge badge-neutral'>0</span>"
            style = " class='leecher-row'" if p["sent"] == 0 and p["received"] > 0 else ""
            balance_rows.append(
                f"<tr{style}>"
                f"<td><strong>{html.escape(p['login'])}</strong></td>"
                f"<td>{p['sent']}</td>"
                f"<td>{p['received']}</td>"
                f"<td>{badge}</td>"
                f"<td>{p['viewers_sent']}</td>"
                f"<td>{p['viewers_recv']}</td>"
                f"</tr>"
            )
        balance_rows_html = "".join(balance_rows) or "<tr><td colspan='6'>Keine Daten</td></tr>"

        # Leecher list
        if leechers:
            leecher_items = "".join(
                f"<li><strong>{html.escape(l['login'])}</strong> — {l['received']} Raids empfangen, 0 gesendet</li>"
                for l in leechers
            )
            leecher_html = f"<div class='alert-card'><h2>Keine Raids zurückgegeben <span class='badge badge-err'>{len(leechers)}</span></h2><ul>{leecher_items}</ul></div>"
        else:
            leecher_html = "<div class='alert-card alert-ok'><h2>Alle aktiven Partner haben bereits geraided ✓</h2></div>"

        # Manual raids table
        if manual_list:
            manual_rows = []
            for m in manual_list:
                status_badge = (
                    '<span class="badge badge-ok">Partner</span>'
                    if m["is_partner"]
                    else '<span class="badge badge-warn">Extern</span>'
                )
                manual_rows.append(
                    f"<tr>"
                    f"<td><strong>{html.escape(m['from'])}</strong></td>"
                    f"<td><strong>{html.escape(m['to'])}</strong></td>"
                    f"<td>{status_badge}</td>"
                    f"<td>{m['viewers']}</td>"
                    f"<td>{html.escape(m['at'])}</td>"
                    f"</tr>"
                )
            manual_rows_html = "".join(manual_rows)
        else:
            manual_rows_html = "<tr><td colspan='5'>Keine manuellen Raids</td></tr>"

        return f"""<!DOCTYPE html>
<html lang="de">
<head>
<meta charset="utf-8">
<title>Raid Analytics</title>
<script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.4/dist/chart.umd.min.js"></script>
<style>
  @import url('https://fonts.googleapis.com/css2?family=Fraunces:wght@500;600;700&family=Space+Grotesk:wght@400;500;600&display=swap');
  :root {{
    color-scheme: dark;
    --bg:#0b0a14; --bg-alt:#141226; --card:#1b1630; --bd:#2c2349; --text:#f2edff; --muted:#a394c7;
    --accent:#7c3aed; --accent-2:#f472b6; --accent-3:#d6ccff;
    --ok-bg:#0f2f24; --ok-bd:#1f9d7a; --ok-fg:#baf7dd;
    --err-bg:#3b0f1c; --err-bd:#b91c1c; --err-fg:#fecaca;
    --warn-bg:#2f210b; --warn-bd:#d97706; --warn-fg:#fde68a;
    --shadow:rgba(0,0,0,.45);
  }}
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{
    font-family: "Space Grotesk", "Segoe UI", sans-serif;
    background: radial-gradient(900px 540px at 5% -10%, rgba(124,58,237,0.35), transparent 60%),
                radial-gradient(900px 540px at 95% 0%, rgba(244,114,182,0.22), transparent 55%),
                linear-gradient(180deg, #0b0a14 0%, #100c1f 55%, #0b0a14 100%);
    color: var(--text);
    padding: 2rem 1.8rem 3rem;
    min-height: 100vh;
  }}
  body::before {{
    content:""; position:fixed; inset:0;
    background: repeating-linear-gradient(135deg, rgba(255,255,255,0.04) 0 1px, transparent 1px 14px);
    opacity:0.2; pointer-events:none; z-index:0;
  }}
  body > * {{ position: relative; z-index: 1; }}
  h1 {{ font-family: "Fraunces", serif; font-size: 2rem; margin-bottom: .3rem; }}
  h2 {{ font-family: "Fraunces", serif; font-size: 1.15rem; margin-bottom: .8rem; color: var(--accent-3); }}
  .meta {{ color: var(--muted); font-size: .85rem; margin-bottom: 2rem; }}
  .nav {{ margin-bottom: 1.8rem; display: flex; gap: .8rem; flex-wrap: wrap; }}
  .nav a {{ color: var(--muted); text-decoration: none; padding: .4rem .8rem; border: 1px solid var(--bd); border-radius: 999px; font-size: .88rem; transition: border-color .15s; }}
  .nav a:hover {{ border-color: var(--accent); color: var(--text); }}
  .grid {{ display: grid; grid-template-columns: 1fr 1fr; gap: 1.4rem; margin-bottom: 1.4rem; }}
  @media (max-width: 900px) {{ .grid {{ grid-template-columns: 1fr; }} }}
  .card {{ background: var(--card); border: 1px solid var(--bd); border-radius: 1rem; padding: 1.4rem; box-shadow: 0 12px 30px var(--shadow); }}
  .card-full {{ grid-column: 1 / -1; }}
  .chart-wrap {{ position: relative; height: 340px; }}
  .chart-wrap-tall {{ position: relative; height: {max(280, len(partner_stats) * 38)}px; }}
  table {{ width: 100%; border-collapse: collapse; font-size: .9rem; }}
  th {{ color: var(--accent-3); text-transform: uppercase; letter-spacing: .07em; font-size: .75rem; padding: .55rem .5rem; border-bottom: 1px solid var(--bd); text-align: left; }}
  td {{ padding: .6rem .5rem; border-bottom: 1px solid rgba(44,35,73,.5); vertical-align: middle; }}
  tr:last-child td {{ border-bottom: none; }}
  tr.leecher-row td {{ background: rgba(185,28,28,.06); }}
  .badge {{ display:inline-flex; align-items:center; padding:.18rem .55rem; border-radius:999px; font-size:.78rem; font-weight:700; border:1px solid; }}
  .badge-ok {{ background:var(--ok-bg); color:var(--ok-fg); border-color:var(--ok-bd); }}
  .badge-err {{ background:var(--err-bg); color:var(--err-fg); border-color:var(--err-bd); }}
  .badge-warn {{ background:var(--warn-bg); color:var(--warn-fg); border-color:var(--warn-bd); }}
  .badge-neutral {{ background:rgba(124,58,237,.15); color:var(--accent-3); border-color:rgba(124,58,237,.35); }}
  .alert-card {{ background: var(--card); border: 1px solid var(--err-bd); border-radius: 1rem; padding: 1.4rem; margin-bottom: 1.4rem; }}
  .alert-card.alert-ok {{ border-color: var(--ok-bd); }}
  .alert-card ul {{ padding-left: 1.2rem; margin-top: .5rem; }}
  .alert-card li {{ margin-bottom: .35rem; color: var(--muted); font-size: .9rem; }}
  .stat-grid {{ display: grid; grid-template-columns: repeat(3, 1fr); gap: 1rem; margin-bottom: 1.4rem; }}
  .stat {{ background: var(--card); border: 1px solid var(--bd); border-radius: .8rem; padding: 1rem 1.2rem; text-align: center; }}
  .stat .num {{ font-family: "Fraunces", serif; font-size: 2rem; color: var(--accent-3); }}
  .stat .lbl {{ font-size: .8rem; color: var(--muted); margin-top: .2rem; }}
</style>
</head>
<body>
<h1>Raid Analytics</h1>
<p class="meta">Zeitraum: {html.escape(date_min)} – {html.escape(date_max)}</p>

<nav class="nav">
  <a href="/twitch/admin">← Admin</a>
  <a href="/twitch/raid/history">Raid History</a>
</nav>

<div class="stat-grid">
  <div class="stat"><div class="num">{total}</div><div class="lbl">Raids gesamt</div></div>
  <div class="stat"><div class="num">{len(partner_stats)}</div><div class="lbl">Aktive Partner</div></div>
  <div class="stat"><div class="num">{len(leechers)}</div><div class="lbl">Nur Empfänger</div></div>
</div>

{leecher_html}

<div class="grid">
  <div class="card card-full">
    <h2>Raids gesendet vs. empfangen pro Partner</h2>
    <div class="chart-wrap-tall">
      <canvas id="barChart"></canvas>
    </div>
  </div>

  <div class="card card-full">
    <h2>Balance-Tabelle (Partner)</h2>
    <table>
      <thead><tr>
        <th>Streamer</th><th>Gesendet</th><th>Empfangen</th><th>Balance</th><th>Viewer gesendet</th><th>Viewer empfangen</th>
      </tr></thead>
      <tbody>{balance_rows_html}</tbody>
    </table>
  </div>

  <div class="card card-full">
    <h2>Manuelle Raids <span class="badge badge-neutral">{len(manual_list)}</span></h2>
    <table>
      <thead><tr>
        <th>Von</th><th>Nach</th><th>Typ</th><th>Viewer</th><th>Zeitpunkt</th>
      </tr></thead>
      <tbody>{manual_rows_html}</tbody>
    </table>
  </div>
</div>

<script>
const labels = {labels};
const sentData = {sent_data};
const recvData = {recv_data};

const ctx = document.getElementById('barChart').getContext('2d');
new Chart(ctx, {{
  type: 'bar',
  data: {{
    labels: labels,
    datasets: [
      {{
        label: 'Gesendet',
        data: sentData,
        backgroundColor: 'rgba(124,58,237,0.75)',
        borderColor: 'rgba(124,58,237,1)',
        borderWidth: 1,
        borderRadius: 4,
      }},
      {{
        label: 'Empfangen',
        data: recvData,
        backgroundColor: 'rgba(244,114,182,0.6)',
        borderColor: 'rgba(244,114,182,1)',
        borderWidth: 1,
        borderRadius: 4,
      }}
    ]
  }},
  options: {{
    indexAxis: 'y',
    responsive: true,
    maintainAspectRatio: false,
    plugins: {{
      legend: {{ labels: {{ color: '#f2edff', font: {{ family: 'Space Grotesk' }} }} }},
      tooltip: {{
        backgroundColor: '#1b1630',
        borderColor: '#2c2349',
        borderWidth: 1,
        titleColor: '#d6ccff',
        bodyColor: '#a394c7',
      }}
    }},
    scales: {{
      x: {{
        grid: {{ color: 'rgba(44,35,73,0.6)' }},
        ticks: {{ color: '#a394c7', stepSize: 1 }},
        beginAtZero: true,
      }},
      y: {{
        grid: {{ display: false }},
        ticks: {{ color: '#f2edff', font: {{ size: 12 }} }},
      }}
    }}
  }}
}});
</script>
</body>
</html>"""

    @staticmethod
    def _raid_oauth_success_redirect_url(candidate: str | None = None) -> str:
        configured = (candidate or "").strip()
        if not configured:
            configured = (os.getenv("TWITCH_RAID_SUCCESS_REDIRECT_URL") or "").strip()
        candidate = configured or DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL
        fallback = DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL

        try:
            parts = urlsplit(candidate)
        except Exception:
            return fallback

        if parts.username or parts.password:
            return fallback

        scheme = (parts.scheme or "").strip().lower()
        host = (parts.hostname or "").strip().lower()
        if scheme not in {"https", "http"}:
            return fallback
        if not host:
            return fallback
        if scheme == "http" and host not in {"127.0.0.1", "localhost", "::1"}:
            return fallback

        path = parts.path or "/"
        return urlunsplit((scheme, parts.netloc, path, parts.query, ""))

    # ------------------------------------------------------------------ #
    # Raid routes                                                          #
    # ------------------------------------------------------------------ #

    def _raid_dashboard_auth_context(self, request: web.Request) -> tuple[str, bool, str]:
        auth_level = ""
        auth_level_getter = getattr(self, "_get_auth_level", None)
        if callable(auth_level_getter):
            try:
                auth_level = str(auth_level_getter(request) or "").strip().lower()
            except Exception:
                auth_level = ""
        is_admin = auth_level in {"admin", "localhost"}

        session_login = ""
        session_getter = getattr(self, "_get_dashboard_auth_session", None)
        if callable(session_getter):
            try:
                dashboard_session = session_getter(request)
            except Exception:
                dashboard_session = None
            if isinstance(dashboard_session, dict):
                session_login = str(dashboard_session.get("twitch_login") or "").strip().lower()
        return auth_level, is_admin, session_login

    @staticmethod
    def _raid_active_partner_login(row: Any, fallback: str = "") -> str:
        if not row:
            return ""
        if hasattr(row, "keys"):
            return str(row.get("twitch_login") or fallback).strip().lower()
        return str((row[2] if len(row) > 2 else fallback) or fallback).strip().lower()

    async def raid_auth_start(self, request: web.Request) -> web.StreamResponse:
        """Create OAuth URL for raid bot authorization.

        Access policy:
        - Streamer dashboard session may only authorize its own Twitch login.
        - Explicit `?login=` overrides require admin token/session gate.
        """
        requested_login = (request.query.get("login") or "").strip().lower()
        login = ""
        session_getter = getattr(self, "_get_dashboard_auth_session", None)
        if callable(session_getter):
            try:
                dashboard_session = session_getter(request)
            except Exception:
                log.debug("Could not resolve dashboard auth session for raid auth", exc_info=True)
                dashboard_session = None
            if isinstance(dashboard_session, dict):
                login = str(dashboard_session.get("twitch_login") or "").strip().lower()

        if requested_login:
            if not login or requested_login != login:
                self._require_token(request)
            login = requested_login
        elif not login:
            # Public streamer onboarding was moved to the website landing page.
            # Unauthenticated visits should no longer enter the raid OAuth flow directly.
            raise web.HTTPFound(location=PUBLIC_STREAMER_ONBOARDING_URL)

        if not login:
            return web.Response(text="Missing login parameter", status=400)

        auth_manager = getattr(getattr(self, "_raid_bot", None), "auth_manager", None)
        if auth_manager:
            client_id = str(getattr(auth_manager, "client_id", "") or "").strip()
            redirect_uri = str(getattr(auth_manager, "redirect_uri", "") or "").strip()
            if not client_id or not redirect_uri:
                return web.Response(text="Raid bot OAuth is not configured", status=503)
            auth_url = str(auth_manager.generate_auth_url(login))
        else:
            raid_auth_url_cb = getattr(self, "_raid_auth_url_cb", None)
            if not callable(raid_auth_url_cb):
                return web.Response(text="Raid bot not initialized", status=503)
            try:
                auth_url = str(await raid_auth_url_cb(login)).strip()
            except Exception as exc:
                status = int(getattr(exc, "status", 503) or 503)
                return web.Response(
                    text=str(getattr(exc, "message", str(exc)) or "Raid bot not initialized"),
                    status=max(400, min(status, 599)),
                )
            if not auth_url:
                return web.Response(text="Raid bot not initialized", status=503)

        raise web.HTTPFound(location=auth_url)

    async def raid_auth_go(self, request: web.Request) -> web.StreamResponse:
        """Kurz-Redirect für Discord-Buttons → leitet zum vollen Twitch-OAuth-URL weiter.

        Kein Token erforderlich – der State ist das Geheimnis (10 Min TTL).
        Discord-Button-URLs sind auf 512 Zeichen limitiert; der volle OAuth-URL
        überschreitet dieses Limit.  Der Button verweist stattdessen auf diesen
        Endpoint, der den gespeicherten URL nachschlägt und weiterleitet.
        """
        state = (request.query.get("state") or "").strip()
        if not state:
            return web.Response(text="Missing state parameter", status=400)

        auth_manager = getattr(getattr(self, "_raid_bot", None), "auth_manager", None)
        if auth_manager:
            full_url = auth_manager.get_pending_auth_url(state)
        else:
            raid_go_url_cb = getattr(self, "_raid_go_url_cb", None)
            if not callable(raid_go_url_cb):
                return web.Response(text="Raid bot not initialized", status=503)
            try:
                full_url = await raid_go_url_cb(state)
            except Exception as exc:
                status = int(getattr(exc, "status", 503) or 503)
                return web.Response(
                    text=str(getattr(exc, "message", str(exc)) or "Raid bot not initialized"),
                    status=max(400, min(status, 599)),
                )
        if not full_url:
            return web.Response(
                text="<html><body>Link abgelaufen oder ungültig. "
                "Bitte erneut auf den Button in Discord klicken.</body></html>",
                content_type="text/html",
                status=410,
            )

        raise web.HTTPFound(location=full_url)

    async def raid_requirements(self, request: web.Request) -> web.StreamResponse:
        """Send raid OAuth requirement DM with one-click fresh link generation."""
        self._require_token(request)
        _auth_level, is_admin, session_login = self._raid_dashboard_auth_context(request)

        login = (request.query.get("login") or "").strip().lower()
        if not login:
            return web.Response(text="Missing login parameter", status=400)

        try:
            with storage.get_conn() as conn:
                row = storage.load_active_partner(conn, twitch_login=login)
                session_partner = (
                    storage.load_active_partner(conn, twitch_login=session_login)
                    if session_login
                    else None
                )
        except Exception:
            safe_login = str(login or "").replace("\r", "\\r").replace("\n", "\\n")
            log.exception(
                "Failed to load partner authorization for raid requirements (%s)",
                safe_login,
            )
            return web.Response(text="Failed to load Discord link", status=500)

        if not row:
            return web.Response(text="Streamer not found", status=404)

        login = self._raid_active_partner_login(row, login)
        session_partner_login = self._raid_active_partner_login(session_partner, session_login)
        if not is_admin:
            if not session_partner_login:
                return web.Response(text="Dashboard streamer session required", status=403)
            if login != session_partner_login:
                return web.Response(text="Forbidden streamer scope", status=403)

        auth_manager = getattr(getattr(self, "_raid_bot", None), "auth_manager", None)
        if not auth_manager:
            raid_requirements_cb = getattr(self, "_raid_requirements_cb", None)
            if not callable(raid_requirements_cb):
                return web.Response(text="Raid bot not initialized", status=503)
            try:
                ok_message = str(await raid_requirements_cb(login))
            except Exception as exc:
                status = int(getattr(exc, "status", 503) or 503)
                return web.Response(
                    text=str(getattr(exc, "message", str(exc)) or "Raid bot not initialized"),
                    status=max(400, min(status, 599)),
                )
            location = self._redirect_location(request, ok=ok_message, default_path="/twitch/admin")
            safe_location = self._safe_internal_redirect(location, fallback="/twitch/admin")
            raise web.HTTPFound(location=safe_location)

        if hasattr(row, "keys"):
            discord_user_id = str(row.get("discord_user_id") or "").strip()
        else:
            discord_user_id = str((row[21] if len(row) > 21 else "") or "").strip()
        if not discord_user_id:
            return web.Response(text="No Discord user linked for this streamer", status=404)

        try:
            user_id_int = int(discord_user_id)
        except (TypeError, ValueError):
            return web.Response(text="Invalid Discord user id", status=400)

        discord_bot = getattr(auth_manager, "_discord_bot", None)
        if not discord_bot:
            return web.Response(text="Discord bot not available", status=503)

        user = discord_bot.get_user(user_id_int)
        if user is None:
            try:
                user = await discord_bot.fetch_user(user_id_int)
            except discord.NotFound:
                user = None
            except discord.HTTPException:
                safe_login = str(login or "").replace("\r", "\\r").replace("\n", "\\n")
                log.exception(
                    "Failed to fetch Discord user %s for %s",
                    user_id_int,
                    safe_login,
                )
                user = None

        if user is None:
            return web.Response(text="Discord user not found", status=404)

        embed = build_raid_requirements_embed(login)
        view = RaidAuthGenerateView(auth_manager=auth_manager, twitch_login=login)

        try:
            await user.send(embed=embed, view=view)
        except discord.Forbidden:
            safe_login = str(login or "").replace("\r", "\\r").replace("\n", "\\n")
            log.warning(
                "Discord DM blocked for %s (%s)",
                safe_login,
                user_id_int,
            )
            return web.Response(text="Discord DM blocked", status=403)
        except discord.HTTPException:
            safe_login = str(login or "").replace("\r", "\\r").replace("\n", "\\n")
            log.exception(
                "Failed to send raid requirements DM to %s (%s)",
                safe_login,
                user_id_int,
            )
            return web.Response(text="Failed to send Discord DM", status=502)

        ok_message = f"Anforderungen per Discord an @{login} gesendet"
        location = self._redirect_location(request, ok=ok_message, default_path="/twitch/admin")
        safe_location = self._safe_internal_redirect(location, fallback="/twitch/admin")
        raise web.HTTPFound(location=safe_location)

    async def raid_history(self, request: web.Request) -> web.StreamResponse:
        """Render raid history table for dashboard operators."""
        self._require_token(request)

        try:
            limit = int((request.query.get("limit") or "50").strip())
        except ValueError:
            limit = 50
        limit = max(1, min(limit, 500))
        from_broadcaster = (request.query.get("from") or "").strip().lower()

        history = await self._raid_history_cb(limit=limit, from_broadcaster=from_broadcaster)
        rows_html = self._build_raid_history_rows(history)
        page_html = self._build_raid_history_page(rows_html)
        return web.Response(text=page_html, content_type="text/html")

    async def raid_analytics(self, request: web.Request) -> web.StreamResponse:
        """Raid analytics: sent/received balance, leechers, manual raids."""
        self._require_token(request)

        with storage.get_conn() as conn:
            # Active partners set
            partner_rows = conn.execute(
                "SELECT twitch_login FROM twitch_streamers_partner_state WHERE is_partner_active = 1"
            ).fetchall()
            partners: set = {r[0].lower() for r in partner_rows}

            # Sent stats
            sent_rows = conn.execute(
                """
                SELECT from_broadcaster_login, COUNT(*) as cnt, SUM(viewer_count) as viewers
                FROM twitch_raid_history WHERE COALESCE(success, FALSE) IS TRUE
                GROUP BY from_broadcaster_login ORDER BY cnt DESC
                """
            ).fetchall()

            # Received stats
            recv_rows = conn.execute(
                """
                SELECT to_broadcaster_login, COUNT(*) as cnt, SUM(viewer_count) as viewers
                FROM twitch_raid_history WHERE COALESCE(success, FALSE) IS TRUE
                GROUP BY to_broadcaster_login ORDER BY cnt DESC
                """
            ).fetchall()

            # Manual raids
            manual_rows = conn.execute(
                """
                SELECT from_broadcaster_login, to_broadcaster_login, viewer_count, executed_at
                FROM twitch_raid_history
                WHERE reason = 'manual_chat_command'
                ORDER BY executed_at DESC
                """
            ).fetchall()

            # Date range
            date_row = conn.execute(
                "SELECT MIN(executed_at), MAX(executed_at), COUNT(*) FROM twitch_raid_history WHERE COALESCE(success, FALSE) IS TRUE"
            ).fetchone()

        sent_map: dict = {r[0].lower(): {"cnt": r[1], "viewers": r[2] or 0} for r in sent_rows}
        recv_map: dict = {r[0].lower(): {"cnt": r[1], "viewers": r[2] or 0} for r in recv_rows}

        # Per-partner balance (only active partners for main table)
        partner_stats = []
        for login in sorted(partners):
            s = sent_map.get(login, {}).get("cnt", 0)
            r = recv_map.get(login, {}).get("cnt", 0)
            sv = sent_map.get(login, {}).get("viewers", 0)
            rv = recv_map.get(login, {}).get("viewers", 0)
            partner_stats.append(
                {
                    "login": login,
                    "sent": s,
                    "received": r,
                    "balance": s - r,
                    "viewers_sent": sv,
                    "viewers_recv": rv,
                }
            )
        partner_stats.sort(key=lambda x: x["balance"], reverse=True)

        leechers = [p for p in partner_stats if p["sent"] == 0 and p["received"] > 0]

        # External receivers of manual raids (non-partner targets)
        manual_list = []
        for row in manual_rows:
            raider = (row[0] or "").lower()
            target = (row[1] or "").lower()
            manual_list.append(
                {
                    "from": raider,
                    "to": target,
                    "viewers": row[2] or 0,
                    "at": str(row[3] or "")[:16],
                    "is_partner": target in partners,
                }
            )

        date_min = str(date_row[0] or "")[:10]
        date_max = str(date_row[1] or "")[:10]
        total = date_row[2] or 0

        page_html = self._build_raid_analytics_page(
            partner_stats=partner_stats,
            leechers=leechers,
            manual_list=manual_list,
            date_min=date_min,
            date_max=date_max,
            total=total,
        )
        return web.Response(text=page_html, content_type="text/html")

    async def raid_oauth_callback(self, request: web.Request) -> web.StreamResponse:
        """Handle Twitch OAuth callback for raid authorization."""
        raid_bot = self._raid_bot
        auth_manager = getattr(raid_bot, "auth_manager", None) if raid_bot else None

        code = (request.query.get("code") or "").strip()
        state = (request.query.get("state") or "").strip()
        error = (request.query.get("error") or "").strip()

        if not raid_bot or not auth_manager:
            raid_oauth_callback_cb = getattr(self, "_raid_oauth_callback_cb", None)
            if callable(raid_oauth_callback_cb):
                try:
                    payload = await raid_oauth_callback_cb(code=code, state=state, error=error)
                except Exception as exc:
                    status = int(getattr(exc, "status", 503) or 503)
                    payload = {
                        "title": "Raid-Bot nicht verfügbar",
                        "body_html": (
                            "<p>"
                            + html.escape(
                                str(getattr(exc, "message", str(exc)) or "Raid bot not initialized"),
                                quote=True,
                            )
                            + "</p>"
                        ),
                        "status": max(400, min(status, 599)),
                    }
                title = str(payload.get("title") or "Autorisierung")
                body_html = str(payload.get("body_html") or "<p>Unbekannte Antwort.</p>")
                try:
                    status_code = int(payload.get("status", 200))
                except (TypeError, ValueError):
                    status_code = 200
                status_code = max(200, min(status_code, 599))
                redirect_candidate = str(payload.get("redirect_url") or "").strip()
                if redirect_candidate and status_code < 400:
                    raise web.HTTPFound(
                        location=self._raid_oauth_success_redirect_url(redirect_candidate)
                    )
                return web.Response(
                    text=self._render_oauth_page(title, body_html),
                    status=status_code,
                    content_type="text/html",
                )

        if error:
            expected_uri = (getattr(auth_manager, "redirect_uri", "") or "").strip()
            expected_html = (
                f"<p><code>{html.escape(expected_uri, quote=True)}</code></p>"
                if expected_uri
                else ""
            )
            if error == "redirect_mismatch":
                message = (
                    "<p>Twitch hat die Redirect-URI abgelehnt (redirect_mismatch).</p>"
                    "<p>Bitte trage diese URL exakt in der Twitch Application unter "
                    "<strong>OAuth Redirect URLs</strong> ein und starte die Autorisierung neu:</p>"
                    f"{expected_html}"
                )
            else:
                message = (
                    "<p>OAuth-Fehler beim Autorisieren.</p>"
                    "<p>Bitte die Autorisierung erneut starten.</p>"
                )
            return web.Response(
                text=self._render_oauth_page("Autorisierung fehlgeschlagen", message),
                status=400,
                content_type="text/html",
            )

        if not code or not state:
            return web.Response(
                text=self._render_oauth_page(
                    "Ungültige Anfrage",
                    "<p>Fehlender OAuth Code oder State.</p>",
                ),
                status=400,
                content_type="text/html",
            )

        if not raid_bot or not auth_manager:
            return web.Response(
                text=self._render_oauth_page(
                    "Raid-Bot nicht verfügbar",
                    "<p>Der Raid-Bot ist aktuell nicht initialisiert. Bitte später erneut versuchen.</p>",
                ),
                status=503,
                content_type="text/html",
            )

        state_info = auth_manager.consume_state_details(state)
        if not state_info:
            return web.Response(
                text=self._render_oauth_page(
                    "Ungültiger State",
                    "<p>Der OAuth-State ist ungültig oder abgelaufen. Bitte den Link neu erzeugen.</p>",
                ),
                status=400,
                content_type="text/html",
            )
        login = state_info.requested_login
        state_discord_user_id = state_info.discord_user_id

        session = getattr(raid_bot, "session", None)
        owns_session = False
        if session is None:
            session = aiohttp.ClientSession(timeout=aiohttp.ClientTimeout(total=20))
            owns_session = True

        try:
            token_data = await auth_manager.exchange_code_for_token(code, session)

            access_token = str(token_data.get("access_token") or "").strip()
            refresh_token = str(token_data.get("refresh_token") or "").strip()
            if not access_token:
                raise RuntimeError("Missing access_token in Twitch OAuth response")
            if not refresh_token:
                raise RuntimeError("Missing refresh_token in Twitch OAuth response")

            headers = {
                "Client-ID": str(auth_manager.client_id),
                "Authorization": f"Bearer {access_token}",
            }
            async with session.get(TWITCH_HELIX_USERS_URL, headers=headers) as user_resp:
                if user_resp.status != 200:
                    body = await user_resp.text()
                    raise RuntimeError(
                        f"Failed to fetch Twitch user info ({user_resp.status}): {body[:300]}"
                    )
                user_payload = await user_resp.json()

            users = user_payload.get("data") if isinstance(user_payload, dict) else None
            if not isinstance(users, list) or not users:
                raise RuntimeError("Missing Twitch user data in OAuth callback")
            user_info = users[0] or {}

            twitch_user_id = str(user_info.get("id") or "").strip()
            twitch_login = str(user_info.get("login") or "").strip().lower()
            if not twitch_user_id or not twitch_login:
                raise RuntimeError("Invalid Twitch user payload in OAuth callback")

            expected_twitch_login = str(state_info.expected_twitch_login or "").strip().lower()
            if not expected_twitch_login:
                requested_login = str(state_info.requested_login or "").strip().lower()
                if requested_login and not requested_login.startswith("discord:"):
                    expected_twitch_login = requested_login
            if expected_twitch_login and twitch_login != expected_twitch_login:
                log.warning(
                    "Raid OAuth callback login mismatch: expected=%s actual=%s state=%s",
                    expected_twitch_login,
                    twitch_login,
                    login,
                )
                return web.Response(
                    text=self._render_oauth_page(
                        "Falscher Twitch-Account",
                        "<p>Die Autorisierung wurde mit dem falschen Twitch-Account abgeschlossen.</p>"
                        "<p>Bitte den Link erneut öffnen und dich mit dem vorgesehenen Kanal anmelden.</p>",
                    ),
                    status=403,
                    content_type="text/html",
                )

            scopes_raw = token_data.get("scope", [])
            if isinstance(scopes_raw, str):
                scopes = [scope for scope in scopes_raw.split() if scope]
            elif isinstance(scopes_raw, list):
                scopes = [str(scope).strip() for scope in scopes_raw if str(scope).strip()]
            else:
                scopes = []
            allowed_scopes = set(scopes_for_profile(state_info.scope_profile))
            unexpected_scopes = sorted({scope for scope in scopes if scope not in allowed_scopes})
            if unexpected_scopes:
                log.warning(
                    "Raid OAuth callback returned scopes outside expected profile for %s: %s",
                    twitch_login,
                    ", ".join(unexpected_scopes),
                )
                return web.Response(
                    text=self._render_oauth_page(
                        "Ungültige Berechtigungen",
                        "<p>Die Autorisierung wurde mit unerwarteten Berechtigungen abgeschlossen.</p>"
                        "<p>Bitte den Vorgang neu starten.</p>",
                    ),
                    status=400,
                    content_type="text/html",
                )

            had_existing_auth = auth_manager.has_saved_auth_record(
                twitch_user_id=twitch_user_id,
                twitch_login=twitch_login,
            )

            auth_manager.save_auth(
                twitch_user_id=twitch_user_id,
                twitch_login=twitch_login,
                access_token=access_token,
                refresh_token=refresh_token,
                expires_in=int(token_data.get("expires_in", 3600) or 3600),
                scopes=scopes,
                scope_profile=state_info.scope_profile,
                activate_raid_features=not had_existing_auth,
            )

            post_setup = getattr(raid_bot, "complete_setup_for_streamer", None)
            if callable(post_setup) and not had_existing_auth:
                asyncio.create_task(
                    post_setup(
                        twitch_user_id,
                        twitch_login,
                        state_discord_user_id=state_discord_user_id,
                        activate_partner_features=not had_existing_auth,
                    ),
                    name="twitch.raid.complete_setup",
                )

            log.info("Raid auth successful for %s", twitch_login)
            raise web.HTTPFound(location=self._raid_oauth_success_redirect_url())
        except web.HTTPException:
            raise
        except Exception:
            log.exception("Raid OAuth callback failed for state login=%s", login)
            return web.Response(
                text=self._render_oauth_page(
                    "Fehler bei der Autorisierung",
                    "<p>Beim Speichern der Twitch-Autorisierung ist ein interner Fehler aufgetreten.</p>"
                    "<p>Bitte den Vorgang erneut starten.</p>",
                ),
                status=500,
                content_type="text/html",
            )
        finally:
            if owns_session:
                await session.close()
