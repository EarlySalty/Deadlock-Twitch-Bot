# tb-stream-audit — Architektur & Funktionsreferenz

> Pfad: `rust/crates/tb-stream-audit/`, `rust/bin/tb-stream-audit/` · Stand: 2026-08-14
>
> Teil der [Architektur-Doku](README.md). Produktdoku:
> [STREAM_COACHING_AUDIT.md](../STREAM_COACHING_AUDIT.md), Betriebsseite im
> Deadlock-Docs-Korpus (`internal/deadlock-twitch-bot/stream-coaching-audit.html`).
> Internes Admin-Werkzeug, kein nutzersichtbares Feature.

> Die frueheren Python-Pfade (`bot/stream_coaching_audit/`, VOD- und
> Datei-Modus, `--authorized`, MiniMax als fester Anbieter) gibt es nicht mehr.

## 1. Zweck & Abgrenzung

Der Dienst schneidet die Streams der eingetragenen eigenen Kanaele **live** mit,
transkribiert sie auf demselben Rechner und sucht im Text nach problematischen
Aeusserungen. Ergebnis ist ein privater Bericht mit Zeitstempeln und, wenn es
etwas zu melden gibt, eine Discord-DM an den Admin. Der Dienst moderiert nicht,
postet nichts oeffentlich und meldet nichts an Twitch.

Kein VOD-Modus: ob ein Kanal seine VODs behaelt, entscheidet der Kanal; ein
Audit, das darauf baut, faellt still aus.

## 2. Einordnung & Abhaengigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Laeuft als** | `deadlock-twitch-stream-coaching-watch.service` (systemd, Rust-Binaerprogramm). |
| **Nutzt** | `tb-transport-twitch` (Helix: wer sendet, `started_at`), `tb-engagement::audio_capture` (streamlink), `tb-engagement::transcribe` (lokaler STT-Dienst), `tb-llm::selection` (Anbieterwahl `stream_audit`), Master-Broker (`/internal/master/v1/discord/send-dm`). |
| **Daten** | `STREAM_AUDIT_OUTPUT_DIR` — im Systemdienst dauerhaft `/var/lib/deadlock-twitch-audit/data/stream_coaching_audits`; Berichte als `.md`/`.json` je Kanal, Aufnahmen unter `aufnahmen/<kanal>/<stream-id>/t<sekunde>-b<nummer>/<capture>/`. |
| **Externe Dienste** | Twitch (Helix, HLS), der Anbieter aus der Twitch-Bot-Konfiguration fuer den Modellschritt. Transkription bleibt lokal. |
| **Secret-Namen** | `TWITCH_CLIENT_ID`/`_SECRET`, Broker-Token, Anbieter-Key aus `tb-llm`. |

## 3. Dateien im Ueberblick

| Datei | Rolle |
|-------|-------|
| `crates/tb-stream-audit/src/rules.rs` | Drei feste Muster, Schwaerzung, Beleg-Hash, Ausschnitt. |
| `crates/tb-stream-audit/src/llm.rs` | Prompt, anonyme Segmentnummern, Antwortpruefung, Normalisierung. |
| `crates/tb-stream-audit/src/plan.rs` | Blocklaenge, Sendungszeit, Deckel, Warteschlange mit Wartezeiten. |
| `crates/tb-stream-audit/src/report.rs` | Bericht als Markdown und JSON, Kurztext fuer die DM. |
| `crates/tb-stream-audit/src/melden.rs` | Broker-Anfrage, Empfaenger, Kuerzung, Idempotenzschluessel. |
| `crates/tb-stream-audit/src/config.rs` | Kanaele, Ablage, Schalter, Aufbewahrung. |
| `bin/tb-stream-audit/src/main.rs` | Aufsicht, Aufnahme, Auswertung, Ablage, Aufraeumen. |

## 4. Datenfluss

1. **Aufsicht** fragt alle 60 Sekunden Helix ab. Je sendendem Kanal laeuft ein
   eigener Aufnahme-Task; der Aufnahmestand (Stream-ID, Zeitversatz) liegt bei
   der Aufsicht, damit ein abgebrochener Task nicht bei null anfaengt.
2. **Aufnahme** in Bloecken von 2 Minuten, hoechstens 24 Stunden aufgenommener
   Zeit. Kurze Bloecke bleiben, weil ein Abbruch nur den letzten Block kostet.
   Vor jedem Block entsteht `<kanal>/<stream-id>/t<sekunde>-b<nummer>/block.json`;
   Zeit und Nummer zusammen, weil zwei sofort abgebrochene Aufnahmen dieselbe
   Sekunde treffen koennen. Start-DM an den Admin. Die Aufnahme pausiert nur
   bei 12 GB Mitschnitt, nicht weil die Auswertung hinterherhaengt.
3. **Warteschlange** im Speicher. Bloecke eines noch sendenden Kanals bleiben
   liegen, bis der Lauf freigegeben ist.
4. **Auswertung** nach Sendungsende, seriell. Ein Last-Gate stellt sie
   zurueck, wenn die hoehere von CPU- und RAM-Auslastung ein volles Fenster
   (Standard 240 s) ueber der Grenze (Standard 90 Prozent) liegt, und gibt sie
   unter der
   Freigabe (80 Prozent) wieder frei; ein Deckel (1800 s) loest auch unter
   Dauerlast. Aufgenommen wird derweil weiter. Dann: Transkription,
   Regelfunde, Modellschritt in Stapeln
   zu 20 Segmenten, Zusammenfassen, Bericht schreiben (JSON zuletzt und ueber
   `rename`), eine Abschluss-DM mit ToS-Funden.
5. **Ablage**: sauberer Block — Aufnahme weg, auch wenn kein Wort fiel. Fund
   oder unvollstaendige Pruefung — Aufnahme bleibt als Beleg, markiert mit
   `ausgewertet.json`.
6. **Aufraeumtakt** stuendlich: Berichte und Aufnahmen aelter als
   `STREAM_AUDIT_RETENTION_DAYS` loeschen, offene Meldungen wieder einreihen,
   nicht zugestellte Hinweise aus `offene-hinweise/` nachreichen.

## 5. Fehlerverhalten

| Fall | Verhalten |
|------|-----------|
| Transkription scheitert | Drei Versuche mit 2 und 4 Minuten Pause; danach DM "aufgegeben", Aufnahme bleibt liegen. |
| DM scheitert | Nur die Meldung wird wiederholt: vier Anlaeufe im Abstand von 30 Minuten, danach sechsstuendlich bis zum zwoelften; danach bleibt `meldung_offen.json` liegen, das der stuendliche Aufraeumtakt wieder aufgreift. |
| Modellschritt faellt aus | Der Bericht sagt "NICHT GELAUFEN", die Aufnahme bleibt liegen. Gemeldet wird gedrosselt: beim ersten betroffenen Block und danach bei jedem zwanzigsten. |
| Block ohne gesprochenes Wort | Normalfall, keine Meldung, Aufnahme weg. Ab 20 stummen Bloecken am Stueck je Kanal: DM bei jedem Vielfachen, und die Aufnahmen bleiben liegen. |
| streamlink liefert nichts | Nach fuenf Anlaeufen je Kanal eine DM. |
| Helix antwortet nicht | Nach fuenf Anlaeufen eine DM. Laufende Aufnahmen laufen weiter; nur neue Kanaele werden nicht erkannt und beendete nicht aufgeraeumt. |
| Broker nimmt eine Ausfallmeldung nicht an | Der Hinweis landet in `offene-hinweise/` und wird stuendlich erneut versucht. |
| Schleife stirbt | Prozess endet mit Code 1, `Restart=on-failure` greift. |

## 6. Datenschutz

- Auf dem STT-Weg verlaesst Audio den Rechner nicht; ein entfernter
  STT-Endpunkt bricht den Start ab (`STREAM_AUDIT_ALLOW_REMOTE_STT`).
- Der durchgehende 1:1-Ton-Mitschnitt geht dagegen nach dem Stream bewusst per
  rclone in den eigenen Google-Drive-Ordner (`STREAM_AUDIT_DRIVE_ARCHIVE`,
  Standard an) und wird lokal erst nach belegtem Upload geloescht. Ein Kanal in
  `STREAM_AUDIT_DRIVE_EXCLUDE` bekommt gar keinen Recorder.
- Mit hoch gehen die Berichte des Laufs (`.json`, `.md`) und, wenn
  `STREAM_AUDIT_KEEP_TRANSCRIPT=1` gesetzt ist, das `.txt` mit dem
  ungeschwaerzten vollen Wortlaut. Das Drive-Archiv ist damit der Vorgang zum
  Zeitpunkt des Uploads, nicht nur der Ton. Ein Nachzuegler nach gesetzter
  `drive_archiviert.json` geht nicht mehr hoch; der haeufige Fall (letzter Block
  noch in Auswertung) ist ueber `offene_fuer_lauf` abgedeckt.
- Geht der Upload nie durch, faellt der Mitschnitt derselben Aufbewahrung zu
  wie die Berichte: `STREAM_AUDIT_RETENTION_DAYS` plus 14 Tage Gnadenfrist
  (`ARCHIV_RUECKSTAU_GNADE_TAGE`). Die Aufnahme ist der Anker, an dem ein
  offener Upload erkannt wird, und darf deshalb nicht frueher verschwinden.
- Im Drive selbst laeuft nichts ab. Der Dienst loescht dort nie etwas.
- An das Modell gehen geschwaerzte Segmenttexte mit anonymer Nummer. Die
  Schwaerzung kennt die drei Muster aus `rules.rs`; anderer Wortlaut geht mit.
  Deshalb ist `STREAM_AUDIT_ALLOW_REMOTE_LLM=1` eine bewusste Einstellung der
  Unit.
- Berichte tragen geschwaerzte Belege plus SHA-256 des Originals, Modus 0600.
  Die DM traegt weder Zitat noch Hash.
- Rohtranskripte nur mit `STREAM_AUDIT_KEEP_TRANSCRIPT=1`.

## 7. Stolperfallen

- `FFMPEG_BIN` bleibt auf `/usr/bin/ffmpeg` gepinnt. Der systemd-Benutzer-PATH
  stellt `~/.local/bin` nach vorn, und der statische Build dort segfaultet mit
  leerem stderr - dann scheitert jeder Block an der Tonspur.
- Der Ausgabeordner gehoert diesem Dienst allein: die Aufbewahrung loescht dort
  Berichte nach Namensmuster und Aufnahmen nach Ordnerform.

### Datenpfad beim Deploy

Die Unit aus `ops/systemd/` und der Ausgabeordner aus `ops/systemd/audit.conf`
gehören zusammen. Beim Aktualisieren einer bestehenden Installation den Wert
`STREAM_AUDIT_OUTPUT_DIR` in `/etc/deadlock-twitch/audit.conf` auf
`/var/lib/deadlock-twitch-audit/data/stream_coaching_audits` setzen; andere
Betreibereinstellungen erhalten. Danach Unit installieren, `daemon-reload` und
den Auditdienst neu starten. Die vorhandenen Daten bleiben an ihrem Ort.
`StateDirectory` gibt diesem Pfad Schreibrechte trotz `ProtectSystem=strict`.
Keine Daten- oder Log-Mounts auf `/opt/deadlock/twitch/current` ergänzen: deren
aufgelöstes Ziel bleibt beim nächsten Releasewechsel am alten Release hängen.
Dienstlogs gehen ins Journal. Nach dem Neustart Dateizuwachs des Mitschnitts
und mindestens einen erfolgreichen Aufnahmeblock prüfen.

### Nur das Audit ausliefern

Nach gemeinsamem Review, Merge und Push das Binary `tb-stream-audit-bin` aus
einem sauberen, isolierten Checkout des freigegebenen Commits bauen. Für einen
Audit-Fix wird der globale `current`-Link nicht umgeschaltet. Stattdessen:

1. Unter `/opt/deadlock/twitch/audit-releases/<vollständiger-git-sha>/` einen
   neuen, root-eigenen Releaseordner erstellen. Dort das gebaute Binary als
   `rust/target/release/tb-stream-audit` und den unveränderten Launcher aus dem
   selben Git-Commit als `rust/scripts/run_stream_audit_service.sh` installieren,
   jeweils Modus `0755`, Eigentümer `root:root`, ohne Gruppenschreibrechte.
   SHA256-Prüfsummen beider Dateien als `SHA256SUMS` festhalten. Einen vorhandenen
   Releaseordner niemals überschreiben.
2. `/etc/systemd/system/deadlock-twitch-stream-coaching-watch.service.d/20-audit-release.conf`
   mit diesem Inhalt installieren (den SHA in beiden Pfaden ersetzen):

   ```ini
   [Service]
   WorkingDirectory=/opt/deadlock/twitch/audit-releases/<vollständiger-git-sha>
   ExecStart=
   ExecStart=/usr/bin/bash /opt/deadlock/twitch/audit-releases/<vollständiger-git-sha>/rust/scripts/run_stream_audit_service.sh
   ```

3. `systemctl daemon-reload`, Unit prüfen und ausschließlich
   `deadlock-twitch-stream-coaching-watch` neu starten. Den tatsächlichen
   Binarypfad über `/proc/<MainPID>/exe` kontrollieren und Prüfsummen vergleichen.
   Vorhandene Mitschnitte bleiben erhalten; der neue Recorder legt einen neuen
   Teil mit aktuellem Zeitstempel an. Anschließend Dateizuwachs des neuen Teils
   und den Abschluss eines Aufnahmeblocks nachweisen.

Das Drop-in pinnt das Audit ausdrücklich auf diesen Commit. Bei späteren
Audit-Deploys wird es auf den neuen geprüften Audit-Release gesetzt; ein
allgemeiner Bot-/Dashboard-Deploy aktualisiert das gepinnte Audit nicht.
Für einen Rücksprung das Drop-in auf den vorherigen Audit-Release setzen und
den Auditdienst neu starten; die dauerhaften Daten werden dabei nicht geändert.
