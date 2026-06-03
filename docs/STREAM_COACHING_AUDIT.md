# Privater Stream-Coaching-Audit

Der Audit analysiert autorisierte Twitch-VODs, zeitlich begrenzte Live-Mitschnitte oder
lokale Aufnahmen. Ziel ist ein privates Coaching-Gespraech mit nachvollziehbaren
Zeitstempeln. Der Audit loest keine automatischen Sanktionen aus.

## Datenschutz und Freigaben

- Jeder Lauf braucht `--authorized`.
- Standardmaessig transkribiert lokales `faster-whisper`.
- `--transcriber openai_api` braucht zusaetzlich `--allow-remote-transcription`.
- `--llm-provider minimax` braucht zusaetzlich `--allow-remote-llm`.
- Der Report speichert kein vollstaendiges Rohtranskript, aber pro Fundstelle einen
  unmaskierten Klartext-Beleg (Wortlaut im Kontext) fuer die Beweisfuehrung. Reports landen
  unter `data/stream_coaching_audits/` mit Dateirechten `0600`. Der Klartext geht zusaetzlich
  nur in die private Admin-DM; der oeffentlichere Webhook-Kanal bleibt maskiert.
- Live-Fundstellen tragen die echte Uhrzeit der Aeusserung, VOD-Fundstellen einen
  `?t=`-Sprunglink direkt auf die Stelle.
- Voice-to-Text und LLM koennen Fehler machen. Jede Fundstelle muss manuell mit dem
  VOD-Kontext geprueft werden.

## Aufruf

Gesamtes Twitch-VOD lokal transkribieren und mit lokalen Hochsignal-Regeln pruefen:

```bash
.venv/bin/python scripts/audit_stream_tos.py \
  --authorized \
  https://www.twitch.tv/videos/123456789
```

Zusaetzliche LLM-Kontextpruefung ueber MiniMax:

```bash
.venv/bin/python scripts/audit_stream_tos.py \
  --authorized \
  --llm-provider minimax \
  --allow-remote-llm \
  https://www.twitch.tv/videos/123456789
```

Fortlaufenden Live-Stream mit etwa einer Minute Verzoegerung pruefen:

```bash
.venv/bin/python scripts/audit_stream_tos.py \
  --authorized \
  --watch-live \
  earlysalty
```

Jedes Live-Fenster umfasst standardmaessig 55 Sekunden. Direkt danach transkribiert und
prueft der Audit den Ausschnitt. Die reale Verzoegerung ist daher etwa eine Minute plus
wenige Sekunden fuer die lokale Transkription. Neue Fundstellen erscheinen sofort in der
Konsole. Mit einem privaten Discord-Webhook koennen sie zusaetzlich direkt an das
Moderationsteam gehen:

```bash
export STREAM_AUDIT_DISCORD_WEBHOOK='...'
.venv/bin/python scripts/audit_stream_tos.py \
  --authorized \
  --watch-live \
  --discord-alerts \
  earlysalty
```

Webhook-Secrets gehoeren in das vorhandene Secret-Setup und nicht in Git. Der Audit postet
keine automatischen Hinweise in den oeffentlichen Twitch-Chat. Fuer die Nachbesprechung
kann nach Stream-Ende weiterhin das gesamte VOD analysiert werden.

Private Bot-DMs an die konfigurierte Admin-ID funktionieren auch fuer mehrere
Partner-Kandidaten parallel. Mit OpenAI-Whisper ist die externe Audio-Uebertragung bewusst
explizit freizugeben:

```bash
scripts/run_with_infisical.sh .venv/bin/python scripts/audit_stream_tos.py \
  --authorized \
  --watch-live \
  --transcriber openai_api \
  --allow-remote-transcription \
  --discord-dm \
  https://www.twitch.tv/helmbombenricky \
  https://www.twitch.tv/skifahrertv
```

Fuer Bot-DMs wird der erste gesetzte Token aus
`STREAM_AUDIT_DISCORD_BOT_TOKEN`, `COACHING_BOT_TOKEN`, `DISCORD_TOKEN` oder `BOT_TOKEN`
verwendet. Das DM-Ziel kommt aus `STREAM_AUDIT_DISCORD_USER_ID`, danach aus der vorhandenen
Admin-Konfiguration.

Der produktive User-Service fuer die aktuell geprueften Partner-Kandidaten liegt unter
`ops/deadlock-twitch-stream-coaching-watch.service`. Er sendet beim Start eine private
Status-DM und wartet bei Offline-Kanaelen automatisch auf den naechsten Live-Start.

## Voraussetzungen

- `ffmpeg`
- Fuer VODs: `yt-dlp`
- Fuer lokale Transkription: Python-Paket `faster-whisper`
- Fuer externe APIs: passende Secrets im vorhandenen Secret-Setup
