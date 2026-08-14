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
   2 Minuten, hoechstens 6 Stunden Sendungszeit je Sendung. Kurze Bloecke,
   weil der lokale STT-Dienst mit Reaktionen und Smalltalk geteilt wird.
3. Fertige Bloecke gehen in eine Warteschlange; ausgewertet wird seriell.
4. Transkription: lokaler STT-Dienst (`deadlock-stt-server`, faster-whisper).
5. Pruefung: drei feste Regeln ueber dem Transkript, danach ein Modellschritt
   ueber den Anbieter des Twitch-Bots. Er sieht alle Segmente des Blocks, nicht
   nur die ohne Regeltreffer, in Stapeln zu 20.
6. Bericht als Markdown und JSON unter `STREAM_AUDIT_OUTPUT_DIR`, Kurzmeldung
   als DM ueber den Master-Broker. Ein Block, dessen Transkription dreimal
   scheiterte, kommt nie so weit: von ihm bleiben die Aufnahme und die
   DM "aufgegeben".

## Datenschutz

- **Audio verlaesst den Rechner nie.** Zeigt `ENGAGEMENT_STT_BASE_URL` auf einen
  fremden Host, bricht der Start ab; nur `STREAM_AUDIT_ALLOW_REMOTE_STT=1` hebt
  das auf.
- An das Modell gehen Transkriptausschnitte, vorher durch die Schwaerzung
  geschickt, mit anonymer Segmentnummer statt Segment-ID. Die Schwaerzung kennt
  die drei Muster aus `rules.rs` und sonst nichts: anderer Wortlaut geht mit.
  Deshalb setzt die Unit `STREAM_AUDIT_ALLOW_REMOTE_LLM=1` ausdruecklich.
- Berichte tragen den geschwaerzten Beleg und SHA-256 des Originals. Geschwaerzt
  werden nur die drei bekannten Muster: was ein Modellfund sonst an Wortlaut im
  Segment mitbringt, steht im Bericht. Er ist deshalb keine zitatfreie Datei,
  sondern eine Akte mit Modus 0600. Die DM traegt weder Zitat noch Hash.
- Die Frist ist nicht die einzige Grenze: ueber `STREAM_AUDIT_MAX_KEEP_GB`
  weichen die aeltesten Aufnahmen, sobald der Bestand die Obergrenze reisst.
  Noetig, weil ein ausgefallener Modellschritt jeden Block als unvollstaendig
  geprueft markiert und damit jede Aufnahme aufhebt. Greift die Grenze, gibt es
  eine DM: die geloeschten Aufnahmen sind als Beleg weg.
- Das Rohtranskript bleibt nur mit `STREAM_AUDIT_KEEP_TRANSCRIPT=1` liegen.
  Aufnahmen mit Fund bleiben liegen, saubere Bloecke werden geloescht. Sie
  liegen unter `STREAM_AUDIT_OUTPUT_DIR/aufnahmen`, nicht in `/tmp`. Die
  aufbewahrte `.ts`-Datei ist die vollstaendige HLS-Spur mit **Bild und Ton** -
  geprueft wird nur der Ton, aufbewahrt wird beides.
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
| `STREAM_AUDIT_MAX_KEEP_GB` | Obergrenze aller aufbewahrten Aufnahmen, `0` = keine | 20 |
| `STREAM_AUDIT_ALLOW_REMOTE_STT` | Transkription auf fremdem Host | aus |
| `STREAM_AUDIT_ALLOW_REMOTE_LLM` | Modellschritt bei fremdem Anbieter | aus im Code, `1` in der Unit |
| `STREAM_AUDIT_DISCORD_USER_ID` | Empfaenger der DM | Admin-ID aus `melden.rs` |
| `ENGAGEMENT_STT_BASE_URL` | lokaler Whisper-Endpunkt | `http://127.0.0.1:8791/v1/audio/transcriptions` |
| `VOICE_REACTION_STREAMLINK_BIN` | streamlink fuer die Aufnahme | `streamlink` aus dem `PATH` |

## Deploy

Der Dienst baut sich nicht selbst. Das Startskript prueft nur, ob das Binary da
ist, und bricht sonst mit Exit 5 und einer klaren Meldung ab (statt mit dem
nackten `203/EXEC`, an dem der Vorgaenger monatelang haengen blieb).

```bash
# im Deploy-Worktree, nicht im Arbeits-Checkout
cd ~/.worktrees/tb-deploy/rust
SQLX_OFFLINE=true cargo build --release -p tb-stream-audit-bin
cp target/release/tb-stream-audit ~/repos/Deadlock-Twitch-Bot/rust/target/release/tb-stream-audit.neu
mv ~/repos/Deadlock-Twitch-Bot/rust/target/release/tb-stream-audit{.neu,}

# Unit einmalig installieren, danach nur noch neu starten
cp ~/repos/Deadlock-Twitch-Bot/ops/systemd/deadlock-twitch-stream-coaching-watch.service \
   ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user restart deadlock-twitch-stream-coaching-watch
```

Der Umweg ueber `.neu` und `mv` ist noetig, weil `cp` auf die laufende Datei mit
`ETXTBSY` scheitert.

## Betrieb

```bash
systemctl --user status deadlock-twitch-stream-coaching-watch
journalctl --user -u deadlock-twitch-stream-coaching-watch -f
```

- Exit 5: das Binary fehlt im Zielpfad, siehe Deploy.
- Exit 4: der Infisical-Loader `dl-infisical-env` fehlt oder ist nicht
  ausfuehrbar.
- Exit 3: `INFISICAL_SERVICE_TOKEN` steht weder in `infisical.conf` noch als
  systemd-Credential.
- Exit 6: `~/.config/deadlock-twitch-bot/infisical.conf` fehlt.
- Exit 2: Konfiguration fehlt (Kanaele, Helix) oder der Schutz gegen entfernte
  Transkription hat gegriffen. Grund steht im Klartext im Journal. Ohne
  `ENGAGEMENT_STT_BASE_URL` faellt der Endpunkt auf localhost zurueck - der
  Dienst startet also, findet aber nur dann etwas, wenn dort wirklich der
  STT-Dienst horcht.
- Exit 1: eine der beiden Schleifen ist gestorben; systemd startet neu.
- Keine DM heisst in aller Regel: nichts gefunden. Gemeldet wird auch, wenn der
  Modellschritt ausfiel, ein Block aufgegeben wurde, die Aufnahme wegen
  Rueckstands pausiert oder die Twitch-Abfrage scheitert. Nimmt der Broker die
  DM ueber Stunden nicht an, liegt der Bericht weiter im Ausgabeordner und der
  Aufraeumtakt bietet die Meldung erneut an - Stille ist dann kein Beweis.
