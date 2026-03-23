"""Route group for market research handlers."""

from __future__ import annotations

from typing import Any

from aiohttp import web


def build_route_defs(server: Any) -> list[web.RouteDef]:
    """Return route definitions for market research routes."""
    return [
        web.get("/twitch/market", server.market_research),
        web.get("/twitch/api/market_data", server.api_market_data),
    ]


async def market_research(server: Any, request: web.Request) -> web.StreamResponse:
    """Serve the internal Market Research dashboard."""
    server._require_token(request)

    page_html = """
        <!DOCTYPE html>
        <html lang="de">
        <head>
            <meta charset="UTF-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            <title>Deadlock Market Research (Internal)</title>
            <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
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
        """
    return web.Response(text=page_html, content_type="text/html")


async def api_market_data(
    server: Any,
    request: web.Request,
    *,
    deps: dict[str, Any],
) -> web.Response:
    """API providing aggregated data for market research including Meta & Sentiment."""
    json_module = deps["json"]
    log = deps["log"]
    storage_module = deps["storage"]
    uuid4_fn = deps["uuid4"]

    admin_token = request.headers.get("X-Admin-Token")
    if not (
        server._is_local_request(request)
        or server._is_discord_admin_request(request)
        or server._check_admin_token(admin_token)
    ):
        return web.json_response({"error": "Unauthorized"}, status=401)

    try:
        with storage_module.readonly_connection() as conn:
            def _to_iso(val: Any) -> Any:
                return val.isoformat() if hasattr(val, "isoformat") else val

            def _json_default(obj: Any) -> str:
                return obj.isoformat() if hasattr(obj, "isoformat") else str(obj)

            rows = conn.execute(
                """
                    SELECT s.twitch_login, l.last_viewer_count
                    FROM twitch_streamers s
                    LEFT JOIN twitch_live_state l ON s.twitch_user_id = l.twitch_user_id
                    WHERE s.is_monitored_only = 1
                """
            ).fetchall()

            channels = []
            total_viewers = 0

            for row in rows:
                login = row[0]
                viewers = row[1] or 0
                total_viewers += viewers

                chat_stats = conn.execute(
                    """
                        SELECT COUNT(*), COUNT(DISTINCT chatter_login)
                        FROM twitch_chat_messages
                        WHERE streamer_login = %s
                          AND message_ts >= CURRENT_TIMESTAMP - INTERVAL '1 hour'
                    """,
                    (login,),
                ).fetchone()

                msgs = chat_stats[0] or 0
                active_chatters = chat_stats[1] or 0

                session_id_row = conn.execute(
                    "SELECT active_session_id FROM twitch_live_state WHERE streamer_login = %s",
                    (login,),
                ).fetchone()

                lurkers = 0
                total_connected = active_chatters
                if session_id_row and session_id_row[0]:
                    lurker_stats = conn.execute(
                        """
                            SELECT COUNT(*), SUM(CASE WHEN messages = 0 THEN 1 ELSE 0 END)
                            FROM twitch_session_chatters WHERE session_id = %s
                        """,
                        (session_id_row[0],),
                    ).fetchone()
                    if lurker_stats:
                        total_connected = lurker_stats[0] or active_chatters
                        lurkers = lurker_stats[1] or 0

                channels.append(
                    {
                        "login": login,
                        "viewers": viewers,
                        "is_live": viewers > 0,
                        "chat_health": min(100, (active_chatters / max(1, viewers)) * 100)
                        if viewers > 0
                        else 0,
                        "lurker_ratio": (lurkers / max(1, total_connected)) * 100,
                        "msg_per_min": msgs / 60.0,
                        "top_topic": "n/a",
                    }
                )

            channels.sort(key=lambda item: item["viewers"], reverse=True)
            avg_health = sum(item["chat_health"] for item in channels) / max(1, len(channels))
            avg_lurker = sum(item["lurker_ratio"] for item in channels) / max(1, len(channels))

            history_rows = conn.execute(
                """
                    SELECT ts_utc, SUM(viewer_count) as total_viewers, COUNT(DISTINCT streamer) as streamer_count
                    FROM twitch_stats_category
                    WHERE ts_utc >= CURRENT_TIMESTAMP - INTERVAL '24 hours'
                    GROUP BY ts_utc
                    ORDER BY ts_utc ASC
                """
            ).fetchall()
            market_history = [
                {"ts": _to_iso(row[0]), "total_viewers": row[1], "streamer_count": row[2]}
                for row in history_rows
            ]

            question_rows = conn.execute(
                """
                    SELECT content, streamer_login, message_ts
                    FROM twitch_chat_messages
                    WHERE message_ts >= CURRENT_TIMESTAMP - INTERVAL '6 hours'
                      AND content LIKE %s
                      AND length(content) > 10
                    ORDER BY message_ts DESC
                    LIMIT 20
                """,
                ("%?%",),
            ).fetchall()
            questions = [
                {"content": row[0], "streamer": row[1], "ts": _to_iso(row[2])}
                for row in question_rows
            ]

            deadlock_terms = [
                "abrams",
                "bebop",
                "dynamo",
                "grey talon",
                "haze",
                "infernus",
                "ivy",
                "kelvin",
                "lady geist",
                "mcginnis",
                "mo & krill",
                "paradox",
                "pocket",
                "seven",
                "vindicta",
                "viscous",
                "warden",
                "wraith",
                "yamato",
                "lash",
                "shiv",
                "urn",
                "midboss",
                "soul",
                "flex slot",
                "build",
                "op",
                "nerf",
                "buff",
                "patch",
            ]
            recent_msgs = conn.execute(
                "SELECT content FROM twitch_chat_messages WHERE message_ts >= CURRENT_TIMESTAMP - INTERVAL '1 hour'"
            ).fetchall()

            term_counts = {term: 0 for term in deadlock_terms}
            sentiment = {"positive": 0, "negative": 0, "neutral": 0}
            pos_words = {"pog", "gg", "nice", "cool", "krass", "lol", "win", "stark"}
            neg_words = {"rip", "bad", "lose", "troll", "cringe", "throw", "sucks", "lag"}

            for row in recent_msgs:
                content = (row[0] or "").lower()
                for term in deadlock_terms:
                    if term in content:
                        term_counts[term] += 1
                is_pos = any(word in content for word in pos_words)
                is_neg = any(word in content for word in neg_words)
                if is_pos and not is_neg:
                    sentiment["positive"] += 1
                elif is_neg and not is_pos:
                    sentiment["negative"] += 1
                else:
                    sentiment["neutral"] += 1

            meta_snapshot = sorted(
                [{"term": key, "count": value} for key, value in term_counts.items() if value > 0],
                key=lambda item: item["count"],
                reverse=True,
            )[:10]
            total_sent = sum(sentiment.values()) or 1
            sent_data = {
                "positive": sentiment["positive"],
                "negative": sentiment["negative"],
                "neutral": sentiment["neutral"],
                "pos_pct": round(sentiment["positive"] / total_sent * 100, 1),
                "neg_pct": round(sentiment["negative"] / total_sent * 100, 1),
                "neu_pct": round(sentiment["neutral"] / total_sent * 100, 1),
            }

            top_logins = [item["login"] for item in channels[:5]]
            overlap = []
            if len(top_logins) >= 2:
                login_slots = (top_logins + ["!unused!"] * 5)[:5]
                rows_overlap = conn.execute(
                    """
                        SELECT c1.streamer_login, c2.streamer_login, COUNT(DISTINCT c1.chatter_login)
                        FROM twitch_chat_messages c1
                        JOIN twitch_chat_messages c2 ON c1.chatter_login = c2.chatter_login AND c1.streamer_login < c2.streamer_login
                        WHERE c1.message_ts >= CURRENT_TIMESTAMP - INTERVAL '6 hours'
                          AND c2.message_ts >= CURRENT_TIMESTAMP - INTERVAL '6 hours'
                          AND c1.streamer_login IN (%s, %s, %s, %s, %s)
                          AND c2.streamer_login IN (%s, %s, %s, %s, %s)
                        GROUP BY 1, 2 ORDER BY 3 DESC LIMIT 5
                    """,
                    tuple(login_slots + login_slots),
                ).fetchall()
                overlap = [{"a": row[0], "b": row[1], "shared": row[2]} for row in rows_overlap]

            payload = {
                "total_monitored": len(channels),
                "total_viewers": total_viewers,
                "avg_chat_health": avg_health,
                "avg_lurker_ratio": avg_lurker,
                "total_messages": len(recent_msgs),
                "market_history": market_history,
                "questions": questions,
                "channels": channels,
                "meta_snapshot": meta_snapshot,
                "sentiment": sent_data,
                "overlap": overlap,
            }

            return web.json_response(
                payload,
                dumps=lambda data: json_module.dumps(data, default=_json_default),
            )
    except Exception:
        error_id = uuid4_fn().hex[:12]
        log.exception("Market API Error id=%s", error_id)
        return web.json_response(
            {
                "error": "market_data_failed",
                "error_id": error_id,
            },
            status=500,
        )
