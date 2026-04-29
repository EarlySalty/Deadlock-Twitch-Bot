from __future__ import annotations

import asyncio
import logging
import re
from datetime import UTC
from datetime import datetime
from pathlib import Path

import aiohttp

from .config import FFMPEG_PATH
from .config import MAX_DISCORD_FILE_MB

log = logging.getLogger("TwitchStreams.HighlightClipper")

_REQUEST_TIMEOUT = aiohttp.ClientTimeout(total=30)
_DURATION_RE = re.compile(r"(?:(?P<hours>\d+)h)?(?:(?P<minutes>\d+)m)?(?:(?P<seconds>\d+)s)?")
_YT_DLP_PATH = Path(__file__).resolve().parents[2] / ".venv" / "bin" / "yt-dlp"


async def get_channel_id(login: str, client_id: str, access_token: str) -> str | None:
    headers = _twitch_headers(client_id, access_token)
    async with aiohttp.ClientSession(timeout=_REQUEST_TIMEOUT, headers=headers) as session:
        async with session.get("https://api.twitch.tv/helix/users", params={"login": login}) as response:
            response.raise_for_status()
            payload = await response.json()
    data = payload.get("data") or []
    if not data:
        return None
    return str(data[0].get("id") or "").strip() or None


async def find_vod_for_match(
    channel_id: str,
    match_start_unix: int,
    match_duration_s: int,
    client_id: str,
    access_token: str,
) -> dict | None:
    headers = _twitch_headers(client_id, access_token)
    async with aiohttp.ClientSession(timeout=_REQUEST_TIMEOUT, headers=headers) as session:
        async with session.get(
            "https://api.twitch.tv/helix/videos",
            params={"user_id": channel_id, "type": "archive", "first": 20},
        ) as response:
            response.raise_for_status()
            payload = await response.json()

    for vod in payload.get("data") or []:
        started_at = _parse_twitch_datetime(vod.get("created_at"))
        duration_s = _parse_duration_seconds(vod.get("duration"))
        if started_at is None or duration_s <= 0:
            continue
        if started_at <= match_start_unix and started_at + duration_s >= match_start_unix + match_duration_s:
            return {
                "vod_id": str(vod.get("id") or "").strip(),
                "vod_started_at": started_at,
            }
    return None


async def download_clip(vod_id: str, clip_start_s: int, clip_end_s: int, output_path: str) -> bool:
    final_path = Path(output_path)
    final_path.parent.mkdir(parents=True, exist_ok=True)
    clip_start_s = max(0, int(clip_start_s))
    clip_end_s = max(clip_start_s + 1, int(clip_end_s))

    raw_prefix = final_path.with_name(f"{final_path.stem}.raw")
    raw_template = str(raw_prefix) + ".%(ext)s"
    await _cleanup_paths(final_path.parent.glob(f"{raw_prefix.name}.*"))
    final_path.unlink(missing_ok=True)

    yt_dlp_cmd = [
        str(_YT_DLP_PATH),
        "--download-sections",
        f"*{_format_hhmmss(clip_start_s)}-{_format_hhmmss(clip_end_s)}",
        "-o",
        raw_template,
        "--merge-output-format",
        "mp4",
        "-f",
        "bestvideo[height<=720]+bestaudio/best[height<=720]",
        f"https://www.twitch.tv/videos/{vod_id}",
    ]
    if not await _run_process(yt_dlp_cmd):
        return False

    raw_candidates = sorted(final_path.parent.glob(f"{raw_prefix.name}.*"))
    if not raw_candidates:
        return False
    raw_path = _pick_downloaded_video(raw_candidates)
    if raw_path is None:
        await _cleanup_paths(raw_candidates)
        return False
    compressed_path = final_path.with_name(f"{final_path.stem}.compressed.mp4")
    compressed_path.unlink(missing_ok=True)

    ffmpeg_cmd = [
        FFMPEG_PATH,
        "-y",
        "-i",
        str(raw_path),
        "-vf",
        "scale=-2:720",
        "-c:v",
        "libx264",
        "-crf",
        "28",
        "-preset",
        "fast",
        "-c:a",
        "aac",
        "-b:a",
        "96k",
        str(compressed_path),
    ]
    if not await _run_process(ffmpeg_cmd):
        await _cleanup_paths(raw_candidates)
        return False

    await _cleanup_paths(raw_candidates)
    if not compressed_path.exists():
        return False
    final_path.unlink(missing_ok=True)
    compressed_path.replace(final_path)

    max_bytes = MAX_DISCORD_FILE_MB * 1024 * 1024
    if not final_path.exists() or final_path.stat().st_size >= max_bytes:
        final_path.unlink(missing_ok=True)
        return False
    return True


async def _run_process(cmd: list[str]) -> bool:
    process = await asyncio.create_subprocess_exec(
        *cmd,
        stdout=asyncio.subprocess.DEVNULL,
        stderr=asyncio.subprocess.PIPE,
    )
    _, stderr = await process.communicate()
    if process.returncode == 0:
        return True
    log.warning(
        "HighlightClipper subprocess failed: %s",
        " ".join(cmd),
    )
    if stderr:
        log.warning(stderr.decode("utf-8", "ignore").strip())
    return False


async def _cleanup_paths(paths) -> None:
    for path in list(paths):
        if isinstance(path, Path):
            path.unlink(missing_ok=True)


def _pick_downloaded_video(paths: list[Path]) -> Path | None:
    for suffix in (".mp4", ".mkv", ".webm", ".ts"):
        for path in paths:
            if path.suffix == suffix:
                return path
    return paths[0] if paths else None


def _twitch_headers(client_id: str, access_token: str) -> dict[str, str]:
    return {
        "Client-Id": client_id,
        "Authorization": f"Bearer {access_token}",
    }


def _parse_twitch_datetime(value: object) -> int | None:
    text = str(value or "").strip()
    if not text:
        return None
    return int(datetime.fromisoformat(text.replace("Z", "+00:00")).astimezone(UTC).timestamp())


def _parse_duration_seconds(value: object) -> int:
    text = str(value or "").strip()
    match = _DURATION_RE.fullmatch(text)
    if match is None:
        return 0
    hours = int(match.group("hours") or 0)
    minutes = int(match.group("minutes") or 0)
    seconds = int(match.group("seconds") or 0)
    return hours * 3600 + minutes * 60 + seconds


def _format_hhmmss(total_seconds: int) -> str:
    hours, remainder = divmod(int(total_seconds), 3600)
    minutes, seconds = divmod(remainder, 60)
    return f"{hours:02d}:{minutes:02d}:{seconds:02d}"
