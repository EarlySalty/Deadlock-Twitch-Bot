# Viewer Presence Timeline

Granulares Presence-Tracking: Wer war wann im Stream – sichtbar als Gantt-Chart im Dashboard.

---

## Überblick

Dieses Feature beantwortet die Frage:
> *„Welche Viewer waren wann anwesend, und sind sie wiedergekommen nachdem sie den Stream verlassen haben?"*

### Was vorher gespeichert wurde
`twitch_session_chatters` enthält pro Viewer und Session nur zwei Zeitstempel:
- `first_message_at` – wann zuerst gesehen (oder erste Nachricht)
- `last_seen_at` – wann zuletzt gesehen (wird bei jedem Poll überschrieben)

Zwischenstände gehen verloren. Lücken (Viewer verlässt den Stream und kommt zurück) sind nicht rekonstruierbar.

### Was jetzt gespeichert wird
Jeder 30-Sekunden-Poll schreibt zusätzlich einen Tick-Row pro Viewer in `twitch_viewer_presence_ticks`. Aus diesen Ticks lassen sich saubere Anwesenheits-Spans berechnen.

---

## Datenfluss

```
Twitch API: GET /helix/chat/chatters
    │  (alle 30 Sekunden, pro Live-Streamer)
    ▼
collect_chatters_data()          bot/analytics/mixin.py
    │
    ├─ INSERT INTO twitch_session_chatters   (bereits vorhanden: first/last seen)
    ├─ INSERT INTO twitch_chatter_rollup     (bereits vorhanden: globale Rollups)
    └─ INSERT INTO twitch_viewer_presence_ticks  ← NEU
           session_id, streamer_login, viewer_login, tick_at
           ON CONFLICT DO NOTHING  (idempotent)
```

---

## Datenbank

### twitch_viewer_presence_ticks

TimescaleDB-Hypertable. Chunk-Intervall: 1 Tag. Kompression nach 3 Tagen.

| Spalte | Typ | Beschreibung |
|--------|-----|--------------|
| `session_id` | BIGINT PK/FK | Referenz auf `twitch_stream_sessions.id` |
| `streamer_login` | TEXT | Twitch-Login des Streamers |
| `viewer_login` | TEXT | Twitch-Login des Viewers (lowercase) |
| `tick_at` | TIMESTAMPTZ PK | Zeitstempel des Polls |

**Primary Key:** `(session_id, viewer_login, tick_at)`

**Index:** `idx_viewer_presence_ticks_session` auf `(session_id, viewer_login, tick_at)`

**Speicherbedarf:** ~500 Viewer × 2 Ticks/min × 240 min = ~240.000 Rows/Stream → nach Kompression ca. 2–5 MB pro Stream.

### Span-Berechnung (Query-Zeit, kein Speicher)

Ticks werden zur Abfragezeit per Window Function zu zusammenhängenden Anwesenheits-Spans gruppiert:

```sql
WITH ticked AS (
    SELECT viewer_login, tick_at,
        EXTRACT(EPOCH FROM (
            tick_at - LAG(tick_at) OVER (PARTITION BY viewer_login ORDER BY tick_at)
        )) / 60 AS gap_min
    FROM twitch_viewer_presence_ticks
    WHERE session_id = $session_id
),
grouped AS (
    SELECT viewer_login, tick_at,
        SUM(CASE WHEN gap_min > 2 OR gap_min IS NULL THEN 1 ELSE 0 END)
            OVER (PARTITION BY viewer_login ORDER BY tick_at) AS span_id
    FROM ticked
)
SELECT viewer_login,
    MIN(tick_at) AS span_start,
    MAX(tick_at) AS span_end
FROM grouped
GROUP BY viewer_login, span_id;
```

**Gap-Schwelle:** `> 2 Minuten` – großzügiger als das 30s-Poll-Intervall, damit kurze API-Aussetzer keinen falschen Split erzeugen. Echte Abwesenheiten (Viewer schließt den Tab für >2 min) werden korrekt als neue Span erkannt.

---

## API

Alle Endpunkte erfordern `_require_v2_auth` + `_require_extended_plan`.
Implementierung: `bot/analytics/api_viewer_timeline.py` → Mixin `_ViewerTimelineMixin`.

### GET `/twitch/api/v2/{streamer}/viewer-timeline`

Gibt alle Viewer einer Session mit ihren Anwesenheits-Spans zurück.

**Query-Parameter:**

| Parameter | Typ | Standard | Beschreibung |
|-----------|-----|----------|--------------|
| `session_id` | integer | – (Pflicht) | Session-ID |
| `min_present_min` | integer | `0` | Mindest-Anwesenheitszeit in Minuten |
| `segment` | string | – | Filter: `dedicated`, `regular`, `casual`, `lurker`, `new` |
| `search` | string | – | Substring-Suche im Viewer-Login |
| `limit` | integer | `200` | Max. Viewer im Response (max. 1000) |

**Response:**
```json
{
  "session_id": 123,
  "session_start": "2026-04-04T18:00:00Z",
  "session_duration_min": 180,
  "viewers": [
    {
      "login": "viewer1",
      "segment": "regular",
      "spans": [
        { "start_min": 0, "end_min": 45 },
        { "start_min": 62, "end_min": 180 }
      ],
      "total_present_min": 163,
      "chat_messages": 12
    }
  ],
  "total_unique_tracked": 847
}
```

- `spans` sind relativ zum Session-Start in Minuten.
- `total_unique_tracked` gibt die Gesamttreffer vor dem `limit`-Schnitt an.
- Sortierung: absteigend nach `total_present_min`, dann `chat_messages`.
- Viewer-Segment wird aus `twitch_session_chatters` + `twitch_chatter_rollup` via `_classify_viewer()` bestimmt.
- Bots werden gefiltert via `_build_viewer_identity_not_in_clause()` + `KNOWN_CHAT_BOTS`.

### GET `/twitch/api/v2/{streamer}/viewer-timeline/profile`

Gibt alle Sessions zurück, in denen ein bestimmter Viewer anwesend war.

**Query-Parameter:**

| Parameter | Typ | Beschreibung |
|-----------|-----|--------------|
| `login` | string | Viewer-Login (Pflicht) |

**Response:**
```json
{
  "streamer": "nani",
  "login": "viewer1",
  "sessions": [
    {
      "session_id": 123,
      "started_at": "2026-04-04T18:00:00Z",
      "total_present_min": 163,
      "chat_messages": 12
    }
  ],
  "total_sessions": 5
}
```

- Enthält Sessions aus Presence-Ticks **und** aus `twitch_session_chatters` (UNION), damit historische Sessions ohne Ticks ebenfalls erscheinen (mit `total_present_min = 0`).

---

## Frontend

### Dashboard-Integration

Die Timeline ist als dritter Tab in `SessionDetail.tsx` eingebunden:

```
Sessions → SessionDetail → [ Übersicht | Events | Viewer-Timeline ]
```

### Komponenten

| Datei | Zweck |
|-------|-------|
| `bot/dashboard_v2/src/pages/ViewerTimeline.tsx` | Gantt-Chart-Seite |
| `bot/dashboard_v2/src/hooks/useAnalytics.ts` | `useViewerTimeline()`, `useViewerTimelineProfile()` |
| `bot/dashboard_v2/src/api/analytics.ts` | `fetchViewerPresenceTimeline()`, `fetchViewerTimelineProfile()` |
| `bot/dashboard_v2/src/types/analytics.ts` | `ViewerTimelineEntry`, `ViewerPresenceSpan`, `ViewerTimelineSessionResponse`, `ViewerTimelineProfileResponse` |

### Gantt-Chart

Jeder Viewer bekommt eine Zeile mit:
- **Links:** Login-Name + Segment-Badge + Message-Count
- **Rechts:** Horizontale Balken (CSS-Grid, kein Recharts) pro Span

Balken-Position und Breite werden aus `start_min / sessionDurationMin * 100%` berechnet. Minimale Balkenbreite: `0.8%` (damit auch sehr kurze Ticks sichtbar sind).

**Gridlines:** 10 gleichmäßige vertikale Linien via CSS-Gradient-Background.

**Farben** per Segment (`SEGMENT_CONFIG` aus `Viewers.tsx`):

| Segment | Farbe |
|---------|-------|
| `dedicated` | `#22c55e` (Grün) |
| `regular` | `#3b82f6` (Blau) |
| `casual` | `#f59e0b` (Amber) |
| `lurker` | `#8b5cf6` (Lila) |
| `new` | `#06b6d4` (Cyan) |
| unbekannt | `#94a3b8` (Grau) |

**Hover-Tooltip:** Login, Presence-Zeit (`Xh Ym`), Segment, Message-Count.

### Filter

| Filter | Beschreibung |
|--------|--------------|
| Min. Presence | Slider + Zahlenfeld, 0–`sessionDurationMin` Minuten |
| Segment | Dropdown (alle / dedicated / regular / casual / lurker / new) |
| Suche | Substring-Suche im Viewer-Login (clientseitig via API-Query) |

### Pagination

Standardmäßig werden die Top 200 Viewer nach Anwesenheitszeit geladen. Ein „Mehr laden"-Button lädt jeweils 200 weitere nach (erhöht `limit` im Query-Parameter).

---

## Bekannte Einschränkungen

| Einschränkung | Grund |
|---------------|-------|
| `total_present_min` zählt die letzte Poll-Periode nicht | Letzter Tick markiert das letzte Mal gesehen, nicht das Ende der Anwesenheit |
| Historische Sessions ohne Ticks | Ticks gibt es erst seit Feature-Deploy; ältere Sessions zeigen `total_present_min = 0` im Profil |
| Keine Lurker ohne Chatters-API-Scope | Viewer, die nicht über `GET /helix/chat/chatters` sichtbar sind (kein Scope), werden nicht getrackt |

---

## Deployment-Hinweis

Nach dem ersten Deploy muss die Migration auf der Produktions-DB ausgeführt werden:

```sql
-- Aus: bot/migrations/twitch_analytics_schema.sql (letzter Block)
CREATE TABLE IF NOT EXISTS twitch_viewer_presence_ticks ( ... );
SELECT create_hypertable(...);
-- usw.
```

Danach füllt sich die Tabelle automatisch mit dem nächsten Live-Stream. Historische Daten werden nicht rückwirkend befüllt.
