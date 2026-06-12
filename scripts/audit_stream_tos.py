"""Run a private coaching audit for an authorized Twitch stream or VOD."""

from __future__ import annotations

import argparse
import asyncio
import logging
import os
import sys
import tempfile
import time
from collections import deque
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from bot.stream_coaching_audit import youtube_archive
from bot.stream_coaching_audit.service import (
    AuditError,
    AuditFinding,
    _channel_login_from_source,
    _require_binary,
    _run_output,
    audit_source,
    notify_findings_discord_dm,
    notify_findings_webhook,
    notify_status_discord_dm,
    resolve_latest_vod_url,
)


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Privater Coaching-Audit: Twitch-VOD, Twitch-Live-Kanal oder lokale Aufnahme "
            "transkribieren und auf moegliche TOS-Risiken pruefen."
        )
    )
    parser.add_argument(
        "source",
        nargs="+",
        help="Twitch-VOD-URL, Twitch-Kanal-URL/Login oder lokale Datei",
    )
    parser.add_argument(
        "--authorized",
        action="store_true",
        help="Bestaetigt, dass die Analyse fuer diese Quelle autorisiert ist",
    )
    parser.add_argument(
        "--source-kind",
        choices=("auto", "vod", "live", "file"),
        default="auto",
        help="Quelltyp; Standard: automatisch erkennen",
    )
    parser.add_argument(
        "--live-seconds",
        type=int,
        default=15 * 60,
        help="Aufnahmedauer fuer Live-Streams; Standard: 900 Sekunden",
    )
    parser.add_argument(
        "--watch-live",
        action="store_true",
        help="Prueft einen Live-Kanal fortlaufend in kurzen Fenstern bis Ctrl+C",
    )
    parser.add_argument(
        "--watch-window-seconds",
        type=int,
        default=55,
        help="Audiofenster im Live-Watch; Standard: 55 Sekunden",
    )
    parser.add_argument(
        "--watch-delay-seconds",
        type=float,
        default=2.0,
        help="Pause zwischen Live-Watch-Fenstern; Standard: 2 Sekunden",
    )
    parser.add_argument(
        "--audit-vod-on-end",
        action="store_true",
        help="Nach Stream-Ende automatisch das komplette VOD pruefen (nur mit --watch-live)",
    )
    parser.add_argument(
        "--watch-record",
        action="store_true",
        help=(
            "Kanal beobachten, Live-Stream lokal mitschneiden, nach Stream-Ende privat "
            "zum YouTube-Audit-Account hochladen und via Auto-Captions pruefen"
        ),
    )
    parser.add_argument(
        "--vod-only",
        action="store_true",
        help=(
            "Keinen Live-Mitschnitt anlegen; nach Stream-Ende das Twitch-VOD herunterladen "
            "und pruefen (geringere Serverlast, kein laufendes ffmpeg waehrend des Streams)"
        ),
    )
    parser.add_argument(
        "--vod-wait-seconds",
        type=float,
        default=120.0,
        help="Wartezeit nach Stream-Ende bevor das VOD abgerufen wird; Standard: 120 Sekunden",
    )
    parser.add_argument(
        "--record-format",
        default=os.getenv("STREAM_AUDIT_RECORD_FORMAT") or youtube_archive.DEFAULT_RECORD_FORMAT,
        help="yt-dlp-Format fuer Mitschnitt/VOD-Download; Standard: max. 720p",
    )
    parser.add_argument(
        "--poll-seconds",
        type=float,
        default=60.0,
        help="Live-Poll-Intervall im Watch-Record; Standard: 60 Sekunden",
    )
    parser.add_argument(
        "--caption-poll-seconds",
        type=float,
        default=600.0,
        help="Poll-Intervall fuer YouTube-Auto-Captions; Standard: 600 Sekunden",
    )
    parser.add_argument(
        "--caption-timeout-hours",
        type=float,
        default=24.0,
        help="Maximale Wartezeit auf Auto-Captions, danach Whisper-Fallback; Standard: 24 h",
    )
    parser.add_argument(
        "--min-free-gb",
        type=float,
        default=20.0,
        help="Mindest-freier Speicher fuer neue Mitschnitte; Standard: 20 GB",
    )
    parser.add_argument(
        "--chunk-seconds",
        type=int,
        default=10 * 60,
        help="Audio-Blockgroesse fuer die Transkription; Standard: 600 Sekunden",
    )
    parser.add_argument(
        "--transcriber",
        choices=("faster_whisper", "openai_api"),
        default="faster_whisper",
        help="Voice-to-Text-Engine; Standard: lokal mit faster-whisper",
    )
    parser.add_argument(
        "--llm-provider",
        choices=("none", "minimax"),
        default="none",
        help="Optionale externe Kontextpruefung; lokale Regeln laufen immer",
    )
    parser.add_argument(
        "--allow-remote-transcription",
        action="store_true",
        help="Erlaubt explizit das Senden von Audio an die externe Transkriptions-API",
    )
    parser.add_argument(
        "--allow-remote-llm",
        action="store_true",
        help="Erlaubt explizit das Senden von Transkript-Segmenten an das externe LLM",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path("data/stream_coaching_audits"),
        help="Privates Report-Verzeichnis",
    )
    parser.add_argument(
        "--discord-alerts",
        action="store_true",
        help="Sendet neue redigierte Hinweise an STREAM_AUDIT_DISCORD_WEBHOOK",
    )
    parser.add_argument(
        "--discord-dm",
        action="store_true",
        help="Sendet neue redigierte Hinweise als private Discord-Bot-DM",
    )
    parser.add_argument(
        "--discord-user-id",
        help="Optionales DM-Ziel; Standard: STREAM_AUDIT_DISCORD_USER_ID bzw. Admin-ID",
    )
    return parser


def _finding_key(finding: AuditFinding) -> tuple[str, str]:
    return finding.category, finding.evidence_sha256


def _new_findings(
    findings: tuple[AuditFinding, ...],
    *,
    seen: set[tuple[str, str]],
    seen_order: deque[tuple[str, str]],
    keep: int = 200,
) -> list[AuditFinding]:
    result: list[AuditFinding] = []
    for finding in findings:
        key = _finding_key(finding)
        if key in seen:
            continue
        result.append(finding)
        seen.add(key)
        seen_order.append(key)
        while len(seen_order) > keep:
            seen.discard(seen_order.popleft())
    return result


def _print_findings(findings: list[AuditFinding]) -> None:
    for finding in findings:
        minutes, seconds = divmod(max(0, int(finding.start_seconds)), 60)
        print(
            f"ALERT +{minutes:02d}:{seconds:02d} [{finding.severity}] "
            f"{finding.category}: {finding.evidence_redacted}",
            flush=True,
        )


async def _run_once(
    args: argparse.Namespace,
    *,
    source: str,
    live_seconds: int | None = None,
):
    return await audit_source(
        source,
        authorized=args.authorized,
        source_kind="live" if args.watch_live else args.source_kind,
        live_seconds=live_seconds if live_seconds is not None else args.live_seconds,
        chunk_seconds=args.chunk_seconds,
        transcriber_engine=args.transcriber,
        llm_provider=args.llm_provider,
        allow_remote_transcription=args.allow_remote_transcription,
        allow_remote_llm=args.allow_remote_llm,
        output_dir=args.output_dir,
    )


def _is_offline_error(message: str) -> bool:
    """Erkennt am Fehlertext, ob der Kanal offline ist (Stream-Ende)."""
    text = message.lower()
    return any(
        marker in text
        for marker in (
            "offline",
            "not currently live",
            "is not live",
            "keine live-stream-url",
            "eventuell offline",
        )
    )


async def _audit_latest_vod(
    args: argparse.Namespace,
    *,
    login: str,
    source: str,
    audited: set[str],
) -> bool:
    """Neuestes VOD komplett pruefen. True = Trigger erledigt (auch wenn schon auditiert)."""
    vod_url = await asyncio.to_thread(resolve_latest_vod_url, login)
    if not vod_url:
        print(f"Live-Watch {source}: Stream beendet, aber kein VOD abrufbar - warte", flush=True)
        return False
    if vod_url in audited:
        return True
    print(f"Live-Watch {source}: Stream beendet -> VOD-Komplettlauf {vod_url}", flush=True)
    try:
        report, _json_path, markdown_path = await audit_source(
            vod_url,
            authorized=args.authorized,
            source_kind="vod",
            chunk_seconds=args.chunk_seconds,
            transcriber_engine=args.transcriber,
            llm_provider=args.llm_provider,
            allow_remote_transcription=args.allow_remote_transcription,
            allow_remote_llm=args.allow_remote_llm,
            output_dir=args.output_dir,
        )
    except AuditError as exc:
        print(f"Live-Watch {source}: VOD-Komplettlauf fehlgeschlagen: {exc}", flush=True)
        return False
    audited.add(vod_url)
    print(
        f"VOD-Komplettlauf fertig ({login}): {len(report.findings)} Fundstelle(n) | {markdown_path}",
        flush=True,
    )
    if args.discord_dm and report.findings:
        sent = await notify_findings_discord_dm(
            report.source_label,
            report.findings,
            discord_user_id=args.discord_user_id,
            source_url=vod_url,
        )
        if not sent:
            print("WARN Discord-Bot konnte VOD-DM nicht senden", flush=True)
    return True


async def _run_live_source(args: argparse.Namespace, source: str) -> None:
    seen: set[tuple[str, str]] = set()
    seen_order: deque[tuple[str, str]] = deque()
    window_seconds = max(30, int(args.watch_window_seconds))
    delay_seconds = max(0.0, float(args.watch_delay_seconds))
    login = _channel_login_from_source(source)
    audited_vods: set[str] = set()
    was_live = False
    pending_vod = False
    print(f"Live-Watch aktiv: {source} | Fenster={window_seconds}s", flush=True)
    while True:
        try:
            # Wanduhr-Zeitpunkt des Fensterbeginns -> echte Uhrzeit der Aeusserung
            capture_start = datetime.now()
            report, _json_path, markdown_path = await _run_once(
                args,
                source=source,
                live_seconds=window_seconds,
            )
            was_live = True
            pending_vod = False
            findings = _new_findings(report.findings, seen=seen, seen_order=seen_order)
            if findings:
                _print_findings(findings)
                print(f"Privater Report: {markdown_path}", flush=True)
                if args.discord_alerts:
                    sent = await notify_findings_webhook(report.source_label, findings)
                    if not sent:
                        print("WARN Discord-Webhook konnte Hinweis nicht senden", flush=True)
                if args.discord_dm:
                    sent = await notify_findings_discord_dm(
                        report.source_label,
                        findings,
                        discord_user_id=args.discord_user_id,
                        occurred_base=capture_start,
                        source_url=source,
                    )
                    if not sent:
                        print("WARN Discord-Bot konnte DM nicht senden", flush=True)
            else:
                print(f"Live-Watch {source}: Fenster geprueft, keine neue Fundstelle", flush=True)
            await asyncio.sleep(delay_seconds)
        except AuditError as exc:
            # Stream-Ende erkannt -> einmalig das komplette VOD nachpruefen
            if (
                args.audit_vod_on_end
                and login
                and (was_live or pending_vod)
                and _is_offline_error(str(exc))
            ):
                pending_vod = True
                was_live = False
                if await _audit_latest_vod(args, login=login, source=source, audited=audited_vods):
                    pending_vod = False
            print(f"WARN Live-Watch {source}: {exc}; neuer Versuch in 30s", flush=True)
            await asyncio.sleep(30)


async def _run_live_watch(args: argparse.Namespace) -> int:
    if not args.authorized:
        raise AuditError("Live-Watch nur mit --authorized erlaubt")
    if not os.getenv("STREAM_AUDIT_DISCORD_WEBHOOK") and args.discord_alerts:
        raise AuditError("--discord-alerts braucht STREAM_AUDIT_DISCORD_WEBHOOK")

    print(
        f"Live-Watch aktiv fuer {len(args.source)} Kanal/Kanaele | Ctrl+C beendet",
        flush=True,
    )
    await asyncio.gather(*(_run_live_source(args, source) for source in args.source))
    return 0


def _is_channel_live(login: str) -> bool:
    """Schneller Live-Check via yt-dlp; False bei offline oder Aufloesungsfehler."""
    yt_dlp = _require_binary("STREAM_AUDIT_YTDLP_BIN", "yt-dlp")
    try:
        output = _run_output(
            [yt_dlp, "--no-playlist", "--format", "worst", "--get-url", f"https://twitch.tv/{login}"]
        )
    except AuditError as exc:
        if not _is_offline_error(str(exc)):
            print(f"WARN Live-Check {login}: {exc}", flush=True)
        return False
    return bool(output.strip())


async def _notify_status_safe(args: argparse.Namespace, content: str) -> None:
    if not args.discord_dm:
        return
    try:
        await notify_status_discord_dm(content, discord_user_id=args.discord_user_id)
    except AuditError as exc:
        print(f"WARN Status-DM nicht moeglich: {exc}", flush=True)


async def _process_recording(
    args: argparse.Namespace,
    *,
    login: str,
    media_path: Path,
    prefer_vod: bool,
    pending: set[Path],
) -> None:
    """Mitschnitt (oder VOD-Fallback) hochladen, auditieren, bei Erfolg lokal loeschen."""
    try:
        recording_valid = False
        if media_path.is_file() and media_path.stat().st_size > 5 * 1024 * 1024:
            try:
                duration = await asyncio.to_thread(youtube_archive.probe_duration_seconds, media_path)
                recording_valid = duration >= 60.0
            except AuditError as exc:
                print(f"WARN {login}: Mitschnitt nicht lesbar ({exc})", flush=True)

        media = media_path
        vod_url: str | None = None
        if prefer_vod or not recording_valid:
            vod_url = await asyncio.to_thread(resolve_latest_vod_url, login)
            if vod_url:
                vod_path = media_path.with_name(media_path.stem + "-vod.mp4")
                try:
                    media = await asyncio.to_thread(
                        youtube_archive.download_vod_video,
                        vod_url,
                        vod_path,
                        record_format=args.record_format,
                    )
                    print(f"{login}: VOD-Fallback geladen ({vod_url})", flush=True)
                except AuditError as exc:
                    print(f"WARN {login}: VOD-Download fehlgeschlagen ({exc})", flush=True)
                    vod_url = None
                    media = media_path
            if media is media_path and not recording_valid:
                raise AuditError("weder verwertbarer Mitschnitt noch abrufbares VOD")

        title = f"[Audit] {login} {datetime.now().strftime('%Y-%m-%d %H:%M')}"
        source_label = vod_url or f"https://twitch.tv/{login}"
        with tempfile.TemporaryDirectory(prefix="stream-audit-yt-") as tmp:
            report, markdown_path, watch_url = await youtube_archive.audit_media_via_youtube(
                media,
                source_label=source_label,
                channel_login=login,
                workdir=Path(tmp),
                llm_provider=args.llm_provider,
                output_dir=args.output_dir,
                caption_poll_seconds=args.caption_poll_seconds,
                caption_timeout_seconds=args.caption_timeout_hours * 3600.0,
                chunk_seconds=args.chunk_seconds,
                upload_title=title,
            )
        print(
            f"Archiv-Audit fertig ({login}): {len(report.findings)} Fundstelle(n) | "
            f"{watch_url} | {markdown_path}",
            flush=True,
        )
        if args.discord_dm and report.findings:
            sent = await notify_findings_discord_dm(
                report.source_label,
                report.findings,
                discord_user_id=args.discord_user_id,
                source_url=watch_url,
            )
            if not sent:
                print("WARN Discord-Bot konnte Archiv-DM nicht senden", flush=True)
        await _notify_status_safe(
            args,
            f"Stream-Archiv {login}: {watch_url} | {len(report.findings)} Fundstelle(n) | "
            f"Transkript: {report.transcriber_engine}",
        )
        # YouTube ist jetzt das Archiv - lokale Dateien nur nach vollem Erfolg loeschen
        media_path.unlink(missing_ok=True)
        if media != media_path:
            media.unlink(missing_ok=True)
    except AuditError as exc:
        print(f"WARN Verarbeitung {login}: {exc} - Datei bleibt fuer Retry liegen", flush=True)
        await _notify_status_safe(args, f"Stream-Archiv {login} fehlgeschlagen: {exc}")
    finally:
        pending.discard(media_path)


async def _record_channel_loop(args: argparse.Namespace, source: str) -> None:
    login = _channel_login_from_source(source)
    if not login:
        raise AuditError(f"Watch-Record braucht Twitch-Login oder Kanal-URL: {source}")
    recordings_dir = args.output_dir / "recordings" / login
    recordings_dir.mkdir(parents=True, exist_ok=True)
    pending: set[Path] = set()
    tasks: set[asyncio.Task] = set()

    def _spawn_processing(path: Path, *, prefer_vod: bool) -> None:
        if path in pending:
            return
        pending.add(path)
        task = asyncio.create_task(
            _process_recording(args, login=login, media_path=path, prefer_vod=prefer_vod, pending=pending)
        )
        tasks.add(task)
        task.add_done_callback(tasks.discard)

    # Liegengebliebene Mitschnitte aus frueheren Laeufen nachziehen. Halbfertige
    # VOD-Zwischendateien vorher entsorgen, sonst wuerde derselbe Stream doppelt
    # hochgeladen (Original + VOD-Variante).
    for stale_vod in sorted(recordings_dir.glob("*-vod.mp4")):
        stale_vod.unlink(missing_ok=True)
    for leftover in sorted(recordings_dir.glob("*.mp4")):
        print(f"{login}: verarbeite liegengebliebenen Mitschnitt {leftover.name}", flush=True)
        _spawn_processing(leftover, prefer_vod=False)

    poll_seconds = max(15.0, float(args.poll_seconds))
    first_poll = True
    session_notified = False
    print(f"Watch-Record aktiv: {source} | Poll={poll_seconds:.0f}s", flush=True)
    while True:
        live = await asyncio.to_thread(_is_channel_live, login)
        if not live:
            first_poll = False
            session_notified = False
            await asyncio.sleep(poll_seconds)
            continue
        # Lief der Stream schon beim Watcher-Start, fehlt der Anfang -> spaeter VOD bevorzugen
        partial = first_poll
        first_poll = False
        try:
            youtube_archive.ensure_disk_space(recordings_dir, min_free_gb=args.min_free_gb)
        except AuditError as exc:
            print(f"WARN {login}: {exc}", flush=True)
            await _notify_status_safe(args, f"Aufnahme {login} uebersprungen: {exc}")
            await asyncio.sleep(max(300.0, poll_seconds))
            continue
        media_path = recordings_dir / f"{login}-{datetime.now().strftime('%Y%m%d-%H%M%S')}.mp4"
        print(f"{login}: live -> Mitschnitt startet ({media_path.name})", flush=True)
        if not session_notified:
            await _notify_status_safe(args, f"Aufnahme gestartet: {login}")
            session_notified = True
        record_start = time.monotonic()
        try:
            await asyncio.to_thread(
                youtube_archive.record_live_stream,
                login,
                media_path,
                record_format=args.record_format,
            )
        except AuditError as exc:
            print(f"WARN Mitschnitt {login}: {exc}", flush=True)
            await asyncio.sleep(poll_seconds)
            continue
        session_notified = False
        recorded_wall = time.monotonic() - record_start
        print(f"{login}: Stream zu Ende ({recorded_wall / 60:.0f} min aufgenommen)", flush=True)
        _spawn_processing(media_path, prefer_vod=partial or recorded_wall < 120.0)
        await asyncio.sleep(poll_seconds)


async def _vod_channel_loop(args: argparse.Namespace, source: str) -> None:
    """Pollt den Kanal; nach Stream-Ende wird das VOD heruntergeladen und geprueft."""
    login = _channel_login_from_source(source)
    if not login:
        raise AuditError(f"VOD-only braucht Twitch-Login oder Kanal-URL: {source}")
    processed_vods: set[str] = set()
    was_live = False
    session_notified = False
    poll_seconds = max(15.0, float(args.poll_seconds))
    vod_wait = max(30.0, float(args.vod_wait_seconds))
    print(f"VOD-only Watch aktiv: {source} | Poll={poll_seconds:.0f}s", flush=True)
    while True:
        live = await asyncio.to_thread(_is_channel_live, login)
        if live:
            if not session_notified:
                print(f"{login}: Stream live erkannt, warte auf Stream-Ende fuer VOD-Download", flush=True)
                await _notify_status_safe(args, f"Stream live: {login} - VOD wird nach Ende heruntergeladen")
                session_notified = True
            was_live = True
            await asyncio.sleep(poll_seconds)
            continue
        if was_live:
            was_live = False
            session_notified = False
            print(f"{login}: Stream beendet, warte {vod_wait:.0f}s auf VOD-Verfuegbarkeit", flush=True)
            await asyncio.sleep(vod_wait)
            vod_url = await asyncio.to_thread(resolve_latest_vod_url, login)
            if not vod_url:
                print(f"WARN {login}: kein VOD abrufbar nach Stream-Ende", flush=True)
                await asyncio.sleep(poll_seconds)
                continue
            if vod_url in processed_vods:
                print(f"{login}: VOD bereits verarbeitet ({vod_url})", flush=True)
                await asyncio.sleep(poll_seconds)
                continue
            processed_vods.add(vod_url)
            print(f"{login}: lade VOD {vod_url}", flush=True)
            await _notify_status_safe(args, f"VOD-Download gestartet: {login} ({vod_url})")
            output_dir = args.output_dir / "recordings" / login
            output_dir.mkdir(parents=True, exist_ok=True)
            vod_path = output_dir / f"{login}-vod-{datetime.now().strftime('%Y%m%d-%H%M%S')}.mp4"
            try:
                media = await asyncio.to_thread(
                    youtube_archive.download_vod_video,
                    vod_url,
                    vod_path,
                    record_format=args.record_format,
                )
                print(f"{login}: VOD geladen ({media})", flush=True)
            except AuditError as exc:
                print(f"WARN {login}: VOD-Download fehlgeschlagen: {exc}", flush=True)
                await _notify_status_safe(args, f"VOD-Download fehlgeschlagen: {login}: {exc}")
                await asyncio.sleep(poll_seconds)
                continue
            try:
                report, _json_path, markdown_path = await audit_source(
                    str(vod_path),
                    authorized=args.authorized,
                    source_kind="file",
                    chunk_seconds=args.chunk_seconds,
                    transcriber_engine=args.transcriber,
                    llm_provider=args.llm_provider,
                    allow_remote_transcription=args.allow_remote_transcription,
                    allow_remote_llm=args.allow_remote_llm,
                    output_dir=args.output_dir,
                )
                print(
                    f"VOD-Audit fertig ({login}): {len(report.findings)} Fundstelle(n) | {markdown_path}",
                    flush=True,
                )
                if args.discord_dm and report.findings:
                    sent = await notify_findings_discord_dm(
                        vod_url,
                        report.findings,
                        discord_user_id=args.discord_user_id,
                        source_url=vod_url,
                    )
                    if not sent:
                        print("WARN Discord-Bot konnte VOD-DM nicht senden", flush=True)
                await _notify_status_safe(
                    args,
                    f"VOD-Audit {login}: {len(report.findings)} Fundstelle(n) | {markdown_path.name}",
                )
            except AuditError as exc:
                print(f"WARN {login}: VOD-Audit fehlgeschlagen: {exc}", flush=True)
            finally:
                vod_path.unlink(missing_ok=True)
        await asyncio.sleep(poll_seconds)


async def _run_vod_watch(args: argparse.Namespace) -> int:
    if not args.authorized:
        raise AuditError("VOD-only Watch nur mit --authorized erlaubt")
    print(
        f"VOD-only Watch aktiv fuer {len(args.source)} Kanal/Kanaele | Ctrl+C beendet",
        flush=True,
    )
    await asyncio.gather(*(_vod_channel_loop(args, source) for source in args.source))
    return 0


async def _run_record_watch(args: argparse.Namespace) -> int:
    if not args.authorized:
        raise AuditError("Watch-Record nur mit --authorized erlaubt")
    if args.llm_provider != "none" and not args.allow_remote_llm:
        raise AuditError("Externe LLM-Pruefung braucht --allow-remote-llm")
    if youtube_archive.load_credentials() is None:
        print(
            "WARN YouTube-Audit-Account nicht eingerichtet "
            "(scripts/setup_youtube_audit_oauth.py) - Mitschnitte bleiben liegen, "
            "Upload klappt erst nach dem Setup",
            flush=True,
        )
    print(
        f"Watch-Record aktiv fuer {len(args.source)} Kanal/Kanaele | Ctrl+C beendet",
        flush=True,
    )
    await asyncio.gather(*(_record_channel_loop(args, source) for source in args.source))
    return 0


async def _run(args: argparse.Namespace) -> int:
    modes = sum([bool(args.watch_record), bool(args.watch_live), bool(args.vod_only)])
    if modes > 1:
        raise AuditError("--watch-record, --watch-live und --vod-only schliessen sich gegenseitig aus")
    if args.vod_only:
        return await _run_vod_watch(args)
    if args.watch_record:
        return await _run_record_watch(args)
    if args.watch_live:
        return await _run_live_watch(args)
    if len(args.source) != 1:
        raise AuditError("Mehrere Quellen sind nur zusammen mit --watch-live erlaubt")
    source = args.source[0]
    report, json_path, markdown_path = await _run_once(args, source=source)
    print(f"Audit fertig: {len(report.findings)} Fundstelle(n)")
    print(f"JSON: {json_path}")
    print(f"Markdown: {markdown_path}")
    if args.discord_alerts and report.findings:
        if not await notify_findings_webhook(report.source_label, report.findings):
            print("WARN Discord-Webhook konnte Hinweis nicht senden", flush=True)
    if args.discord_dm and report.findings:
        sent = await notify_findings_discord_dm(
            report.source_label,
            report.findings,
            discord_user_id=args.discord_user_id,
            source_url=source,
        )
        if not sent:
            print("WARN Discord-Bot konnte DM nicht senden", flush=True)
    return 0


def main() -> int:
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")
    # Der statische ffmpeg in ~/.local/bin segfaultet beim Twitch-HLS-Mitschnitt;
    # daher per Default den funktionierenden System-ffmpeg nutzen (ueberschreibbar).
    os.environ.setdefault("FFMPEG_BIN", "/usr/bin/ffmpeg")
    os.environ.setdefault("FFPROBE_BIN", "/usr/bin/ffprobe")
    args = _parser().parse_args()
    try:
        return asyncio.run(_run(args))
    except KeyboardInterrupt:
        return 130
    except AuditError as exc:
        print(f"Audit fehlgeschlagen: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
