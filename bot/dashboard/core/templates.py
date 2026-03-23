"""HTML helpers for the Twitch dashboard."""

from __future__ import annotations

import html


class DashboardTemplateMixin:
    def _tabs(self, active: str) -> str:
        def anchor(href: str, label: str, key: str, extra_cls: str = "") -> str:
            cls = f"tab{' active' if key == active else ''}{' ' + extra_cls if extra_cls else ''}"
            return f'<a class="{cls}" href="{href}">{label}</a>'

        return (
            '<nav class="tabs">'
            f"{anchor('/twitch/admin', 'Admin', 'live')}"
            f"{anchor('/twitch/admin/roadmap', 'Roadmap', 'roadmap')}"
            f"{anchor('/twitch/stats', 'Stats', 'stats')}"
            f"{anchor('/twitch/dashboard-v2', 'Analytics v2', 'v2')}"
            f"{anchor('/social-media', 'Social Media', 'social')}"
            "</nav>"
        )

    def _html(
        self,
        body: str,
        active: str,
        msg: str = "",
        err: str = "",
        nav: str | None = None,
    ) -> str:
        flash = ""
        if msg:
            flash = f'<div class="flash ok">{html.escape(msg)}</div>'
        elif err:
            flash = f'<div class="flash err">{html.escape(err)}</div>'
        nav_html = self._tabs(active) if nav is None else nav
        template = """
<!doctype html>
<meta charset="utf-8">
<title>Deadlock Twitch Posting – Admin</title>
<script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.4/dist/chart.umd.min.js" integrity="sha384-NrKB+u6Ts6AtkIhwPixiKTzgSKNblyhlk0Sohlgar9UHUBzai/sgnNNWWd291xqt" crossorigin="anonymous"></script>
<style>
  @import url('https://fonts.googleapis.com/css2?family=Sora:wght@500;600;700&family=Manrope:wght@400;500;600&display=swap');
  :root {{
    color-scheme: dark;
    --bg:#07151d; --bg-alt:#0b1d28; --card:#102635; --card-hover:#173448;
    --bd:rgba(194,221,240,0.14); --bd-hover:rgba(194,221,240,0.3);
    --text:#e9f1f7; --muted:#9bb3c5;
    --accent:#ff7a18; --accent-hover:#ff8d39;
    --teal:#10b7ad; --teal-hover:#1dd4ca;
    --ok-bg:rgba(46,204,113,0.12); --ok-bd:rgba(46,204,113,0.35); --ok-fg:#2ecc71;
    --err-bg:rgba(255,107,94,0.12); --err-bd:rgba(255,107,94,0.35); --err-fg:#ff6b5e;
    --warn-bg:rgba(245,182,66,0.12); --warn-bd:rgba(245,182,66,0.35); --warn-fg:#f5b642;
    --shadow:rgba(0,0,0,.35); --shadow-strong:rgba(0,0,0,.5);
    --chip-bg:rgba(255,122,24,.15);
  }}
  * {{ box-sizing: border-box; }}
  body {{
    font-family: "Manrope", "Segoe UI", sans-serif;
    width: 100%; max-width: none; margin: 0 auto;
    padding: 2.6rem 1.8rem 3.4rem;
    color: var(--text);
    background:
      radial-gradient(1200px 540px at 92% -10%, rgba(255,122,24,0.22), transparent 65%),
      radial-gradient(940px 500px at 9% -18%, rgba(16,183,173,0.25), transparent 60%),
      linear-gradient(160deg, #07151d 0%, #081a24 55%, #0a202c 100%);
    position: relative;
  }}
  body::before {{
    content:""; position:fixed; inset:0; pointer-events:none;
    background-image:
      linear-gradient(rgba(255,255,255,0.03) 1px, transparent 1px),
      linear-gradient(90deg, rgba(255,255,255,0.03) 1px, transparent 1px);
    background-size: 36px 36px;
    mask-image: radial-gradient(ellipse at top, black 40%, transparent 75%);
    opacity: 0.3; z-index: 0;
  }}
  body > * {{ position: relative; z-index: 1; }}
  h1, h2, h3 {{ font-family: "Sora", "Segoe UI", sans-serif; letter-spacing: -0.02em; }}
  h1 {{ font-size: 2.2rem; margin: 0; }}
  h2 {{ font-size: 1.2rem; }}
  a {{ color: inherit; }}
  .tabs {{ display:flex; gap:.6rem; margin:0 0 1.8rem 0; flex-wrap:wrap; align-items:center; }}
  .tab {{ padding:.5rem .9rem; border-radius:999px; text-decoration:none; color:var(--muted); background:rgba(16,38,53,0.7); border:1px solid var(--bd); font-weight:600; font-size:.875rem; transition:all .15s ease; }}
  .tab:hover {{ color:var(--text); border-color:var(--bd-hover); background:var(--card-hover); }}
  .tab.active {{ background:linear-gradient(135deg, rgba(255,122,24,.25), rgba(255,122,24,.06)); border-color:rgba(255,122,24,.5); color:var(--accent); }}
  .card {{ background:linear-gradient(160deg, rgba(16,38,53,.92), rgba(10,30,42,.92)); border:1px solid var(--bd); border-radius:1rem; padding:1.2rem; box-shadow:0 10px 30px var(--shadow); }}
  .row {{ display:flex; gap:1.2rem; align-items:center; flex-wrap:wrap; }}
  .btn {{ background:var(--accent); color:white; border:none; padding:.55rem .9rem; border-radius:.65rem; cursor:pointer; font-weight:600; letter-spacing:.01em; box-shadow:0 8px 20px rgba(255,122,24,.3); transition:all .15s ease; font-family:inherit; }}
  .btn:hover {{ background:var(--accent-hover); transform: translateY(-1px); box-shadow:0 12px 24px rgba(255,122,24,.4); }}
  .btn:disabled {{ opacity:.6; cursor:not-allowed; box-shadow:none; transform:none; }}
  .btn-small {{ padding:.35rem .6rem; font-size:.85rem; }}
  .btn-secondary {{ background:var(--bg-alt); color:var(--teal); border:1px solid var(--bd); box-shadow:none; }}
  .btn-secondary:hover {{ background:var(--card); border-color:var(--bd-hover); box-shadow:none; transform:none; }}
  .btn-danger {{ background:#c0392b; box-shadow:none; }}
  .btn-danger:hover {{ background:#e74c3c; box-shadow:none; }}
  .btn-warn {{ background:var(--warn-fg); color:#000; box-shadow:none; }}
  .btn-ghost {{ background:transparent; color:var(--accent); border:1px dashed var(--bd); box-shadow:none; }}
  .btn-ghost:hover {{ background:rgba(255,122,24,0.08); box-shadow:none; transform:none; }}
  .btn-teal {{ background:var(--teal); color:#fff; box-shadow:0 8px 20px rgba(16,183,173,.25); }}
  .btn-teal:hover {{ background:var(--teal-hover); box-shadow:0 12px 24px rgba(16,183,173,.35); }}
  table {{ width:100%; border-collapse: collapse; }}
  th, td {{ border-bottom:1px solid var(--bd); padding:.75rem .6rem; text-align:left; vertical-align: top; }}
  th {{ color:var(--teal); text-transform:uppercase; letter-spacing:.08em; font-size:.72rem; font-weight:700; }}
  tr:hover td {{ background:rgba(255,122,24,0.05); }}
  input[type="text"], input[type="number"], textarea, select {{
    background:var(--bg-alt); border:1px solid var(--bd); color:var(--text);
    padding:.5rem .65rem; border-radius:.6rem; font-family:inherit; font-size:.9rem;
    transition:border-color .15s ease;
  }}
  input[type="text"] {{ width:100%; min-width:0; max-width:100%; }}
  input:focus, textarea:focus, select:focus {{ outline:none; border-color:rgba(255,122,24,.5); }}
  input:disabled, select:disabled, textarea:disabled {{ opacity:.65; cursor:not-allowed; }}
  textarea {{ width:100%; min-width:0; max-width:100%; resize:vertical; }}
  small {{ color:var(--muted); }}
  .flash {{ margin:.7rem 0; padding:.65rem .9rem; border-radius:.7rem; font-size:.9rem; }}
  .flash.ok {{ background:var(--ok-bg); border:1px solid var(--ok-bd); color:var(--ok-fg); }}
  .flash.err {{ background:var(--err-bg); border:1px solid var(--err-bd); color:var(--err-fg); }}
  form.inline {{ display:inline; }}
  .card-header {{ display:flex; justify-content:space-between; align-items:center; gap:1.2rem; flex-wrap:wrap; }}
  .badge {{ display:inline-flex; align-items:center; gap:.35rem; padding:.2rem .6rem; border-radius:999px; font-size:.75rem; font-weight:700; border:1px solid var(--bd); background:var(--bg-alt); }}
  .badge-ok {{ background:var(--ok-bg); color:var(--ok-fg); border-color:var(--ok-bd); }}
  .badge-warn {{ background:var(--warn-bg); color:var(--warn-fg); border-color:var(--warn-bd); }}
  .badge-neutral {{ background:rgba(16,183,173,.15); color:var(--teal); border-color:rgba(16,183,173,.35); }}
  .status-meta {{ font-size:.8rem; color:var(--muted); margin-top:.2rem; }}
  .action-stack {{ display:flex; flex-wrap:wrap; gap:.6rem; align-items:center; }}
  .countdown-ok {{ color:var(--ok-fg); font-weight:700; }}
  .countdown-warn {{ color:var(--err-fg); font-weight:700; }}
  table.sortable-table th[data-sort-type] {{ cursor:pointer; user-select:none; position:relative; padding-right:1.4rem; }}
  table.sortable-table th[data-sort-type]::after {{ content:"⇅"; position:absolute; right:.4rem; color:var(--muted); font-size:.75rem; top:50%; transform:translateY(-50%); }}
  table.sortable-table th[data-sort-type][data-sort-dir="asc"]::after {{ content:"↑"; color:var(--accent); }}
  table.sortable-table th[data-sort-type][data-sort-dir="desc"]::after {{ content:"↓"; color:var(--accent); }}
  .filter-form {{ margin-top:.6rem; }}
  .filter-form .row {{ align-items:flex-end; gap:1.2rem; }}
  .filter-label {{ display:flex; flex-direction:column; gap:.3rem; font-size:.9rem; color:var(--muted); }}
  .filter-card {{ margin-top:1.4rem; }}
  .filter-row {{ align-items:flex-end; gap:1.2rem; flex-wrap:wrap; }}
  .filter-row .filter-label {{ display:flex; flex-direction:column; gap:.3rem; font-size:.85rem; color:var(--muted); }}
  .filter-row select {{ min-width:12rem; }}
  .add-streamer-card {{ margin-top:1.4rem; }}
  .add-streamer-card h2 {{ margin:0 0 .6rem 0; font-size:1.1rem; color:var(--teal); }}
  .add-streamer-card form {{ display:flex; flex-direction:column; gap:.8rem; }}
  .add-streamer-card .form-grid {{ display:grid; grid-template-columns:repeat(auto-fit, minmax(190px, 1fr)); gap:1rem; align-items:end; }}
  .add-streamer-card label {{ display:flex; flex-direction:column; gap:.3rem; font-size:.85rem; color:var(--muted); min-width:0; }}
  .add-streamer-card input[type="text"] {{ width:100%; min-width:0; }}
  .add-streamer-card .form-actions {{ display:flex; gap:.8rem; align-items:center; flex-wrap:wrap; }}
  .add-streamer-card .hint {{ margin-top:.2rem; font-size:.8rem; color:var(--muted); max-width:38rem; }}
  .non-partner-card {{ margin-top:2.2rem; padding:1.2rem; background:linear-gradient(160deg, rgba(255,122,24,.15), rgba(16,183,173,.08)); border-radius:1rem; border:1px solid rgba(255,122,24,.25); box-shadow:0 12px 24px var(--shadow); }}
  .non-partner-card h2 {{ margin:0 0 .4rem 0; font-size:1.1rem; color:var(--accent); letter-spacing:.01em; }}
  .non-partner-card p {{ margin:0 0 1rem 0; font-size:.85rem; color:var(--muted); }}
  .non-partner-list {{ list-style:none; margin:0; padding:0; display:flex; flex-direction:column; gap:1.2rem; }}
  .non-partner-item {{ display:flex; flex-direction:column; gap:.8rem; padding:.9rem 1rem; background:rgba(7,21,29,.9); border:1px solid var(--bd); border-radius:.8rem; position:relative; overflow:hidden; transition:border-color .2s ease; }}
  .non-partner-item:hover {{ border-color:rgba(255,122,24,.4); }}
  .non-partner-header {{ display:flex; justify-content:space-between; align-items:center; gap:.8rem; flex-wrap:wrap; }}
  .non-partner-header strong {{ font-size:1rem; color:var(--accent); letter-spacing:.01em; }}
  .non-partner-badges {{ display:flex; gap:.6rem; flex-wrap:wrap; }}
  .non-partner-meta {{ display:flex; flex-direction:column; gap:.25rem; font-size:.8rem; color:var(--muted); padding-left:.2rem; }}
  .non-partner-meta span {{ display:flex; align-items:center; gap:.45rem; flex-wrap:wrap; }}
  .non-partner-meta .meta-label {{ color:var(--teal); font-weight:700; min-width:5.4rem; text-transform:uppercase; letter-spacing:.06em; font-size:.7rem; }}
  .non-partner-warning {{ color:var(--err-fg); font-weight:600; font-size:.75rem; }}
  .non-partner-manage {{ background:var(--bg-alt); border:1px solid var(--bd); border-radius:.7rem; padding:.6rem; }}
  .non-partner-manage > summary {{ cursor:pointer; font-size:.8rem; color:var(--accent); font-weight:600; list-style:none; }}
  .non-partner-manage[open] > summary {{ color:var(--teal); }}
  .non-partner-manage .manage-body {{ margin-top:.5rem; display:flex; flex-direction:column; gap:.8rem; }}
  .non-partner-actions {{ display:flex; flex-wrap:wrap; gap:.6rem; }}
  .non-partner-note {{ font-size:.75rem; color:var(--muted); }}
  .chart-panel {{ background:var(--bg-alt); border:1px solid var(--bd); border-radius:.9rem; padding:1.2rem; margin-top:1.4rem; }}
  .chart-panel h3 {{ margin:0 0 .6rem 0; font-size:1.1rem; color:var(--teal); }}
  .chart-panel canvas {{ width:100%; height:320px; max-height:360px; }}
  .chart-note {{ margin-top:.6rem; font-size:.85rem; color:var(--muted); }}
  .chart-empty {{ margin-top:1rem; font-size:.9rem; color:var(--muted); font-style:italic; }}
  .analysis-controls {{ margin-top:1rem; }}
  .user-form {{ margin-top:1rem; }}
  .user-hint {{ margin-top:.4rem; font-size:.8rem; color:var(--muted); }}
  .user-warning {{ margin-top:.6rem; color:var(--err-fg); font-weight:600; }}
  .user-summary {{ display:flex; flex-wrap:wrap; gap:1.2rem; margin-top:1.2rem; }}
  .user-summary-item {{ background:var(--bg-alt); border:1px solid var(--bd); border-radius:.7rem; padding:.6rem .9rem; min-width:140px; }}
  .user-summary-item .label {{ display:block; color:var(--muted); font-size:.8rem; }}
  .user-summary-item .value {{ display:block; color:var(--accent); font-size:1.05rem; font-weight:700; }}
  .user-meta {{ margin-top:.8rem; font-size:.85rem; color:var(--muted); }}
  .user-meta strong {{ color:var(--teal); font-weight:700; }}
  .user-chart-grid {{ display:flex; flex-wrap:wrap; gap:1.4rem; margin-top:1.4rem; }}
  .user-chart-panel {{ flex:1 1 300px; }}
  .user-section-empty {{ margin-top:1rem; font-size:.9rem; color:var(--muted); font-style:italic; }}
  .toggle-group {{ display:flex; gap:.6rem; flex-wrap:wrap; }}
  .btn-active {{ background:var(--accent); color:#fff; border:1px solid var(--accent); }}
  .discord-status {{ display:flex; flex-direction:column; gap:.3rem; }}
  .discord-icon {{ font-weight:700; }}
  .discord-warning {{ color:var(--err-fg); font-size:.8rem; font-weight:600; }}
  .discord-cell {{ display:flex; flex-direction:column; gap:.3rem; align-items:flex-start; }}
  .discord-cell .discord-main {{ display:flex; align-items:center; gap:.4rem; }}
  .discord-cell .discord-flag {{ font-weight:700; }}
  details.discord-inline {{ display:inline-block; }}
  details.discord-inline > summary {{
    cursor:pointer; display:inline-flex; align-items:center; justify-content:center;
    width:1.6rem; height:1.6rem; border-radius:999px; border:1px solid var(--bd);
    background:var(--bg-alt); color:var(--accent); font-weight:700; margin:0;
  }}
  details.discord-inline[open] > summary {{ background:var(--accent); color:#fff; border-color:var(--accent); }}
  .discord-inline-body {{
    margin-top:.4rem; background:var(--bg-alt); border:1px solid var(--bd);
    border-radius:.6rem; padding:.6rem; display:flex; flex-direction:column; gap:.6rem;
  }}
  .discord-inline-body label {{ display:flex; flex-direction:column; gap:.3rem; font-size:.8rem; color:var(--muted); }}
  .discord-inline-body input[type="text"] {{ width:100%; min-width:0; max-width:100%; }}
  .discord-inline-body .form-actions {{ display:flex; gap:.6rem; }}
  details.advanced-details {{ margin-top:.6rem; width:100%; }}
  details.advanced-details > summary {{ cursor:pointer; font-size:.85rem; color:var(--accent); }}
  details.advanced-details[open] > summary {{ color:var(--teal); }}
  .advanced-content {{ margin-top:.8rem; display:flex; flex-direction:column; gap:.8rem; background:var(--bg-alt); padding:.8rem; border:1px solid var(--bd); border-radius:.6rem; }}
  .advanced-content .form-row {{ display:grid; grid-template-columns:repeat(auto-fit, minmax(180px, 1fr)); gap:1rem; align-items:end; }}
  .advanced-content label {{ display:flex; flex-direction:column; gap:.3rem; font-size:.85rem; color:var(--muted); min-width:0; }}
  .advanced-content input[type="text"] {{ padding:.4rem .6rem; border-radius:.5rem; width:100%; min-width:0; max-width:100%; }}
  .discord-preview {{ display:flex; flex-direction:column; gap:.4rem; padding:.6rem; background:var(--bg-alt); border:1px solid var(--bd); border-radius:.5rem; font-size:.8rem; color:var(--muted); }}
  .discord-preview-row {{ display:flex; gap:.8rem; align-items:center; flex-wrap:wrap; }}
  .discord-preview-row .preview-label {{ color:var(--accent); font-weight:700; min-width:4.5rem; }}
  .discord-preview-row .preview-empty {{ color:var(--muted); font-style:italic; }}
  .checkbox-label {{ display:flex; align-items:center; gap:.6rem; font-size:.85rem; color:var(--muted); }}
  .checkbox-label input[type="checkbox"] {{ width:1rem; height:1rem; }}
  .advanced-content .hint {{ font-size:.75rem; color:var(--muted); }}
  .hero {{ display:flex; justify-content:space-between; align-items:flex-end; gap:2rem; flex-wrap:wrap; margin:0 0 2rem 0; }}
  .hero-actions {{ display:flex; gap:.8rem; flex-wrap:wrap; align-items:center; }}
  .eyebrow {{ text-transform:uppercase; letter-spacing:.18em; font-size:.7rem; color:var(--muted); margin:0 0 .4rem 0; }}
  .lead {{ margin:.4rem 0 0 0; color:var(--muted); max-width:32rem; }}
  .panel-grid {{ display:grid; gap:2.2rem; grid-template-columns:repeat(auto-fit, minmax(280px, 1fr)); }}
  .panel-grid .card {{ margin-top:0; animation: rise .6s ease both; min-width:0; }}
  .panel-grid .card:nth-child(2) {{ animation-delay:.08s; }}
  .panel-grid .card:nth-child(3) {{ animation-delay:.16s; }}
  .table-wrap {{ overflow-x:auto; border-radius:1rem; border:1px solid var(--bd); background:var(--card); box-shadow:0 10px 30px var(--shadow); animation: rise .6s ease both; animation-delay:.15s; }}
  .table-card {{ padding:1.1rem; }}
  .table-card table {{ margin-top:0; }}
  .pill {{ display:inline-flex; align-items:center; gap:.4rem; padding:.3rem .6rem; border-radius:999px; background:var(--bg-alt); border:1px solid var(--bd); font-size:.75rem; color:var(--muted); font-weight:700; }}
  .pill.ok {{ background:var(--ok-bg); border-color:var(--ok-bd); color:var(--ok-fg); }}
  .pill.warn {{ background:var(--warn-bg); border-color:var(--warn-bd); color:var(--warn-fg); }}
  .pill.err {{ background:var(--err-bg); border-color:var(--err-bd); color:var(--err-fg); }}
  .pill.neutral {{ background:rgba(16,183,173,.15); color:var(--teal); border-color:rgba(16,183,173,.35); }}
  .chip {{ display:inline-flex; align-items:center; gap:.4rem; padding:.25rem .55rem; border-radius:999px; background:var(--chip-bg); color:var(--accent); border:1px solid rgba(255,122,24,.3); font-size:.75rem; font-weight:600; }}
  .chip-crit {{ background:rgba(255,107,94,.18); border-color:rgba(255,107,94,.55); color:var(--err-fg); }}
  .raid-form {{ display:grid; grid-template-columns:repeat(auto-fit, minmax(200px, 1fr)); gap:1rem; align-items:end; margin-top:1rem; }}
  .raid-form label {{ display:flex; flex-direction:column; gap:.35rem; font-size:.85rem; color:var(--muted); min-width:0; }}
  .raid-form input[type="text"] {{ width:100%; min-width:0; max-width:100%; }}
  .raid-meta {{ display:flex; flex-wrap:wrap; gap:.6rem; margin-top:1rem; }}
  .raid-metrics {{ display:flex; flex-wrap:wrap; gap:1rem; margin-top:.8rem; }}
  .mini-stat {{ background:var(--bg-alt); border:1px solid var(--bd); border-radius:.7rem; padding:.6rem .8rem; display:flex; flex-direction:column; min-width:120px; }}
  .mini-stat strong {{ font-size:1.1rem; color:var(--accent); }}
  .mini-stat span {{ font-size:.75rem; color:var(--muted); }}
  .raid-cell {{ display:flex; flex-direction:column; gap:.4rem; align-items:flex-start; }}
  .scope-card {{ margin-top:1.4rem; }}
  .scope-header {{ white-space:nowrap; text-align:center; min-width:5.4rem; }}
  .scope-check {{ text-align:center; font-size:1rem; font-weight:700; white-space:nowrap; }}
  .scope-check.yes {{ color:var(--ok-fg); }}
  .scope-check.no {{ color:var(--err-fg); }}
  .scope-missing {{ display:flex; flex-wrap:wrap; gap:.35rem; margin-top:.3rem; }}
  .scope-row.scope-reauth td {{ background:rgba(255,107,94,.06); }}
  .scope-row.scope-critical td {{ background:rgba(245,182,66,.05); }}
  .scope-row.scope-partial td {{ background:rgba(16,183,173,.04); }}
  .scope-row.scope-full td {{ background:rgba(46,204,113,.04); }}
  /* Roadmap Kanban */
  .kanban-board {{ display:grid; grid-template-columns:repeat(auto-fit, minmax(260px, 1fr)); gap:1.2rem; margin-top:1.2rem; }}
  .kanban-col {{ background:var(--bg-alt); border:1px solid var(--bd); border-radius:.9rem; padding:1rem; min-height:200px; display:flex; flex-direction:column; gap:.8rem; transition:border-color .2s ease; }}
  .kanban-col.drag-over {{ border-color:rgba(255,122,24,.5); background:rgba(255,122,24,.04); }}
  .kanban-col-header {{ display:flex; align-items:center; justify-content:space-between; gap:.5rem; margin-bottom:.2rem; }}
  .kanban-col-title {{ font-weight:700; font-size:.85rem; text-transform:uppercase; letter-spacing:.08em; }}
  .col-planned .kanban-col-title {{ color:var(--muted); }}
  .col-in_progress .kanban-col-title {{ color:var(--accent); }}
  .col-done .kanban-col-title {{ color:var(--ok-fg); }}
  .kanban-count {{ font-size:.75rem; color:var(--muted); background:var(--card); border:1px solid var(--bd); border-radius:999px; padding:.1rem .5rem; }}
  .kanban-card {{ background:var(--card); border:1px solid var(--bd); border-radius:.7rem; padding:.75rem .9rem; cursor:grab; transition:all .15s ease; position:relative; }}
  .kanban-card:hover {{ border-color:var(--bd-hover); background:var(--card-hover); }}
  .kanban-card.dragging {{ opacity:.5; cursor:grabbing; }}
  .kanban-card-title {{ font-weight:600; font-size:.9rem; color:var(--text); margin:0 0 .3rem 0; padding-right:1.4rem; }}
  .kanban-card-desc {{ font-size:.78rem; color:var(--muted); line-height:1.5; margin:0; }}
  .kanban-card-delete {{ position:absolute; top:.5rem; right:.5rem; background:transparent; border:none; color:var(--muted); cursor:pointer; font-size:1rem; line-height:1; padding:.1rem .3rem; border-radius:.3rem; transition:color .15s ease; }}
  .kanban-card-delete:hover {{ color:var(--err-fg); }}
  .kanban-empty {{ text-align:center; color:var(--muted); font-size:.82rem; padding:1.5rem .5rem; font-style:italic; }}
  .add-item-form {{ margin-top:1.4rem; background:var(--bg-alt); border:1px solid var(--bd); border-radius:.9rem; padding:1.1rem; display:flex; flex-direction:column; gap:.8rem; }}
  .add-item-form h3 {{ margin:0 0 .2rem 0; font-size:1rem; color:var(--accent); }}
  .add-item-form .form-row {{ display:grid; grid-template-columns:repeat(auto-fit, minmax(180px, 1fr)); gap:.8rem; align-items:end; }}
  .add-item-form label {{ display:flex; flex-direction:column; gap:.3rem; font-size:.82rem; color:var(--muted); }}
  .add-item-form .form-actions {{ display:flex; gap:.6rem; flex-wrap:wrap; }}
  @keyframes rise {{
    from {{ opacity:0; transform: translateY(12px); }}
    to {{ opacity:1; transform: translateY(0); }}
  }}
  @media (max-width: 720px) {{
    body {{ padding:1.6rem 1.1rem 2.2rem; }}
    h1 {{ font-size:1.8rem; }}
    input[type="text"] {{ width:100%; }}
    .add-streamer-card input[type="text"] {{ min-width:0; width:100%; }}
  }}
</style>
{nav_html}
{flash}
{body}
<script>
  document.addEventListener("click", (event) => {{
    const link = event.target.closest("a[data-same-tab='1']");
    if (!link) return;
    const href = link.getAttribute("href");
    if (!href) return;
    event.preventDefault();
    window.location.assign(href);
  }});
</script>
"""
        template = template.replace("{{", "{").replace("}}", "}")
        return (
            template.replace("{nav_html}", nav_html)
            .replace("{flash}", flash)
            .replace("{body}", body)
        )

    def _streamer_detail_view(self, data: dict, active: str) -> str:
        login = data["login"]

        stats = data.get("stats_30d", {})
        sessions = data.get("recent_sessions", [])

        # Prepare chart data for recent sessions (reversed to show chronological order in chart)
        chart_labels = [s["started_at"][5:16] for s in reversed(sessions)]
        chart_viewers = [s["avg_viewers"] for s in reversed(sessions)]
        chart_peaks = [s["peak_viewers"] for s in reversed(sessions)]

        body = f"""
        <div class="card-header">
            <h1>Analytics: {login}</h1>
            <a href="/twitch/stats" class="btn btn-secondary btn-small">← Back to List</a>
        </div>

        <div class="user-summary">
            <div class="user-summary-item">
                <span class="label">Total Streams (30d)</span>
                <span class="value">{stats.get("total_streams", 0)}</span>
            </div>
            <div class="user-summary-item">
                <span class="label">Avg Viewers</span>
                <span class="value">{int(stats.get("avg_avg_viewers") or 0)}</span>
            </div>
            <div class="user-summary-item">
                <span class="label">Peak Viewers</span>
                <span class="value">{stats.get("max_peak", 0)}</span>
            </div>
             <div class="user-summary-item">
                <span class="label">New Followers</span>
                <span class="value">{stats.get("total_follower_delta", 0)}</span>
            </div>
            <div class="user-summary-item">
                <span class="label">Unique Chatters</span>
                <span class="value">{stats.get("total_unique_chatters", 0)}</span>
            </div>
        </div>

        <div class="chart-panel">
            <h3>Viewer Trends (Recent Sessions)</h3>
            <canvas id="streamerChart"></canvas>
        </div>

        <div class="card" style="margin-top: 1.4rem;">
            <h3>Recent Sessions</h3>
            <table>
                <thead>
                    <tr>
                        <th>Date</th>
                        <th>Title</th>
                        <th>Duration</th>
                        <th>Avg Viewers</th>
                        <th>Peak</th>
                        <th>Followers</th>
                        <th>Details</th>
                    </tr>
                </thead>
                <tbody>
        """
        for s in sessions:
            dur_min = (s["duration_seconds"] or 0) // 60
            body += f"""
                    <tr>
                        <td>{s["started_at"]}</td>
                        <td><small>{html.escape(s["stream_title"] or "")}</small></td>
                        <td>{dur_min} min</td>
                        <td>{s["avg_viewers"]}</td>
                        <td>{s["peak_viewers"]}</td>
                        <td>{s["follower_delta"] or 0}</td>
                        <td><a href="/twitch/session/{s["id"]}" class="btn btn-small">Analysis</a></td>
                    </tr>
            """
        body += (
            """
                </tbody>
            </table>
        </div>

        <script>
            const ctx = document.getElementById('streamerChart');
            new Chart(ctx, {
                type: 'line',
                data: {
                    labels: """
            + str(chart_labels)
            + """,
                    datasets: [{
                        label: 'Avg Viewers',
                        data: """
            + str(chart_viewers)
            + """,
                        borderColor: '#6d4aff',
                        backgroundColor: 'rgba(109, 74, 255, 0.1)',
                        tension: 0.3,
                        fill: true
                    }, {
                        label: 'Peak Viewers',
                        data: """
            + str(chart_peaks)
            + """,
                        borderColor: '#9bb0ff',
                        borderDash: [5, 5],
                        tension: 0.3
                    }]
                },
                options: {
                    responsive: true,
                    maintainAspectRatio: false,
                    plugins: {
                        legend: { labels: { color: '#eeeeee' } }
                    },
                    scales: {
                        y: { 
                            beginAtZero: true, 
                            grid: { color: '#2a3044' },
                            ticks: { color: '#9aa4b2' }
                        },
                        x: {
                            grid: { display: false },
                            ticks: { color: '#9aa4b2' }
                        }
                    }
                }
            });
        </script>
        """
        )
        return self._html(body, active)

    def _session_detail_view(self, data: dict, active: str) -> str:
        s = data["session"]
        timeline = data.get("timeline", [])
        top_chatters = data.get("top_chatters", [])

        t_labels = [f"{t['minutes_from_start']}m" for t in timeline]
        t_values = [t["viewer_count"] for t in timeline]

        body = f"""
        <div class="card-header">
            <h1>Session Analysis</h1>
            <a href="/twitch/streamer/{s["streamer_login"]}" class="btn btn-secondary btn-small">← Back to Streamer</a>
        </div>
        <div class="card" style="margin-top: 1.4rem;">
             <div class="row">
                <div class="discord-cell">
                    <span class="label" style="color:var(--muted)">Streamer</span>
                    <strong>{s["streamer_login"]}</strong>
                </div>
                <div class="discord-cell">
                    <span class="label" style="color:var(--muted)">Date</span>
                    <strong>{s["started_at"]}</strong>
                </div>
                 <div class="discord-cell">
                    <span class="label" style="color:var(--muted)">Duration</span>
                    <strong>{(s["duration_seconds"] or 0) // 60} min</strong>
                </div>
                 <div class="discord-cell">
                    <span class="label" style="color:var(--muted)">Avg Viewers</span>
                    <strong>{s["avg_viewers"]}</strong>
                </div>
                 <div class="discord-cell">
                    <span class="label" style="color:var(--muted)">Max Peak</span>
                    <strong>{s["peak_viewers"]}</strong>
                </div>
            </div>
            <div style="margin-top: 1rem; color: var(--accent-2);">
                {html.escape(s["stream_title"] or "")}
            </div>
        </div>

        <div class="chart-panel">
            <h3>Viewer Retention (Timeline)</h3>
            <canvas id="sessionChart"></canvas>
        </div>

        <div class="row" style="align-items: flex-start; margin-top: 1rem;">
            <div class="card" style="flex: 1;">
                <h3>Engagement Metrics</h3>
                <ul>
                    <li><strong>Retention 5m:</strong> {s.get("retention_5m") or "-"}%</li>
                    <li><strong>Retention 10m:</strong> {s.get("retention_10m") or "-"}%</li>
                    <li><strong>Dropoff:</strong> {s.get("dropoff_pct") or "-"}% ({s.get("dropoff_label") or "N/A"})</li>
                    <li><strong>Unique Chatters:</strong> {s.get("unique_chatters")}</li>
                    <li><strong>New Chatters:</strong> {s.get("first_time_chatters")}</li>
                    <li><strong>Returning Chatters:</strong> {s.get("returning_chatters")}</li>
                </ul>
            </div>
            <div class="card" style="flex: 1;">
                <h3>Top Chatters</h3>
                <table>
                    <thead><tr><th>User</th><th>Messages</th></tr></thead>
                    <tbody>
        """
        for c in top_chatters:
            body += f"<tr><td>{c['chatter_login']}</td><td>{c['messages']}</td></tr>"

        body += (
            """
                    </tbody>
                </table>
            </div>
        </div>

        <script>
            const ctx = document.getElementById('sessionChart');
            new Chart(ctx, {
                type: 'line',
                data: {
                    labels: """
            + str(t_labels)
            + """,
                    datasets: [{
                        label: 'Viewers',
                        data: """
            + str(t_values)
            + """,
                        borderColor: '#6d4aff',
                        backgroundColor: 'rgba(109, 74, 255, 0.2)',
                        fill: true,
                        tension: 0.4,
                        pointRadius: 0,
                        pointHoverRadius: 6
                    }]
                },
                options: {
                    responsive: true,
                    maintainAspectRatio: false,
                    interaction: {
                        mode: 'index',
                        intersect: false,
                    },
                    plugins: {
                        legend: { display: false }
                    },
                    scales: {
                        y: { 
                            beginAtZero: true, 
                            grid: { color: '#2a3044' },
                            ticks: { color: '#9aa4b2' }
                        },
                        x: {
                            grid: { display: false },
                            ticks: { maxTicksLimit: 20, color: '#9aa4b2' }
                        }
                    }
                }
            });
        </script>
        """
        )
        return self._html(body, active)

    def _comparison_view(self, data: dict, active: str) -> str:
        cat = data.get("category", {})
        track = data.get("tracked_avg", {})
        top = data.get("top_streamers", [])

        body = f"""
        <div class="card-header">
            <h1>Market Analysis (Last 30 Days)</h1>
        </div>
        
        <div class="user-summary">
            <div class="user-summary-item" style="border-color: var(--accent);">
                <span class="label">Tracked Avg Viewers</span>
                <span class="value">{int(track.get("avg_viewers") or 0)}</span>
            </div>
            <div class="user-summary-item" style="border-color: var(--muted);">
                <span class="label">Deadlock Category Avg</span>
                <span class="value">{int(cat.get("avg_viewers") or 0)}</span>
            </div>
             <div class="user-summary-item">
                <span class="label">Category Peak</span>
                <span class="value">{cat.get("peak_viewers") or 0}</span>
            </div>
        </div>

        <div class="chart-panel">
            <h3>Top 5 Performers (Avg Viewers)</h3>
            <canvas id="topChart"></canvas>
        </div>

        <div class="card" style="margin-top: 1.4rem;">
            <h3>Top Streamers Table</h3>
            <table>
                <thead>
                    <tr>
                        <th>Rank</th>
                        <th>Streamer</th>
                        <th>Avg Viewers</th>
                        <th>Action</th>
                    </tr>
                </thead>
                <tbody>
        """

        chart_labels = []
        chart_data = []

        for i, s in enumerate(top, 1):
            chart_labels.append(s["streamer_login"])
            chart_data.append(s["val"])
            body += f"""
                    <tr>
                        <td>#{i}</td>
                        <td>{s["streamer_login"]}</td>
                        <td>{int(s["val"])}</td>
                        <td><a href="/twitch/streamer/{s["streamer_login"]}" class="btn btn-small">Stats</a></td>
                    </tr>
            """

        body += (
            """
                </tbody>
            </table>
        </div>

        <script>
            const ctx = document.getElementById('topChart');
            new Chart(ctx, {
                type: 'bar',
                data: {
                    labels: """
            + str(chart_labels)
            + """,
                    datasets: [{
                        label: 'Avg Viewers',
                        data: """
            + str(chart_data)
            + """,
                        backgroundColor: [
                            'rgba(109, 74, 255, 0.8)',
                            'rgba(109, 74, 255, 0.6)',
                            'rgba(109, 74, 255, 0.4)',
                            'rgba(109, 74, 255, 0.3)',
                            'rgba(109, 74, 255, 0.2)'
                        ],
                        borderColor: '#6d4aff',
                        borderWidth: 1
                    }]
                },
                options: {
                    responsive: true,
                    maintainAspectRatio: false,
                    plugins: {
                        legend: { display: false }
                    },
                    scales: {
                        y: { 
                            beginAtZero: true, 
                            grid: { color: '#2a3044' },
                            ticks: { color: '#9aa4b2' }
                        },
                        x: {
                            grid: { display: false },
                            ticks: { color: '#eeeeee' }
                        }
                    }
                }
            });
        </script>
        """
        )
        return self._html(body, active)
