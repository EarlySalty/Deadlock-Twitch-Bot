"""Route group for dashboard entry, admin, and utility handlers."""

from __future__ import annotations

from typing import Any

from aiohttp import web

from .live.live import DashboardLiveMixin


ROADMAP_BODY = """
<div class="hero">
  <div>
    <p class="eyebrow">Admin</p>
    <h1>Roadmap</h1>
    <p class="lead">Feature-Planung verwalten – Drag &amp; Drop zwischen den Spalten um den Status zu ändern.</p>
  </div>
</div>

<div id="kanban-root">
  <div class="kanban-board">
    <div class="kanban-col col-planned" id="col-planned" data-status="planned"
         ondragover="event.preventDefault();this.classList.add('drag-over')"
         ondragleave="this.classList.remove('drag-over')"
         ondrop="handleDrop(event,'planned')">
      <div class="kanban-col-header">
        <span class="kanban-col-title">Geplant</span>
        <span class="kanban-count" id="cnt-planned">0</span>
      </div>
      <div id="cards-planned"></div>
    </div>
    <div class="kanban-col col-in_progress" id="col-in_progress" data-status="in_progress"
         ondragover="event.preventDefault();this.classList.add('drag-over')"
         ondragleave="this.classList.remove('drag-over')"
         ondrop="handleDrop(event,'in_progress')">
      <div class="kanban-col-header">
        <span class="kanban-col-title">In Arbeit</span>
        <span class="kanban-count" id="cnt-in_progress">0</span>
      </div>
      <div id="cards-in_progress"></div>
    </div>
    <div class="kanban-col col-done" id="col-done" data-status="done"
         ondragover="event.preventDefault();this.classList.add('drag-over')"
         ondragleave="this.classList.remove('drag-over')"
         ondrop="handleDrop(event,'done')">
      <div class="kanban-col-header">
        <span class="kanban-col-title">Fertig</span>
        <span class="kanban-count" id="cnt-done">0</span>
      </div>
      <div id="cards-done"></div>
    </div>
  </div>
</div>

<div class="add-item-form" id="add-form">
  <h3>Neues Feature hinzufügen</h3>
  <div class="form-row">
    <label>Titel<input type="text" id="new-title" placeholder="Feature-Titel" /></label>
    <label>Status
      <select id="new-status">
        <option value="planned">Geplant</option>
        <option value="in_progress">In Arbeit</option>
        <option value="done">Fertig</option>
      </select>
    </label>
    <label>Priorität (höher = oben)<input type="number" id="new-priority" value="0" style="width:100%" /></label>
  </div>
  <label>Beschreibung (optional)<textarea id="new-desc" rows="2" placeholder="Kurze Beschreibung..."></textarea></label>
  <div class="form-actions">
    <button class="btn" onclick="addItem()">Hinzufügen</button>
    <span id="add-status" style="font-size:.85rem;color:var(--muted)"></span>
  </div>
</div>

<script>
let dragId = null;
const STATUSES = ['planned','in_progress','done'];

async function loadRoadmap() {
  try {
    const res = await fetch('/twitch/api/v2/roadmap');
    const data = await res.json();
    STATUSES.forEach(s => {
      const container = document.getElementById('cards-' + s);
      const count = document.getElementById('cnt-' + s);
      const items = data[s] || [];
      count.textContent = items.length;
      container.innerHTML = '';
      if (items.length === 0) {
        container.innerHTML = '<div class="kanban-empty">Keine Einträge</div>';
        return;
      }
      items.forEach(item => container.appendChild(makeCard(item)));
    });
  } catch(e) {
    console.error('Roadmap laden fehlgeschlagen', e);
  }
}

function makeCard(item) {
  const card = document.createElement('div');
  card.className = 'kanban-card';
  card.draggable = true;
  card.dataset.id = item.id;
  card.innerHTML = `
    <button class="kanban-card-delete" title="Löschen" onclick="deleteItem(${item.id})">✕</button>
    <p class="kanban-card-title">${escHtml(item.title)}</p>
    ${item.description ? `<p class="kanban-card-desc">${escHtml(item.description)}</p>` : ''}
  `;
  card.addEventListener('dragstart', () => {
    dragId = item.id;
    card.classList.add('dragging');
  });
  card.addEventListener('dragend', () => card.classList.remove('dragging'));
  return card;
}

function escHtml(str) {
  return String(str || '').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}

async function handleDrop(event, newStatus) {
  event.preventDefault();
  event.currentTarget.classList.remove('drag-over');
  if (!dragId) return;
  const id = dragId;
  dragId = null;
  try {
    const res = await fetch('/twitch/api/v2/roadmap/' + id, {
      method: 'PATCH',
      headers: {'Content-Type':'application/json'},
      body: JSON.stringify({status: newStatus})
    });
    if (!res.ok) throw new Error(await res.text());
    loadRoadmap();
  } catch(e) {
    console.error('Drag-Drop Fehler', e);
    loadRoadmap();
  }
}

async function deleteItem(id) {
  if (!confirm('Eintrag wirklich löschen?')) return;
  try {
    await fetch('/twitch/api/v2/roadmap/' + id, { method: 'DELETE' });
    loadRoadmap();
  } catch(e) { console.error(e); }
}

async function addItem() {
  const title = document.getElementById('new-title').value.trim();
  if (!title) { alert('Titel fehlt'); return; }
  const status = document.getElementById('new-status').value;
  const priority = parseInt(document.getElementById('new-priority').value) || 0;
  const desc = document.getElementById('new-desc').value.trim();
  const statusEl = document.getElementById('add-status');
  try {
    statusEl.textContent = 'Speichern...';
    const res = await fetch('/twitch/api/v2/roadmap', {
      method: 'POST',
      headers: {'Content-Type':'application/json'},
      body: JSON.stringify({title, status, priority, description: desc || null})
    });
    if (!res.ok) throw new Error(await res.text());
    document.getElementById('new-title').value = '';
    document.getElementById('new-desc').value = '';
    document.getElementById('new-priority').value = '0';
    statusEl.textContent = 'Gespeichert!';
    setTimeout(() => statusEl.textContent = '', 2000);
    loadRoadmap();
  } catch(e) {
    statusEl.textContent = 'Fehler: ' + e.message;
    console.error(e);
  }
}

loadRoadmap();
</script>
"""


def build_route_defs(server: Any) -> list[web.RouteDef]:
    """Return route definitions for entry, admin, and utility routes."""
    return [
        web.get("/", server.public_home),
        web.get("/dashboads", server.legacy_dashboard_redirect),
        web.get("/dashboards", server.legacy_dashboard_redirect),
        web.get("/twitch", server.index),
        web.get("/twitch/", server.index),
        web.get("/twitch/admin", server.admin),
        web.get("/twitch/admin/announcements", server.admin_announcements_page),
        web.post("/twitch/admin/announcements", server.admin_announcements_save),
        web.get("/twitch/admin/roadmap", server.admin_roadmap_page),
        web.get("/twitch/live", server.admin),
        web.get("/twitch/live-announcement", server.live_announcement_page),
        web.post("/twitch/add_any", server.add_any),
        web.post("/twitch/add_url", server.add_url),
        web.post("/twitch/add_login/{login}", server.add_login),
        web.post("/twitch/add_streamer", server.add_streamer),
        web.post("/twitch/admin/chat_action", server.admin_partner_chat_action),
        web.post("/twitch/admin/manual-plan", server.admin_manual_plan_save),
        web.post("/twitch/admin/manual-plan/clear", server.admin_manual_plan_clear),
        web.post("/twitch/remove", server.remove),
        web.post("/twitch/verify", server.verify),
        web.post("/twitch/archive", server.archive),
        web.post("/twitch/discord_flag", server.discord_flag),
        web.get("/twitch/stats", server.stats),
        web.get("/twitch/partners", server.partner_stats),
        web.get("/twitch/dashboads", server.legacy_dashboard_redirect),
        web.get("/twitch/dashboards", server.legacy_dashboard_redirect),
        web.get("/twitch/auth/logout", server.auth_logout),
        web.post("/twitch/discord_link", server.discord_link),
        web.post("/twitch/reload", server.reload_cog),
    ]


async def index(server: Any, request: web.Request) -> web.StreamResponse:
    """Entrypoint with local-first admin behavior."""
    if server._is_local_request(request) or server._is_discord_admin_request(request):
        destination = "/twitch/admin"
        fallback = "/twitch/admin"
    else:
        destination = "/twitch/dashboard"
        fallback = "/twitch/dashboard"
    if request.query_string:
        destination = f"{destination}?{request.query_string}"
    safe_destination = server._safe_internal_redirect(destination, fallback=fallback)
    raise web.HTTPFound(safe_destination)


async def public_home(server: Any, request: web.Request) -> web.StreamResponse:
    """Root entrypoint redirects to admin or canonical dashboard landing."""
    if server._is_local_request(request) or server._is_discord_admin_request(request):
        destination = "/twitch/admin"
        fallback = "/twitch/admin"
    else:
        destination = "/twitch/dashboard"
        fallback = "/twitch/dashboard"
    if request.query_string:
        destination = f"{destination}?{request.query_string}"
    safe_destination = server._safe_internal_redirect(destination, fallback=fallback)
    raise web.HTTPFound(safe_destination)


async def legacy_dashboard_redirect(server: Any, request: web.Request) -> web.StreamResponse:
    """Redirect legacy dashboard paths to the canonical dashboard landing."""
    destination = "/twitch/dashboard"
    if request.query_string:
        destination = f"{destination}?{request.query_string}"
    safe_destination = server._safe_internal_redirect(destination, fallback="/twitch/dashboard")
    raise web.HTTPFound(safe_destination)


async def admin(server: Any, request: web.Request) -> web.StreamResponse:
    """Legacy partner admin surface."""
    return await DashboardLiveMixin.index(server, request)


async def stats_entry(
    server: Any,
    request: web.Request,
    *,
    deps: dict[str, Any],
) -> web.StreamResponse:
    """Canonical public entrypoint that links old and beta analytics dashboards."""
    html_module = deps["html"]
    json_module = deps["json"]
    log = deps["log"]
    storage_module = deps["storage"]
    critical_scopes = deps["critical_scopes"]
    required_scopes = deps["required_scopes"]
    scope_column_labels = deps["scope_column_labels"]
    dashboards_discord_login_url = deps["dashboards_discord_login_url"]
    dashboards_login_url = deps["dashboards_login_url"]

    if not server._check_v2_auth(request):
        login_url = (
            dashboards_discord_login_url
            if server._should_use_discord_admin_login(request)
            else dashboards_login_url
        )
        response = server._dashboard_auth_redirect_or_unavailable(
            request,
            next_path="/twitch/dashboard",
            fallback_login_url=login_url,
        )
        if isinstance(response, web.HTTPException):
            raise response
        return response

    legacy_url = server._resolve_legacy_stats_url()
    beta_url = "/twitch/dashboard-v2"
    logout_url = (
        "/twitch/auth/discord/logout"
        if server._is_discord_admin_request(request)
        else "/twitch/auth/logout"
    )

    session = server._get_dashboard_auth_session(request)
    twitch_login = (session or {}).get("twitch_login", "")
    missing_scopes: list[str] = []
    missing_critical: list[str] = []
    if twitch_login:
        try:
            with storage_module.readonly_connection() as conn:
                row = conn.execute(
                    "SELECT scopes FROM twitch_raid_auth WHERE LOWER(twitch_login) = LOWER(%s)",
                    [twitch_login],
                ).fetchone()
            if row:
                token_scopes = set((row[0] or "").split())
                missing_scopes = [scope for scope in required_scopes if scope not in token_scopes]
                missing_critical = [scope for scope in missing_scopes if scope in critical_scopes]
            else:
                missing_scopes = list(required_scopes)
                missing_critical = [scope for scope in required_scopes if scope in critical_scopes]
        except Exception:
            log.exception("stats_entry: failed to load scopes for %s", twitch_login)

    if twitch_login and missing_scopes:
        scope_items = "".join(
            f"<li style='margin-bottom:4px;'>"
            f"<span style='color:{'#f87171' if scope in critical_scopes else '#fbbf24'};margin-right:6px;'>"
            f"{'⚠' if scope in critical_scopes else '○'}</span>"
            f"<code style='font-size:12px;background:#1f2937;padding:1px 5px;border-radius:4px;'>{html_module.escape(scope)}</code>"
            f"<span style='color:#94a3b8;font-size:12px;margin-left:6px;'>{html_module.escape(scope_column_labels.get(scope, ''))}</span>"
            f"</li>"
            for scope in missing_scopes
        )
        critical_note = (
            f"<p style='color:#f87171;font-size:13px;margin-top:8px;'>"
            f"⚠ {len(missing_critical)} kritische Scope(s) fehlen — einige Features sind deaktiviert.</p>"
            if missing_critical
            else ""
        )
        scope_panel = (
            "<div style='background:#111827;border:1px solid #7f1d1d;border-radius:12px;"
            "padding:18px;margin-bottom:20px;'>"
            "<h3 style='margin:0 0 10px;color:#fca5a5;font-size:15px;'>Fehlende OAuth-Scopes</h3>"
            f"<p style='color:#94a3b8;font-size:13px;margin:0 0 10px;'>"
            f"Für <strong style='color:#e2e8f0;'>{html_module.escape(twitch_login)}</strong> fehlen "
            f"{len(missing_scopes)} von {len(required_scopes)} Scopes. "
            f"Bitte neu authentifizieren.</p>"
            f"<ul style='list-style:none;margin:0;padding:0;'>{scope_items}</ul>"
            f"{critical_note}"
            "</div>"
        )
    elif twitch_login:
        scope_panel = (
            "<div style='background:#111827;border:1px solid #14532d;border-radius:12px;"
            "padding:14px 18px;margin-bottom:20px;display:flex;align-items:center;gap:10px;'>"
            "<span style='color:#4ade80;font-size:18px;'>✓</span>"
            "<span style='color:#86efac;font-size:14px;'>Alle OAuth-Scopes vorhanden</span>"
            "</div>"
        )
    else:
        scope_panel = ""

    page_html = (
        "<!doctype html><html lang='de'><head><meta charset='utf-8'>"
        "<meta name='viewport' content='width=device-width,initial-scale=1'>"
        "<title>Twitch Analytics</title>"
        "<style>"
        "* { box-sizing: border-box; }"
        "body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif; "
        "background: #0f172a; color: #e2e8f0; margin: 0; line-height: 1.5; }"
        ".wrap { max-width: 1200px; margin: 0 auto; padding: 24px 18px; }"
        ".header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 32px; "
        "padding-bottom: 20px; border-bottom: 1px solid #1f2937; }"
        ".header-left { display: flex; align-items: center; gap: 12px; }"
        ".logo { width: 32px; height: 32px; background: #9333ea; border-radius: 50%; display: flex; "
        "align-items: center; justify-content: center; font-weight: bold; color: #fff; }"
        ".header-title { font-size: 22px; font-weight: 600; color: #e2e8f0; margin: 0; }"
        ".header-right { display: flex; align-items: center; gap: 16px; }"
        ".user-chip { background: #1f2937; border: 1px solid #374151; padding: 8px 14px; border-radius: 20px; "
        "font-size: 14px; color: #e2e8f0; }"
        ".logout-btn { color: #60a5fa; text-decoration: none; font-size: 14px; cursor: pointer; "
        "padding: 8px 12px; border: none; background: none; transition: color 0.2s; }"
        ".logout-btn:hover { color: #93c5fd; }"
        ".welcome { font-size: 20px; font-weight: 600; margin: 0 0 24px; color: #e2e8f0; }"
        ".kpi-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); "
        "gap: 16px; margin-bottom: 32px; }"
        ".kpi-tile { background: #111827; border: 1px solid #1f2937; border-radius: 12px; padding: 20px; }"
        ".kpi-label { font-size: 13px; color: #94a3b8; text-transform: uppercase; letter-spacing: 0.5px; "
        "margin-bottom: 12px; font-weight: 500; }"
        ".kpi-value { font-size: 32px; font-weight: 700; color: #e2e8f0; margin: 0; }"
        ".kpi-trend { font-size: 12px; color: #4ade80; margin-top: 8px; }"
        ".skeleton { background: linear-gradient(90deg, #1f2937 25%, #2d3748 50%, #1f2937 75%); "
        "background-size: 200% 100%; animation: shimmer 1.5s infinite; border-radius: 6px; height: 40px; "
        "margin-top: 8px; }"
        "@keyframes shimmer { 0% { background-position: 200% 0; } 100% { background-position: -200% 0; } }"
        ".nav-cards { display: grid; grid-template-columns: repeat(2, 1fr); "
        "gap: 20px; margin-bottom: 32px; }"
        "@media (max-width: 768px) { .nav-cards { grid-template-columns: 1fr; } }"
        ".card { background: #111827; border: 1px solid #1f2937; border-radius: 12px; padding: 28px; "
        "transition: transform 0.2s, box-shadow 0.2s, border-color 0.2s; min-height: 220px; "
        "display: flex; flex-direction: column; }"
        ".card:hover { transform: translateY(-2px); box-shadow: 0 12px 32px rgba(0, 0, 0, 0.4); "
        "border-color: #374151; }"
        ".card .btn { margin-top: auto; }"
        ".card-accent-purple { border-left: 4px solid #9333ea; }"
        ".card-accent-blue { border-left: 4px solid #2563eb; }"
        ".card-accent-teal { border-left: 4px solid #14b8a6; }"
        ".card-title { font-size: 18px; font-weight: 600; margin: 0 0 8px; color: #e2e8f0; }"
        ".card-badge { display: inline-block; background: #1f2937; color: #fbbf24; font-size: 11px; "
        "padding: 4px 8px; border-radius: 4px; margin-bottom: 12px; font-weight: 600; }"
        ".card-desc { color: #94a3b8; font-size: 14px; margin: 12px 0; }"
        ".card-bullets { list-style: none; padding: 0; margin: 12px 0; }"
        ".card-bullets li { color: #cbd5e1; font-size: 13px; margin-bottom: 8px; padding-left: 20px; "
        "position: relative; }"
        ".card-bullets li:before { content: '•'; position: absolute; left: 8px; color: #64748b; }"
        ".btn { display: inline-block; margin-top: 16px; padding: 12px 18px; border-radius: 8px; "
        "text-decoration: none; background: #2563eb; color: #fff; font-weight: 600; font-size: 14px; "
        "cursor: pointer; border: none; transition: all 0.2s; }"
        ".btn:hover { background: #1d4ed8; transform: translateX(2px); }"
        ".btn:active { transform: translateX(0); }"
        ".insights-panel { background: linear-gradient(135deg, #1f2937 0%, #111827 100%); "
        "border: 1px solid #1f2937; border-radius: 12px; padding: 24px; border-left: 4px solid #8b5cf6; "
        "margin-top: 32px; }"
        ".insights-title { font-size: 16px; font-weight: 600; color: #e2e8f0; margin: 0 0 16px; display: flex; "
        "align-items: center; gap: 8px; }"
        ".insights-list { list-style: none; padding: 0; margin: 0; }"
        ".insights-list li { color: #cbd5e1; font-size: 14px; margin-bottom: 8px; display: flex; "
        "align-items: flex-start; gap: 10px; }"
        ".insights-list li:before { content: '💡'; font-size: 16px; flex-shrink: 0; }"
        ".hidden { display: none; }"
        "</style></head><body><div class='wrap'>"
        "<div class='header'>"
        "<div class='header-left'><div class='logo'>◉</div>"
        "<h1 class='header-title'>Twitch Analytics</h1></div>"
        "<div class='header-right'>"
        f"<span class='user-chip'>{html_module.escape(twitch_login)}</span>"
        f"<a class='logout-btn' href='{logout_url}'>Logout</a>"
        "</div></div>"
        f"<h2 class='welcome'>Willkommen, {html_module.escape(twitch_login)}!</h2>"
        f"{scope_panel}"
        "<div class='kpi-grid'>"
        "<div class='kpi-tile'><div class='kpi-label'>Ø Viewer</div>"
        "<p class='kpi-value' id='kpi-viewers'>—</p><div class='skeleton' id='skeleton-viewers'></div>"
        "<div id='trend-viewers' class='kpi-trend'></div></div>"
        "<div class='kpi-tile'><div class='kpi-label'>Streams (30 Tage)</div>"
        "<p class='kpi-value' id='kpi-streams'>—</p><div class='skeleton' id='skeleton-streams'></div>"
        "<div id='trend-streams' class='kpi-trend'></div></div>"
        "<div class='kpi-tile'><div class='kpi-label'>Neue Follower</div>"
        "<p class='kpi-value' id='kpi-followers'>—</p><div class='skeleton' id='skeleton-followers'></div>"
        "<div id='trend-followers' class='kpi-trend'></div></div>"
        "<div class='kpi-tile'><div class='kpi-label'>Retention</div>"
        "<p class='kpi-value' id='kpi-retention'>—</p><div class='skeleton' id='skeleton-retention'></div>"
        "<div id='trend-retention' class='kpi-trend'></div></div>"
        "</div>"
        "<div class='nav-cards'>"
        "<div class='card card-accent-purple'>"
        "<span class='card-badge'>BETA</span>"
        "<h3 class='card-title'>📊 Analyse Dashboard</h3>"
        "<p class='card-desc'>Umfangreiche Analyse deiner Stream-Performance mit erweiterten Insights.</p>"
        "<ul class='card-bullets'>"
        "<li>Retention & Raid-Tracking</li>"
        "<li>Zuschauer-Rankings</li>"
        "<li>Trendanalysen</li>"
        "</ul>"
        f"<a class='btn' href='{beta_url}'>Öffnen →</a>"
        "</div>"
        "<div class='card card-accent-blue'>"
        "<h3 class='card-title'>📈 Stats (Alt)</h3>"
        "<p class='card-desc'>Klassisches Dashboard mit detaillierten Statistiken und Logs.</p>"
        "<ul class='card-bullets'>"
        "<li>Viewer-Verlauf</li>"
        "<li>Stream-Logs</li>"
        "</ul>"
        f"<a class='btn' href='{legacy_url}'>Öffnen →</a>"
        "</div>"
        "<div class='card card-accent-teal'>"
        "<h3 class='card-title'>🎨 Live Message Builder</h3>"
        "<p class='card-desc'>Baue deine Go-Live Nachricht mit Text, Embed, Feldern und Button inklusive Live-Vorschau.</p>"
        "<ul class='card-bullets'>"
        "<li>Placeholder-System ({channel}, {title}, {viewer_count})</li>"
        "<li>Rollen-Ping & Allowed Mentions</li>"
        "<li>Testversand per Discord-DM</li>"
        "</ul>"
        "<a class='btn' href='/twitch/live-announcement'>Öffnen →</a>"
        "</div>"
        + (
            "<div class='card card-accent-teal' style='grid-column: span 2; opacity: 0.7; border-color: #334155;'>"
            "<span class='card-badge' style='background: #1e293b; color: #64748b;'>GEPLANT</span>"
            "<h3 class='card-title' style='color: #94a3b8;'>📱 Social Media Publisher</h3>"
            "<p class='card-desc'>Verwalte deine Twitch-Clips und veröffentliche auf TikTok, YouTube & Instagram.</p>"
            "<p style='color: #64748b; font-size: 13px; margin-top: 12px;'>✨ Kommendes Feature — wird in Kürze verfügbar sein</p>"
            "</div>"
            if twitch_login.lower() == "earlysalty"
            else ""
        )
        + "</div>"
        "<div class='insights-panel hidden' id='insights-panel'>"
        "<h3 class='insights-title'>💡 Insights</h3>"
        "<ul class='insights-list' id='insights-list'></ul>"
        "</div>"
        "</div>"
        "<script>"
        "async function loadStats() {"
        f"  const login = {json_module.dumps(twitch_login)};"
        "  try {"
        "    const res = await fetch(`/twitch/api/v2/overview?streamer=${login}&days=30`);"
        "    if (!res.ok) throw new Error(`HTTP ${res.status}`);"
        "    const data = await res.json();"
        "    if (data.error || !data.overview) return;"
        "    const o = data.overview;"
        "    const hideSkeletons = () => {"
        "      ['viewers', 'streams', 'followers', 'retention'].forEach(k => {"
        "        const skel = document.getElementById(`skeleton-${k}`);"
        "        if (skel) skel.style.display = 'none';"
        "      });"
        "    };"
        "    if (o.avg_viewers != null) {"
        "      const rounded = Math.round(o.avg_viewers);"
        "      document.getElementById('kpi-viewers').textContent = rounded.toLocaleString('de-DE');"
        "      if (o.avg_viewers_trend != null && o.avg_viewers_trend !== 0) {"
        "        const trend = o.avg_viewers_trend > 0 ? '▲' : '▼';"
        "        const color = o.avg_viewers_trend > 0 ? '#4ade80' : '#f87171';"
        "        const sign = o.avg_viewers_trend > 0 ? '+' : '';"
        "        const elem = document.getElementById('trend-viewers');"
        "        elem.textContent = `${trend} ${sign}${o.avg_viewers_trend.toFixed(1)}%`;"
        "        elem.style.color = color;"
        "      }"
        "    }"
        "    if (o.streams_count != null) {"
        "      document.getElementById('kpi-streams').textContent = o.streams_count.toString();"
        "    }"
        "    if (o.new_followers != null) {"
        "      document.getElementById('kpi-followers').textContent = o.new_followers.toLocaleString('de-DE');"
        "    }"
        "    if (o.retention != null) {"
        "      const pct = Math.round(o.retention * 100);"
        "      document.getElementById('kpi-retention').textContent = pct + '%';"
        "    }"
        "    hideSkeletons();"
        "    if (data.findings && Array.isArray(data.findings) && data.findings.length > 0) {"
        "      const list = document.getElementById('insights-list');"
        "      data.findings.slice(0, 2).forEach(f => {"
        "        if (f) {"
        "          const li = document.createElement('li');"
        "          li.textContent = f;"
        "          list.appendChild(li);"
        "        }"
        "      });"
        "      document.getElementById('insights-panel').classList.remove('hidden');"
        "    }"
        "  } catch (err) {"
        "    console.error('Failed to load stats:', err);"
        "  }"
        "}"
        "document.addEventListener('DOMContentLoaded', loadStats);"
        "</script>"
        "</body></html>"
    )
    return web.Response(text=page_html, content_type="text/html")


async def auth_logout(
    server: Any,
    request: web.Request,
    *,
    deps: dict[str, Any],
) -> web.StreamResponse:
    """Logout and clear dashboard session cookie."""
    log = deps["log"]
    dashboard_v2_login_url = deps["dashboard_v2_login_url"]

    session_id = (request.cookies.get(server._session_cookie_name) or "").strip()
    if session_id:
        session = server._auth_sessions.pop(session_id, None)
        twitch_login = (session or {}).get("twitch_login", "unknown") if session else "unknown"
        log.info(
            "AUDIT dashboard logout: twitch=%s peer=%s",
            server._sanitize_log_value(twitch_login),
            server._sanitize_log_value(server._peer_host(request)),
        )
        try:
            from ..storage import sessions_db

            sessions_db.delete_session(session_id)
        except Exception as exc:
            log.debug("Could not delete dashboard session from DB: %s", exc)

    response = server._dashboard_auth_redirect_or_unavailable(
        request,
        next_path="/twitch/dashboard-v2",
        fallback_login_url=dashboard_v2_login_url,
    )
    server._clear_session_cookie(response, request)
    if isinstance(response, web.HTTPException):
        raise response
    return response


async def discord_link(
    server: Any,
    request: web.Request,
    *,
    deps: dict[str, Any],
) -> web.StreamResponse:
    """Persist Discord profile metadata from the stats dashboard."""
    log = deps["log"]

    server._require_token(request)
    if not callable(server._discord_profile):
        location = server._redirect_location(request, err="Discord-Link ist aktuell nicht verfügbar")
        safe_location = server._safe_internal_redirect(location, fallback="/twitch/stats")
        raise web.HTTPFound(location=safe_location)

    data = await request.post()
    csrf_token = str(data.get("csrf_token") or "").strip()
    if not server._csrf_verify_token(request, csrf_token):
        location = server._redirect_location(request, err="Ungültiges CSRF-Token")
        safe_location = server._safe_internal_redirect(location, fallback="/twitch/stats")
        raise web.HTTPFound(location=safe_location)

    login = (data.get("login") or "").strip()
    discord_user_id = (data.get("discord_user_id") or "").strip()
    discord_display_name = (data.get("discord_display_name") or "").strip()
    member_raw = (data.get("member_flag") or "").strip().lower()
    mark_member = member_raw in {"1", "true", "on", "yes"}

    try:
        message = await server._discord_profile(
            login,
            discord_user_id=discord_user_id or None,
            discord_display_name=discord_display_name or None,
            mark_member=mark_member,
        )
        location = server._redirect_location(request, ok=message)
    except ValueError as exc:
        location = server._redirect_location(request, err=str(exc))
    except Exception:
        log.exception("dashboard discord_link failed")
        location = server._redirect_location(
            request, err="Discord-Daten konnten nicht gespeichert werden"
        )

    safe_location = server._safe_internal_redirect(location, fallback="/twitch/stats")
    raise web.HTTPFound(location=safe_location)


async def reload_cog(
    server: Any,
    request: web.Request,
    *,
    deps: dict[str, Any],
) -> web.Response:
    """Optional reload endpoint for admin tooling compatibility."""
    log = deps["log"]

    await request.post()
    header_token = request.headers.get("X-Admin-Token")
    is_authorized = (
        server._is_local_request(request)
        or server._is_discord_admin_request(request)
        or server._check_admin_token(header_token)
    )
    if not is_authorized:
        log.warning(
            "AUDIT dashboard reload_cog: unauthorized attempt from peer=%s",
            server._sanitize_log_value(server._peer_host(request)),
        )
        return web.Response(text="Unauthorized", status=401)

    log.info(
        "AUDIT dashboard reload_cog: triggered by peer=%s",
        server._sanitize_log_value(server._peer_host(request)),
    )
    if server._reload_cb:
        msg = await server._reload_cb()
        return web.Response(text=msg)
    return web.Response(text="Kein Reload-Handler definiert", status=501)


async def admin_roadmap_page(server: Any, request: web.Request) -> web.StreamResponse:
    """Kanban board for managing roadmap items."""
    if not (server._is_local_request(request) or server._is_discord_admin_request(request)):
        raise web.HTTPFound("/twitch/admin")

    return web.Response(
        content_type="text/html",
        text=server._html(ROADMAP_BODY, "roadmap"),
    )
