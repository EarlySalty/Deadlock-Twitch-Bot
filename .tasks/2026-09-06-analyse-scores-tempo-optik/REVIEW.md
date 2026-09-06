# Review: Analyse-Dashboard Scores, Tempo und Optik

status: aktiv
datum: 2026-09-06
contract: CONTRACT.md
geprüfter Stand: Branch `fix/analyse-scores-tempo-optik` nach Merge von `fix/analyse-optik-frontend` und origin/main (d4991ba0)

Frischer Reviewer (Opus 4.8, read-only, beide Stränge zusammen gegen Contract und Diff).

## Ergebnis: FREIGABE, 0 BLOCKING, 4 NITs

Verifiziert:
- Kontrollzahlen: Bindung 0,585 ergibt 58, Raids 25/15 bei 44 Sessions ergibt 45 (`retention_score_saettigt_nicht_bei_88_prozent` grün).
- `returns_metrics_for_known_streamer`: retention 60 aus Bindung, `summary.retention10m` bleibt 90, network 67, `total`-Gewichte unverändert (INV-01).
- `avg_bindung`-SQL: gleiches Fenster und `until`, `LEAST(1.0, ...)`, `NULLIF` gegen Division durch 0, NULL bei 0 Samples.
- Rampe REQ-09: Luminanzabstände 12,6 / 16,4 / 15,4 / 17,9 pp, Tokens in `ALLOWED_HEX` (INV-04).
- M3: Schreibpfad `rust/crates/tb-chat/src/chatter_tracking.rs:101` legt `streamer_login` kleingeschrieben ab, `$2` ebenfalls lowercased; `LOWER()`-Entfernung sicher (INV-08).
- Tests: Frontend 274/274, `overview` 13/13, `audience_demographics` 4/4 gegen Test-DB. Keine gelöschten Tests (INV-03), API-Form kompatibel (INV-02), keine neuen Kommentare, keine Em-Dashes.
- Raid-Sortierung und Filter rechnen per `useMemo` neu, leere Suche zeigt alle.

## NITs (nicht blockierend)

1. `rust/crates/tb-dashboard-api/src/handlers/overview.rs:574`, `rust/crates/tb-analytics/src/overview.rs:91`: Retention-Score gatet über `retention_sample_count` (zusätzlich `retention_10m IS NOT NULL`), REQ-01 nennt nur `avg_viewers >= 3 AND peak_viewers > 0`. Deckungsgleich für alle Sessions aus `retention_at`; divergiert nur bei Altdaten mit NULL-`retention_10m` (Score bliebe 50). Folgeauftrag: eigener `bindung_sample_count`.
2. `bot/dashboard_v2/package.json`: außerhalb des Contract-Scopes, Test-Registrierung nötig; Freigabe-Datei des Users.
3. `bot/dashboard_v2/src/components/charts/RetentionRadar.tsx:148`: Eck-Zahl bei Score 100 nah an der Achsenbeschriftung; live bei maximalem Score prüfen.
4. `bot/dashboard_v2/src/pages/Schedule.tsx:369-386`: Farbschwelle nutzt den ungerundeten Wert (4,6 zeigt "+5 %" neutral). Kosmetisch.

## Offen vor Live

- Freigabe-Datei für `package.json` (und Auth-Pfade für M3b).
- Live-Prüfung `/analyse?days=30&streamer=earlysalty` (Retention rund 58, Network rund 45) und Journal-Latenzen der Audience-Endpunkte bei `days=365`.
