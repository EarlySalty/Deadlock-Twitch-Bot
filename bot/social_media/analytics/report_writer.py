from __future__ import annotations

import logging
from collections import defaultdict
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from statistics import mean
from typing import Any

from ...storage import readonly_connection
from ..llm.dispatcher import LLMDispatcher
from . import SocialMediaReportRecord, get_existing_report, insert_report

log = logging.getLogger("TwitchStreams.SocialMedia.ReportWriter")

REPORT_SYSTEM_PROMPT = """\
Du schreibst operative Social-Media-Performance-Reports fuer deutsche Streamer.

Regeln:
- Antworte ausschliesslich auf Deutsch.
- Gib valides Markdown ohne Codeblock-Zaun aus.
- Erfinde keine Fakten, nutze nur die gelieferten Zahlen.
- Sei konkret und knapp: TL;DR, staerkste Muster, schwache Muster, 3 Massnahmen.
- Wenn Daten lueckig sind, benenne die Luecke offen.
"""


@dataclass(frozen=True)
class ClipPerformance:
    clip_db_id: int
    title: str
    streamer_login: str
    clip_url: str | None
    game_name: str | None
    created_at: str | None
    platforms: tuple[str, ...]
    views: int
    likes: int
    comments: int
    shares: int
    watch_time_seconds: int
    ctr_percent: float | None
    engagement_rate: float | None
    score: float


@dataclass(frozen=True)
class StreamerPerformance:
    streamer_login: str
    clip_count: int
    views: int
    likes: int
    comments: int
    shares: int
    watch_time_seconds: int
    engagement_rate: float | None
    top_clip_title: str | None


class SocialMediaReportWriter:
    """Generiert und persistiert Phase-3-Reports via LLMDispatcher."""

    def __init__(self, *, dispatcher: LLMDispatcher | None = None) -> None:
        self.dispatcher = dispatcher or LLMDispatcher()

    async def write_streamer_report(
        self,
        streamer_login: str,
        *,
        period_start: datetime | None = None,
        period_end: datetime | None = None,
        force: bool = False,
    ) -> SocialMediaReportRecord:
        end, start = _coerce_period(period_start, period_end, default="week")
        existing = None if force else get_existing_report(
            kind="streamer",
            streamer_login=streamer_login,
            period_start=start,
            period_end=end,
        )
        if existing is not None:
            return existing

        clips = _load_clip_performance(
            bucket="7d",
            period_start=start,
            period_end=end,
            streamer_login=streamer_login,
        )
        content = await self._render_streamer_report(streamer_login, start, end, clips)
        model = getattr(content, "model", None)
        markdown = getattr(content, "content", content)
        return insert_report(
            kind="streamer",
            streamer_login=streamer_login,
            period_start=start,
            period_end=end,
            content_md=markdown,
            model=model,
        )

    async def write_cross_report(
        self,
        *,
        period_start: datetime | None = None,
        period_end: datetime | None = None,
        force: bool = False,
    ) -> SocialMediaReportRecord:
        end, start = _coerce_period(period_start, period_end, default="month")
        existing = None if force else get_existing_report(
            kind="cross",
            period_start=start,
            period_end=end,
        )
        if existing is not None:
            return existing

        clips = _load_clip_performance(bucket="30d", period_start=start, period_end=end)
        content = await self._render_cross_report(start, end, clips)
        model = getattr(content, "model", None)
        markdown = getattr(content, "content", content)
        return insert_report(
            kind="cross",
            streamer_login=None,
            period_start=start,
            period_end=end,
            content_md=markdown,
            model=model,
        )

    async def write_admin_weekly_report(
        self,
        *,
        period_start: datetime | None = None,
        period_end: datetime | None = None,
        force: bool = False,
    ) -> SocialMediaReportRecord:
        end, start = _coerce_period(period_start, period_end, default="week")
        existing = None if force else get_existing_report(
            kind="admin",
            period_start=start,
            period_end=end,
        )
        if existing is not None:
            return existing

        clips = _load_clip_performance(bucket="7d", period_start=start, period_end=end)
        content = await self._render_admin_report(start, end, clips)
        model = getattr(content, "model", None)
        markdown = getattr(content, "content", content)
        return insert_report(
            kind="admin",
            streamer_login=None,
            period_start=start,
            period_end=end,
            content_md=markdown,
            model=model,
        )

    async def _render_streamer_report(
        self,
        streamer_login: str,
        period_start: datetime,
        period_end: datetime,
        clips: list[ClipPerformance],
    ):
        if not clips:
            return _fallback_no_data_report(
                heading=f"# Wochenreport · {streamer_login}",
                period_start=period_start,
                period_end=period_end,
                note="Fuer diesen Zeitraum liegen noch keine Phase-3-Analytics vor.",
            )

        ranked = sorted(clips, key=lambda clip: clip.score, reverse=True)
        top = ranked[:5]
        bottom = ranked[-5:] if len(ranked) > 5 else ranked
        totals = _aggregate_totals(clips)
        prompt = (
            f"Erstelle einen Wochenreport fuer @{streamer_login}.\n"
            f"Zeitraum: {_format_period(period_start, period_end)}\n"
            f"Clips mit verwertbaren Daten: {len(clips)}\n"
            f"Gesamtwerte: Views={totals['views']}, Likes={totals['likes']}, "
            f"Kommentare={totals['comments']}, Shares={totals['shares']}, "
            f"WatchTimeSekunden={totals['watch_time_seconds']}\n"
            f"Durchschnittliche Engagement-Rate: {_fmt_pct(totals['engagement_rate'])}\n\n"
            f"Top 5 Clips:\n{_format_clip_list(top)}\n\n"
            f"Bottom 5 Clips:\n{_format_clip_list(bottom)}\n\n"
            "Struktur:\n"
            "- Titelzeile\n"
            "- TL;DR (2 Saetze)\n"
            "- Abschnitt 'Top 5'\n"
            "- Abschnitt 'Bottom 5'\n"
            "- Abschnitt 'Massnahmen naechste Woche' mit genau 3 Bulletpoints\n"
        )
        return await self._render_with_llm(
            prompt,
            fallback=_fallback_streamer_report(streamer_login, period_start, period_end, top, bottom, totals),
        )

    async def _render_cross_report(
        self,
        period_start: datetime,
        period_end: datetime,
        clips: list[ClipPerformance],
    ):
        if not clips:
            return _fallback_no_data_report(
                heading="# Monatsreport · Cross-Streamer",
                period_start=period_start,
                period_end=period_end,
                note="Keine 30d-Analytics im ausgewaehlten Zeitraum gefunden.",
            )

        streamers = _aggregate_streamers(clips)
        prompt = (
            "Erstelle einen monatlichen Cross-Streamer-Report.\n"
            f"Zeitraum: {_format_period(period_start, period_end)}\n"
            f"Streamer mit Daten: {len(streamers)}\n"
            f"Top Streamer nach Views:\n{_format_streamer_list(streamers[:8])}\n\n"
            f"Top Clips plattformuebergreifend:\n{_format_clip_list(sorted(clips, key=lambda clip: clip.score, reverse=True)[:8])}\n\n"
            "Struktur:\n"
            "- Titel\n"
            "- Gesamtbild\n"
            "- Gewinner / Verlierer\n"
            "- 3 konkrete Hebel fuer die naechsten 30 Tage\n"
        )
        return await self._render_with_llm(
            prompt,
            fallback=_fallback_cross_report(period_start, period_end, streamers, clips),
        )

    async def _render_admin_report(
        self,
        period_start: datetime,
        period_end: datetime,
        clips: list[ClipPerformance],
    ):
        if not clips:
            return _fallback_no_data_report(
                heading="# Admin-Wochenreport · Social Media",
                period_start=period_start,
                period_end=period_end,
                note="Es gibt noch keine verwertbaren Analytics-Snapshots fuer den Wochenversand.",
            )

        streamers = _aggregate_streamers(clips)
        top_clips = sorted(clips, key=lambda clip: clip.score, reverse=True)[:6]
        prompt = (
            "Erstelle einen Admin-Wochenreport fuer die gesamte Social-Media-Pipeline.\n"
            f"Zeitraum: {_format_period(period_start, period_end)}\n"
            f"Streamer-Ranking:\n{_format_streamer_list(streamers[:10])}\n\n"
            f"Top Clips:\n{_format_clip_list(top_clips)}\n\n"
            "Struktur:\n"
            "- Titel\n"
            "- Executive Summary\n"
            "- Auffaellige Streamer\n"
            "- Risiko-/Problemzonen\n"
            "- 3 Admin-Aktionen fuer die kommende Woche\n"
        )
        return await self._render_with_llm(
            prompt,
            fallback=_fallback_admin_report(period_start, period_end, streamers, top_clips),
        )

    async def _render_with_llm(self, prompt: str, *, fallback: str):
        try:
            response = await self.dispatcher.generate_text(
                REPORT_SYSTEM_PROMPT,
                prompt,
                max_tokens=1400,
                temperature=0.2,
            )
        except Exception:
            log.warning("LLM report generation failed; using fallback markdown", exc_info=True)
            return fallback
        content = str(response.content or "").strip()
        if not content:
            return fallback
        return type(
            "RenderedReport",
            (),
            {
                "content": content,
                "model": f"{response.provider}:{response.model}",
            },
        )()


def _load_clip_performance(
    *,
    bucket: str,
    period_start: datetime,
    period_end: datetime,
    streamer_login: str | None = None,
) -> list[ClipPerformance]:
    where = [
        "a.bucket = %s",
        "a.synced_at >= %s",
        "a.synced_at < %s",
        "c.discarded_at IS NULL",
        "COALESCE(a.provider, '') NOT LIKE 'error:%'",
    ]
    params: list[Any] = [
        bucket,
        period_start.isoformat(),
        period_end.isoformat(),
    ]
    if streamer_login:
        where.append("LOWER(c.streamer_login) = LOWER(%s)")
        params.append(streamer_login)

    with readonly_connection() as conn:
        rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
            f"""
            SELECT c.id AS clip_db_id,
                   c.streamer_login,
                   c.clip_title,
                   c.clip_url,
                   c.created_at,
                   c.game_name,
                   a.platform,
                   a.views,
                   a.likes,
                   a.comments,
                   a.shares,
                   a.watch_time_seconds,
                   a.ctr_percent,
                   a.engagement_rate,
                   a.synced_at
              FROM twitch_clips_social_analytics a
              JOIN twitch_clips_social_media c ON c.id = a.clip_id
             WHERE {' AND '.join(where)}
             ORDER BY c.id ASC, a.platform ASC, a.synced_at DESC
            """,
            tuple(params),
        ).fetchall()

    latest_by_platform: dict[tuple[int, str], Any] = {}
    for row in rows:
        key = (int(row["clip_db_id"]), str(row["platform"]))
        latest_by_platform.setdefault(key, row)

    grouped: dict[int, list[Any]] = defaultdict(list)
    for row in latest_by_platform.values():
        grouped[int(row["clip_db_id"])].append(row)

    clips: list[ClipPerformance] = []
    for clip_rows in grouped.values():
        first = clip_rows[0]
        views = sum(int(row["views"] or 0) for row in clip_rows)
        likes = sum(int(row["likes"] or 0) for row in clip_rows)
        comments = sum(int(row["comments"] or 0) for row in clip_rows)
        shares = sum(int(row["shares"] or 0) for row in clip_rows)
        watch_time_seconds = sum(int(row["watch_time_seconds"] or 0) for row in clip_rows)
        ctr_values = [float(row["ctr_percent"]) for row in clip_rows if row["ctr_percent"] is not None]
        engagement_values = [
            float(row["engagement_rate"]) for row in clip_rows if row["engagement_rate"] is not None
        ]
        score = float(views) + (likes * 4.0) + (comments * 8.0) + (shares * 10.0)
        if engagement_values:
            score += mean(engagement_values) * 15.0
        clips.append(
            ClipPerformance(
                clip_db_id=int(first["clip_db_id"]),
                title=str(first["clip_title"] or f"Clip {first['clip_db_id']}"),
                streamer_login=str(first["streamer_login"]),
                clip_url=str(first["clip_url"]) if first["clip_url"] else None,
                game_name=str(first["game_name"]) if first["game_name"] else None,
                created_at=str(first["created_at"]) if first["created_at"] else None,
                platforms=tuple(sorted(str(row["platform"]) for row in clip_rows)),
                views=views,
                likes=likes,
                comments=comments,
                shares=shares,
                watch_time_seconds=watch_time_seconds,
                ctr_percent=round(mean(ctr_values), 2) if ctr_values else None,
                engagement_rate=round(mean(engagement_values), 2) if engagement_values else None,
                score=round(score, 2),
            )
        )
    return clips


def _aggregate_streamers(clips: list[ClipPerformance]) -> list[StreamerPerformance]:
    grouped: dict[str, list[ClipPerformance]] = defaultdict(list)
    for clip in clips:
        grouped[clip.streamer_login].append(clip)

    items: list[StreamerPerformance] = []
    for streamer_login, streamer_clips in grouped.items():
        top_clip = max(streamer_clips, key=lambda clip: clip.score, default=None)
        engagement_values = [
            clip.engagement_rate for clip in streamer_clips if clip.engagement_rate is not None
        ]
        items.append(
            StreamerPerformance(
                streamer_login=streamer_login,
                clip_count=len(streamer_clips),
                views=sum(clip.views for clip in streamer_clips),
                likes=sum(clip.likes for clip in streamer_clips),
                comments=sum(clip.comments for clip in streamer_clips),
                shares=sum(clip.shares for clip in streamer_clips),
                watch_time_seconds=sum(clip.watch_time_seconds for clip in streamer_clips),
                engagement_rate=(
                    round(mean(engagement_values), 2) if engagement_values else None
                ),
                top_clip_title=top_clip.title if top_clip else None,
            )
        )
    return sorted(items, key=lambda item: (item.views, item.watch_time_seconds), reverse=True)


def _aggregate_totals(clips: list[ClipPerformance]) -> dict[str, Any]:
    engagement_values = [clip.engagement_rate for clip in clips if clip.engagement_rate is not None]
    return {
        "views": sum(clip.views for clip in clips),
        "likes": sum(clip.likes for clip in clips),
        "comments": sum(clip.comments for clip in clips),
        "shares": sum(clip.shares for clip in clips),
        "watch_time_seconds": sum(clip.watch_time_seconds for clip in clips),
        "engagement_rate": round(mean(engagement_values), 2) if engagement_values else None,
    }


def _coerce_period(
    period_start: datetime | None,
    period_end: datetime | None,
    *,
    default: str,
) -> tuple[datetime, datetime]:
    if period_end is None:
        now = datetime.now(UTC)
        if default == "month":
            anchor = now.replace(day=1, hour=0, minute=0, second=0, microsecond=0)
            period_end = anchor
            previous_month_day = anchor - timedelta(days=1)
            period_start = previous_month_day.replace(day=1, hour=0, minute=0, second=0, microsecond=0)
        else:
            anchor = (now - timedelta(days=now.weekday())).replace(
                hour=0, minute=0, second=0, microsecond=0
            )
            period_end = anchor
            period_start = anchor - timedelta(days=7)
    elif period_start is None:
        period_start = period_end - (timedelta(days=30) if default == "month" else timedelta(days=7))

    assert period_start is not None
    assert period_end is not None
    if period_start.tzinfo is None:
        period_start = period_start.replace(tzinfo=UTC)
    if period_end.tzinfo is None:
        period_end = period_end.replace(tzinfo=UTC)
    return period_end.astimezone(UTC), period_start.astimezone(UTC)


def _format_period(period_start: datetime, period_end: datetime) -> str:
    return (
        f"{period_start.astimezone(UTC):%d.%m.%Y} bis "
        f"{(period_end - timedelta(seconds=1)).astimezone(UTC):%d.%m.%Y}"
    )


def _fmt_pct(value: float | None) -> str:
    if value is None:
        return "n/a"
    return f"{value:.2f}%"


def _format_clip_list(clips: list[ClipPerformance]) -> str:
    lines: list[str] = []
    for index, clip in enumerate(clips, start=1):
        lines.append(
            f"{index}. {clip.title} | streamer={clip.streamer_login} | "
            f"views={clip.views} | likes={clip.likes} | comments={clip.comments} | "
            f"shares={clip.shares} | er={_fmt_pct(clip.engagement_rate)} | "
            f"plattformen={','.join(clip.platforms)}"
        )
    return "\n".join(lines) or "- keine Clips"


def _format_streamer_list(streamers: list[StreamerPerformance]) -> str:
    lines: list[str] = []
    for index, item in enumerate(streamers, start=1):
        lines.append(
            f"{index}. @{item.streamer_login} | clips={item.clip_count} | views={item.views} | "
            f"shares={item.shares} | er={_fmt_pct(item.engagement_rate)} | "
            f"top_clip={item.top_clip_title or 'n/a'}"
        )
    return "\n".join(lines) or "- keine Streamer"


def _fallback_no_data_report(
    *,
    heading: str,
    period_start: datetime,
    period_end: datetime,
    note: str,
) -> str:
    return (
        f"{heading}\n\n"
        f"Zeitraum: {_format_period(period_start, period_end)}\n\n"
        "## Status\n"
        f"{note}\n"
    )


def _fallback_streamer_report(
    streamer_login: str,
    period_start: datetime,
    period_end: datetime,
    top: list[ClipPerformance],
    bottom: list[ClipPerformance],
    totals: dict[str, Any],
) -> str:
    return (
        f"# Wochenreport · @{streamer_login}\n\n"
        f"Zeitraum: {_format_period(period_start, period_end)}\n\n"
        f"## TL;DR\n"
        f"In der Woche kamen {totals['views']} Views und {totals['shares']} Shares zusammen. "
        f"Die durchschnittliche Engagement-Rate lag bei {_fmt_pct(totals['engagement_rate'])}.\n\n"
        f"## Top 5\n{_format_clip_list(top)}\n\n"
        f"## Bottom 5\n{_format_clip_list(bottom)}\n\n"
        "## Massnahmen naechste Woche\n"
        "- Mehr Varianten des staerksten Clip-Musters in den ersten 3 Sekunden testen.\n"
        "- Schwache Clips mit geringer Share-Quote auf Hook und Caption ueberarbeiten.\n"
        "- Plattformen mit niedriger CTR im Dashboard gezielt gegen die Top-Clips vergleichen.\n"
    )


def _fallback_cross_report(
    period_start: datetime,
    period_end: datetime,
    streamers: list[StreamerPerformance],
    clips: list[ClipPerformance],
) -> str:
    top_clips = sorted(clips, key=lambda clip: clip.score, reverse=True)[:8]
    return (
        "# Monatsreport · Cross-Streamer\n\n"
        f"Zeitraum: {_format_period(period_start, period_end)}\n\n"
        "## Gesamtbild\n"
        f"Mit Daten vertreten: {len(streamers)} Streamer.\n\n"
        f"## Streamer-Ranking\n{_format_streamer_list(streamers[:10])}\n\n"
        f"## Top Clips\n{_format_clip_list(top_clips)}\n\n"
        "## Naechste 30 Tage\n"
        "- Erfolgreiche Hook-Muster der Top-Streamer uebertragen.\n"
        "- Schwache Streamer zuerst auf Share- und Comment-Quote optimieren.\n"
        "- 30d-Buckets gegen 7d-Buckets vergleichen, um Ausreisser schnell zu erkennen.\n"
    )


def _fallback_admin_report(
    period_start: datetime,
    period_end: datetime,
    streamers: list[StreamerPerformance],
    top_clips: list[ClipPerformance],
) -> str:
    return (
        "# Admin-Wochenreport · Social Media\n\n"
        f"Zeitraum: {_format_period(period_start, period_end)}\n\n"
        "## Executive Summary\n"
        f"Verwertbare Daten liegen fuer {len(streamers)} Streamer vor.\n\n"
        f"## Auffaellige Streamer\n{_format_streamer_list(streamers[:10])}\n\n"
        f"## Top Clips\n{_format_clip_list(top_clips)}\n\n"
        "## Admin-Aktionen\n"
        "- Ausreisser mit hoher Engagement-Rate fuer Layout-/Title-Patterns markieren.\n"
        "- Streamer mit wenig Views, aber hoher CTR auf mehr Upload-Volumen pushen.\n"
        "- Fehlende Plattformdaten im Analytics-Tab pruefen und OAuth/API-Ausfaelle verfolgen.\n"
    )
