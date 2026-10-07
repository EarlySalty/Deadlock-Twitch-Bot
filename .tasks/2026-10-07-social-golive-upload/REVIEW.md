# Lokaler Merge-Gate für Auftrag D

## Stand

Gate-Runde 1 am 7. Oktober 2026: **BLOCK**, Modell `gpt-6.1-sol`. Geprüfter Kandidat: f2b64b39706417ca63071e5fcf8cc857ab9dff01. Basis: frisches origin/main e0b0dbaf662d7680c4ceaa210bf15f1443693cd8. Kein Push auf main, kein Merge, keine Produktionsmigration und kein Deploy.

## Mängelliste aus Runde 1

1. **BLOCKING, bestätigt:** `rust/crates/tb-social-media/src/upload_worker.rs:850`, zusätzlich `rust/crates/tb-social-media/src/clip_queue.rs:285,295`. Wartende Aufträge verbrauchen die begrenzte Auswahl gemeinsam mit ausführbaren Aufträgen. Bei 100 älteren Aufträgen ohne Verbindung, 20 Zeilen pro Scan und 60 Sekunden Intervall werden die ältesten nach fünf Scans erneut fällig, bevor ein neuerer verbundener Auftrag erreicht wird. Die Wiederaufnahme muss fair auswählen oder wartende Aufträge separat prüfen. Ein größeres Limit ist keine Behebung.
2. **NIT, bestätigt:** `bot/dashboard_v2/src/pages/SocialMedia.tsx:2409`; beide Meldungen aus `capabilities.rs:70,85` werden direkt auf Deutsch ausgegeben, auch im englischen Dashboard. Die Übersetzung braucht einen strukturierten gemeinsamen Grund und Plattformnamen, nicht Tests am Wortlaut.
3. **NIT, Nachweise vorhanden, im Gate nicht eingebunden:** Der Gate konnte die Gestaltung nicht abnehmen. Tatsächliche Viewportaufnahmen wurden aus dem gebauten Produktionsbundle erzeugt und in dieser Session betrachtet: `/home/nathanael/.claude/sichtpruefung/social-golive-upload/upload-wait-1440.png`, `upload-wait-390.png`, `upload-connections-1440.png`, `upload-connections-390.png`. Der Browserlauf bestätigt den Recovery-Knopf und keinen Dokumentüberlauf. Dem nächsten Gate diese Sichtnachweise ausdrücklich zugänglich machen.

## Originalurteil

```text
[gpt-6.1-sol] BLOCK: Connection-wait polling can starve usable uploads.

1. rust/crates/tb-social-media/src/upload_worker.rs:850 | BLOCKING: Waiting jobs consume the bounded upload scan. Complete sites: this scan limit and `rust/crates/tb-social-media/src/clip_queue.rs:285,295` (five-minute eligibility and oldest-first ordering). Using the supplied 60-second interval default, 100 older unavailable jobs fill five successive 20-row scans; the oldest become eligible again before a newer connected job is reached. Recovery needs fair selection or a separate waiting-job scan.
2. bot/dashboard_v2/src/pages/SocialMedia.tsx:2409 | NIT: Connection-wait messages bypass translation. Both missing and incomplete connection messages from `capabilities.rs:70,85` render directly in German in the English dashboard.
3. bot/dashboard_v2/src/pages/SocialMedia.tsx:1985 | NIT: Appearance is unverified. No registered visual-review project or supplied screenshots establish the changed layouts’ appearance; the screenshot assertions in the diff do not settle that check.
```

## Übergabe

Ein frischer Fixer aus der Pyramide ist nötig. Der Blattauftrag verbietet zusätzliche Threads; diese Session startet deshalb keinen Fixer und repariert die Gate-Funde nicht selbst. Auftraggeber entscheidet die Zuweisung. Branch und Worktree bleiben für die Übernahme erhalten. Die nächste Gate-Runde verwendet dasselbe Modell wie Runde 1. Kein Modellwechsel nach inhaltlichem BLOCK.

Release-Build des abgelehnten Kandidaten gestoppt, nur Teilcache vorhanden. Frontends aus dem sauberen Kandidaten wurden gebaut. Keine alten oder unvollständigen Binaries ausliefern. Echte Streamer-Verbindungen blieben unangetastet.
