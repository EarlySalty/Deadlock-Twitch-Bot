"""Reusable HTML builders for dashboard route pages."""

from __future__ import annotations

import html
import json
from collections.abc import Iterable, Mapping, Sequence


def build_roadmap_body() -> str:
    return """
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
""".strip()


def build_scope_panel(
    *,
    twitch_login: str,
    missing_scopes: Sequence[str],
    missing_critical: Sequence[str],
    required_scopes: Sequence[str],
    critical_scopes: Iterable[str],
    scope_column_labels: Mapping[str, str],
) -> str:
    login = str(twitch_login or "").strip()
    if not login:
        return ""

    missing = [str(scope) for scope in missing_scopes if str(scope).strip()]
    if not missing:
        return (
            "<div style='background:#111827;border:1px solid #14532d;border-radius:12px;"
            "padding:14px 18px;margin-bottom:20px;display:flex;align-items:center;gap:10px;'>"
            "<span style='color:#4ade80;font-size:18px;'>✓</span>"
            "<span style='color:#86efac;font-size:14px;'>Alle OAuth-Scopes vorhanden</span>"
            "</div>"
        )

    critical_set = {str(scope).strip() for scope in critical_scopes}
    required_total = len(list(required_scopes))
    critical_count = len([scope for scope in missing_critical if str(scope).strip()])
    scope_items = "".join(
        f"<li style='margin-bottom:4px;'>"
        f"<span style='color:{'#f87171' if scope in critical_set else '#fbbf24'};margin-right:6px;'>"
        f"{'⚠' if scope in critical_set else '○'}</span>"
        f"<code style='font-size:12px;background:#1f2937;padding:1px 5px;border-radius:4px;'>{html.escape(scope)}</code>"
        f"<span style='color:#94a3b8;font-size:12px;margin-left:6px;'>{html.escape(scope_column_labels.get(scope, ''))}</span>"
        f"</li>"
        for scope in missing
    )
    critical_note = (
        f"<p style='color:#f87171;font-size:13px;margin-top:8px;'>"
        f"⚠ {critical_count} kritische Scope(s) fehlen — einige Features sind deaktiviert.</p>"
        if critical_count
        else ""
    )
    return (
        "<div style='background:#111827;border:1px solid #7f1d1d;border-radius:12px;"
        "padding:18px;margin-bottom:20px;'>"
        "<h3 style='margin:0 0 10px;color:#fca5a5;font-size:15px;'>Fehlende OAuth-Scopes</h3>"
        f"<p style='color:#94a3b8;font-size:13px;margin:0 0 10px;'>"
        f"Für <strong style='color:#e2e8f0;'>{html.escape(login)}</strong> fehlen "
        f"{len(missing)} von {required_total} Scopes. Bitte neu authentifizieren.</p>"
        f"<ul style='list-style:none;margin:0;padding:0;'>{scope_items}</ul>"
        f"{critical_note}"
        "</div>"
    )


def build_stats_entry_page(
    *,
    twitch_login: str,
    logout_url: str,
    legacy_url: str,
    beta_url: str,
    scope_panel: str,
) -> str:
    login = str(twitch_login or "").strip()
    login_json = json.dumps(login)
    return (
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
        f"<span class='user-chip'>{html.escape(login)}</span>"
        f"<a class='logout-btn' href='{logout_url}'>Logout</a>"
        "</div></div>"
        f"<h2 class='welcome'>Willkommen, {html.escape(login)}!</h2>"
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
            if login.lower() == "earlysalty"
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
        f"  const login = {login_json};"
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


def build_market_research_page() -> str:
    return """
<!DOCTYPE html>
<html lang="de">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Deadlock Market Research (Internal)</title>
    <script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.4/dist/chart.umd.min.js" integrity="sha384-NrKB+u6Ts6AtkIhwPixiKTzgSKNblyhlk0Sohlgar9UHUBzai/sgnNNWWd291xqt" crossorigin="anonymous"></script>
    <style>
        body { font-family: 'Segoe UI', sans-serif; background: #0f172a; color: #e2e8f0; margin: 0; padding: 20px; }
        .container { max-width: 1400px; margin: 0 auto; }
        h1 { color: #f8fafc; border-bottom: 1px solid #334155; padding-bottom: 10px; }
        .card { background: #1e293b; border-radius: 8px; padding: 20px; margin-bottom: 20px; border: 1px solid #334155; }
        .grid-2 { display: grid; grid-template-columns: 1fr 1fr; gap: 20px; }
        table { width: 100%; border-collapse: collapse; margin-top: 10px; }
        th, td { text-align: left; padding: 12px; border-bottom: 1px solid #334155; }
        th { color: #94a3b8; font-weight: 600; text-transform: uppercase; font-size: 0.85rem; }
        tr:hover { background: #334155; }
        .badge { padding: 4px 8px; border-radius: 4px; font-size: 0.8rem; font-weight: 600; }
        .badge-live { background: #ef4444; color: white; }
        .stat-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 20px; margin-bottom: 20px; }
        .stat-box { background: #0f172a; padding: 15px; border-radius: 6px; text-align: center; border: 1px solid #334155; }
        .stat-val { font-size: 2rem; font-weight: bold; color: #38bdf8; }
        .stat-label { color: #94a3b8; font-size: 0.9rem; }
        .progress-bar { background: #334155; height: 8px; border-radius: 4px; overflow: hidden; margin-top: 5px; }
        .progress-fill { height: 100%; background: #38bdf8; }
        .sentiment-pos { color: #4ade80; }
        .sentiment-neg { color: #f87171; }
        .question-item { border-left: 4px solid #38bdf8; padding: 10px; margin-bottom: 10px; background: #0f172a; border-radius: 0 4px 4px 0; }
        .question-meta { font-size: 0.8rem; color: #94a3b8; margin-top: 5px; }
    </style>
</head>
<body>
    <div class="container">
        <h1>Deadlock DACH Market Research 🕵️‍♂️</h1>

        <div class="stat-grid" id="kpi">
            <!-- Loaded via JS -->
        </div>

        <div class="card">
            <h2>📈 Market Volume (24h)</h2>
            <div style="height: 300px; position: relative;">
                <canvas id="marketChart"></canvas>
            </div>
        </div>

        <div class="grid-2">
            <div class="card">
                <h2>🔥 Meta Snapshot (Top Mentions 1h)</h2>
                <table id="meta-table">
                    <thead><tr><th>Term</th><th>Mentions</th><th>Trend</th></tr></thead>
                    <tbody></tbody>
                </table>
            </div>
            <div class="card">
                <h2>🌡️ Sentiment Analysis</h2>
                <div id="sentiment-chart" style="padding: 20px; text-align: center;"></div>
            </div>
        </div>

        <div class="grid-2">
            <div class="card">
                <h2>🕸️ Viewer Overlap (Shared Chatters)</h2>
                <table id="overlap-table">
                    <thead><tr><th>Streamer A</th><th>Streamer B</th><th>Shared Users</th></tr></thead>
                    <tbody></tbody>
                </table>
            </div>
            <div class="card">
                <h2>❓ Question Radar (Latest)</h2>
                <div id="questions" style="max-height: 400px; overflow-y: auto; padding-right: 10px;">
                    <!-- Questions go here -->
                </div>
            </div>
        </div>

        <div class="card">
            <h2>Live Monitored Channels</h2>
            <table id="channels">
                <thead>
                    <tr>
                        <th>Streamer</th>
                        <th>Viewers</th>
                        <th>Chat Activity</th>
                        <th>Lurker %</th>
                        <th>Msg/Min</th>
                        <th>Top Topic</th>
                    </tr>
                </thead>
                <tbody></tbody>
            </table>
        </div>
    </div>

    <script>
        let marketChart = null;
        const escapeHtml = (value) => String(value ?? '')
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');

        const showError = (msg) => {
            const kpi = document.getElementById('kpi');
            if (kpi) {
                kpi.innerHTML = `
                    <div class="stat-box">
                        <div class="stat-val" style="color:#f87171;">Fehler</div>
                        <div class="stat-label">${escapeHtml(msg)}</div>
                    </div>
                `;
            }
        };

        async function loadData() {
            const res = await fetch('/twitch/api/market_data');
            let data = null;
            try {
                data = await res.json();
            } catch (err) {
                console.error('market_data: invalid JSON', err);
                showError('Daten konnten nicht geladen werden.');
                return;
            }

            if (!res.ok || !data || data.error) {
                const msg = (data && data.error) ? data.error : `${res.status} ${res.statusText}`;
                console.error('market_data: request failed', msg);
                showError(msg);
                return;
            }

            const {
                total_monitored = 0,
                total_viewers = 0,
                avg_chat_health = 0,
                total_messages = 0,
                avg_lurker_ratio = 0,
                market_history = [],
                questions = [],
                meta_snapshot = [],
                sentiment = { positive: 0, negative: 0, neutral: 0, pos_pct: 0, neg_pct: 0, neu_pct: 0 },
                overlap = [],
                channels = [],
            } = data || {};

            const safeNumber = (val) => {
                const num = Number(val);
                return Number.isFinite(num) ? num : 0;
            };

            document.getElementById('kpi').innerHTML = `
                <div class="stat-box">
                    <div class="stat-val">${total_monitored}</div>
                    <div class="stat-label">Active Monitored Channels</div>
                </div>
                <div class="stat-box">
                    <div class="stat-val">${safeNumber(total_viewers).toLocaleString()}</div>
                    <div class="stat-label">Total Deadlock Viewers (DACH)</div>
                </div>
                <div class="stat-box">
                    <div class="stat-val">${safeNumber(avg_chat_health).toFixed(1)}%</div>
                    <div class="stat-label">Avg Chat Engagement</div>
                </div>
                <div class="stat-box">
                    <div class="stat-val">${safeNumber(total_messages).toLocaleString()}</div>
                    <div class="stat-label">Messages Analyzed (1h)</div>
                </div>
                <div class="stat-box">
                    <div class="stat-val">${safeNumber(avg_lurker_ratio).toFixed(1)}%</div>
                    <div class="stat-label">Avg Lurker Ratio</div>
                </div>
            `;

            const ctx = document.getElementById('marketChart').getContext('2d');
            const chartLabels = market_history.map(h => {
                const d = new Date(h.ts + 'Z');
                return d.getHours().toString().padStart(2, '0') + ':' + d.getMinutes().toString().padStart(2, '0');
            });

            const chartData = {
                labels: chartLabels,
                datasets: [
                    {
                        label: 'Total Viewers',
                        data: market_history.map(h => safeNumber(h.total_viewers)),
                        borderColor: '#38bdf8',
                        backgroundColor: 'rgba(56, 189, 248, 0.1)',
                        fill: true,
                        tension: 0.4
                    },
                    {
                        label: 'Streamer Count',
                        data: market_history.map(h => safeNumber(h.streamer_count) * 10),
                        borderColor: '#f472b6',
                        borderDash: [5, 5],
                        tension: 0.1,
                        yAxisID: 'y1'
                    }
                ]
            };

            if (marketChart) {
                marketChart.data = chartData;
                marketChart.update();
            } else {
                marketChart = new Chart(ctx, {
                    type: 'line',
                    data: chartData,
                    options: {
                        responsive: true,
                        maintainAspectRatio: false,
                        scales: {
                            y: { beginAtZero: true, grid: { color: '#334155' } },
                            y1: { position: 'right', beginAtZero: true, grid: { display: false } },
                            x: { grid: { display: false } }
                        },
                        plugins: { legend: { labels: { color: '#e2e8f0' } } }
                    }
                });
            }

            document.getElementById('questions').innerHTML = questions.map(q => `
                <div class="question-item">
                    <div>${escapeHtml(q.content)}</div>
                    <div class="question-meta">in @${escapeHtml(q.streamer)} • ${((q.ts || '').split('T')[1] || '').substring(0, 5) || '--:--'} Uhr</div>
                </div>
            `).join('');

            document.getElementById('meta-table').querySelector('tbody').innerHTML = meta_snapshot.map(m => `
                <tr>
                    <td><strong>${escapeHtml(m.term)}</strong></td>
                    <td>${safeNumber(m.count)}</td>
                    <td><div class="progress-bar"><div class="progress-fill" style="width: ${Math.min(100, safeNumber(m.count) * 2)}%"></div></div></td>
                </tr>
            `).join('');

            const sent = sentiment;
            document.getElementById('sentiment-chart').innerHTML = `
                <div style="display: flex; justify-content: space-around; font-size: 1.2rem;">
                    <div class="sentiment-pos">Positiv: ${sent.positive} (${sent.pos_pct}%)</div>
                    <div style="color: #94a3b8;">Neutral: ${sent.neutral} (${sent.neu_pct}%)</div>
                    <div class="sentiment-neg">Negativ: ${sent.negative} (${sent.neg_pct}%)</div>
                </div>
                <div style="display: flex; height: 20px; margin-top: 15px; border-radius: 10px; overflow: hidden;">
                    <div style="width: ${sent.pos_pct}%; background: #4ade80;"></div>
                    <div style="width: ${sent.neu_pct}%; background: #94a3b8;"></div>
                    <div style="width: ${sent.neg_pct}%; background: #f87171;"></div>
                </div>
            `;

            document.getElementById('overlap-table').querySelector('tbody').innerHTML = overlap.map(o => `
                <tr>
                    <td>${escapeHtml(o.a)}</td>
                    <td>${escapeHtml(o.b)}</td>
                    <td>${safeNumber(o.shared)}</td>
                </tr>
            `).join('');

            const tbody = document.querySelector('#channels tbody');
            tbody.innerHTML = channels.map(c => `
                <tr>
                    <td>
                        <strong>${escapeHtml(c.login)}</strong>
                        ${c.is_live ? '<span class="badge badge-live">LIVE</span>' : ''}
                    </td>
                    <td>${safeNumber(c.viewers)}</td>
                    <td>${safeNumber(c.chat_health).toFixed(1)}%</td>
                    <td>${safeNumber(c.lurker_ratio).toFixed(1)}%</td>
                    <td>${safeNumber(c.msg_per_min).toFixed(1)}</td>
                    <td>${escapeHtml(c.top_topic || '-')}</td>
                </tr>
            `).join('');
        }
        loadData();
        setInterval(loadData, 30000);
    </script>
</body>
</html>
""".strip()
