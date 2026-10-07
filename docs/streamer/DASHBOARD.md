# Dashboard und Seiten

Stand: `2026-03-13`

## Uebersicht der Streamer-Surfaces

| URL | Zweck | Fuer Streamer relevant |
| --- | --- | --- |
| `/twitch/dashboard` | Interne Startseite / Home | Ja |
| `/twitch/dashboard-v2` | Analytics-Dashboard mit Tabs | Ja |
| `/twitch/verwaltung` | OAuth, Discord, Profil | Ja |
| `/twitch/pricing` | Planvergleich im React-Frontend | Ja |
| `/twitch/affiliate/portal` | Affiliate-Zusammenfassung | Ja |
| `/twitch/abbo` | Billing, Rechnungen, Lurker Steuer, Promo-Einstellungen | Ja |
| `/twitch/live-announcement` | Go-Live Builder fuer Discord | Ja |
| `/twitch/social-media` | Clip- und Upload-Dashboard | Ja |
| `/twitch/demo` | Oeffentliche Demo mit Demodaten | Eher Preview |

## `/twitch/dashboard`

`/twitch/dashboard` ist die aktuelle Streamer-Startseite. Im Code landet diese Route nicht mehr in einem alten Server-Rendered Statistikscreen, sondern im "Internal Home".

Verwendete APIs:

- `GET /twitch/api/v2/internal-home`
- `GET /twitch/api/v2/auth-status`
- `POST /twitch/api/v2/internal-home/changelog` nur fuer Admins

Die Seite zeigt aktuell:

- Welcome-/Statusbereich fuer den eingeloggten Streamer
- Health Score mit Subscores
- Zusammenfassung des letzten Streams
- Wochenvergleich fuer wichtige KPIs
- Quick Actions zu Analyse-Dashboard, Verwaltung und Pricing
- Changelog-Eintraege
- Bot-Aktionslog, zum Beispiel Raids, Bans und Service-Warnungen

Wichtig:

- Streamer sehen hier nur den eigenen Account.
- Admins koennen den Streamer-Kontext wechseln.
- Die Home-API liefert auch OAuth-, Discord- und Raid-Statusdaten fuer die Unterseiten.

## `/twitch/dashboard-v2`

Das ist das eigentliche Analytics-Dashboard. Die Route ist eine React-SPA mit Tab-Navigation, Plan-Gating und Preview-Modus fuer gesperrte Inhalte.

Gemeinsam genutzte APIs:

- `GET /twitch/api/v2/auth-status`
- `GET /twitch/api/v2/streamers`

### Tabs nach Tier

| Tier | Tabs |
| --- | --- |
| `free` | `overview` Uebersicht, `streams`, `schedule`, `category` |
| `basic` | plus `chat`, `growth`, `audience`, `compare` |
| `extended` | plus `viewers`, `coaching`, `monetization`, `experimental`, `ai` |

### Tabs, Datenquellen und Inhalte

| Tab | Tier | Wichtige APIs | Was Streamer dort sehen |
| --- | --- | --- | --- |
| `overview` | `free` | `overview`, `hourly-heatmap`, `calendar-heatmap`, `viewer-timeline` | KPI-Board, Health Scores, Session-Tabelle, Heatmaps, Viewer-Timeline, Insights |
| `streams` | `free` | `overview`, `session/{id}` | Session-Liste mit Drilldown auf Retention, Viewer-Verlauf und Top-Chatters |
| `schedule` | `free` | `hourly-heatmap`, `weekly-stats` | beste Slots, Tageszeiten, Heatmap, Timing-Tipps |
| `category` | `free` | `category-leaderboard`, `category-activity-series` | Deadlock-Leaderboards, Filter, Suche, Sortierung |
| `chat` | `basic` | `chat-analytics`, `viewer-profiles`, `coaching`, `chat-hype-timeline`, `chat-content-analysis`, `chat-social-graph` | Chat-KPIs, Loyalitaet, Tageszeiten, Viewer-Profile, Hype-Momente, Themen. `chat-social-graph` und die sichtbare Metrik `Chat Penetration` sind derzeit im UI deaktiviert. |
| `growth` | `basic` | `monthly-stats`, `weekly-stats`, `tag-analysis-extended`, `title-performance`, `raid-retention` | Monats-/Wochenwachstum, Titel/Tags, Scheduling, Raid-Retention |
| `audience` | `basic` | `watch-time-distribution`, `follower-funnel`, `tag-analysis-extended`, `title-performance`, `audience-demographics`, `lurker-analysis` | Watch-Time, Funnel, Demografie, Lurker-Sicht, Titel-/Tag-Performance |
| `compare` | `basic` | `category-comparison`, `viewer-overlap`, `audience-sharing` | Benchmark vs. Kategorie, Overlap, Audience-Sharing |
| `viewers` | `extended` | `viewer-directory`, `viewer-detail`, `viewer-segments` | Viewer-Suche, Segmente, Churn-Risiko, Einzelprofil-Deep-Dive |
| `coaching` | `extended` | `coaching` | priorisierte Handlungsempfehlungen |
| `monetization` | `extended` | `monetization` | Ads, Viewer-Drop, Recovery, Bits, Subs, Hype-Train |
| `experimental` | `extended` | `exp/overview`, `exp/game-breakdown`, `exp/game-transitions`, `exp/growth-curves` | Multi-Game- und Transitions-Analyse |
| `ai` | `extended` | `ai/analysis`, `ai/history` | aktuell fuer normale Streamer faktisch nicht nutzbar |

### Preview-Modus

- Im Header kann auf die "Erweitert"-Vorschau umgeschaltet werden.
- Gesperrte Tabs und Cards werden dann sichtbar, aber bleiben gelockt.
- Upgrade-CTAs zeigen auf `/twitch/pricing`.

### Card-Level-Gating innerhalb der Analytics

Auch in freigeschalteten Tabs koennen einzelne Cards auf `extended` limitiert sein. Aktuell betrifft das unter anderem:

- `health_scores`
- `calendar_heatmap`
- `insights_panel`
- `stream_timeline_detail`
- `chatter_list`
- `hype_timeline`
- `chat_content_analysis`
- `chat_social_graph`
- `title_performance`
- `raid_retention`
- `lurker_analysis`
- `audience_sharing`
- `viewer_overlap`
- `category_timings`
- `category_activity_series`
- `rankings_extended`

### Wichtige Einschraenkung

- Der Tab `ai` ist im Frontend als Extended-Tab modelliert.
- Die APIs `/twitch/api/v2/ai/analysis` und `/twitch/api/v2/ai/history` sind aktuell trotzdem admin-only.
- Fuer normale Streamer bedeutet das: UI-Tab sichtbar moeglich, produktive Daten aber derzeit nicht als normaler Partner nutzbar.
- Das Feature `chat_social_graph` ist implementiert, aber aktuell bewusst nicht im Streamer-UI aktiv. Details: [Chat-Netzwerk deaktiviert](CHAT_NETWORK_DISABLED.md)
- Die Metrik `Chat Penetration` wird aktuell bewusst nicht im Streamer-UI ausgespielt. Details: [Chat Penetration deaktiviert](CHAT_PENETRATION_DISABLED.md)

## `/twitch/verwaltung`

Die Verwaltungsseite ist die Streamer-Ansicht fuer Verbindungen und Account-Status.

Sie zeigt:

- Twitch-OAuth-Status
- aktive Scopes
- fehlende Scopes
- Reconnect-Link fuer Twitch
- Discord-Status und Reconnect-/Connect-Link
- Twitch-Login, Display Name und User-ID

Wenn Scopes fehlen, ist diese Seite die erste Stelle fuer Re-Auth.

## `/twitch/pricing`

Die Pricing-Seite ist die moderne Planvergleichsseite im React-Frontend.

Verwendete API:

- `GET /twitch/api/v2/billing/catalog`

Sie zeigt:

- `Free`
- `Basic`
- `Erweitert`
- Vergleichsmatrix mit `4 / 8 / 13` Analytics-Tabs
- Upgrade-CTA auf `/twitch/abbo`

Wichtig:

- `/twitch/pricing` ist die Vergleichsseite.
- `/twitch/abbo` ist die eigentliche Abo- und Billing-Seite.

## `/twitch/affiliate/portal`

Die aktuelle Streamer-Surface fuer Affiliates zeigt:

Verwendete API:

- `GET /twitch/api/v2/affiliate/portal`

- persoenlichen Referral-Link
- Gesamt-Claims
- Gesamt-Provision
- Claims des laufenden Monats
- ausstehende Auszahlung
- letzte Claims

Wenn kein Affiliate-Account hinterlegt ist, zeigt die Seite "Du bist noch kein Affiliate".

## Weitere Streamer-Seiten

### `/twitch/live-announcement`

- Builder fuer Discord-Go-Live-Posts
- Preview, Validierung, Testsendung und Rollenverwaltung

### `/twitch/social-media`

- Tabs fuer `Dashboard`, `Clips`, `Templates` und `Settings`
- Clip-Listing, Upload-Queue, Batch-Upload, Plattform-Verbindungen

### `/twitch/demo`

- Oeffentliche Demo des Analytics-UIs
- Nutzt Demodaten, nicht den Streamer-Account
