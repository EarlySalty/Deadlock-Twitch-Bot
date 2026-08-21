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
   2 Minuten, hoechstens 24 Stunden aufgenommener Zeit je Sendung. Mehrere
   Kanaele werden parallel mitgeschnitten. Die Aufnahme pausiert nicht, weil
   die Auswertung hinterherhaengt; sie pausiert nur, wenn die Aufnahmen
   12 GB ueberschreiten.
3. Beim Start einer Aufnahme geht eine DM an den Admin. Fertige Bloecke
   warten, bis der Kanal offline ist. Dann wird seriell transkribiert und
   geprueft, und eine Abschluss-DM listet die ToS-Funde der ganzen Sendung.
4. Transkription: lokaler STT-Dienst (`deadlock-stt-server`, faster-whisper).
   Ein Last-Gate stellt die Auswertung zurueck, wenn die **hoehere** von CPU-
   und RAM-Auslastung des Servers ununterbrochen ueber der Grenze (Standard
   90 Prozent, `STREAM_AUDIT_LOAD_LIMIT`) liegt, und zwar erst nach einem vollen
   Fenster (Standard 240 s, `STREAM_AUDIT_LOAD_WINDOW_SECS`). Es faellt wieder unter
   der Freigabe (Standard 80 Prozent, `STREAM_AUDIT_LOAD_RELEASE`). Aufgenommen
   wird die ganze Zeit weiter; die Bloecke werden nachgeholt. Ein Deckel
   (Standard 1800 s, `STREAM_AUDIT_LOAD_MAX_HOLD_SECS`) loest das Gate auch
   unter Dauerlast einmal, damit die Aufbewahrung keine ungeprueften Aufnahmen
   loescht.
5. Pruefung: drei feste Regeln ueber dem Transkript, danach ein Modellschritt
   ueber den Anbieter des Twitch-Bots. Er sieht alle Segmente des Blocks, nicht
   nur die ohne Regeltreffer, in Stapeln zu 20.
6. Bericht als Markdown und JSON unter `STREAM_AUDIT_OUTPUT_DIR`. Die
   Abschluss-DM nennt nur Funde, die Twitch ahnden wuerde. Ein Block, dessen
   Transkription dreimal scheiterte, kommt nie so weit: von ihm bleiben die
   Aufnahme und die DM "aufgegeben".
7. Parallel zu den kurzen Auswertungs-Bloecken laeuft je Stream ein
   **durchgehender Ton-Recorder** (streamlink liefert den Stream, ffmpeg zieht
   ohne Neucodierung nur die Tonspur heraus), der eine saubere 1:1-Aufnahme
   (`mitschnitt-<zeit>.aac`) schreibt - nicht aus den Auswertungs-Haeppchen
   zusammengestueckelt. Im Normalfall ist es eine Datei; ein Dienst-Neustart oder
   ein streamlink-Aussetzer waehrend der Sendung erzeugt wenige Teile (jeweils
   mit eigenem Zeitstempel), die alle in denselben Stream-Ordner hochgeladen
   werden. Video braucht das Coaching nicht; ohne Video ist die Datei winzig. Ist der Stream vorbei und jeder Block ausgewertet, wird
   diese Aufnahme zusammen mit den Berichten in einen eigenen Ordner je Stream
   unter `STREAM_AUDIT_DRIVE_REMOTE` geladen (rclone-Remote `gdrive:`). Erst wenn
   der Upload belegt ist (`rclone lsf` fuehrt jede einzelne hochgeladene Datei
   namentlich auf), wird lokal geloescht - so bleibt die
   Platte frei. Scheitert der Upload, bleibt alles liegen und der stuendliche
   Aufraeumtakt versucht es erneut; noch nicht hochgeladene Aufnahmen werden nie
   geloescht. Faellt der freie Platz unter `STREAM_AUDIT_DRIVE_MIN_FREE_GB`,
   startet **kein neuer** Recorder mehr (bestehende laufen weiter), damit die
   geteilte Platte nicht volllaeuft. Laesst sich der freie Platz nicht messen
   (kein `df`), wird trotzdem aufgenommen und das im Protokoll vermerkt: der Ton
   ist winzig, und ein df-Aussetzer soll die Aufnahme nicht abwuergen. Archiviert werden nur Streams **mit**
   Aufnahme; faellt der Recorder ganz aus, bleiben allein die lokalen Berichte
   der normalen Aufbewahrung. `STREAM_AUDIT_DRIVE_ARCHIVE=0` schaltet Recorder
   und Upload fuer alle ab; `STREAM_AUDIT_DRIVE_EXCLUDE=<kanal>` nur fuer
   einzelne Kanaele, die widersprochen haben. Fuer einen ausgenommenen Kanal
   startet kein Recorder und es geht nichts hoch; was von frueher noch liegt,
   faellt der normalen Aufbewahrung zu. Die Auswertungs-Haeppchen bleiben davon unberuehrt: sie werden
   wie bisher nach der Pruefung lokal geloescht.
8. **Wenn der Recorder scheitert.** Ein Recorder, der kurz nach dem Start endet,
   mit Fehler abbricht oder haengt (die Aufnahmedatei waechst 15 Minuten lang
   nicht), gilt als Fehlversuch: er wird abgeraeumt, die letzten
   stderr-Zeilen von streamlink und ffmpeg stehen in der Meldung, und der
   Wiederanlauf wartet mit sich verdoppelnder Pause. Nach fuenf Fehlversuchen
   startet fuer diesen Lauf kein Recorder mehr, und es gibt eine Fehlermeldung.
   Ohne diesen Deckel wuerde ein dauerhaft kaputtes ffmpeg im Minutentakt neu
   starten und jedes Mal Erfolg melden.
9. **Wenn der Upload haengt.** Solange das Drive-Archiv eines Laufs aussteht,
   bleiben seine Berichte von der Aufbewahrung verschont - aber hoechstens
   14 Tage ueber die Frist hinaus. Danach greift `STREAM_AUDIT_RETENTION_DAYS`
   wieder. Gewarnt wird schon ab 14 Tagen Rueckstau, hoechstens einmal je Lauf
   und Tag. Sonst waere die
   Aufbewahrungsfrist bei kaputtem rclone still ausser Kraft, und Berichte mit
   vollem Wortlaut laegen unbegrenzt da. Dasselbe gilt fuer die
   1:1-Ton-Mitschnitte: gelingt der Upload nie, werden sie nach
   `STREAM_AUDIT_RETENTION_DAYS` lokal geloescht, mit deutlicher Warnung. Ohne
   das ueberlebte ausgerechnet das sensibelste Artefakt jede Frist.
10. **Nachholen laeuft nacheinander.** Der stuendliche Takt arbeitet
   ausstehende Uploads in einem einzigen Hintergrund-Task nacheinander ab. Nach
   einem mehrtaegigen rclone-Ausfall waeren es sonst Dutzende gleichzeitiger
   rclone-Prozesse auf der geteilten Maschine.

## Datenschutz

- **Zur Transkription verlaesst das Audio den Rechner nie.** Zeigt
  `ENGAGEMENT_STT_BASE_URL` auf einen fremden Host, bricht der Start ab; nur
  `STREAM_AUDIT_ALLOW_REMOTE_STT=1` hebt das auf. Das betrifft den STT-Weg. Der
  durchgehende Ton-Mitschnitt wird dagegen bewusst ins eigene Google
  Drive archiviert (Schritt 7) - eine gewollte Ablage im eigenen Speicher, kein
  Versand an einen Transkriptionsdienst. `STREAM_AUDIT_DRIVE_ARCHIVE=0` schaltet
  den durchgehenden Recorder samt Upload ganz ab: dann entsteht diese
  1:1-Aufnahme gar nicht erst, und nur die kurzen Auswertungs-Bloecke laufen.
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
| `STREAM_AUDIT_LOAD_LIMIT` | Auslastung in Prozent, ab der das Gate greift | 90 |
| `STREAM_AUDIT_LOAD_RELEASE` | Auslastung in Prozent, unter der das Gate faellt | 80 |
| `STREAM_AUDIT_LOAD_WINDOW_SECS` | Sekunden Dauerlast, bevor das Gate greift | 240 |
| `STREAM_AUDIT_LOAD_MAX_HOLD_SECS` | Deckel, wie lange das Gate am Stueck haelt, `0` = kein Deckel | 1800 |
| `STREAM_AUDIT_DRIVE_ARCHIVE` | Fertigen Stream nach Drive archivieren, `0`/`aus` schaltet ab | an |
| `STREAM_AUDIT_DRIVE_EXCLUDE` | Kanaele ohne Ton-Mitschnitt (Widerspruch), kommagetrennt | leer |
| `STREAM_AUDIT_DRIVE_REMOTE` | Ziel-Basisordner im Drive (rclone-Remote) | `gdrive:Deadlock/Coaching-Audit` |
| `STREAM_AUDIT_DRIVE_MIN_FREE_GB` | Untergrenze freier Platz in ganzen GB; darunter startet kein neuer Recorder | 20 |
| `STREAM_AUDIT_RCLONE_BIN` | rclone-Binaerprogramm | PATH-Aufloesung `rclone`, in der Unit auf `/usr/local/bin/rclone` gepinnt |
| `STREAM_AUDIT_FFMPEG_BIN` | ffmpeg fuer den Recorder | `FFMPEG_BIN`, sonst `/usr/bin/ffmpeg` |
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
- Start und Ende jeder Sendung erzeugen eine DM. Die Abschluss-DM nennt ToS-Funde
  oder sagt, dass keine vorlagen. Gemeldet wird ausserdem, wenn der
  Modellschritt ausfiel, ein Block aufgegeben wurde, die Platte die
  Aufnahmegrenze erreicht oder die Twitch-Abfrage scheitert. Nimmt der Broker die
  DM ueber Stunden nicht an, liegt der Bericht weiter im Ausgabeordner und der
  Aufraeumtakt bietet die Meldung erneut an - Stille ist dann kein Beweis.
