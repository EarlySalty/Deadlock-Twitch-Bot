# Onboarding nach Deadlock-Aktivität

Stand: 2026-09-08. Implementierung: `af906f3e`, Basis `76789c00`.
Branch: `fix/onboarding-deadlock-aktivitaet`.

## Ergebnis

- Vorschläge erfordern mehr als 50 % deutsche Twitch-Sprach-Sichtungen im gewählten Zeitraum. Die letzte Sichtung muss deutsch sein; unbekannte Sprache und widersprüchliche Sprachen am selben letzten Zeitstempel schließen den Kanal aus. Globale Forschungsdaten bleiben erhalten.
- Priorität: 60 % Deadlock-Stunden, 35 % geschätzte Deadlock-Stream-Anzahl, 5 % Zuschauer. Vergleich jeweils gegen die Partnerverteilung: `(kleiner + 0,5 × gleich) / Partnerzahl × 100`, bei leerer Vergleichsgruppe null. Oberhalb aller Partner ist das Perzentil 100; keine Ausreißer-Max-Normalisierung.
- Aktualität: sieben Tage neutral, danach Faktor `2^(-(Tage seit letzter Sichtung - 7)/14)`. Die gewichtete Summe wird mit diesem Faktor multipliziert und gerundet. Bei Gleichstand entscheiden Stunden, Streams und letzte Sichtung.
- Streams bleiben eine offen benannte Schätzung aus Kategorie-Sichtungen: Lücken über 30 Minuten beginnen einen neuen Stream. Stunden addieren nur zusammenhängende Sichtungsintervalle. Das ist keine exakte Twitch-Stream-ID-Zählung.
- Adminseite zeigt Stunden und Streams früh, Zuschauer nachrangig; Priorität und Aktualitätsabwertung sind erklärt.
- Schreibursache nebenbei geschlossen: `StatsSample` und beide Stats-Tabellen erhalten künftig die bereits vorhandene echte Helix-Twitch-User-ID. Kein Secret, kein Modellwechsel, keine Migration.

## Identitätsgrenze

Live enthalten 5.434.040 von 6.226.091 Kategorie-Sichtungen der letzten 90 Tage keine ID (rund 87 %). Der bisherige Writer verwirft sie. Bestehende Snapshot-Zuordnungen für 311 Logins sowie kombinierte aktuelle Identitätsquellen sind eindeutig, beweisen aber keine historischen Besitzverhältnisse eines wiederverwendeten Logins. Deshalb kein geratenes Backfill und kein Ausblenden dieser Historie. Die bestehende loginbasierte Readeraggregation bleibt in diesem ausdrücklich abgestimmten Umfang bestehen, der Writer ist künftig korrekt. Eine vollständige historische ID-Reparatur ist damit ausdrücklich nicht behauptet.

## Nachweise

Auf der ursprünglichen Basis `273ae126`, Implementierungsstand `f346dff9`:

- `cargo test -p tb-dashboard-api admin_research`: 14 echte Tests gegen isolierte lokale PostgreSQL-Datenbank bestanden, darunter kleiner regelmäßiger Kanal vor großem Einmalstreamer, verlassener Vielstreamer, Aktualitätsgrenzen, Streamlücken, Sprachmehrheit, unbekannte Sprache, 50/50 und widersprüchlicher letzter Sprachwert.
- `cargo test -p tb-monitoring stats_speichern_plattform_id`: ein echter DB-Test bestanden; beide Tabellen speichern ID und Sprache.
- Testdatenbank ausschließlich über temporäre normale Datei `rust/test-database-url.txt`, lokaler Peer-DSN ohne Kennwort, keine ENV-Konfiguration. Datei wird nicht committet.
- Admin-Frontend gebaut. Nach konfliktfreiem Rebase auf `76789c00` alle drei Frontends durch unabhängigen Worker mit `npm ci` und `npm run build` erneut erfolgreich gebaut.
- SQL-Reviewer: Live-EXPLAIN ANALYZE für 90 Tage alter Query 10,584 s, deterministischer Sprachfilter mit MAX FILTER 11,746 s (473 Kandidaten). Kein Array-Sortieren erforderlich. Performance ist bei Millionen Snapshots weiterhin nicht interaktiv schnell.
- `git diff --check` erfolgreich. Unabhängiger Quality-Review ohne fachlichen Blocker.

## Externe Blockaden und offene Verifikation

- Tatsächliche Selbstprüfung: `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-onboarding-aktivitaet --base main --head fix/onboarding-deadlock-aktivitaet`. Abbruch: Claude-Wochenlimit, Reset laut Ausgabe 22 Uhr Europe/Berlin. Kein Codeurteil und keine Gatefreigabe.
- Nach Rebase scheitert `cargo check -p tb-dashboard-api -p tb-monitoring` vor den Zielcrates in unverändertem `tb-crypto`: `rand::RngCore` und `rand::rngs::OsRng` fehlen nach dem zwischenzeitlichen main-Bump auf rand 0.10.2. Fünf Fehler in `field.rs` und `token.rs`. Crypto-Quellen und Cargo-Manifeste/Lock sind gegen origin/main unverändert. Keine fremden Major-Upgrades in diesem Task repariert.
- Noch kein Merge, Releasebuild, Deploy oder Neustart. Neue Live-API-Ausgabe und neue Stats-Schreibwerte müssen nach einem freigegebenen erfolgreichen Deploy geprüft werden.
- Browserwerkzeug nicht verfügbar; keine visuelle Live-Abnahme behauptet.

Unveränderter Live-Stand zum Abschluss: Release `a8d14ebc2cc47eb81f1aadaeff33296f7ec1da4e`, beide Systemdienste aktiv, `/readyz` HTTP 200 mit Datenbank und interner API bereit. Dieser Nachweis betrifft ausdrücklich den alten Stand.

Logs der Sitzung liegen lokal unter `/tmp/tb-onboarding-*.log`.
