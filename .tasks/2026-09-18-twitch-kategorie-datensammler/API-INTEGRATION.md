# Geprüfte Admin-API liegt auf GitHub

18.09.2026. Der Folge-Arbeitsgang nach „weiter“ hat die fehlende Admin-API getrennt fertiggestellt, ohne parallele Collector-/Frontend-Dateien zu überschreiben.

Commit: `ed6c78d96fa07e5662318d933997bad7d11f2878`
Branch: `feat/category-admin-api-20260918`
Worktree: `/home/nathanael/repos/tb-category-admin-api-20260918`
Remote-Referenz per `git ls-remote` geprüft.

Integration: diesen Commit nach Abschluss des eigenen WIP in den Collector-Integrationsstand übernehmen. Enthält ausschließlich `tb-dashboard-api` (neuer Handler, SQL, 13 Tests, Modul-/Router-Einträge) und `API-ABNAHME.md`, keine Migration und keine Collector-/Frontend-Änderung. Die Route lautet wie vereinbart `/twitch/api/v2/admin/category-collector?days=7`. Keine Doppelimplementierung nötig.

Abnahme: 13 API-Tests grün, davon zehn mit isoliertem PostgreSQL16, echter Router-/Cache-Auth-Test; cargo check und clippy Exit 0. Bestehende Warnungen in uplink_config.rs, keine neuen Sammler-Warnungen. Echte TDD-Rotprobe ist im Bericht belegt. Zusätzlich sieben anonyme Transporttests unabhängig grün. Keine Produktions-Datenänderung.

## Vor vollständigem Deployment weiter offen

- Collector-Konfiguration, Migration und Betriebsunit fertigstellen. Bei der letzten Live-Prüfung gab es keine category_*-Tabellen und keine Collector-Unit; Hauptbot/Dashboard laufen unverändert auf Release44cc2aa9.
- Der Snapshot-Store braucht UNIQUE/Index auf `(poll_id, stream_id)` für Idempotenz und die API-Zeitraumabfrage.
- Das API liest `retention_days` nullable aus der DB. Keine Löschung durch das API; null muss im Frontend korrekt angezeigt werden. Die parallele Entscheidung zur Aufbewahrung muss im Writer/Schema eindeutig abgebildet sein, nicht nur in einer Doku.
- Die 15 bestehenden Frontend-Tests sind grün, decken aber einen tatsächlichen Fehler nicht ab: drei stündliche Trendpunkte0,0,100 werden durch `buildTrendreihe` zu einem Tagespunkt50 statt drei Stundenpunkten. Die Zusatzprobe ist real ausgeführt. H1/H2 aus FRONTEND-REVIEW bleiben relevant: API-Buckets übernehmen, Intraday-Lücken erhalten, nullable Schnitte zulassen. Der API liefert stündliche Buckets und keine Tages-Mittelwerte.
- Die derzeitige UI erklärt jeden503 als fehlende Tabellen. API503 kann auch Timeout/DB-Störung sein; generische Fehlermeldung bzw. gelieferten Fehlercode auswerten.
- Keine vollständige Live-Abnahme oder24–48h-Messung durch das API-Paket behaupten.

Push-Gate ließ den Commit zu. gitleaks/RustSec/deny/trivy ohne blockierenden Befund; OSV meldete bestehende Hinweise im unveränderten Legacy-Python-Manifest `ops/highlight-detector/requirements.txt`. Kein pauschales „gesamtes Repository sicher“ aus diesem Push ableiten.
