"""Audio-Capture für Voice-Reaction.

Pullt einen kurzen Twitch-Stream-Ausschnitt via streamlink in eine Temp-Datei
(`.ts`), sodass der bestehende `transcribe_clip()`-Helfer in
`bot/social_media/transcription/whisper.py` direkt darauf arbeiten kann.

Der Modul ist bewusst klein und seitenarm: keine Whisper-Calls, keine DB,
keine Konversations-Logik. Die einzige Aufgabe ist robustes Capture +
Cleanup von Temp-Verzeichnissen.
"""

from __future__ import annotations

import asyncio
import logging
import os
import shutil
import tempfile
import time
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Awaitable, Callable

log = logging.getLogger("TwitchStreams.VoiceReaction.AudioCapture")

CAPTURE_TMP_PREFIX = "voice-reaction-"
DEFAULT_QUALITY = "worst"
DEFAULT_DURATION_SECONDS = 75
_MIN_USEFUL_BYTES = 32 * 1024  # < 32 KB → wahrscheinlich Connect-Failure


class AudioCaptureError(RuntimeError):
    """Capture konnte nicht erfolgreich abgeschlossen werden."""


@dataclass(frozen=True)
class CaptureResult:
    """Erfolgreiches Capture-Ergebnis."""

    media_path: Path
    workdir: Path
    quality: str
    requested_duration_seconds: int
    actual_duration_seconds: float
    bytes: int

    def cleanup(self) -> None:
        cleanup_workdir(self.workdir)


# ---------- Public API ----------


def streamlink_bin() -> str:
    return os.getenv("VOICE_REACTION_STREAMLINK_BIN") or "streamlink"


async def capture(
    login: str,
    *,
    duration_seconds: int = DEFAULT_DURATION_SECONDS,
    quality: str = DEFAULT_QUALITY,
    workdir_root: Path | None = None,
    runner: Callable[..., Awaitable[tuple[int, bytes]]] | None = None,
) -> CaptureResult:
    """Lädt `duration_seconds` Sekunden Audio des angegebenen Twitch-Streams.

    Args:
        login: Twitch-Login des Streamers.
        duration_seconds: Hartes Cap, default 75 s.
        quality: streamlink-Quality (`worst` / `audio_only` / `best`).
        workdir_root: optionales Eltern-Verzeichnis für die Temp-Files
            (Standard `/tmp`).
        runner: Test-Hook — wenn gesetzt, wird statt `asyncio.create_subprocess_exec`
            diese Coroutine aufgerufen mit den exakten Streamlink-Argumenten.
            Sie muss ein Tupel `(returncode, stderr_bytes)` zurückgeben und
            soll selbst die Datei `media_path` erzeugen, falls sie ein Capture
            simuliert.

    Returns:
        CaptureResult mit `.media_path` (TS-File). Aufrufer muss
        `result.cleanup()` aufrufen, sobald die Datei nicht mehr gebraucht wird.

    Raises:
        AudioCaptureError: wenn streamlink fehlschlägt oder Datei zu klein ist.
    """
    normalized = str(login or "").strip().lower()
    if not normalized:
        raise AudioCaptureError("login leer")
    if duration_seconds < 5:
        raise AudioCaptureError(f"duration_seconds zu klein: {duration_seconds}")

    workdir = _make_workdir(workdir_root)
    media_path = workdir / "audio.ts"
    target_url = f"https://twitch.tv/{normalized}"
    args = [
        streamlink_bin(),
        "--hls-duration",
        _format_hls_duration(duration_seconds),
        "--twitch-disable-ads",
        "--quiet",
        "-o",
        str(media_path),
        target_url,
        quality,
    ]

    log.debug(
        "VoiceReaction: starte Capture login=%s duration=%ss quality=%s workdir=%s",
        normalized,
        duration_seconds,
        quality,
        workdir,
    )
    started_at = time.monotonic()
    try:
        if runner is not None:
            returncode, stderr_bytes = await runner(*args)
        else:
            returncode, stderr_bytes = await _run_streamlink(args, duration_seconds)
    except FileNotFoundError as exc:
        cleanup_workdir(workdir)
        raise AudioCaptureError(f"streamlink binary nicht gefunden: {exc}") from exc
    except Exception as exc:
        cleanup_workdir(workdir)
        raise AudioCaptureError(f"streamlink-Aufruf fehlgeschlagen: {exc}") from exc
    elapsed = time.monotonic() - started_at

    if not media_path.exists():
        cleanup_workdir(workdir)
        stderr_text = (stderr_bytes or b"").decode("utf-8", "ignore").strip()
        raise AudioCaptureError(
            f"streamlink lieferte keine Datei (rc={returncode}): {stderr_text[:300]}"
        )

    size_bytes = media_path.stat().st_size
    if size_bytes < _MIN_USEFUL_BYTES:
        cleanup_workdir(workdir)
        stderr_text = (stderr_bytes or b"").decode("utf-8", "ignore").strip()
        raise AudioCaptureError(
            f"Capture zu klein ({size_bytes} bytes, rc={returncode}): {stderr_text[:300]}"
        )

    if returncode != 0:
        # streamlink kappt teilweise mit non-zero exit, obwohl Daten vorhanden sind.
        log.debug(
            "VoiceReaction: streamlink rc=%s, Datei aber gültig (%s bytes) — fahre fort",
            returncode,
            size_bytes,
        )

    return CaptureResult(
        media_path=media_path,
        workdir=workdir,
        quality=quality,
        requested_duration_seconds=duration_seconds,
        actual_duration_seconds=round(elapsed, 2),
        bytes=size_bytes,
    )


def cleanup_workdir(workdir: Path | str) -> None:
    """Löscht ein Capture-Verzeichnis, wenn es dem erwarteten Schema entspricht."""
    try:
        path = Path(workdir)
    except Exception:
        return
    if not path.exists():
        return
    if not path.name.startswith(CAPTURE_TMP_PREFIX):
        return
    shutil.rmtree(path, ignore_errors=True)


def cleanup_stale_capture_dirs(
    *,
    max_age_seconds: int = 3600,
    workdir_root: Path | None = None,
) -> int:
    """Räumt alte Capture-Verzeichnisse auf (Boot-Zeit-GC)."""
    root = Path(workdir_root) if workdir_root else Path(tempfile.gettempdir())
    if not root.exists():
        return 0

    cutoff = time.time() - max(0, int(max_age_seconds))
    removed = 0
    for entry in root.iterdir():
        try:
            if not entry.name.startswith(CAPTURE_TMP_PREFIX):
                continue
            if not entry.is_dir():
                continue
            mtime = entry.stat().st_mtime
        except OSError:
            continue

        if mtime > cutoff:
            continue

        try:
            shutil.rmtree(entry, ignore_errors=True)
            removed += 1
        except Exception:
            log.debug("VoiceReaction: konnte Stale-Dir %s nicht löschen", entry, exc_info=True)
    if removed:
        log.info("VoiceReaction: %d Stale-Capture-Dirs entfernt (root=%s)", removed, root)
    return removed


# ---------- Internals ----------


def _make_workdir(workdir_root: Path | None) -> Path:
    root = Path(workdir_root) if workdir_root else Path(tempfile.gettempdir())
    root.mkdir(parents=True, exist_ok=True)
    suffix = uuid.uuid4().hex[:12]
    workdir = root / f"{CAPTURE_TMP_PREFIX}{suffix}"
    workdir.mkdir(parents=True, exist_ok=False)
    return workdir


def _format_hls_duration(seconds: int) -> str:
    seconds = max(1, int(seconds))
    minutes, sec = divmod(seconds, 60)
    hours, minutes = divmod(minutes, 60)
    return f"{hours:02d}:{minutes:02d}:{sec:02d}"


async def _run_streamlink(args: list[str], duration_seconds: int) -> tuple[int, bytes]:
    """Startet streamlink und kappt nach 1.5 × duration_seconds hart."""
    proc = await asyncio.create_subprocess_exec(
        *args,
        stdin=asyncio.subprocess.DEVNULL,
        stdout=asyncio.subprocess.DEVNULL,
        stderr=asyncio.subprocess.PIPE,
    )
    hard_timeout = max(30, int(duration_seconds * 1.5) + 15)
    try:
        _, stderr = await asyncio.wait_for(proc.communicate(), timeout=hard_timeout)
    except asyncio.TimeoutError:
        log.warning(
            "VoiceReaction: streamlink-Timeout (%ss) — terminate forced",
            hard_timeout,
        )
        try:
            proc.terminate()
            await asyncio.wait_for(proc.wait(), timeout=5)
        except (asyncio.TimeoutError, ProcessLookupError):
            try:
                proc.kill()
            except ProcessLookupError:
                pass
        return (proc.returncode or 124), b"timeout"
    return (proc.returncode or 0), stderr or b""


__all__ = [
    "AudioCaptureError",
    "CaptureResult",
    "CAPTURE_TMP_PREFIX",
    "DEFAULT_QUALITY",
    "DEFAULT_DURATION_SECONDS",
    "capture",
    "cleanup_workdir",
    "cleanup_stale_capture_dirs",
    "streamlink_bin",
]
