"""Private, evidence-based coaching audits for authorized Twitch recordings."""

from __future__ import annotations

import asyncio
import hashlib
import json
import logging
import os
import re
import shutil
import subprocess
import tempfile
from dataclasses import asdict, dataclass
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any, Iterable, Sequence
from urllib.parse import urlparse

log = logging.getLogger("TwitchStreams.StreamCoachingAudit")

DEFAULT_OUTPUT_DIR = Path("data/stream_coaching_audits")
DEFAULT_LIVE_SECONDS = 15 * 60
DEFAULT_CHUNK_SECONDS = 10 * 60
MAX_LIVE_SECONDS = 6 * 60 * 60
DEFAULT_ADMIN_DISCORD_USER_ID = "662995601738170389"  # nosemgrep: discord-client-id

_N_WORD_RE = re.compile(
    r"\bn[\W_]*[i1!|][\W_]*g[\W_]*g[\W_]*(?:e[\W_]*r|a)(?:[\W_]*s)?\b",
    re.IGNORECASE,
)
_HOMOPHOBIC_SLUR_RE = re.compile(
    r"\b(?:f[\W_]*a[\W_]*g(?:[\W_]*g[\W_]*o[\W_]*t)?|schwuchtel)\w*\b",
    re.IGNORECASE,
)
_THREAT_RE = re.compile(
    r"\b(?:ich\s+(?:bring|mach)\s+dich\s+um|kill\s+yourself|kys)\b",
    re.IGNORECASE,
)
_RULES = (
    (
        _N_WORD_RE,
        "hate_speech_slur",
        "high",
        "Moegliche rassistische Beleidigung. Kontext und Transkript manuell pruefen.",
    ),
    (
        _HOMOPHOBIC_SLUR_RE,
        "hate_speech_slur",
        "high",
        "Moegliche diskriminierende Beleidigung. Kontext und Transkript manuell pruefen.",
    ),
    (
        _THREAT_RE,
        "threat_or_self_harm",
        "medium",
        "Moegliche Drohung oder Aufforderung zur Selbstverletzung. Kontext manuell pruefen.",
    ),
)


class AuditError(RuntimeError):
    """Audit could not be completed."""


@dataclass(frozen=True, slots=True)
class AuditSegment:
    segment_id: str
    start_seconds: float
    end_seconds: float
    text: str


@dataclass(frozen=True, slots=True)
class AuditFinding:
    segment_id: str
    start_seconds: float
    end_seconds: float
    category: str
    severity: str
    detector: str
    confidence: str
    reason: str
    evidence_redacted: str
    evidence_sha256: str
    evidence_raw: str = ""


@dataclass(frozen=True, slots=True)
class AuditReport:
    report_id: str
    created_at: str
    source_type: str
    source_label: str
    channel_login: str | None
    transcriber_engine: str
    transcriber_model: str | None
    llm_provider: str
    raw_transcript_persisted: bool
    analyzed_segments: int
    findings: tuple[AuditFinding, ...]

    def to_dict(self) -> dict[str, Any]:
        payload = asdict(self)
        payload["findings"] = [asdict(finding) for finding in self.findings]
        return payload


@dataclass(slots=True)
class _AcquiredMedia:
    path: Path
    source_type: str
    source_label: str
    channel_login: str | None = None
    cleanup: Any | None = None


def _collapse_space(text: str) -> str:
    return " ".join(str(text or "").split())


def redact_text(text: str) -> str:
    """Redact known slurs before evidence leaves the transient transcript."""
    redacted = str(text or "")
    for pattern, *_unused in _RULES:
        redacted = pattern.sub("[REDACTED]", redacted)
    return _collapse_space(redacted)


def _evidence_hash(text: str) -> str:
    return hashlib.sha256(str(text or "").encode("utf-8")).hexdigest()


def _evidence_excerpt(text: str, match: re.Match[str] | None = None) -> str:
    if match is None:
        excerpt = text[:240]
    else:
        excerpt = text[max(0, match.start() - 100) : match.end() + 100]
    return redact_text(excerpt)


def _evidence_excerpt_raw(text: str, match: re.Match[str] | None = None) -> str:
    """Wie _evidence_excerpt, aber UNMASKIERT - nur fuer den privaten Admin-Beleg."""
    if match is None:
        excerpt = text[:240]
    else:
        excerpt = text[max(0, match.start() - 100) : match.end() + 100]
    return _collapse_space(excerpt)


def _vod_jump_url(source_url: str | None, seconds: float) -> str:
    """Twitch-VOD-Sprunglink auf die Fundstelle (?t=1h2m3s); leer wenn kein VOD."""
    if not source_url or "/videos/" not in source_url:
        return ""
    total = max(0, int(seconds))
    hours, remainder = divmod(total, 3600)
    minutes, secs = divmod(remainder, 60)
    base = source_url.split("?", 1)[0]
    return f"{base}?t={hours}h{minutes}m{secs}s"


def detect_rule_findings(segments: Iterable[AuditSegment]) -> list[AuditFinding]:
    """Return high-signal local findings without sending transcripts to a third party."""
    findings: list[AuditFinding] = []
    for segment in segments:
        for pattern, category, severity, reason in _RULES:
            for match in pattern.finditer(segment.text):
                findings.append(
                    AuditFinding(
                        segment_id=segment.segment_id,
                        start_seconds=segment.start_seconds,
                        end_seconds=segment.end_seconds,
                        category=category,
                        severity=severity,
                        detector="local_rule",
                        confidence="high",
                        reason=reason,
                        evidence_redacted=_evidence_excerpt(segment.text, match),
                        evidence_sha256=_evidence_hash(segment.text),
                        evidence_raw=_evidence_excerpt_raw(segment.text, match),
                    )
                )
    return findings


def _extract_json_object(raw: str) -> dict[str, Any]:
    cleaned = re.sub(r"<think>.*?</think>", "", str(raw or ""), flags=re.DOTALL | re.IGNORECASE)
    match = re.search(r"\{.*\}", cleaned, flags=re.DOTALL)
    if not match:
        raise AuditError("LLM lieferte kein JSON-Objekt")
    try:
        payload = json.loads(match.group())
    except json.JSONDecodeError as exc:
        raise AuditError("LLM lieferte ungueltiges JSON") from exc
    if not isinstance(payload, dict):
        raise AuditError("LLM-Antwort ist kein JSON-Objekt")
    return payload


def _segment_batches(
    segments: Sequence[AuditSegment],
    *,
    max_chars: int = 12_000,
) -> list[list[AuditSegment]]:
    batches: list[list[AuditSegment]] = []
    current: list[AuditSegment] = []
    current_chars = 0
    for segment in segments:
        segment_chars = len(segment.text) + 120
        if current and current_chars + segment_chars > max_chars:
            batches.append(current)
            current = []
            current_chars = 0
        current.append(segment)
        current_chars += segment_chars
    if current:
        batches.append(current)
    return batches


async def _detect_llm_findings_minimax(segments: Sequence[AuditSegment]) -> list[AuditFinding]:
    from bot.core.llm_providers import get_minimax_client

    client = get_minimax_client(timeout=60.0, async_client=True)
    model = os.getenv("STREAM_AUDIT_MINIMAX_MODEL") or "MiniMax-M3"
    findings: list[AuditFinding] = []
    for batch in _segment_batches(segments):
        prompt_segments = [
            {
                "id": segment.segment_id,
                "start_seconds": round(segment.start_seconds, 2),
                "end_seconds": round(segment.end_seconds, 2),
                "text": segment.text,
            }
            for segment in batch
        ]
        response = await client.chat.completions.create(
            model=model,
            messages=[
                {
                    "role": "system",
                    "content": (
                        "Du pruefst autorisierte Stream-Transkripte fuer privates Coaching. "
                        "Finde nur wahrscheinliche Twitch-Sicherheitsrisiken: hate_speech_slur, "
                        "harassment, threat_or_self_harm, sexual_content oder discriminatory_speech. "
                        "Nicht automatisch flaggen: Zitate zur Kritik, Songtexte, Diskussionen ueber "
                        "Moderation oder unsichere Transkriptionsfehler. Antworte ausschliesslich als "
                        'JSON: {"findings":[{"segment_id":"s00001","category":"harassment",'
                        '"severity":"low|medium|high","confidence":"low|medium|high",'
                        '"reason":"kurze sachliche Begruendung"}]}. '
                        "Erfinde keine IDs und zitiere keine problematischen Begriffe."
                    ),
                },
                {
                    "role": "user",
                    "content": json.dumps({"segments": prompt_segments}, ensure_ascii=False),
                },
            ],
            max_tokens=1200,
            temperature=0.0,
        )
        raw = response.choices[0].message.content if response.choices else ""
        payload = _extract_json_object(raw or "")
        by_id = {segment.segment_id: segment for segment in batch}
        for item in payload.get("findings") or []:
            if not isinstance(item, dict):
                continue
            segment = by_id.get(str(item.get("segment_id") or ""))
            if segment is None:
                continue
            category = str(item.get("category") or "other").strip().lower()
            severity = str(item.get("severity") or "medium").strip().lower()
            confidence = str(item.get("confidence") or "medium").strip().lower()
            if severity not in {"low", "medium", "high"}:
                severity = "medium"
            if confidence not in {"low", "medium", "high"}:
                confidence = "medium"
            findings.append(
                AuditFinding(
                    segment_id=segment.segment_id,
                    start_seconds=segment.start_seconds,
                    end_seconds=segment.end_seconds,
                    category=category[:80],
                    severity=severity,
                    detector="minimax",
                    confidence=confidence,
                    reason=_collapse_space(str(item.get("reason") or "LLM-Pruefung erforderlich"))[
                        :300
                    ],
                    evidence_redacted=_evidence_excerpt(segment.text),
                    evidence_sha256=_evidence_hash(segment.text),
                    evidence_raw=_evidence_excerpt_raw(segment.text),
                )
            )
    return findings


def _merge_findings(findings: Iterable[AuditFinding]) -> tuple[AuditFinding, ...]:
    severity_rank = {"low": 1, "medium": 2, "high": 3}
    merged: dict[tuple[str, str], AuditFinding] = {}
    for finding in findings:
        key = (finding.segment_id, finding.category)
        current = merged.get(key)
        if current is None or severity_rank.get(finding.severity, 0) > severity_rank.get(
            current.severity, 0
        ):
            merged[key] = finding
    return tuple(
        sorted(
            merged.values(),
            key=lambda item: (item.start_seconds, -severity_rank.get(item.severity, 0)),
        )
    )


def _classify_source(source: str, source_kind: str) -> str:
    if source_kind != "auto":
        return source_kind
    if Path(source).expanduser().is_file():
        return "file"
    parsed = urlparse(source if "://" in source else f"https://twitch.tv/{source}")
    if parsed.netloc.lower() not in {"twitch.tv", "www.twitch.tv", "m.twitch.tv"}:
        raise AuditError("Nur lokale Dateien oder Twitch-URLs sind erlaubt")
    return "vod" if re.fullmatch(r"/videos/\d+/?", parsed.path) else "live"


def _channel_login_from_source(source: str) -> str | None:
    parsed = urlparse(source if "://" in source else f"https://twitch.tv/{source}")
    if parsed.netloc.lower() not in {"twitch.tv", "www.twitch.tv", "m.twitch.tv"}:
        return None
    path_parts = [part for part in parsed.path.split("/") if part]
    if len(path_parts) != 1 or path_parts[0].lower() == "videos":
        return None
    return path_parts[0].lower()


def _require_binary(env_name: str, default: str) -> str:
    configured = os.getenv(env_name) or default
    resolved = shutil.which(configured)
    if not resolved:
        raise AuditError(f"Binary nicht gefunden: {configured}")
    return resolved


def _run_checked(args: Sequence[str]) -> None:
    try:
        subprocess.run(
            list(args),
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
    except FileNotFoundError as exc:
        raise AuditError(f"Binary nicht gefunden: {args[0]}") from exc
    except subprocess.CalledProcessError as exc:
        stderr = (exc.stderr or b"").decode("utf-8", "ignore").strip()
        raise AuditError(f"Command fehlgeschlagen: {stderr[-500:]}") from exc


def _run_output(args: Sequence[str]) -> str:
    try:
        result = subprocess.run(
            list(args),
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
    except FileNotFoundError as exc:
        raise AuditError(f"Binary nicht gefunden: {args[0]}") from exc
    except subprocess.CalledProcessError as exc:
        stderr = str(exc.stderr or "").strip()
        raise AuditError(f"Command fehlgeschlagen: {stderr[-500:]}") from exc
    return str(result.stdout or "").strip()


def _download_vod(source: str, workdir: Path) -> Path:
    yt_dlp = _require_binary("STREAM_AUDIT_YTDLP_BIN", "yt-dlp")
    output_template = str(workdir / "source.%(ext)s")
    # Nur den Ton holen (Twitch: "Audio_Only", ~286 MB/3h statt ~8 GB Video).
    # Fuer die Transkription reicht Audio; faellt auf "worst" zurueck, falls
    # kein reiner Audio-Stream existiert.
    _run_checked(
        [
            yt_dlp,
            "--no-playlist",
            "--format",
            "bestaudio/worst",
            "--output",
            output_template,
            source,
        ]
    )
    candidates = sorted(workdir.glob("source.*"))
    if not candidates:
        raise AuditError("yt-dlp lieferte keine VOD-Datei")
    return candidates[0]


def _capture_live_audio(login: str, workdir: Path, *, duration_seconds: int) -> Path:
    """Resolve the current Twitch HLS URL and record a short audio-only window."""
    yt_dlp = _require_binary("STREAM_AUDIT_YTDLP_BIN", "yt-dlp")
    ffmpeg = _require_binary("FFMPEG_BIN", "ffmpeg")
    manifest_output = _run_output(
        [
            yt_dlp,
            "--no-playlist",
            "--format",
            "worst",
            "--get-url",
            f"https://twitch.tv/{login}",
        ]
    )
    manifests = [line.strip() for line in manifest_output.splitlines() if line.strip()]
    if not manifests:
        raise AuditError("yt-dlp lieferte keine Live-Stream-URL; Kanal eventuell offline")

    output_path = workdir / "live-audio.wav"
    _run_checked(
        [
            ffmpeg,
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            manifests[-1],
            "-t",
            str(max(30, min(int(duration_seconds), MAX_LIVE_SECONDS))),
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
            str(output_path),
        ]
    )
    if not output_path.is_file() or output_path.stat().st_size < 32 * 1024:
        raise AuditError("Live-Capture lieferte keine verwertbare Audiodatei")
    return output_path


async def _acquire_media(
    source: str,
    *,
    source_kind: str,
    live_seconds: int,
    workdir: Path,
) -> _AcquiredMedia:
    kind = _classify_source(source, source_kind)
    if kind == "file":
        path = Path(source).expanduser().resolve()
        if not path.is_file():
            raise AuditError(f"Lokale Datei nicht gefunden: {path}")
        return _AcquiredMedia(path=path, source_type="file", source_label=path.name)
    if kind == "vod":
        parsed = urlparse(source)
        if parsed.netloc.lower() not in {"twitch.tv", "www.twitch.tv", "m.twitch.tv"}:
            raise AuditError("VOD muss eine Twitch-URL sein")
        path = await asyncio.to_thread(_download_vod, source, workdir)
        return _AcquiredMedia(path=path, source_type="vod", source_label=source)
    if kind == "live":
        login = _channel_login_from_source(source)
        if not login:
            raise AuditError("Live-Quelle muss Twitch-Login oder Twitch-Kanal-URL sein")
        duration = max(30, min(int(live_seconds), MAX_LIVE_SECONDS))
        path = await asyncio.to_thread(
            _capture_live_audio,
            login,
            workdir,
            duration_seconds=duration,
        )
        return _AcquiredMedia(
            path=path,
            source_type="live",
            source_label=f"https://twitch.tv/{login}",
            channel_login=login,
        )
    raise AuditError(f"Unbekannter source-kind: {kind}")


def _split_audio(media_path: Path, chunk_dir: Path, *, chunk_seconds: int) -> list[Path]:
    chunk_dir.mkdir(parents=True, exist_ok=True)
    ffmpeg = _require_binary("FFMPEG_BIN", "ffmpeg")
    _run_checked(
        [
            ffmpeg,
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            str(media_path),
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
            "-f",
            "segment",
            "-segment_time",
            str(max(30, int(chunk_seconds))),
            "-reset_timestamps",
            "1",
            str(chunk_dir / "chunk-%05d.wav"),
        ]
    )
    chunks = sorted(chunk_dir.glob("chunk-*.wav"))
    if not chunks:
        raise AuditError("ffmpeg lieferte keine Audio-Bloecke")
    return chunks


def _transcription_segments(
    transcript: Any,
    *,
    chunk_index: int,
    chunk_seconds: int,
    next_segment_number: int,
) -> list[AuditSegment]:
    chunk_offset = float(chunk_index * chunk_seconds)
    transcript_segments = tuple(getattr(transcript, "segments", ()) or ())
    if transcript_segments:
        return [
            AuditSegment(
                segment_id=f"s{next_segment_number + index:05d}",
                start_seconds=chunk_offset + float(getattr(segment, "start", 0.0) or 0.0),
                end_seconds=chunk_offset + float(getattr(segment, "end", 0.0) or 0.0),
                text=_collapse_space(str(getattr(segment, "text", "") or "")),
            )
            for index, segment in enumerate(transcript_segments)
            if _collapse_space(str(getattr(segment, "text", "") or ""))
        ]
    text = _collapse_space(str(getattr(transcript, "text", "") or ""))
    if not text:
        return []
    duration = float(getattr(transcript, "duration_seconds", 0.0) or chunk_seconds)
    return [
        AuditSegment(
            segment_id=f"s{next_segment_number:05d}",
            start_seconds=chunk_offset,
            end_seconds=chunk_offset + duration,
            text=text,
        )
    ]


async def _transcribe_chunks(
    chunks: Sequence[Path],
    *,
    transcriber_engine: str,
    chunk_seconds: int,
) -> tuple[list[AuditSegment], str | None]:
    from bot.social_media.transcription.whisper import get_transcriber

    transcriber = get_transcriber(transcriber_engine)
    segments: list[AuditSegment] = []
    for chunk_index, chunk_path in enumerate(chunks):
        log.info("Transkribiere Audio-Block %d/%d", chunk_index + 1, len(chunks))
        transcript = await asyncio.to_thread(transcriber.transcribe, chunk_path)
        new_segments = _transcription_segments(
            transcript,
            chunk_index=chunk_index,
            chunk_seconds=chunk_seconds,
            next_segment_number=len(segments) + 1,
        )
        segments.extend(new_segments)
    return segments, str(getattr(transcriber, "model_size", None) or getattr(transcriber, "model", None) or "") or None


def _format_timestamp(seconds: float) -> str:
    total = max(0, int(seconds))
    hours, remainder = divmod(total, 3600)
    minutes, secs = divmod(remainder, 60)
    return f"{hours:02d}:{minutes:02d}:{secs:02d}"


def _markdown_report(report: AuditReport) -> str:
    lines = [
        "# Privater Stream-Coaching-Audit",
        "",
        f"- Report: `{report.report_id}`",
        f"- Erstellt: `{report.created_at}`",
        f"- Quelle: `{report.source_label}`",
        f"- Quelle-Typ: `{report.source_type}`",
        f"- Transkription: `{report.transcriber_engine}`",
        f"- LLM-Pruefung: `{report.llm_provider}`",
        f"- Gepruefte Transkript-Segmente: `{report.analyzed_segments}`",
        f"- Fundstellen: `{len(report.findings)}`",
        "",
        "Dieser Bericht ist fuer privates Coaching. Jede Fundstelle muss wegen moeglicher "
        "Voice-to-Text-Fehler und Kontextfragen manuell geprueft werden. Es werden keine "
        "automatischen Sanktionen ausgeloest.",
        "",
        "## Fundstellen",
        "",
    ]
    if not report.findings:
        lines.append("Keine Fundstellen erkannt.")
    for finding in report.findings:
        timestamp = _format_timestamp(finding.start_seconds)
        jump = _vod_jump_url(report.source_label, finding.start_seconds)
        heading = f"### {timestamp} - {finding.category}"
        block = [
            heading,
            "",
            f"- Schweregrad: `{finding.severity}`",
            f"- Erkennung: `{finding.detector}`",
            f"- Konfidenz: `{finding.confidence}`",
            f"- Grund: {finding.reason}",
            f"- Wortlaut (Klartext, nur Admin): {finding.evidence_raw or finding.evidence_redacted}",
            f"- Redigierter Kontext: `{finding.evidence_redacted}`",
        ]
        if jump:
            block.append(f"- VOD-Sprung: {jump}")
        block.extend([f"- Beleg-Hash: `{finding.evidence_sha256}`", ""])
        lines.extend(block)
    return "\n".join(lines).rstrip() + "\n"


def write_report(report: AuditReport, *, output_dir: Path = DEFAULT_OUTPUT_DIR) -> tuple[Path, Path]:
    output_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    json_path = output_dir / f"{report.report_id}.json"
    markdown_path = output_dir / f"{report.report_id}.md"
    json_path.write_text(
        json.dumps(report.to_dict(), ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    markdown_path.write_text(_markdown_report(report), encoding="utf-8")
    json_path.chmod(0o600)
    markdown_path.chmod(0o600)
    return json_path, markdown_path


async def notify_findings_webhook(
    source_label: str,
    findings: Sequence[AuditFinding],
    *,
    webhook_url: str | None = None,
) -> bool:
    """Send redacted findings to a private Discord webhook when configured."""
    target_url = webhook_url or os.getenv("STREAM_AUDIT_DISCORD_WEBHOOK") or ""
    if not target_url or not findings:
        return False
    lines = [
        "**Privater Stream-Coaching-Hinweis**",
        f"Quelle: {source_label}",
        "Bitte Fundstellen manuell im Stream-Kontext pruefen:",
    ]
    for finding in findings[:8]:
        lines.append(
            f"- `{_format_timestamp(finding.start_seconds)}` `{finding.severity}` "
            f"`{finding.category}`: {finding.evidence_redacted[:240]}"
        )
    if len(findings) > 8:
        lines.append(f"- plus {len(findings) - 8} weitere Fundstelle(n) im privaten Report")
    payload = {"content": "\n".join(lines)[:1950]}
    try:
        import aiohttp

        timeout = aiohttp.ClientTimeout(total=15)
        async with aiohttp.ClientSession(timeout=timeout) as session:
            async with session.post(target_url, json=payload) as response:
                return 200 <= int(response.status) < 300
    except Exception:
        log.warning("Discord-Webhook fuer Stream-Audit fehlgeschlagen", exc_info=True)
        return False


def _discord_dm_bot_token() -> str:
    for env_name in (
        "STREAM_AUDIT_DISCORD_BOT_TOKEN",
        "COACHING_BOT_TOKEN",
        "DISCORD_TOKEN",
        "BOT_TOKEN",
    ):
        value = str(os.getenv(env_name) or "").strip()
        if value:
            return value
    return ""


def _discord_dm_user_id(user_id: str | None = None) -> str:
    if str(user_id or "").strip():
        return str(user_id).strip()
    for env_name in (
        "STREAM_AUDIT_DISCORD_USER_ID",
        "TWITCH_ADMIN_DISCORD_USER_ID",
        "SOCIAL_MEDIA_REPORT_ADMIN_DISCORD_USER_ID",
    ):
        value = str(os.getenv(env_name) or "").strip()
        if value:
            return value
    return DEFAULT_ADMIN_DISCORD_USER_ID


def _finding_time_label(
    finding: AuditFinding,
    *,
    occurred_base: datetime | None,
    source_url: str | None,
) -> str:
    """Live -> echte Uhrzeit der Aeusserung; VOD/Datei -> Position (+ Sprunglink)."""
    if occurred_base is not None:
        occurred = occurred_base + timedelta(seconds=finding.start_seconds)
        return f"Live {occurred.strftime('%H:%M:%S')} Uhr"
    timestamp = _format_timestamp(finding.start_seconds)
    jump = _vod_jump_url(source_url, finding.start_seconds)
    return f"{timestamp} (<{jump}>)" if jump else timestamp


def _discord_dm_content(
    source_label: str,
    findings: Sequence[AuditFinding],
    *,
    occurred_base: datetime | None = None,
    source_url: str | None = None,
) -> str:
    quelle_typ = "Live-Mitschnitt" if occurred_base is not None else "VOD-/Datei-Pruefung"
    lines = [
        "**Privater Stream-Coaching-Hinweis**",
        f"Quelle: {source_label} ({quelle_typ})",
        "Klartext nur fuer dich. Am Stream/VOD gegenpruefen, keine automatische Sanktion:",
    ]
    for finding in findings[:8]:
        label = _finding_time_label(finding, occurred_base=occurred_base, source_url=source_url)
        wortlaut = (finding.evidence_raw or finding.evidence_redacted)[:300]
        lines.append(f"- `{label}` `{finding.severity}` `{finding.category}`")
        lines.append(f"  Wortlaut: {wortlaut}")
    if len(findings) > 8:
        lines.append(f"- plus {len(findings) - 8} weitere Fundstelle(n) im privaten Report")
    return "\n".join(lines)[:1950]


async def notify_findings_discord_dm(
    source_label: str,
    findings: Sequence[AuditFinding],
    *,
    discord_user_id: str | None = None,
    bot_token: str | None = None,
    sender: Any | None = None,
    occurred_base: datetime | None = None,
    source_url: str | None = None,
) -> bool:
    """Send findings (Klartext) as a private Discord bot DM to the admin."""
    if not findings:
        return False
    token = str(bot_token or _discord_dm_bot_token()).strip()
    user_id = _discord_dm_user_id(discord_user_id)
    if not token:
        raise AuditError(
            "Discord-Bot-Token fehlt: STREAM_AUDIT_DISCORD_BOT_TOKEN, "
            "COACHING_BOT_TOKEN, DISCORD_TOKEN oder BOT_TOKEN setzen"
        )
    if not user_id.isdigit():
        raise AuditError("Discord-DM-Ziel ist keine gueltige User-ID")
    payload = {
        "content": _discord_dm_content(
            source_label, findings, occurred_base=occurred_base, source_url=source_url
        )
    }
    return await _send_discord_dm_payload(token, user_id, payload, sender=sender)


async def notify_status_discord_dm(
    content: str,
    *,
    discord_user_id: str | None = None,
    bot_token: str | None = None,
    sender: Any | None = None,
) -> bool:
    """Send a short private status DM when a live watch starts."""
    token = str(bot_token or _discord_dm_bot_token()).strip()
    user_id = _discord_dm_user_id(discord_user_id)
    if not token:
        raise AuditError(
            "Discord-Bot-Token fehlt: STREAM_AUDIT_DISCORD_BOT_TOKEN, "
            "COACHING_BOT_TOKEN, DISCORD_TOKEN oder BOT_TOKEN setzen"
        )
    if not user_id.isdigit():
        raise AuditError("Discord-DM-Ziel ist keine gueltige User-ID")
    payload = {"content": str(content or "").strip()[:1950]}
    if not payload["content"]:
        return False
    return await _send_discord_dm_payload(token, user_id, payload, sender=sender)


async def _send_discord_dm_payload(
    token: str,
    user_id: str,
    payload: dict[str, str],
    *,
    sender: Any | None = None,
) -> bool:
    if sender is not None:
        return bool(await sender(token, user_id, payload))

    try:
        import aiohttp

        headers = {"Authorization": f"Bot {token}", "Content-Type": "application/json"}
        timeout = aiohttp.ClientTimeout(total=15)
        async with aiohttp.ClientSession(timeout=timeout, headers=headers) as session:
            async with session.post(
                "https://discord.com/api/v10/users/@me/channels",
                json={"recipient_id": user_id},
            ) as response:
                if not 200 <= int(response.status) < 300:
                    return False
                dm_channel = await response.json()
            channel_id = str(dm_channel.get("id") or "").strip()
            if not channel_id:
                return False
            async with session.post(
                f"https://discord.com/api/v10/channels/{channel_id}/messages",
                json=payload,
            ) as response:
                return 200 <= int(response.status) < 300
    except Exception:
        log.warning("Discord-Bot-DM fuer Stream-Audit fehlgeschlagen", exc_info=True)
        return False


async def audit_source(
    source: str,
    *,
    authorized: bool,
    source_kind: str = "auto",
    live_seconds: int = DEFAULT_LIVE_SECONDS,
    chunk_seconds: int = DEFAULT_CHUNK_SECONDS,
    transcriber_engine: str = "faster_whisper",
    llm_provider: str = "none",
    allow_remote_transcription: bool = False,
    allow_remote_llm: bool = False,
    output_dir: Path = DEFAULT_OUTPUT_DIR,
) -> tuple[AuditReport, Path, Path]:
    """Audit an authorized source and write private coaching reports."""
    if not authorized:
        raise AuditError("Audit nur mit bestaetigter Autorisierung erlaubt")
    if transcriber_engine == "openai_api" and not allow_remote_transcription:
        raise AuditError("OpenAI-Transkription braucht --allow-remote-transcription")
    if llm_provider not in {"none", "minimax"}:
        raise AuditError(f"Nicht unterstuetzter LLM-Provider: {llm_provider}")
    if llm_provider != "none" and not allow_remote_llm:
        raise AuditError("Externe LLM-Pruefung braucht --allow-remote-llm")

    with tempfile.TemporaryDirectory(prefix="stream-coaching-audit-") as tmp:
        workdir = Path(tmp)
        acquired = await _acquire_media(
            source,
            source_kind=source_kind,
            live_seconds=live_seconds,
            workdir=workdir,
        )
        try:
            chunks = await asyncio.to_thread(
                _split_audio,
                acquired.path,
                workdir / "chunks",
                chunk_seconds=chunk_seconds,
            )
            segments, transcriber_model = await _transcribe_chunks(
                chunks,
                transcriber_engine=transcriber_engine,
                chunk_seconds=max(30, int(chunk_seconds)),
            )
            findings = detect_rule_findings(segments)
            if llm_provider == "minimax":
                findings.extend(await _detect_llm_findings_minimax(segments))
        finally:
            if acquired.cleanup is not None:
                acquired.cleanup()

    created_at = datetime.now(UTC)
    report = AuditReport(
        report_id=f"stream-audit-{created_at.strftime('%Y%m%dT%H%M%S%fZ')}",
        created_at=created_at.isoformat(),
        source_type=acquired.source_type,
        source_label=acquired.source_label,
        channel_login=acquired.channel_login,
        transcriber_engine=transcriber_engine,
        transcriber_model=transcriber_model,
        llm_provider=llm_provider,
        raw_transcript_persisted=False,
        analyzed_segments=len(segments),
        findings=_merge_findings(findings),
    )
    json_path, markdown_path = write_report(report, output_dir=output_dir)
    return report, json_path, markdown_path


__all__ = [
    "AuditError",
    "AuditFinding",
    "AuditReport",
    "AuditSegment",
    "audit_source",
    "detect_rule_findings",
    "notify_findings_discord_dm",
    "notify_findings_webhook",
    "notify_status_discord_dm",
    "redact_text",
    "write_report",
]
