# Lokale Transkription in Rust

Der lokale STT-Dienst ist ein Rust-Binary: `rust/bin/tb-stt-server`. Er hält
Whisper dauerhaft im Speicher und spricht weiterhin die bestehende
OpenAI-kompatible Schnittstelle:

- `GET /health`
- `POST /v1/audio/transcriptions`
- Multipart-Felder `file`, `model`, `language`, `response_format`
- `verbose_json` mit `text`, `duration`, `language`, `model` und
  Segment-Zeitstempeln

Damit bleibt `rust/crates/tb-engagement/src/transcribe.rs` unverändert auf
`http://127.0.0.1:8791/v1/audio/transcriptions`.

## Warum Rust / whisper.cpp

Der frühere Sidecar bestand aus Python, FastAPI, Uvicorn, NumPy,
`faster-whisper` und CTranslate2. Der neue Dienst nutzt `whisper-rs` als
Rust-Bindung an whisper.cpp. Damit entfallen Python-Interpreter, Python-Webstack
und das separate venv im Produktionspfad.

Standardmodell ist `large-v3-turbo-q5_0` (ca. 574 MB). Es ist deutlich kleiner
als das unquantisierte Turbo-Modell und passt zum CPU-only Host. Zusätzlich wird
Silero VAD v6.2.0 geladen. Beide Dateien werden beim ersten Start einmalig nach
`~/.cache/deadlock-stt/` geladen und vor Aktivierung per SHA-256 geprüft.

## Build

```bash
cd rust
cargo test --locked -j2 -p tb-stt-server
cargo build --locked --release -j2 -p tb-stt-server
```

## Betrieb

Die User-Unit liegt versioniert unter:

`ops/stt-server/deadlock-stt-server.service`

Installation/Aktualisierung:

```bash
mkdir -p ~/.config/systemd/user
cp ops/stt-server/deadlock-stt-server.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now deadlock-stt-server.service
curl -fsS http://127.0.0.1:8791/health
```

Der Dienst hat keine Authentifizierung und bindet deshalb standardmäßig nur an
`127.0.0.1`.

## Konfiguration

| Variable | Default | Bedeutung |
|---|---|---|
| `STT_HOST` | `127.0.0.1` | Bind-Adresse |
| `STT_PORT` | `8791` | HTTP-Port |
| `STT_THREADS` | `8` | Whisper CPU-Threads |
| `STT_MODEL` | `ggml-large-v3-turbo-q5_0` | Modellname in Health/API |
| `STT_MODEL_PATH` | `~/.cache/deadlock-stt/ggml-large-v3-turbo-q5_0.bin` | lokaler Modellpfad |
| `STT_MODEL_URL` | offizielles whisper.cpp HF-Modell | Downloadquelle |
| `STT_MODEL_SHA256` | gepinnter Hash | Integritätsprüfung |
| `STT_VAD_MODEL_PATH` | `~/.cache/deadlock-stt/ggml-silero-v6.2.0.bin` | lokaler VAD-Pfad |
| `STT_LANGUAGE` | leer | leer = automatische Spracherkennung |
| `STT_NO_SPEECH_MAX` | `0.6` | Halluzinationsfilter |
| `STT_AVG_LOGPROB_MIN` | `-1.0` | Halluzinationsfilter |

`model` und `language` aus dem HTTP-Request werden wie beim bisherigen
Python-Dienst aus Kompatibilitätsgründen akzeptiert. Die lokale Modellwahl kommt
aus der Serverkonfiguration; ohne `STT_LANGUAGE` wird die Sprache automatisch
erkannt.

## Verhalten gegenüber dem alten Dienst

Beibehalten:

- Port 8791 und Pfade
- OpenAI-kompatibles Multipart
- 25-MiB Uploadgrenze
- 16-kHz/16-bit/Mono-PCM
- Beam Search mit Breite 5
- VAD
- automatische Spracherkennung
- `condition_on_previous_text=false`-Äquivalent via `no_context`
- Segment-Zeitstempel
- No-Speech-/Logprob-Filter
- ein dauerhaft geladenes Modell

Geändert:

- kein Python-Prozess/venv mehr
- whisper.cpp statt CTranslate2/faster-whisper
- q5_0-quantisiertes large-v3-turbo als ressourcenschonender Default
- Inferenz wird absichtlich auf eine Anfrage gleichzeitig begrenzt, weil jeder
  Lauf intern bereits acht CPU-Threads nutzt
