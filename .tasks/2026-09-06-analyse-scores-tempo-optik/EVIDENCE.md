# Evidence: Analyse-Dashboard Scores, Tempo und Optik

status: aktiv
datum: 2026-09-06
contract: CONTRACT.md

Stand: origin/main 542e6c17. Live-Zahlen aus `twitch_analytics` vom 2026-09-06.

## Befund 1: Retention- und Network-Score stehen fast immer auf 100

- rust/crates/tb-dashboard-api/src/handlers/overview.rs:107-111: `retention = min(100, retention_10m_pct * 1.5)`; ab 67 % 10m-Retention ist der Score 100.
- rust/crates/tb-dashboard-api/src/handlers/overview.rs:123-127: `network = clamp(0, 100, (sent + received) * 8 + min(sent, received) * 10)`; ab 13 Raids im Fenster ist der Score 100, unabhängig von Fenster und Streamzahl.
- rust/crates/tb-dashboard-api/src/handlers/overview.rs:573-584: Aufruf `calculate_health_scores` mit `curr_ret`, `per_hour(total_followers)`, `metrics.session_count`, `net`.
- rust/crates/tb-dashboard-api/src/handlers/overview.rs:894-903 und :961-975: bestehende Tests `returns_metrics_for_known_streamer` und `health_scores_formel_exakt` pinnen die alten Formeln (retention 50/60, network 26/42); sie werden auf die neuen Formeln umgeschrieben, nicht gelöscht.
- rust/crates/tb-analytics/src/overview.rs:79-87: `avg_retention_10m` und `retention_sample_count` (nur Sessions mit `avg_viewers >= 3 AND peak_viewers > 0`).
- rust/crates/tb-analytics/src/overview.rs:14-26: `OverviewMetricsRow` (sqlx::FromRow, runtime `query_as`, keine `.sqlx`-Offline-Datei betroffen); neue Spalte `avg_bindung` kommt hier dazu.
- rust/crates/tb-analytics/src/overview.rs:254-296: `overview_network_stats` liefert `sent`, `sent_viewers`, `received` aus `twitch_raid_history` (`executed_at >= since`, `success`); `sqlx::query!`-Makro, also `.sqlx`-Datei bei Query-Änderung.
- rust/crates/tb-monitoring/src/sessions/metrics.rs:14-34: `retention_at` teilt Zuschauer nach Minute 10 durch den Peak davor; bei kleinen, wachsenden Streams fast immer 1,0.
- bot/dashboard_v2/src/pages/StreamReports.tsx:355: Streams-Tab zeigt bereits "Bindung" (Ø Zuschauer geteilt durch Peak); dieselbe Definition wird für den Score genutzt.
- Live-Zahlen earlysalty, 30 Tage: 10m-Retention Ø 0,88 (20 von 28 Sessions exakt 1,0), Bindung Ø 0,585, Raids 25 gesendet und 15 erhalten, 44 Sessions. Netzwerk gesamt: 10m-Retention aller Partner 0,88 bis 1,00 (alle Score 100), Bindung 0,46 bis 0,81; Raids je Partner 8 bis 37 gesendet (alle ab 13 Score 100).

## Befund 2: Radar und Score-Karten

- bot/dashboard_v2/src/components/charts/RetentionRadar.tsx:24-54: sechs Achsen aus `scores`, `categoryAvg` wird von Overview nie übergeben; Growth und Monetization stehen bei earlysalty auf 0 und ziehen das Polygon ins Zentrum.
- bot/dashboard_v2/src/components/charts/RetentionRadar.tsx:82-100: `RadarChart` mit `outerRadius="70%"`, `PolarRadiusAxis tick={false}`, kein Wert an den Ecken.
- bot/dashboard_v2/src/components/cards/ScoreGauge.tsx:33-77: Ring plus Zahl, keine Erklärung, woraus der Score entsteht.
- bot/dashboard_v2/src/pages/Overview.tsx:151-156: vier `ScoreGauge` (Growth, Revenue, Network, Retention).
- bot/dashboard_v2/src/components/cards/HealthScoreCard.tsx:11-15: Teilscores Reach, Ret., Eng. aus denselben `scores`.
- bot/dashboard_v2/src/utils/formatters.ts:133-138: `getScoreColor` (80 gut, 60 ok, 40 schwach).

## Befund 3: Tage-Feld reagiert nicht auf Enter

- bot/dashboard_v2/src/components/layout/Header.tsx:71-82: `uebernehmeTage` parst `tageInput`, `clampDays`, ruft `onDaysChange` nur bei Änderung.
- bot/dashboard_v2/src/components/layout/Header.tsx:305-320: `<input type="number">` mit `onKeyDown` Enter, dann `uebernehmeTage()` plus `blur()`; `onBlur` ruft `uebernehmeTage`.
- bot/dashboard_v2/src/App.tsx:153, :183-189, :238-245: `days` als `useState<TimeRange>(30)`, URL-Parse per `parseDaysParam`, Rückschreiben in die URL.
- bot/dashboard_v2/src/utils/zeitraum.ts:1-7: `clampDays` 7..365.
- bot/dashboard_v2/src/types/analytics.ts:767: `TimeRange = number`.
- Live-Reproduktion durch den Nutzer: Zahl eingeben, Enter, nichts passiert. Ursache im Code noch nicht belegt; Kandidaten: Live-Bundle älter als 62f041e0/ff1a951a, Enter-Event wird von einem umschließenden Element (Form, Segment-Container) geschluckt, `blur()` vor `setTageInput` löst ein zweites `uebernehmeTage` mit altem State aus. Der Implementierer belegt die Ursache mit einem roten Test vor dem Fix.

## Befund 4: Audience-Überblick braucht bei 365 Tagen rund 20 s

- bot/dashboard_v2/src/pages/Audience.tsx:25-41: Hooks `useWatchTimeDistribution`, `useFollowerFunnel`, `useAudienceDemographics`, `useLurkerAnalysis`, `useViewerProfiles`.
- Dashboard-Log 2026-09-06 11:00 bis 11:02 UTC: Requests mit `latency_ms=22219`, `24412`, `12411`, `9536`, `8002`; slow statements auf `twitch_chat_messages` (1,5 s), `twitch_session_chatters`-Self-Join (1,5 s), `twitch_stats_category` DOW/HOUR-Aggregat (14,7 s und 29,9 s).
- rust/crates/tb-analytics/src/coaching.rs:443-465: `schedule_optimizer` aggregiert `twitch_stats_category` über das ganze Fenster (`WHERE ts_utc >= $1 GROUP BY DOW, HOUR`).
- `twitch_stats_category`: 9.350.392 Zeilen ab 2025-10-10, Indizes nur auf `id`, `twitch_user_id`, `streamer`, `lower(streamer)`, `ts_utc` (asc und desc); kein Rollup.
- Messung je Endpunkt: siehe RESEARCH.md (Explore-Agent).

## Befund 5: Farben in Follower-Funnel und Audience Demographics wirken ausgeblasst

- bot/dashboard_v2/src/components/charts/FollowerFunnel.tsx:106: Conversion-Karte `bg-gradient-to-r from-primary/10 to-success/10 border-primary/20` (grünlicher Schimmer, wirkt wie der abgelehnte Oliv-Look).
- bot/dashboard_v2/src/components/charts/FollowerFunnel.tsx:61-75: Stufenfarben `from-primary to-primary`, `from-accent to-accent`, `from-success to-success`.
- bot/dashboard_v2/src/components/charts/FollowerFunnel.tsx:205-208: Skala `bg-danger/40`, `bg-warning/40`, `bg-success/40`.
- bot/dashboard_v2/src/components/charts/FollowerFunnel.tsx:259-264: Balken-Track `bg-background`, Füllung `bg-gradient-to-r ${stage.color}`.
- bot/dashboard_v2/src/components/charts/AudienceDemographics.tsx:51-57: `VIEWER_COLORS` primary, success, warning, secondary, accent (Beige, Grau, Grün, Orange nebeneinander).
- bot/dashboard_v2/src/index.css:27-50: Tokens primary #C5A059, accent #E0BE86, success #43b581, warning #E8A33D, danger #FF5A3C, secondary #9d968a.
- bot/dashboard_v2/tests/brandPalette.test.ts:20-40: `ALLOWED_HEX`; jede neue Hex-Farbe in Komponenten schlägt fehl, neue Töne gehören als Token nach `index.css`.

## Befund 6: Topic-Donut ist eingequetscht, Farben fremd

- bot/dashboard_v2/src/pages/chatAnalyticsDeepSections.tsx:374-386: Container `h-[120px] w-[120px]` im Flex ohne `shrink-0`, `innerRadius 30`, `outerRadius 50`; wird bei schmaler Karte zusammengedrückt.
- bot/dashboard_v2/src/pages/chatAnalyticsDeepSections.tsx:45-62: `TOPIC_COLORS` mit Cyan `#00D9FF`, Rot `#FF5A3C`, Neongrün `#00C46A` (dreimal wiederholt für 15 Themen), Fallback `#B5A488` (:309, :624).

## Befund 7: Raid-Details ohne Sortierung und Filter

- bot/dashboard_v2/src/components/charts/RaidRetention.tsx:55-95: statische Tabelle, `raids.map` in Backend-Reihenfolge, keine Sortier-Header, keine Suche.
- bot/dashboard_v2/src/hooks/useAnalytics.ts:340-343: `useRaidRetention(streamer, days)`.
- rust/crates/tb-dashboard-api/src/handlers/raid_analytics.rs:227-260: `raid_retention_handler` liest `twitch_raid_retention` (`executed_at >= $1`, `LOWER(from_broadcaster_login) = $2`).
- Daten-Nebenbefund (nicht Teil dieses Contracts): von 1496 Zeilen haben nur 498 `chatters_at_plus5m > 0`; die jüngsten Raids von earlysalty stehen auf 0/0/0 bei gesetzter `target_session_id`. Schreiber ist `rust/crates/tb-monitoring/src/raid_retention.rs:40-150` (`compute_raid_retention`, Fenster 7 Tage, `window_count` über `session_chatters.last_seen_at`).

## Befund 8: Wochentags-Balken kaum unterscheidbar

- bot/dashboard_v2/src/pages/Schedule.tsx:353-359: `maxViewers = max(avgViewers)`, `viewerPct = avg / max * 100`; bei Werten 2,6 bis 4,1 liegen alle Balken zwischen 63 % und 100 %.
- bot/dashboard_v2/src/pages/Schedule.tsx:389-403: Balkenhöhe `max(8, viewerPct)%`, Zahl `Math.round(day.avgViewers)` (3, 3, 3, 3, 4, 3, 2).

## Bestehende Abstraktionen (werden wiederverwendet, nicht nachgebaut)

- bot/dashboard_v2/src/components/cards/ScoreGauge.tsx: Ring-Karte, bekommt einen optionalen Untertitel.
- bot/dashboard_v2/src/components/layout/SubTabs.tsx: Tab-Muster für Filter-Chips.
- bot/dashboard_v2/src/motion/Rise.tsx: Kartenwrapper.
- bot/dashboard_v2/src/utils/formatters.ts: `formatNumber`, `formatPercent`, `getScoreColor`.
- rust/crates/tb-dashboard-api/src/handlers/overview.rs:87-142: `calculate_health_scores` bleibt die einzige Score-Quelle.

## Relevante Tests (laufen vorher, laufen nachher)

- rust/crates/tb-dashboard-api/src/handlers/overview.rs:826-919, :961-980: Overview-Handler-Test gegen Docker-Test-DB, Formel-Test.
- bot/dashboard_v2/tests/brandPalette.test.ts: Hex-Allowlist.
- bot/dashboard_v2/tests/scoreColors.test.ts: Score-Farbschwellen.
- bot/dashboard_v2/tests/dashboardShell.test.ts: Shell-Struktur.

## Öffentliche Schnittstellen und Verträge (dürfen nicht brechen)

- `GET /twitch/api/v2/overview`: JSON-Form `scores.{total,reach,retention,engagement,growth,monetization,network}` bleibt (Zahlen 0..100), `summary.retention10m` bleibt.
- `GET /twitch/api/v2/raid-retention`: unverändert.
- Audience-Endpunkte: Antwortform unverändert, nur schneller.

## Änderungsfläche

- rust/crates/tb-dashboard-api/src/handlers/overview.rs: Formeln, Tests.
- rust/crates/tb-analytics/src/overview.rs: `avg_bindung` in `OverviewMetricsRow`.
- rust/crates/tb-analytics/src/coaching.rs oder die von RESEARCH.md benannten Abfragen: Tempo.
- rust/migrations/: Index oder Rollup, falls RESEARCH.md das ergibt (Migration wird als `postgres` von Hand angewendet, siehe AGENTS.md).
- bot/dashboard_v2/src/components/charts/RetentionRadar.tsx, cards/ScoreGauge.tsx, pages/Overview.tsx: Radar und Gauges.
- bot/dashboard_v2/src/components/layout/Header.tsx: Tage-Feld.
- bot/dashboard_v2/src/components/charts/FollowerFunnel.tsx, AudienceDemographics.tsx, pages/chatAnalyticsDeepSections.tsx, index.css: Farben, Donut.
- bot/dashboard_v2/src/components/charts/RaidRetention.tsx: Sortierung, Filter.
- bot/dashboard_v2/src/pages/Schedule.tsx: Wochentags-Balken.

## Offene Architekturfrage

- keine; der Tempo-Fix wird nach RESEARCH.md im Contract festgelegt.
