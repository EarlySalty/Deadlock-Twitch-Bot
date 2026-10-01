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

Vorher die unten beschriebene normale Konfiguration installieren. Hauptdienst
und Recovery lesen dieselbe Datei im Benutzerverzeichnis; die geschützte
vollständige Bot-Konfiguration wird dafür weder kopiert noch freigegeben.

```bash
mkdir -p ~/.local/bin ~/.config/systemd/user
install -d -m 0700 ~/.config/deadlock-stt
test -r ~/.config/deadlock-stt/stt.toml
install -m 0755 rust/target/release/tb-stt-server ~/.local/bin/tb-stt-server
cp ops/stt-server/deadlock-stt-server.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now deadlock-stt-server.service
curl -fsS http://127.0.0.1:8791/health
```

Der Dienst hat keine Authentifizierung und bindet deshalb standardmäßig nur an
`127.0.0.1`.

## Healthcheck und begrenzte Wiederherstellung

`deadlock-stt-recovery.timer` startet alle fünf Minuten einen kurzen Healthcheck
im vorhandenen Rust-Binary. Er fragt nur `GET /health` am Loopback-Endpunkt aus
der normalen Konfiguration `~/.config/deadlock-stt/stt.toml` ab; Modell und
Inferenz werden nicht geladen.
Eine Wiederherstellung erfolgt ausschließlich bei einer weiterhin aktivierten
und aktiven Haupt-Unit. Der feste Aufruf nutzt `try-restart`, startet also keine
administrativ gestoppte Unit. Ein persistenter Status in der systemd-
StateDirectory begrenzt Wiederholungen mit wachsendem Abstand bis höchstens
sechs Stunden. Wiederholte Recoverymeldungen werden höchstens einmal pro Tag
und zweimal innerhalb sieben Tagen protokolliert; die Zahl unterdrückter
Wiederholungen steht in der nächsten Meldung. Jede neue aktive Startgeneration
bekommt zuerst 15 Minuten Startup-Karenz. Die Zeit beginnt bei der ersten
ungesunden Probe dieser von systemd gemeldeten Startgeneration; ein Neustart
setzt sie zurück. Dadurch kann die Recovery nach einem gemeldeten Neustart bis
zu 15 Minuten zusätzlich warten. Das Backoff läuft über Recovery-Neustarts
weiter und wird erst nach einem erfolgreichen Healthcheck zurückgesetzt.

Installation neben der Haupt-Unit:

```bash
cp ops/stt-server/deadlock-stt-recovery.service ~/.config/systemd/user/
cp ops/stt-server/deadlock-stt-recovery.timer ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now deadlock-stt-recovery.timer
```

## Konfiguration

Beide Units verwenden ausdrücklich `--config` mit dem absoluten, durch systemd
aufgelösten Pfad `~/.config/deadlock-stt/stt.toml`. Die Datei gehört dem
Dienstbenutzer und hat Modus `0600`; das Verzeichnis hat Modus `0700`.
Umgebungsvariablen konfigurieren diese Einstellungen nicht.

Der vorhandene `BotConfigSnapshot` verlangt neben `[stt]` die Schema-Version
und drei öffentliche Twitch-Pflichtfelder. Dafür ausschließlich die bestehenden
öffentlichen Werte übernehmen, keine Zugangsdaten oder vollständige Bot-Datei.
Für die bestehende Installation ist der minimale Schemaumschlag:

```toml
schema_version = 1

[twitch]
bot_user_id = "1422558159"
notify_channel_id = "1304169815505637458"
eventsub_callback_url = "https://deutsche-deadlock-community.de/twitch/eventsub/callback"

[stt]
host = "127.0.0.1"
port = 8791
model = "ggml-large-v3-turbo-q5_0"
threads = 8
```

Der STT-Dienst baut daraus keine Twitch-Verbindung auf. Fehlende optionale
STT-Felder verwenden die bestehenden Defaults:

| Feld unter `[stt]` | Default | Bedeutung |
|---|---|---|
| `host` | `127.0.0.1` | Bind-Adresse |
| `port` | `8791` | HTTP-Port |
| `threads` | `8` | Whisper CPU-Threads |
| `model` | `ggml-large-v3-turbo-q5_0` | Modellname in Health/API |
| `language` | nicht gesetzt | automatische Spracherkennung |
| `no_speech_max` | `0.6` | Halluzinationsfilter |
| `avg_logprob_min` | `-1.0` | Halluzinationsfilter |
| `max_upload_bytes` | `26214400` | Uploadgrenze in Bytes |
| `timeout_seconds` | `60` | Anfragezeitlimit der Aufrufer |
| `extraction_timeout_seconds` | `300` | Zeitlimit der Audioextraktion |

Modelldateien liegen unter `~/.cache/deadlock-stt/`. Modellnamen, Downloadquellen
und SHA-256-Werte sind im Rust-Dienst fest gebunden; dafür gibt es keine
Konfigurationsfelder für beliebige Ersatzmodelle.

`model` und `language` aus dem HTTP-Request werden wie beim bisherigen
Python-Dienst aus Kompatibilitätsgründen akzeptiert. Die lokale Modellwahl kommt
aus der Serverkonfiguration; ohne `stt.language` wird die Sprache automatisch
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
