"""Lokaler STT-Dienst mit ausschließlich expliziten, geprüften Startargumenten.

Der produktive Einstieg ist ``tb-stt-launcher --config /.../config/bot.toml``.
Der Rust-Lader besitzt das Schema und prüft die gesamte Bot-Konfiguration,
bevor dieser Python-Ausnahmedienst gestartet wird. Hier gibt es weder einen
zweiten TOML-Lader noch STT-Einstellungen aus der Prozessumgebung.
"""
from __future__ import annotations

import argparse
import io
import ipaddress
import math
import re
import time
import wave
from dataclasses import dataclass
from pathlib import Path
from typing import Callable


@dataclass(frozen=True, repr=False)
class Settings:
    host: str
    port: int
    model: str
    threads: int
    language: str
    no_speech_max: float
    avg_logprob_min: float
    max_upload_bytes: int
    cache_directory: str
    config_fingerprint: str

    def validate(self) -> None:
        """Verteidigt auch einen direkten CLI-Aufruf, ohne Eingaben auszugeben."""
        try:
            local = ipaddress.ip_address(self.host).is_loopback
        except ValueError:
            local = False
        model_path = Path(self.model)
        # Der Launcher löst lokale Modelle gegen die Config-Datei auf. Deren
        # Verzeichnis darf Leerzeichen/Umlaute enthalten und länger sein als
        # ein Hub-Bezeichner. Existierende absolute Pfade getrennt validieren.
        try:
            local_model = (
                model_path.is_absolute()
                and not any(ord(char) < 32 or ord(char) == 127 for char in self.model)
                and model_path.is_dir()
            )
        except (OSError, ValueError):
            local_model = False
        valid = (
            local
            and 1 <= self.port <= 65535
            and 1 <= self.threads <= 64
            and (local_model or (not model_path.is_absolute()
                                and bool(re.fullmatch(r"[A-Za-z0-9/_.-]{1,512}", self.model))))
            and (not self.language or bool(re.fullmatch(r"[a-z-]{1,16}", self.language)))
            and math.isfinite(self.no_speech_max)
            and 0 <= self.no_speech_max <= 1
            and math.isfinite(self.avg_logprob_min)
            and -20 <= self.avg_logprob_min <= 0
            and 1024 <= self.max_upload_bytes <= 25 * 1024 * 1024
            and Path(self.cache_directory).is_absolute()
            and not any(ord(char) < 32 for char in self.cache_directory)
            and bool(re.fullmatch(r"[a-f0-9]{64}", self.config_fingerprint))
        )
        if not valid:
            raise ValueError("Die geprüften STT-Startargumente sind ungültig.")


class RedactedParser(argparse.ArgumentParser):
    def error(self, message: str) -> None:
        # argparse würde sonst den falschen Wert oder unbekannte Argumente
        # wiedergeben. Die numerischen Grenzen stehen in der Support-Doku.
        self.exit(2, "Die STT-Startargumente sind unvollständig oder ungültig.\n")


def parse_arguments(arguments: list[str] | None = None) -> Settings:
    parser = RedactedParser(description="Lokaler Twitch-STT-Dienst; Start über tb-stt-launcher.")
    for name in ("host", "model", "language", "cache-directory", "config-fingerprint"):
        parser.add_argument(f"--{name}", required=True)
    for name in ("port", "threads", "max-upload-bytes"):
        parser.add_argument(f"--{name}", required=True, type=int)
    for name in ("no-speech-max", "avg-logprob-min"):
        parser.add_argument(f"--{name}", required=True, type=float)
    settings = Settings(**vars(parser.parse_args(arguments)))
    try:
        settings.validate()
    except ValueError:
        parser.error("invalid")
    return settings


def is_speech(segment, settings: Settings) -> bool:
    """Nur Segmente innerhalb der beiden konfigurierten Qualitätsschranken."""
    no_speech = getattr(segment, "no_speech_prob", 0.0) or 0.0
    avg_logprob = getattr(segment, "avg_logprob", 0.0) or 0.0
    return no_speech <= settings.no_speech_max and avg_logprob >= settings.avg_logprob_min


def create_app(settings: Settings, *, model_factory: Callable | None = None):
    # Vollständig vor dem Framework- und Modellaufbau prüfen.
    settings.validate()
    import numpy as np
    from fastapi import FastAPI, Form, HTTPException, UploadFile

    app = FastAPI(title="tb-stt")
    app.state.model = None
    app.state.transcriptions_started = 0
    app.state.transcriptions_completed = 0
    app.state.last_audio_at = None

    @app.on_event("startup")
    def load_model() -> None:
        factory = model_factory
        if factory is None:
            from faster_whisper import WhisperModel
            factory = WhisperModel
        started = time.perf_counter()
        app.state.model = factory(
            settings.model,
            device="cpu",
            compute_type="int8",
            cpu_threads=settings.threads,
            download_root=settings.cache_directory,
        )
        print(
            "TWITCH_STT_CONFIG_V1 "
            f"fingerprint={settings.config_fingerprint} threads={settings.threads} "
            f"model_loaded_seconds={time.perf_counter() - started:.1f}",
            flush=True,
        )

    @app.get("/health")
    def health() -> dict:
        return {
            "status": "ok" if app.state.model else "loading",
            "model": settings.model,
            "threads": settings.threads,
            "config_fingerprint": settings.config_fingerprint,
            "transcriptions_started": app.state.transcriptions_started,
            "transcriptions_completed": app.state.transcriptions_completed,
            "last_audio_at": app.state.last_audio_at,
        }

    def decode_wav(raw: bytes):
        try:
            with wave.open(io.BytesIO(raw)) as audio:
                if audio.getsampwidth() != 2 or audio.getnchannels() != 1:
                    raise HTTPException(400, "erwartet 16-bit Mono-PCM")
                rate = audio.getframerate()
                frames = audio.readframes(audio.getnframes())
            if rate <= 0 or not frames:
                raise HTTPException(400, "leeres oder ungültiges PCM-Audio")
            pcm = np.frombuffer(frames, dtype=np.int16).astype(np.float32) / 32768.0
            return pcm, len(pcm) / rate
        except (wave.Error, EOFError, ValueError):
            raise HTTPException(400, "ungültiges PCM-Audio") from None

    # Keine künftigen Annotationen auf diesen lokal importierten FastAPI-Typen:
    # FastAPI muss UploadFile beim Aufbau der Route auflösen können.
    async def transcriptions(
        file,
        model: str = Form(default=settings.model),
        language: str = Form(default=""),
        response_format: str = Form(default="verbose_json"),
    ) -> dict:
        current_model = app.state.model
        if current_model is None:
            raise HTTPException(503, "Modell lädt noch")
        raw = await file.read(settings.max_upload_bytes + 1)
        if not raw or len(raw) > settings.max_upload_bytes:
            raise HTTPException(400, "leeres oder zu großes Audio")
        pcm, duration = decode_wav(raw)
        app.state.transcriptions_started += 1
        app.state.last_audio_at = time.time()
        started = time.perf_counter()
        segments, info = current_model.transcribe(
            pcm,
            language=settings.language or None,
            beam_size=5,
            vad_filter=True,
            condition_on_previous_text=False,
        )
        raw_segments = list(segments)
        segments = [segment for segment in raw_segments if is_speech(segment, settings)]
        text = " ".join(segment.text.strip() for segment in segments).strip()
        app.state.transcriptions_completed += 1
        elapsed = time.perf_counter() - started
        print(
            f"transcribe: audio_seconds={duration:.1f} elapsed_seconds={elapsed:.2f} "
            f"segments={len(segments)} rejected_segments={len(raw_segments) - len(segments)}",
            flush=True,
        )
        body = {
            "text": text,
            "duration": duration,
            "language": getattr(info, "language", language),
            "model": settings.model,
        }
        if response_format == "verbose_json":
            body["segments"] = [
                {"id": index, "start": segment.start, "end": segment.end, "text": segment.text.strip()}
                for index, segment in enumerate(segments)
            ]
        return body

    transcriptions.__annotations__["file"] = UploadFile
    app.post("/v1/audio/transcriptions")(transcriptions)
    return app


def main() -> None:
    settings = parse_arguments()
    app = create_app(settings)
    import uvicorn
    uvicorn.run(app, host=settings.host, port=settings.port, log_level="warning")


if __name__ == "__main__":
    main()
