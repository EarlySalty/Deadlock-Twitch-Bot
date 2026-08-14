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
| **Daten** | `STREAM_AUDIT_OUTPUT_DIR` — Berichte als `.md`/`.json` je Kanal, Aufnahmen unter `aufnahmen/<kanal>/<stream-id>/t<sekunde>-b<nummer>/<capture>/`. |
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
2. **Aufnahme** in Bloecken von 2 Minuten, hoechstens 6 Stunden Sendungszeit.
   Kurze Bloecke, weil der lokale STT-Dienst geteilt wird und eine Anfrage nach
   der anderen abarbeitet.
   Vor jedem Block entsteht `<kanal>/<stream-id>/t<sekunde>-b<nummer>/block.json`;
   Zeit und Nummer zusammen, weil zwei sofort abgebrochene Aufnahmen dieselbe
   Sekunde treffen koennen.
3. **Warteschlange** im Speicher, mit Wartezeiten je Block. Ab 180 wartenden
   Bloecken pausiert die Aufnahme, und der Admin bekommt eine DM.
4. **Auswertung** seriell: Transkription (30 Minuten Zeitgrenze), Regelfunde,
   Modellschritt in Stapeln zu 20 Segmenten, Zusammenfassen, Bericht schreiben
   (JSON zuletzt und ueber `rename`), DM senden.
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

- Audio verlaesst den Rechner nicht; ein entfernter STT-Endpunkt bricht den
  Start ab (`STREAM_AUDIT_ALLOW_REMOTE_STT`).
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

