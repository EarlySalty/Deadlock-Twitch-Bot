# Privater Stream-Coaching-Audit

Der Dienst nimmt die Streams der eigenen Leute live auf, transkribiert sie auf
diesem Rechner und meldet auffaellige Stellen als Discord-DM an den Admin. Ziel
ist ein privates Coaching-Gespraech mit nachvollziehbaren Zeitstempeln. Der
Audit loest keine Sanktionen aus, schreibt in keinen Chat und meldet nichts an
Twitch.

Ausfuehrliche Fassung: `internal/deadlock-twitch-bot/stream-coaching-audit.html`
im Deadlock-Docs-Korpus.

> Die frueheren Python-Kommandos (`scripts/audit_stream_tos.py`, `--authorized`,
> VOD- und Datei-Modus) gibt es nicht mehr. Die Python-Laufzeit ist abgeraeumt;
> der Dienst laeuft als Rust-Binaerprogramm unter systemd.

## Was laeuft

- Unit: `deadlock-twitch-stream-coaching-watch.service`
  (`ops/systemd/`), Start ueber `rust/scripts/run_stream_audit_service.sh`.
- Code: `rust/crates/tb-stream-audit` (Regeln, Plan, Bericht, Meldung),
  `rust/bin/tb-stream-audit` (Aufnahme, Auswertung, Ablage).
- Kein VOD, kein lokaler Datei-Modus: ob ein Kanal seine VODs behaelt,
  entscheidet der Kanal. Ein Audit, das darauf baut, faellt still aus.

## Ablauf

1. Alle 60 Sekunden fragt der Dienst ueber Helix ab, wer sendet.
2. Je sendendem Kanal laeuft eine eigene Aufnahmeschleife, in Bloecken von
   10 Minuten, hoechstens 6 Stunden Sendungszeit je Sendung.
3. Fertige Bloecke gehen in eine Warteschlange; ausgewertet wird seriell.
4. Transkription: lokaler STT-Dienst (`deadlock-stt-server`, faster-whisper).
5. Pruefung: drei feste Regeln ueber dem Transkript, danach ein Modellschritt
   ueber den Anbieter des Twitch-Bots. Er sieht alle Segmente des Blocks, nicht
   nur die ohne Regeltreffer, in Stapeln zu 20.
6. Bericht als Markdown und JSON unter `STREAM_AUDIT_OUTPUT_DIR`, Kurzmeldung
   als DM ueber den Master-Broker.

## Datenschutz

- **Audio verlaesst den Rechner nie.** Zeigt `ENGAGEMENT_STT_BASE_URL` auf einen
  fremden Host, bricht der Start ab; nur `STREAM_AUDIT_ALLOW_REMOTE_STT=1` hebt
  das auf.
- An das Modell gehen Transkriptausschnitte, vorher durch die Schwaerzung
  geschickt, mit anonymer Segmentnummer statt Segment-ID. Die Schwaerzung kennt
  die drei Muster aus `rules.rs` und sonst nichts: anderer Wortlaut geht mit.
  Deshalb setzt die Unit `STREAM_AUDIT_ALLOW_REMOTE_LLM=1` ausdruecklich.
- Berichte tragen den geschwaerzten Beleg und SHA-256 des Originals, nie ein
  ungefiltertes Zitat. Die DM traegt weder Zitat noch Hash.
- Das Rohtranskript bleibt nur mit `STREAM_AUDIT_KEEP_TRANSCRIPT=1` liegen.
  Aufnahmen mit Fund bleiben liegen, saubere Bloecke werden geloescht. Sie
  liegen unter `STREAM_AUDIT_OUTPUT_DIR/aufnahmen`, nicht in `/tmp`.
- Dateien mit Modus `0600`, Verzeichnisse ueber `UMask=0077`.
- Voice-to-Text und Modell koennen irren. Jede Fundstelle ist ein Verdacht und
  muss von Hand geprueft werden.

## Schalter

| Variable | Bedeutung | Default |
| --- | --- | --- |
| `STREAM_AUDIT_CHANNELS` | Kanaele, Komma/Leerzeichen getrennt | leer (Start bricht ab) |
| `STREAM_AUDIT_OUTPUT_DIR` | Ablage der Berichte | `data/stream_coaching_audits` |
| `STREAM_AUDIT_KEEP_TRANSCRIPT` | Rohtranskript behalten | aus |
| `STREAM_AUDIT_RETENTION_DAYS` | Aufbewahrung, `0` = unbegrenzt | 30 |
| `STREAM_AUDIT_ALLOW_REMOTE_STT` | Transkription auf fremdem Host | aus |
| `STREAM_AUDIT_ALLOW_REMOTE_LLM` | Modellschritt bei fremdem Anbieter | aus im Code, `1` in der Unit |
| `STREAM_AUDIT_DISCORD_USER_ID` | Empfaenger der DM | Admin-ID aus `melden.rs` |
| `ENGAGEMENT_STT_BASE_URL` | lokaler Whisper-Endpunkt | Start bricht ohne ab |
| `VOICE_REACTION_STREAMLINK_BIN` | streamlink fuer die Aufnahme | `streamlink` aus dem `PATH` |

## Betrieb

```bash
systemctl --user status deadlock-twitch-stream-coaching-watch
journalctl --user -u deadlock-twitch-stream-coaching-watch -f
```

- Exit 2: Konfiguration fehlt (Kanaele, Helix, STT-URL) oder der Schutz gegen
  entfernte Transkription hat gegriffen. Grund steht im Klartext im Journal.
- Exit 1: eine der beiden Schleifen ist gestorben; systemd startet neu.
- Keine DM heisst: nichts gefunden. Gemeldet wird auch, wenn der Modellschritt
  ausfiel oder ein Block endgueltig aufgegeben wurde.
