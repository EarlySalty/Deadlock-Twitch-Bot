"""Run a private coaching audit for an authorized Twitch stream or VOD."""

from __future__ import annotations

import argparse
import asyncio
import logging
import os
import sys
from collections import deque
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from bot.stream_coaching_audit.service import (
    AuditError,
    AuditFinding,
    _channel_login_from_source,
    audit_source,
    notify_findings_discord_dm,
    notify_findings_webhook,
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


async def _run(args: argparse.Namespace) -> int:
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
