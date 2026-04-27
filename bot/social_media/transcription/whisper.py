"""Speech-to-text adapters for the Phase 2 enrichment pipeline.

Supports three engines (selected via `SOCIAL_MEDIA_TRANSCRIBER` env):
- faster_whisper (default)  - lokal via ctranslate2
- openai_api                - OpenAI Whisper-1 API (optional, braucht OPENAI_API_KEY)
- none                      - kein Transkript, Pipeline ueberspringt Whisper-Stage

Audio wird zuerst aus dem MP4 mit `ffmpeg` extrahiert (16 kHz mono WAV) und
dann an die gewaehlte Engine gegeben. Ergebnis ist ein
`TranscriptionResult` mit Segmenten und sprachlich erkannter Locale.
"""

from __future__ import annotations

import asyncio
import logging
import os
import shutil
import subprocess
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable, Sequence

log = logging.getLogger("TwitchStreams.SocialMedia.Whisper")


class TranscriberUnavailable(RuntimeError):
    """Raised wenn die gewaehlte Transcriber-Engine nicht verfuegbar ist."""


@dataclass(frozen=True)
class TranscriptSegment:
    start: float
    end: float
    text: str

    def to_dict(self) -> dict[str, Any]:
        return {"start": self.start, "end": self.end, "text": self.text}


@dataclass(frozen=True)
class TranscriptionResult:
    text: str
    segments: tuple[TranscriptSegment, ...] = field(default_factory=tuple)
    language: str | None = None
    duration_seconds: float | None = None
    engine: str = "faster_whisper"
    model: str | None = None

    def segments_as_dicts(self) -> list[dict[str, Any]]:
        return [seg.to_dict() for seg in self.segments]


# ---------- Audio-Extraktion ----------

def _ffmpeg_path() -> str:
    return os.getenv("FFMPEG_BIN") or "ffmpeg"


def _extract_audio(video_path: Path, target_wav: Path) -> Path:
    """Extrahiere Audio als 16kHz mono WAV mit ffmpeg."""
    if not video_path.exists():
        raise FileNotFoundError(f"video file not found: {video_path}")
    target_wav.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        _ffmpeg_path(),
        "-y",
        "-i",
        str(video_path),
        "-vn",
        "-ac",
        "1",
        "-ar",
        "16000",
        "-c:a",
        "pcm_s16le",
        str(target_wav),
    ]
    try:
        subprocess.run(
            cmd,
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
    except FileNotFoundError as exc:
        raise TranscriberUnavailable("ffmpeg binary not available") from exc
    except subprocess.CalledProcessError as exc:
        log.error("ffmpeg audio-extraction failed: %s", exc.stderr.decode("utf-8", "ignore"))
        raise
    return target_wav


# ---------- Engine: faster-whisper ----------

class _FasterWhisperEngine:
    name = "faster_whisper"

    def __init__(self, model_size: str | None = None) -> None:
        try:
            from faster_whisper import WhisperModel  # type: ignore
        except Exception as exc:
            raise TranscriberUnavailable(
                "faster-whisper not installed. pip install faster-whisper"
            ) from exc
        self._WhisperModel = WhisperModel
        self.model_size = model_size or os.getenv("FASTER_WHISPER_MODEL") or "small"
        self.device = os.getenv("FASTER_WHISPER_DEVICE", "auto")
        self.compute_type = os.getenv("FASTER_WHISPER_COMPUTE_TYPE", "default")
        self._model: Any = None

    def _ensure_model(self) -> Any:
        if self._model is None:
            log.info(
                "Lade faster-whisper Modell %s (device=%s, compute_type=%s)",
                self.model_size,
                self.device,
                self.compute_type,
            )
            self._model = self._WhisperModel(
                self.model_size,
                device=self.device,
                compute_type=self.compute_type,
            )
        return self._model

    def transcribe(self, audio_path: Path) -> TranscriptionResult:
        model = self._ensure_model()
        segments_iter, info = model.transcribe(
            str(audio_path),
            beam_size=int(os.getenv("FASTER_WHISPER_BEAM_SIZE", "5") or 5),
            vad_filter=True,
            language=os.getenv("FASTER_WHISPER_LANG") or None,
        )
        segments: list[TranscriptSegment] = []
        text_parts: list[str] = []
        for seg in segments_iter:
            text_part = (seg.text or "").strip()
            if not text_part:
                continue
            text_parts.append(text_part)
            segments.append(
                TranscriptSegment(
                    start=float(seg.start or 0.0),
                    end=float(seg.end or 0.0),
                    text=text_part,
                )
            )
        return TranscriptionResult(
            text=" ".join(text_parts).strip(),
            segments=tuple(segments),
            language=str(getattr(info, "language", "") or "") or None,
            duration_seconds=float(getattr(info, "duration", 0.0) or 0.0) or None,
            engine=self.name,
            model=self.model_size,
        )


# ---------- Engine: OpenAI Whisper API ----------

class _OpenAIWhisperEngine:
    name = "openai_api"

    def __init__(self, model: str | None = None) -> None:
        api_key = os.getenv("OPENAI_API_KEY")
        if not api_key:
            raise TranscriberUnavailable("OPENAI_API_KEY not set")
        try:
            from openai import OpenAI  # type: ignore
        except Exception as exc:
            raise TranscriberUnavailable("openai SDK not installed") from exc
        self._client = OpenAI(api_key=api_key)
        self.model = model or os.getenv("OPENAI_WHISPER_MODEL") or "whisper-1"

    def transcribe(self, audio_path: Path) -> TranscriptionResult:
        with audio_path.open("rb") as fh:
            response = self._client.audio.transcriptions.create(
                model=self.model,
                file=fh,
                response_format="verbose_json",
                timestamp_granularities=["segment"],
            )
        segments_payload = getattr(response, "segments", None) or []
        segments = tuple(
            TranscriptSegment(
                start=float(seg.get("start", 0.0) or 0.0),
                end=float(seg.get("end", 0.0) or 0.0),
                text=str(seg.get("text", "") or "").strip(),
            )
            for seg in segments_payload
            if str(seg.get("text", "") or "").strip()
        )
        return TranscriptionResult(
            text=str(getattr(response, "text", "") or "").strip(),
            segments=segments,
            language=str(getattr(response, "language", "") or "") or None,
            duration_seconds=float(getattr(response, "duration", 0.0) or 0.0) or None,
            engine=self.name,
            model=self.model,
        )


# ---------- Public API ----------

class _NullTranscriber:
    name = "none"

    def transcribe(self, audio_path: Path) -> TranscriptionResult:  # noqa: ARG002
        raise TranscriberUnavailable("transcriber disabled (SOCIAL_MEDIA_TRANSCRIBER=none)")


def get_transcriber(engine: str | None = None) -> _FasterWhisperEngine | _OpenAIWhisperEngine | _NullTranscriber:
    """Resolve transcriber engine. Default: env var or faster_whisper."""
    selected = (engine or os.getenv("SOCIAL_MEDIA_TRANSCRIBER") or "faster_whisper").strip().lower()
    if selected == "none":
        return _NullTranscriber()
    if selected == "openai_api":
        return _OpenAIWhisperEngine()
    if selected == "faster_whisper":
        return _FasterWhisperEngine()
    raise TranscriberUnavailable(f"Unknown transcriber engine: {engine!r}")


async def transcribe_clip(
    video_path: str | Path,
    *,
    engine: Any | None = None,
) -> TranscriptionResult:
    """Async-Wrapper: extrahiert Audio aus dem Clip und transkribiert ihn.

    Args:
        video_path: Pfad zum (lokalen) MP4 / Source-File.
        engine: Optionale Transcriber-Instanz (zum Mocken in Tests).
    """
    video = Path(video_path)
    transcriber = engine if engine is not None else get_transcriber()

    def _run_blocking() -> TranscriptionResult:
        with tempfile.TemporaryDirectory(prefix="social-media-whisper-") as tmp:
            wav_path = Path(tmp) / "audio.wav"
            _extract_audio(video, wav_path)
            return transcriber.transcribe(wav_path)

    return await asyncio.to_thread(_run_blocking)
