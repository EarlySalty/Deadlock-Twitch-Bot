# Plan: Analyse-Dashboard Scores, Tempo und Optik

status: aktiv
datum: 2026-09-06
contract: CONTRACT.md (Ziel, REQ, INV dort)
evidence: EVIDENCE.md, RESEARCH.md

Zwei Stränge in getrennten Worktrees und Branches, beide merge-fähig nach main:

- Strang A (Backend, Branch `fix/analyse-scores-tempo-optik`, Worktree `~/.worktrees/tb-analyse-scores`): M1 bis M3.
- Strang B (Frontend, Branch `fix/analyse-optik-frontend`, Worktree `~/.worktrees/tb-analyse-optik`): M4 bis M9.

Status je Milestone unten eintragen (offen, rot belegt, grün, verifiziert).

## Strang A: Backend

### M1: Roter Test für die alten Formeln (REQ-03)

- Änderungen: neuer Test in `rust/crates/tb-dashboard-api/src/handlers/overview.rs` (`retention_score_saettigt_nicht_bei_88_prozent`), der `calculate_health_scores` mit 88 % 10m-Retention und 25/15 Raids bei 44 Sessions aufruft und Retention 58, Network 45 erwartet.
- Erwartet: Test rot, Fehlermeldung "left: 100, right: 58" (Testname und Meldung hier eintragen).
- Validierung: `cargo test -p tb-dashboard-api retention_score_saettigt_nicht` (Toolchain 1.97.1 im PATH, `SQLX_OFFLINE=1`).
- Stop-Regel: ist der Test grün, ist er falsch geschrieben; nicht weiter.
- Status: offen

### M2: Bindung und Raids je Stream als Score (REQ-01, REQ-02, REQ-03, INV-01, INV-02)

- Änderungen: `rust/crates/tb-analytics/src/overview.rs` bekommt in `overview_metrics` die Spalte `avg_bindung` (`AVG(LEAST(1.0, s.avg_viewers / NULLIF(s.peak_viewers, 0)))` über Sessions mit `avg_viewers >= 3 AND peak_viewers > 0`) und das Feld in `OverviewMetricsRow`; `calculate_health_scores` in `overview.rs` bekommt `bindung: Option<f64>` statt `retention_10m_pct` für den Retention-Score (Sample < 3 bleibt 50) und rechnet Network nach REQ-02 aus `net.sent`, `net.received`, `session_count`. Bestehende Tests `returns_metrics_for_known_streamer` und `health_scores_formel_exakt` auf die neuen Erwartungen umschreiben (Werte aus REQ-03), Testdaten in `returns_metrics_for_known_streamer` so wählen, dass Bindung und Raids je Stream prüfbar sind.
- Erwartet: M1-Test grün, alle Overview-Tests grün, `summary.retention10m` unverändert.
- Validierung: `rust/scripts/test_db.sh` starten, dann `cargo test -p tb-dashboard-api -p tb-analytics`; Formelprobe mit den Live-Zahlen aus EVIDENCE.md (0,585 ergibt 58; 25/15/44 ergibt 45).
- Stop-Regel: ändert sich ein anderer Score als Retention oder Network, zurück.
- Status: offen

### M3: Audience-Abfragen Hygiene (REQ-07 Teil, INV-02)

- Änderungen: `rust/crates/tb-dashboard-api/src/handlers/audience_demographics.rs:322` bekommt `AND sv.ts_utc >= $1`; für `:473` prüfen, ob `twitch_chat_messages.streamer_login` beim Schreiben kleingeschrieben abgelegt wird (`SELECT COUNT(*) FROM twitch_chat_messages WHERE streamer_login <> LOWER(streamer_login)`; Zugang lesend über den Loader aus EVIDENCE.md-Muster): ist der Zähler 0, `LOWER()` entfernen, sonst Migration `rust/migrations/<zeit>_chat_messages_streamer_lower_ts.sql` mit `CREATE INDEX CONCURRENTLY idx_twitch_chat_messages_streamer_lower_ts ON twitch_chat_messages (lower(streamer_login), message_ts)` (Timescale: ohne CONCURRENTLY, falls die Hypertable es verlangt; in PLAN.md festhalten). `.sqlx`-Offline-Dateien nachziehen (`cargo sqlx prepare --workspace` gegen die Test-DB).
- Erwartet: EXPLAIN von Q5 zeigt Chunk-Pruning (Chunks des Zeitfensters statt 240).
- Validierung: `cargo test -p tb-dashboard-api audience_demographics`, `cargo build -p tb-dashboard`.
- Stop-Regel: ändert sich die Antwortform, zurück.
- Status: offen

### M3b: Broker-Cache und Timeout (REQ-07 Kern), erst nach User-Freigabe

- Voraussetzung: Freigabe-Datei `~/.claude/.contract-approvals/2026-09-06-analyse-scores-tempo-optik` mit den Pfaden `rust/crates/tb-dashboard-api/src/auth/level.rs`, `rust/crates/tb-dashboard-api/src/auth/discord_admin_login.rs`, `rust/crates/tb-dashboard-api/src/auth/session.rs`.
- Änderungen: Ergebnis von `validate_session` in den bestehenden `TimedCache` (session.rs:433-451), Schlüssel `session_id`, TTL 30 s; `BROKER_TIMEOUT` 20 s auf 2 s; bei Fehler Rückfall auf `state.load_admin_session` (level.rs:377) statt `continue`. Test: Broker-Stub, der nicht antwortet, Request endet unter 3 s mit lokaler Session.
- Validierung: `cargo test -p tb-dashboard-api auth`; nach Deploy Journal ohne 20-s-Latenzen bei erreichbarem und bei gestopptem Broker.
- Stop-Regel: ohne Freigabe-Datei nicht anfassen.
- Status: offen (wartet auf Freigabe)

## Strang B: Frontend

### M4: Roter Test Tage-Feld (REQ-06)

- Änderungen: neuer Test `bot/dashboard_v2/tests/headerTageEnter.test.tsx` (oder `.test.ts` nach Muster von `dashboardShell.test.ts`), der den Header rendert, "14" tippt, Enter sendet und `onDaysChange(14)` sowie Fokusverlust erwartet. Vorher am Live-Bundle prüfen, ob der Enter-Handler enthalten ist (`curl` der Asset-URLs aus `/analyse` mit Session-Cookie ist nicht möglich; stattdessen Datum des Live-Release gegen 62f041e0 und ff1a951a vergleichen: `readlink /opt/deadlock/twitch/current`, `git -C ~/repos/Deadlock-Twitch-Bot merge-base --is-ancestor ff1a951a <sha>`).
- Erwartet: Test rot mit Meldung, oder Nachweis "Live-Bundle ist älter als der Fix" in dieser Datei. In beiden Fällen bleibt der Test.
- Validierung: `npm test -- headerTageEnter` in `bot/dashboard_v2`.
- Status: verifiziert (grün, kein Code-Fehler auf main)
- Befund: Der Enter-Fehler ist auf main NICHT reproduzierbar. Live-Release ist `01454d056f402b936c4a49ac2a8c4b98321848f6` und enthält laut Orchestrator die Commits 62f041e0, ff1a951a und 48e9979e; der Live-Check per readlink/merge-base entfällt darum. Empirische Prüfung: Vite-Dev-Build dieses Worktrees, gesteuert per Chrome DevTools Protocol mit echten Tastatur-Events (`Input.dispatchKeyEvent` Enter), sechs Szenarien. Ergebnis in allen Fällen korrekt: "14" -> days=14, "90" -> 90, "200" -> 200, "3" -> clamp 7, leeres Feld -> behält 7, "45" per Blur -> 45; Header-Text, URL-Parameter `days` und Fokusverlust (`document.activeElement` = BODY) folgen. Ursache des ursprünglichen Fehlers waren die drei genannten Commits (Zahlenfeld, leeres Feld, Blur-Guard `if (naechster !== days)`); sie sind bereits in main und live. Kein globaler KeyDown-Handler schluckt Enter (nur Escape-Listener in AnalyticsTour/TrialExpiryModal/Header), der URL-Parse-Effekt in App.tsx läuft nur einmal (deps []). Der Test `tests/headerTageEnter.test.ts` bleibt als grüne Absicherung der Übernahme-Logik und der Enter/Blur-Verdrahtung (Muster wie `dashboardShell.test.ts`, da `npm test` ohne DOM-Bibliothek läuft und Header.tsx `@/`-Importe hat). Neue Testdatei in `package.json` (test-Skript) registriert; package.json steht nicht im Contract-Scope und braucht bei Gate-Block eine Freigabe-Datei.

### M5: Score-Karten und Radar (REQ-04, REQ-05)

- Änderungen: `ScoreGauge.tsx` bekommt `hint?: string` unter dem Label; `Overview.tsx` übergibt die vier Texte aus REQ-04; `RetentionRadar.tsx` bekommt `label`-Renderer an den Ecken (Recharts `Radar` mit `dot` und `label`, Zahl in `--color-text-primary`, 11 px, versetzt nach außen), `fillOpacity` 0.35, Achsenticks mit größerem Abstand (`tickLine={false}`, Margin anpassen).
- Validierung: `npm run build`, Sichtprüfung per Vite-Preview und Headless-Chrome (Memory `dashboard-sichtpruefung-headless-chrome`), Screenshot nach `.tasks/2026-09-06-analyse-scores-tempo-optik/screens/`.
- Status: offen

### M6: Tage-Feld Fix (REQ-06)

- Änderungen: nach Befund aus M4 in `Header.tsx` (Reihenfolge `setTageInput` vor `blur()`, `preventDefault` auf Enter, kein doppelter Aufruf) oder nur Deploy.
- Validierung: M4-Test grün.
- Status: entfällt (kein Code-Fix nötig, siehe M4-Befund; Header.tsx bleibt unverändert)

### M7: Farben Funnel, Demographics, Segmente, Topic-Donut (REQ-08, REQ-09, REQ-10, INV-04)

- Änderungen: `index.css` bekommt `--color-chart-1` bis `--color-chart-5` (Bronze bis Messing, z. B. #7A5A2E, #9C7A3C, #C5A059, #DDBD7A, #F1D9A6; Luminanzabstand prüfen) plus Tailwind-Klassen, falls das Projekt Farben über `@theme` registriert; `brandPalette.test.ts` nur um diese Tokens erweitern, wenn er sonst blockt. `FollowerFunnel.tsx`: Conversion-Karte auf `bg-black/25 border border-primary/40 shadow-inner`, Stufenbalken `from-primary to-accent`, Skala `/70`. `AudienceDemographics.tsx`, `ViewerProfiles.tsx`: `VIEWER_COLORS` auf die Rampe. `chatAnalyticsDeepSections.tsx`: `TOPIC_COLORS` auf Rampe plus primary, accent, warning; Fallback `var(--color-chart-3)`; Donut-Box `h-[160px] w-[160px] shrink-0`, innen 45, außen 78.
- Validierung: `npm test` (brandPalette), Sichtprüfung Audience-Tab und Chat-Aktivität.
- Status: offen

### M8: Raid-Details sortieren und filtern (REQ-11)

- Änderungen: `RaidRetention.tsx` bekommt `useState` für Sortierspalte und Richtung (Default `viewersSent` absteigend), Suchfeld über der Tabelle, Zeile "N von M Raids", klickbare `th` mit Pfeil-Icon (lucide `ChevronUp`/`ChevronDown`), `useMemo` für die gefilterte, sortierte Liste.
- Validierung: Unit-Test für die Sortier- und Filterfunktion (reine Funktion aus der Komponente exportieren), Sichtprüfung.
- Status: offen

### M9: Wochentags-Balken (REQ-12)

- Änderungen: `Schedule.tsx:353-403`: Skala min bis max (`viewerPct = 15 + 85 * (avg - min) / (max - min)`, bei `max == min` 100), Zahl mit einer Nachkommastelle (`toLocaleString('de-DE', {maximumFractionDigits: 1})`), Abweichung vom Wochenschnitt als Chip unter der Zahl.
- Validierung: Unit-Test für die Skalenfunktion, Sichtprüfung Planning-Tab.
- Status: offen

## Abschluss (beide Stränge)

- `gate_hook.py --review` gegen die eigene Arbeit vor der Fertigmeldung, dann frischer Reviewer (Diff plus Contract), Fixes als neue Commits.
- `git -C ~/repos/_ttb-main-deploy pull --ff-only origin main` vor dem Merge-Gate, Merge nach main, Release nach Memory `twitch-release-deploy-weg` (Migration aus M3 als `postgres` anwenden, Rechte an `twitchbot` und `twitchdash`), Restart beider Units, Live-Prüfung auf `/analyse?days=30&tab=overview&streamer=earlysalty` (Retention rund 58, Network rund 45) und Journal-Latenzen der fünf Audience-Endpunkte bei `days=365`.
- Branches und Worktrees löschen.
