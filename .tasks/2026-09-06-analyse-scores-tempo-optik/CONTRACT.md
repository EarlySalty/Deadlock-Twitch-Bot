# Contract: Analyse-Dashboard Scores, Tempo und Optik

status: aktiv
datum: 2026-09-06
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu. Wer ein REQ oder INV ändern will, schreibt ein
Amendment mit Begründung; Produkt-, API- oder Datenänderungen entscheidet der User.

## Ziel

Auf `/analyse` zeigen Retention- und Network-Score echte, unterscheidbare Werte statt dauerhaft 100, das Radar liest sich als Chart, das Tage-Feld übernimmt Enter, der Audience-Überblick lädt auch bei einem Jahr in wenigen Sekunden, und Funnel, Demographics, Topic-Donut, Raid-Details und Wochentags-Balken passen optisch und funktional in den Gold-Look.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: Der Retention-Score ist die mittlere Bindung der Sessions im Zeitraum (Ø Zuschauer geteilt durch Peak je Session, nur Sessions mit `avg_viewers >= 3` und `peak_viewers > 0`, gedeckelt auf 1,0), mal 100, abgerundet auf ganze Zahl. Bei weniger als 3 solcher Sessions bleibt der Wert 50 wie bisher. Für earlysalty über 30 Tage ergibt das rund 58 statt 100; `summary.retention10m` und die KPI-Karte "Retention (10m)" bleiben unverändert.
- REQ-02: Der Network-Score rechnet Raids je Stream im Zeitraum: `min(50, round(sent / sessions * 50)) + min(50, round(received / sessions * 50))`, mit `sessions` = beendete Sessions im Zeitraum (`session_count`), 0 Sessions ergibt 0. Für earlysalty über 30 Tage (25 gesendet, 15 erhalten, 44 Sessions) ergibt das 45 statt 100.
- REQ-03: Die beiden Rust-Tests, die die alten Formeln pinnen (`returns_metrics_for_known_streamer`, `health_scores_formel_exakt`), werden auf die neuen Formeln umgeschrieben und decken die Grenzfälle ab: Retention mit weniger als 3 Samples = 50, Bindung 0,585 = 58, Network 0 Sessions = 0, Network-Deckel bei mehr Raids als Streams = 100. Ein neuer Test belegt vor dem Fix, dass 88 % 10m-Retention bisher 100 ergab (roter Lauf mit Testname und Fehlermeldung in PLAN.md festhalten).
- REQ-04: Jede der vier Score-Karten (Growth, Revenue, Network, Retention) trägt unter dem Label eine einzeilige Erklärung in Nutzersprache, woraus der Wert entsteht (z. B. "Ø Zuschauer im Verhältnis zum Peak", "Raids je Stream, gesendet und erhalten", "Neue Follower je Stunde", "Subs, Bits und Hype Trains je Stream").
- REQ-05: Das Radar "Performance Mix" zeigt an jeder der sechs Ecken den Score als Zahl, die Achsenbeschriftung bleibt lesbar (keine Überlappung mit der Zahl), das Polygon ist auch bei zwei Null-Werten als Fläche erkennbar (Füllung mindestens 0,35 Deckkraft, Eckpunkte als Marker). Die Fußzeile bleibt "Scores von 0 bis 100 je Bereich".
- REQ-06: Im Tage-Feld des Headers übernimmt Enter die eingegebene Zahl: Zeitraum wechselt, URL-Parameter `days` und Kopfzeile folgen, das Feld verliert den Fokus. Die Ursache wird vor dem Fix mit einem roten Test belegt (Header-Test mit Tastatur-Event, roter Lauf in PLAN.md). Hat das Live-Bundle den Fehler nicht (nur veraltet), wird das in PLAN.md festgehalten und der Test bleibt als Absicherung.
- REQ-07: Der Audience-Überblick (`watch-time-distribution`, `follower-funnel`, `audience-demographics`, `lurker-analysis`, `viewer-profiles`) antwortet für earlysalty bei `days=365` je Endpunkt in unter 3 s, gemessen am Live-Dienst nach dem Deploy (Log `latency_ms`). Der Fix behebt die Ursache der teuren Abfragen (Index, Abfrage-Umbau oder vorab aggregierte Tabelle mit Schreibpfad und einmaligem Backfill); ein reiner Ergebnis-Cache im Prozess ist kein Fix. Betrifft die Messung Endpunkte außerhalb der fünf (etwa `coaching`), werden diese im Plan benannt und mit behandelt, wenn der Audience-Tab sie lädt.
- REQ-08: Follower-Funnel: die Conversion-Karte hat keinen grünen Farbschimmer mehr (kein `success`-Anteil im Hintergrund), sondern versenkten dunklen Grund mit Goldkante wie die übrigen Innenkacheln; die drei Stufenbalken laufen als Gold-Verlauf (`primary` nach `accent`) mit voller Deckkraft auf dunklem Track, die Skala unter der Conversion bleibt farbig (danger, warning, success), aber mit mindestens 0,7 Deckkraft. Icon-Kachel und Balken einer Stufe haben dieselbe Farbe.
- REQ-09: Audience Demographics und Zuschauer-Segmente: die Donut-Segmente nutzen eine fünfstufige Gold-Bronze-Rampe aus Tokens (neue Tokens `--color-chart-1` bis `--color-chart-5` in `index.css`, von tiefem Bronze bis hellem Messing, ohne Grün, Grau oder Orange), Legende und Prozentwerte passen dazu; Segmente heben sich sichtbar voneinander ab (benachbarte Stufen mindestens 12 % Luminanzabstand).
- REQ-10: Topic-Verteilung (Chat-Aktivität): der Donut hat einen festen Durchmesser von 160 px, wird im Flex nicht gestaucht (`shrink-0`), Ring proportional (innen 45, außen 78); die 15 Themenfarben kommen aus der Rampe nach REQ-09 plus `primary`, `accent`, `warning`, ohne Cyan, Rot oder Neongrün; Fallback-Farbe ist ein Token statt `#B5A488`.
- REQ-11: Raid-Details: jede Spalte ist per Klick auf den Kopf sortierbar (auf- und absteigend, aktive Spalte und Richtung sichtbar markiert, Standard: Gesendet absteigend); über der Tabelle steht ein Suchfeld, das nach Ziel-Streamer filtert (Teilstring, ohne Groß-/Kleinschreibung), und eine Zeile "N von M Raids". Sortierung und Filter laufen im Frontend auf den geladenen Daten.
- REQ-12: Wochentags-Analyse: die Balkenhöhe bildet Unterschiede sichtbar ab: Skala von min bis max der Ø-Zuschauer über die Tage (Balken des Minimums 15 % Höhe, Maximum 100 %); die Zahl unter dem Balken zeigt eine Nachkommastelle (z. B. 3,4); jede Tageskarte zeigt zusätzlich die Abweichung vom Wochenschnitt in Prozent mit Vorzeichen und Farbe (positiv `success`, negativ `danger`, Betrag unter 5 % neutral).
- REQ-13: `cargo test -p tb-dashboard-api -p tb-analytics` (gegen die Docker-Test-DB aus `rust/scripts/test_db.sh`) sowie `npm run build`, `npm run lint`, `npm test` in `bot/dashboard_v2` sind grün; `brandPalette.test.ts` und `scoreColors.test.ts` bleiben unverändert grün.

## Invarianten (darf sich nicht ändern)

- INV-01: `calculate_health_scores` in `overview.rs` bleibt die einzige Quelle der sechs Scores; Reach, Engagement, Growth, Monetization und die Gewichtung von `total` bleiben unverändert.
- INV-02: Antwortform von `GET /twitch/api/v2/overview` (Feldnamen, Typen, Wertebereich 0..100) und aller Audience-Endpunkte bleibt kompatibel; keine neuen Pflichtparameter.
- INV-03: Bestehende Tests werden nicht gelöscht oder abgeschwächt; Formel-Tests werden umgeschrieben, nicht entfernt.
- INV-04: Farbwerte in Komponenten kommen aus Tokens (`var(--color-*)`, Tailwind-Klassen); neue Hex-Werte nur in `src/index.css` als Token und in der `ALLOWED_HEX`-Liste von `brandPalette.test.ts`, wenn der Test sie sonst blockt.
- INV-05: Keine Code-Kommentare, echte Umlaute in nutzersichtbaren Texten, keine Em-Dashes, keine neuen npm- oder Cargo-Abhängigkeiten.
- INV-06: Neue Migrationen liegen unter `rust/migrations/` und werden nicht vom Bot selbst angewendet (`TB_DB_MIGRATE=0`); der Deploy-Schritt wendet sie als `postgres` an und vergibt Rechte an `twitchbot` und `twitchdash` (AGENTS.md). Keine Migration ändert bestehende Spalten oder löscht Daten.
- INV-07: Verwaltungs-, Uplink- und Social-Media-Dashboard bleiben optisch unverändert; die Heatmaps behalten ihre Cyan-Skala.
- INV-08: Identitäten und Filter im Backend laufen weiter über die bestehenden Schlüssel der jeweiligen Abfragen; kein neuer Namens-Nachschlag im Handler.

## Nicht-Ziele

- Kategorie-Vergleichsserie im Radar.
- Neuberechnung von Reach, Engagement, Growth, Monetization.
- Die Datenlücke in `twitch_raid_retention` (5m/15m/30m stehen bei jüngeren Raids auf 0): eigener Auftrag.
- Änderungen an KPI-Karten, Insights, Session-Tabelle, Kalender- und Stunden-Heatmap.
- Serverseitige Sortierung oder Paginierung der Raid-Details.

## Erlaubter Änderungsbereich

- rust/crates/tb-dashboard-api/src/handlers/overview.rs
- rust/crates/tb-analytics/src/overview.rs
- rust/crates/tb-analytics/src/coaching.rs
- rust/crates/tb-analytics/src/audience.rs
- rust/crates/tb-analytics/src/viewer_profiles.rs
- rust/crates/tb-analytics/src/lurker.rs
- rust/crates/tb-analytics/src/follower_funnel.rs
- rust/crates/tb-analytics/src/watch_time.rs
- rust/crates/tb-dashboard-api/src/handlers/audience.rs
- rust/crates/tb-dashboard-api/src/handlers/audience_demographics.rs
- rust/crates/tb-dashboard-api/src/handlers/viewer_profiles.rs
- rust/crates/tb-dashboard-api/src/handlers/lurker.rs
- rust/crates/tb-dashboard-api/src/handlers/follower_funnel.rs
- rust/crates/tb-dashboard-api/src/handlers/watch_time.rs
- rust/bin/tb-bot/src/partner_recruit.rs
- rust/crates/tb-internal-api/src/handlers/market_share.rs
- rust/migrations/
- rust/.sqlx/
- rust/crates/tb-db/tests/fresh_schema_snapshot.txt
- bot/dashboard_v2/src/index.css
- bot/dashboard_v2/src/components/charts/RetentionRadar.tsx
- bot/dashboard_v2/src/components/cards/ScoreGauge.tsx
- bot/dashboard_v2/src/pages/Overview.tsx
- bot/dashboard_v2/src/components/layout/Header.tsx
- bot/dashboard_v2/src/App.tsx
- bot/dashboard_v2/src/components/charts/FollowerFunnel.tsx
- bot/dashboard_v2/src/components/charts/AudienceDemographics.tsx
- bot/dashboard_v2/src/components/charts/ViewerProfiles.tsx
- bot/dashboard_v2/src/pages/chatAnalyticsDeepSections.tsx
- bot/dashboard_v2/src/pages/chatAnalyticsShared.tsx
- bot/dashboard_v2/src/components/charts/RaidRetention.tsx
- bot/dashboard_v2/src/pages/Schedule.tsx
- bot/dashboard_v2/src/types/analytics.ts
- bot/dashboard_v2/tests/
- .tasks/2026-09-06-analyse-scores-tempo-optik/

## Verbotene Änderungen

- rust/crates/tb-monitoring/ (Session-Tracker, Raid-Retention-Schreiber)
- rust/crates/tb-dashboard-api/src/auth/
- Caddyfile, systemd-Units, Config-Dateien der Dienste
- bot/admin_dashboard/, website/
- Lint- und Test-Konfiguration (eslint, tsconfig, Cargo-Features), außer neue Testdateien unter `bot/dashboard_v2/tests/`

## Offene Produktfragen

- keine

## Amendments

- 2026-09-06, Erlaubter Änderungsbereich, Pfadkorrektur: `rust/crates/tb-dashboard-api/src/handlers/lurker.rs` -> `rust/crates/tb-dashboard-api/src/handlers/lurker_analysis.rs`; die genannten `rust/crates/tb-analytics/src/{audience,viewer_profiles,lurker,follower_funnel}.rs` existieren nicht, die Abfragen liegen in den Handler-Dateien; zusätzlich `rust/crates/tb-dashboard-api/src/handlers/coaching.rs`, falls der Audience-Tab den Coaching-Endpunkt lädt. Grund: Dateinamen beim Schreiben geraten. Entschieden von Orchestrator (nur technisch, reversibel); Freigabe für diff-policy legt der User bei Bedarf an.
