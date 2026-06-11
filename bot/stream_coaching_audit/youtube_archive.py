"""YouTube-Archiv-Pipeline fuer Stream-Coaching-Audits.

Mitschnitt bzw. VOD wird privat auf einen separaten Audit-YouTube-Account
hochgeladen. Die YouTube-Auto-Captions (ASR) dienen als Transkript; von
YouTube zensierte Stellen ("[ __ ]") werden lokal mit faster-whisper
nachtranskribiert, damit der echte Wortlaut im Audit landet.

Credentials: Env (YOUTUBE_AUDIT_CLIENT_ID/-SECRET/-REFRESH_TOKEN, z. B. aus
Infisical) oder Fallback-Datei ~/.config/deadlock-twitch-bot/youtube_audit_oauth.json
(legt scripts/setup_youtube_audit_oauth.py an).
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import re
import shutil
import subprocess
import time
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Sequence

from bot.stream_coaching_audit.service import (
    AuditError,
    AuditReport,
    AuditSegment,
    _collapse_space,
    _detect_llm_findings_minimax,
    _merge_findings,
    _require_binary,
    _run_checked,
    _run_output,
    _split_audio,
    _transcribe_chunks,
    detect_rule_findings,
    write_report,
)

log = logging.getLogger("TwitchStreams.StreamCoachingAudit.YouTubeArchive")

OAUTH_TOKEN_URL = "https://oauth2.googleapis.com/token"
API_BASE = "https://www.googleapis.com/youtube/v3"
UPLOAD_URL = "https://www.googleapis.com/upload/youtube/v3/videos"
CREDENTIALS_FILE = Path.home() / ".config" / "deadlock-twitch-bot" / "youtube_audit_oauth.json"

DEFAULT_RECORD_FORMAT = "best[height<=720]/best"
UPLOAD_CHUNK_BYTES = 64 * 1024 * 1024
# YouTube maskiert Kraftausdruecke in Auto-Captions als "[ __ ]" (Unterstriche variieren).
CENSOR_RE = re.compile(r"\[\s*_+\s*\]")
MAX_SPOT_CHECKS = 60

_SRT_TIME_RE = re.compile(
    r"(\d{1,2}):(\d{2}):(\d{2})[,.](\d{1,3})\s*-->\s*(\d{1,2}):(\d{2}):(\d{2})[,.](\d{1,3})"
)


@dataclass(frozen=True, slots=True)
class YouTubeCredentials:
    client_id: str
    client_secret: str
    refresh_token: str


def load_credentials() -> YouTubeCredentials | None:
    """Env zuerst (Infisical), sonst die Setup-Datei; None wenn Setup fehlt."""
    client_id = str(os.getenv("YOUTUBE_AUDIT_CLIENT_ID") or "").strip()
    client_secret = str(os.getenv("YOUTUBE_AUDIT_CLIENT_SECRET") or "").strip()
    refresh_token = str(os.getenv("YOUTUBE_AUDIT_REFRESH_TOKEN") or "").strip()
    if client_id and client_secret and refresh_token:
        return YouTubeCredentials(client_id, client_secret, refresh_token)
    try:
        payload = json.loads(CREDENTIALS_FILE.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    client_id = str(payload.get("client_id") or "").strip()
    client_secret = str(payload.get("client_secret") or "").strip()
    refresh_token = str(payload.get("refresh_token") or "").strip()
    if not (client_id and client_secret and refresh_token):
        return None
    return YouTubeCredentials(client_id, client_secret, refresh_token)


class YouTubeArchiveClient:
    """Schmaler Data-API-v3-Client (Token-Refresh, resumable Upload, Captions)."""

    def __init__(self, credentials: YouTubeCredentials) -> None:
        import requests

        self._credentials = credentials
        self._session = requests.Session()
        self._access_token = ""
        self._token_expires_at = 0.0

    def _token(self) -> str:
        if self._access_token and time.monotonic() < self._token_expires_at - 60:
            return self._access_token
        response = self._session.post(
            OAUTH_TOKEN_URL,
            data={
                "client_id": self._credentials.client_id,
                "client_secret": self._credentials.client_secret,
                "refresh_token": self._credentials.refresh_token,
                "grant_type": "refresh_token",
            },
            timeout=30,
        )
        if response.status_code != 200:
            error = ""
            try:
                error = str(response.json().get("error") or "")
            except ValueError:
                pass
            raise AuditError(f"YouTube-OAuth-Refresh fehlgeschlagen (HTTP {response.status_code} {error})")
        payload = response.json()
        self._access_token = str(payload.get("access_token") or "")
        if not self._access_token:
            raise AuditError("YouTube-OAuth-Refresh lieferte kein access_token")
        self._token_expires_at = time.monotonic() + float(payload.get("expires_in") or 3600)
        return self._access_token

    def _auth_headers(self) -> dict[str, str]:
        return {"Authorization": f"Bearer {self._token()}"}

    def upload_video(self, path: Path, *, title: str, description: str = "") -> str:
        """Resumable Upload als PRIVATES Video; gibt die Video-ID zurueck."""
        total = path.stat().st_size
        if total <= 0:
            raise AuditError(f"Upload-Datei ist leer: {path}")
        metadata = {
            "snippet": {
                "title": title[:95],
                "description": description[:4500],
                "categoryId": "20",
            },
            "status": {"privacyStatus": "private", "selfDeclaredMadeForKids": False},
        }
        response = self._session.post(
            f"{UPLOAD_URL}?uploadType=resumable&part=snippet,status",
            headers={
                **self._auth_headers(),
                "Content-Type": "application/json; charset=UTF-8",
                "X-Upload-Content-Type": "video/mp4",
                "X-Upload-Content-Length": str(total),
            },
            json=metadata,
            timeout=60,
        )
        if response.status_code != 200:
            raise AuditError(
                f"YouTube-Upload-Init fehlgeschlagen (HTTP {response.status_code}): "
                f"{response.text[:300]}"
            )
        upload_url = str(response.headers.get("Location") or "")
        if not upload_url:
            raise AuditError("YouTube-Upload-Init lieferte keine Upload-URL")

        offset = 0
        failures = 0
        with path.open("rb") as handle:
            while offset < total:
                handle.seek(offset)
                chunk = handle.read(UPLOAD_CHUNK_BYTES)
                end = offset + len(chunk) - 1
                try:
                    response = self._session.put(
                        upload_url,
                        headers={
                            "Content-Length": str(len(chunk)),
                            "Content-Range": f"bytes {offset}-{end}/{total}",
                        },
                        data=chunk,
                        timeout=600,
                    )
                except OSError:
                    response = None
                if response is not None and response.status_code in (200, 201):
                    payload = response.json()
                    video_id = str(payload.get("id") or "")
                    if not video_id:
                        raise AuditError("YouTube-Upload lieferte keine Video-ID")
                    return video_id
                if response is not None and response.status_code == 308:
                    offset = _next_offset_from_range(response.headers.get("Range"))
                    failures = 0
                    continue
                failures += 1
                if failures > 5:
                    status = response.status_code if response is not None else "Netzwerkfehler"
                    raise AuditError(f"YouTube-Upload abgebrochen (HTTP {status})")
                time.sleep(min(120, 5 * 2**failures))
                offset = self._probe_upload_offset(upload_url, total, fallback=offset)
        raise AuditError("YouTube-Upload endete ohne Bestaetigung")

    def _probe_upload_offset(self, upload_url: str, total: int, *, fallback: int) -> int:
        try:
            response = self._session.put(
                upload_url,
                headers={"Content-Length": "0", "Content-Range": f"bytes */{total}"},
                timeout=60,
            )
        except OSError:
            return fallback
        if response.status_code == 308:
            return _next_offset_from_range(response.headers.get("Range"))
        return fallback

    def find_asr_caption_id(self, video_id: str) -> str | None:
        """Caption-Track-ID der Auto-Untertitel (ASR); None solange nicht fertig."""
        response = self._session.get(
            f"{API_BASE}/captions",
            params={"part": "snippet", "videoId": video_id},
            headers=self._auth_headers(),
            timeout=30,
        )
        if response.status_code != 200:
            log.warning(
                "captions.list fehlgeschlagen (HTTP %s): %s",
                response.status_code,
                response.text[:200],
            )
            return None
        for item in response.json().get("items") or []:
            snippet = item.get("snippet") or {}
            if str(snippet.get("trackKind") or "").lower() == "asr" and not snippet.get(
                "isDraft", False
            ):
                return str(item.get("id") or "") or None
        return None

    def download_caption_srt(self, caption_id: str) -> str | None:
        response = self._session.get(
            f"{API_BASE}/captions/{caption_id}",
            params={"tfmt": "srt"},
            headers=self._auth_headers(),
            timeout=120,
        )
        if response.status_code != 200:
            log.warning(
                "captions.download fehlgeschlagen (HTTP %s): %s",
                response.status_code,
                response.text[:200],
            )
            return None
        text = response.text or ""
        return text if text.strip() else None

    @staticmethod
    def watch_url(video_id: str) -> str:
        return f"https://www.youtube.com/watch?v={video_id}"


def _next_offset_from_range(range_header: str | None) -> int:
    """308-Range-Header ("bytes=0-N") -> naechstes Offset; ohne Header ab 0."""
    match = re.fullmatch(r"bytes=0-(\d+)", str(range_header or "").strip())
    return int(match.group(1)) + 1 if match else 0


def parse_srt(srt_text: str) -> list[AuditSegment]:
    """SRT -> AuditSegments; Zeitstempel bleiben video-relativ erhalten."""
    segments: list[AuditSegment] = []
    for block in re.split(r"\n\s*\n", str(srt_text or "").replace("\r\n", "\n")):
        lines = [line.strip() for line in block.strip().splitlines()]
        if len(lines) < 2:
            continue
        time_line_index = 0 if _SRT_TIME_RE.search(lines[0]) else 1
        if time_line_index >= len(lines):
            continue
        match = _SRT_TIME_RE.search(lines[time_line_index])
        if not match:
            continue
        h1, m1, s1, ms1, h2, m2, s2, ms2 = (int(value) for value in match.groups())
        start = h1 * 3600 + m1 * 60 + s1 + ms1 / 1000
        end = h2 * 3600 + m2 * 60 + s2 + ms2 / 1000
        text = _collapse_space(" ".join(lines[time_line_index + 1 :]))
        if not text:
            continue
        segments.append(
            AuditSegment(
                segment_id=f"s{len(segments) + 1:05d}",
                start_seconds=start,
                end_seconds=max(start, end),
                text=text,
            )
        )
    return segments


def _cut_snippet(media_path: Path, target: Path, *, start: float, end: float) -> Path:
    ffmpeg = _require_binary("FFMPEG_BIN", "ffmpeg")
    begin = max(0.0, start - 8.0)
    duration = max(6.0, (end - begin) + 8.0)
    _run_checked(
        [
            ffmpeg,
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-ss",
            f"{begin:.2f}",
            "-t",
            f"{duration:.2f}",
            "-i",
            str(media_path),
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
            str(target),
        ]
    )
    if not target.is_file() or target.stat().st_size < 8 * 1024:
        raise AuditError("Zensur-Schnipsel lieferte keine verwertbare Audiodatei")
    return target


def spot_check_censored_segments(
    segments: Sequence[AuditSegment],
    media_path: Path,
    workdir: Path,
) -> tuple[list[AuditSegment], int]:
    """Ersetzt "[ __ ]"-Segmente durch lokalen Whisper-Wortlaut aus dem Mitschnitt."""
    censored = [index for index, segment in enumerate(segments) if CENSOR_RE.search(segment.text)]
    if not censored:
        return list(segments), 0
    if len(censored) > MAX_SPOT_CHECKS:
        log.warning(
            "%d zensierte Stellen, pruefe nur die ersten %d lokal nach",
            len(censored),
            MAX_SPOT_CHECKS,
        )
        censored = censored[:MAX_SPOT_CHECKS]

    from bot.social_media.transcription.whisper import get_transcriber

    transcriber = get_transcriber("faster_whisper")
    workdir.mkdir(parents=True, exist_ok=True)
    result = list(segments)
    checked = 0
    for position, index in enumerate(censored):
        segment = result[index]
        snippet_path = workdir / f"censored-{position:03d}.wav"
        try:
            _cut_snippet(media_path, snippet_path, start=segment.start_seconds, end=segment.end_seconds)
            transcript = transcriber.transcribe(snippet_path)
        except Exception:  # noqa: BLE001 - Einzel-Schnipsel darf nie den Audit kippen
            log.warning("Zensur-Nachpruefung bei %.0fs fehlgeschlagen", segment.start_seconds, exc_info=True)
            continue
        finally:
            snippet_path.unlink(missing_ok=True)
        text = _collapse_space(str(getattr(transcript, "text", "") or ""))
        if text:
            result[index] = AuditSegment(
                segment_id=segment.segment_id,
                start_seconds=segment.start_seconds,
                end_seconds=segment.end_seconds,
                text=text,
            )
            checked += 1
    return result, checked


def ensure_disk_space(path: Path, *, min_free_gb: float) -> None:
    usage = shutil.disk_usage(path)
    free_gb = usage.free / (1024**3)
    if free_gb < min_free_gb:
        raise AuditError(
            f"Zu wenig freier Speicher fuer Aufnahme: {free_gb:.1f} GB frei, "
            f"Minimum {min_free_gb:.0f} GB"
        )


def probe_duration_seconds(path: Path) -> float:
    ffprobe = _require_binary("FFPROBE_BIN", "ffprobe")
    output = _run_output(
        [
            ffprobe,
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            str(path),
        ]
    )
    try:
        return float(output.splitlines()[0])
    except (IndexError, ValueError) as exc:
        raise AuditError(f"ffprobe lieferte keine Dauer fuer {path.name}") from exc


def record_live_stream(login: str, output_path: Path, *, record_format: str) -> None:
    """Blockiert, bis der Live-Stream endet; schreibt den Mitschnitt nach output_path."""
    yt_dlp = _require_binary("STREAM_AUDIT_YTDLP_BIN", "yt-dlp")
    output_path.parent.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(
        [
            yt_dlp,
            "--no-playlist",
            "--format",
            record_format,
            "--merge-output-format",
            "mp4",
            "--no-part",
            "--output",
            str(output_path),
            f"https://twitch.tv/{login}",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
    )
    # yt-dlp endet bei Stream-Ende teils mit !=0 (abgerissener HLS) - die Datei zaehlt.
    if not output_path.is_file() or output_path.stat().st_size < 5 * 1024 * 1024:
        stderr = (result.stderr or b"").decode("utf-8", "ignore").strip()
        raise AuditError(f"Live-Mitschnitt lieferte keine verwertbare Datei: {stderr[-400:]}")


def download_vod_video(vod_url: str, target: Path, *, record_format: str) -> Path:
    """VOD als Video laden (fuers Archiv; das Audio-only von service._download_vod reicht hier nicht)."""
    yt_dlp = _require_binary("STREAM_AUDIT_YTDLP_BIN", "yt-dlp")
    target.parent.mkdir(parents=True, exist_ok=True)
    _run_checked(
        [
            yt_dlp,
            "--no-playlist",
            "--format",
            record_format,
            "--merge-output-format",
            "mp4",
            "--output",
            str(target),
            vod_url,
        ]
    )
    if not target.is_file() or target.stat().st_size < 1024 * 1024:
        raise AuditError("VOD-Download lieferte keine verwertbare Datei")
    return target


async def wait_for_captions(
    client: YouTubeArchiveClient,
    video_id: str,
    *,
    poll_seconds: float,
    timeout_seconds: float,
) -> str | None:
    """Pollt bis die ASR-Captions abrufbar sind; None bei Timeout/Download-Sperre."""
    deadline = time.monotonic() + max(60.0, timeout_seconds)
    while time.monotonic() < deadline:
        caption_id = await asyncio.to_thread(client.find_asr_caption_id, video_id)
        if caption_id:
            srt = await asyncio.to_thread(client.download_caption_srt, caption_id)
            if srt:
                return srt
            # Track existiert, Download verweigert (kommt bei ASR vor) -> Whisper-Fallback
            return None
        await asyncio.sleep(max(30.0, poll_seconds))
    return None


async def audit_media_via_youtube(
    media_path: Path,
    *,
    source_label: str,
    channel_login: str | None,
    workdir: Path,
    llm_provider: str = "none",
    output_dir: Path,
    caption_poll_seconds: float = 600.0,
    caption_timeout_seconds: float = 24 * 3600.0,
    chunk_seconds: int = 600,
    upload_title: str,
) -> tuple[AuditReport, Path, str]:
    """Upload -> Captions -> (Spot-Check|Whisper-Fallback) -> Findings -> Report.

    Gibt (Report, Markdown-Pfad, YouTube-Watch-URL) zurueck.
    """
    credentials = load_credentials()
    if credentials is None:
        raise AuditError(
            "YouTube-Audit-Account nicht eingerichtet: scripts/setup_youtube_audit_oauth.py "
            "ausfuehren oder YOUTUBE_AUDIT_CLIENT_ID/-SECRET/-REFRESH_TOKEN setzen"
        )
    client = YouTubeArchiveClient(credentials)
    log.info("Lade %s zu YouTube hoch (%.1f MB)", media_path.name, media_path.stat().st_size / 1024**2)
    video_id = await asyncio.to_thread(
        client.upload_video,
        media_path,
        title=upload_title,
        description=f"Privates Stream-Coaching-Archiv. Quelle: {source_label}",
    )
    watch_url = client.watch_url(video_id)
    log.info("Upload fertig: %s - warte auf Auto-Captions", watch_url)

    srt = await wait_for_captions(
        client,
        video_id,
        poll_seconds=caption_poll_seconds,
        timeout_seconds=caption_timeout_seconds,
    )
    transcriber_model: str | None = None
    if srt:
        segments = parse_srt(srt)
        segments, checked = await asyncio.to_thread(
            spot_check_censored_segments, segments, media_path, workdir / "censor-check"
        )
        transcriber_engine = "youtube_captions"
        if checked:
            log.info("%d zensierte Caption-Stellen lokal nachtranskribiert", checked)
    else:
        log.warning("Keine Auto-Captions erhalten - lokaler faster-whisper-Fallback")
        chunks = await asyncio.to_thread(
            _split_audio, media_path, workdir / "chunks", chunk_seconds=chunk_seconds
        )
        segments, transcriber_model = await _transcribe_chunks(
            chunks, transcriber_engine="faster_whisper", chunk_seconds=chunk_seconds
        )
        transcriber_engine = "faster_whisper_fallback"

    findings = detect_rule_findings(segments)
    if llm_provider == "minimax":
        findings.extend(await _detect_llm_findings_minimax(segments))

    created_at = datetime.now(UTC)
    report = AuditReport(
        report_id=f"stream-audit-{created_at.strftime('%Y%m%dT%H%M%S%fZ')}",
        created_at=created_at.isoformat(),
        source_type="youtube_archive",
        source_label=f"{source_label} | Archiv: {watch_url}",
        channel_login=channel_login,
        transcriber_engine=transcriber_engine,
        transcriber_model=transcriber_model,
        llm_provider=llm_provider,
        raw_transcript_persisted=False,
        analyzed_segments=len(segments),
        findings=_merge_findings(findings),
    )
    json_path, markdown_path = write_report(report, output_dir=output_dir)
    del json_path
    return report, markdown_path, watch_url


__all__ = [
    "CENSOR_RE",
    "DEFAULT_RECORD_FORMAT",
    "YouTubeArchiveClient",
    "YouTubeCredentials",
    "audit_media_via_youtube",
    "download_vod_video",
    "ensure_disk_space",
    "load_credentials",
    "parse_srt",
    "probe_duration_seconds",
    "record_live_stream",
    "spot_check_censored_segments",
    "wait_for_captions",
]
