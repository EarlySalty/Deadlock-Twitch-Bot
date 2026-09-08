# Onboarding nach Deadlock-Aktivität

Stand: 2026-09-08. Eigene Implementierung: `af906f3e`; Integration neuer main-Basis `d55880ab` per regulärem Merge: `bd2e2fbd`.
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

## Erneute Prüfung nach Gate- und Build-Umbau

- Die neue Gatearchitektur startet GPT-6 Astra/high; Claude Opus 5 ist nur Ersatz bei fehlendem Urteil (Exit 2), kein zweites Würfeln nach fachlichem BLOCK. Tatsächliche neue Aufrufe auf `bd2e2fbd` und dem gemeinsamen Integrationsstand `e4f43268` erhielten **ALLOW**. Der Reviewer weist auf nicht verfügbare Repositorytools hin; unabhängiger Quality-Review bestätigt zusätzlich den Fachpatch. Das alte Wochenlimit ist überholt.
- Die zuvor dokumentierten rand-Fehler auf unverändertem main wurden erneut gemessen. Build-Kompatibilitätsfix `bc691d95` ist jetzt regulär mitgemergt: OS-Zufall über die aktuelle rand-API, HMAC-Konstruktoren, explizite Hex-Fingerprints; sqlx auf 0.8.6 und cbc auf 0.1.2 begrenzt, um eine umfangsfremde SQL-/Cipher-Migration zu vermeiden. Crypto-Quellen, Manifeste und Lockdatei **sind damit bewusst Teil des finalen Diffs**. Isolierte Crypto-Tests (19) und reale Bot-/Dashboard-Binary-Checks waren beim Buildworker erfolgreich; unabhängiger Security-Review begleitet diese Änderungen.
- Die integrierten Research-Tests trafen zusätzlich eine vorbestehende Test-API-Inkompatibilität durch argon2 0.6 in `demo_login.rs`. Test-only-Anpassung `995c9fb9` wurde integriert: gleiche dekodierte Testsalze mit der aktuellen Argon2-API. Anschließend 14 Research-DB-Tests, ein Stats-ID-Persistenztest und vier Demo-Login-Unit-Tests erfolgreich; keine Tests ausgeblendet.
- Integrierter Stand `48a31d8f` erhielt erneut Gate-ALLOW. Der erste Fullrelease fand zusätzlich die gleiche entfernte SHA256-LowerHex-Formatierung im dritten Binary `tb-stream-audit`; dafür wird identisches Hex-Encoding mit bekannten SHA256-Referenzwerten abgesichert. Ein vollständiger erfolgreicher Build aller drei Binaries bleibt Voraussetzung vor Deploy; keine alten Binaries werden übernommen. Main-Merge und Live-Abnahme werden im Abschluss ergänzt.
- Browserwerkzeug nicht verfügbar; keine visuelle Live-Abnahme behauptet.

Unveränderter Live-Stand beim ursprünglichen Abschluss: Release `a8d14ebc2cc47eb81f1aadaeff33296f7ec1da4e`, beide Systemdienste aktiv, `/readyz` HTTP 200. Dieser Nachweis betrifft ausdrücklich den alten Stand und muss nach Deploy erneuert werden.

Logs der Sitzung liegen lokal unter `/tmp/tb-onboarding-*.log`.
