"""Reusable HTML builders for raid dashboard pages."""

from __future__ import annotations

import html
import json


def build_raid_auth_start_html(login: str, auth_url: str) -> str:
    safe_login = html.escape(login, quote=True)
    safe_auth_url = html.escape(auth_url, quote=True)
    return "".join(
        [
            "<html><head><title>Bot für deinen Kanal aktivieren</title></head>",
            "<body style='font-family: sans-serif; max-width: 680px; margin: 48px auto;'>",
            "<h1>Bot für deinen Kanal aktivieren</h1>",
            "<p>Streamer: <strong>",
            safe_login,
            "</strong></p>",
            "<p>Klicke auf den Link unten, um deinen Kanal mit dem Deadlock-Partnernetzwerk zu verbinden:</p>",
            "<p><a href='",
            safe_auth_url,
            "' style='padding: 10px 20px; background: #9146FF; color: white; text-decoration: none; border-radius: 5px;'>",
            "Bot für deinen Kanal aktivieren</a></p>",
            "<p style='color: #666; font-size: 0.9em;'>",
            "Wenn du Deadlock streamst und offline gehst, kann der Bot danach automatisch einen passenden Partner raiden.",
            "</p></body></html>",
        ]
    )


def build_raid_history_rows(history: list[dict]) -> str:
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


def build_raid_history_page(rows_html: str) -> str:
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


def build_raid_analytics_page(
    *,
    partner_stats: list,
    leechers: list,
    manual_list: list,
    date_min: str,
    date_max: str,
    total: int,
) -> str:
    labels = json.dumps([p["login"] for p in partner_stats])
    sent_data = json.dumps([p["sent"] for p in partner_stats])
    recv_data = json.dumps([p["received"] for p in partner_stats])

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

    if leechers:
        leecher_items = "".join(
            f"<li><strong>{html.escape(leecher['login'])}</strong> — {leecher['received']} Raids empfangen, 0 gesendet</li>"
            for leecher in leechers
        )
        leecher_html = f"<div class='alert-card'><h2>Keine Raids zurückgegeben <span class='badge badge-err'>{len(leechers)}</span></h2><ul>{leecher_items}</ul></div>"
    else:
        leecher_html = "<div class='alert-card alert-ok'><h2>Alle aktiven Partner haben bereits geraided ✓</h2></div>"

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
<script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.4/dist/chart.umd.min.js" integrity="sha384-NrKB+u6Ts6AtkIhwPixiKTzgSKNblyhlk0Sohlgar9UHUBzai/sgnNNWWd291xqt" crossorigin="anonymous"></script>
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
