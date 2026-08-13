"""Lokaler Transkriptions-Dienst, OpenAI-kompatibel.

Spricht `POST /v1/audio/transcriptions` in genau der Form, die
`rust/crates/tb-engagement/src/transcribe.rs` bereits erwartet: Multipart mit
`file`/`model`/`language`/`response_format`, Antwort als `verbose_json` mit
`text` und `duration`. Damit bleibt der Rust-Pfad unveraendert bis auf die
Basis-URL.

Das Modell wird beim Start einmal geladen und bleibt im Speicher — ein
Neuladen je Anfrage wuerde die Latenz vervielfachen.

Start:
    stt_server.py            # Default: 127.0.0.1:8791, large-v3-turbo, 8 Threads

Konfiguration ueber Umgebung:
    STT_MODEL      Modell-ID oder Pfad (Default large-v3-turbo ct2)
    STT_THREADS    CPU-Threads je Anfrage (Default 8 — auf dieser Maschine
                   gemessen schneller als 16, siehe ops/stt-server/README.md)
    STT_HOST/PORT  Bindung; nur lokal binden, der Dienst hat keine Auth.
"""
import io
import os
import time
import wave

import numpy as np
from fastapi import FastAPI, Form, HTTPException, UploadFile
from faster_whisper import WhisperModel

MODEL_ID = os.environ.get("STT_MODEL", "deepdml/faster-whisper-large-v3-turbo-ct2")
THREADS = int(os.environ.get("STT_THREADS", "8"))
DEFAULT_LANGUAGE = os.environ.get("STT_LANGUAGE", "de")
MAX_UPLOAD_BYTES = 25 * 1024 * 1024  # gleiche Grenze wie der Rust-Aufrufer

app = FastAPI(title="tb-stt")
_model: WhisperModel | None = None


@app.on_event("startup")
def _load() -> None:
    global _model
    started = time.perf_counter()
    _model = WhisperModel(MODEL_ID, device="cpu", compute_type="int8", cpu_threads=THREADS)
    print(f"Modell geladen: {MODEL_ID} ({THREADS} Threads) in {time.perf_counter()-started:.1f}s",
          flush=True)


@app.get("/health")
def health() -> dict:
    return {"status": "ok" if _model else "loading", "model": MODEL_ID, "threads": THREADS}


def _decode_wav(raw: bytes) -> tuple[np.ndarray, float]:
    """16-kHz-Mono-PCM-WAV -> float32. Der Aufrufer liefert genau dieses Format."""
    with wave.open(io.BytesIO(raw)) as w:
        if w.getsampwidth() != 2 or w.getnchannels() != 1:
            raise HTTPException(400, "erwartet 16-bit Mono-PCM")
        frames = w.readframes(w.getnframes())
        rate = w.getframerate()
    pcm = np.frombuffer(frames, dtype=np.int16).astype(np.float32) / 32768.0
    return pcm, len(pcm) / rate if rate else 0.0


@app.post("/v1/audio/transcriptions")
async def transcriptions(
    file: UploadFile,
    model: str = Form(default=MODEL_ID),
    language: str = Form(default=DEFAULT_LANGUAGE),
    response_format: str = Form(default="verbose_json"),
) -> dict:
    if _model is None:
        raise HTTPException(503, "Modell laedt noch")
    raw = await file.read()
    if not raw or len(raw) > MAX_UPLOAD_BYTES:
        raise HTTPException(400, "leeres oder zu grosses Audio")

    pcm, duration = _decode_wav(raw)
    started = time.perf_counter()
    segments, info = _model.transcribe(
        pcm,
        language=language or None,
        beam_size=5,
        vad_filter=True,
        condition_on_previous_text=False,
    )
    segments = list(segments)
    text = " ".join(s.text.strip() for s in segments).strip()
    elapsed = time.perf_counter() - started
    print(f"transcribe: {duration:.1f}s Audio in {elapsed:.2f}s (RTF {elapsed/max(duration,0.01):.3f}), "
          f"{len(segments)} Segmente", flush=True)

    body = {
        "text": text,
        "duration": duration,
        "language": getattr(info, "language", language),
    }
    if response_format == "verbose_json":
        body["segments"] = [
            {"id": i, "start": s.start, "end": s.end, "text": s.text.strip()}
            for i, s in enumerate(segments)
        ]
    return body


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(
        app,
        host=os.environ.get("STT_HOST", "127.0.0.1"),
        port=int(os.environ.get("STT_PORT", "8791")),
        log_level="warning",
    )
