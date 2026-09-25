# Schnittstelle und eindeutige Zuständigkeiten A/B

A (Thread 50f38cec): ALLE Rust-Dateien, Migrationen, systemd/Runtime/Runbook, Sicherheitsfix Hauptbot, Backend-Route. KEINE Änderungen unter bot/dashboard_v2. Auftrag AUFTRAG.md für Backend bleibt sonst unverändert. A liefert REPORT.md.
B (neuer Frontend-Thread): ausschließlich bot/dashboard_v2 und FRONTEND-REPORT.md. Keine Rust-Dateien, keine Migration, kein Deploy, keine Secrets/Twitch-Zugriffe, kein main-Merge, keine Unter-Threads. Derselbe Feature-Worktree, disjunkte Pfade. Git-Commits nur eigene exakte Pfade, niemals git add -A; Orchestrator integriert. Beide Threads keine gegenseitigen Dateireverts.

## JSON: GET /twitch/api/v2/admin/category-collector?days=7
7, 30 oder 90. Serverseitig Admin-only, 401 ohne Session, 403 für Nichtadmin. Kein Rohchat. Alle Zeitstempel RFC3339 UTC. Alle counts >=0, nicht beobachtete Schnitte null statt 0. Außergewöhnliche Backend-Fehler HTTP-Fehler, nicht Nullstatistik.

```
{
  "meta": {
    "days": 7,
    "period_start": "2026-09-11T00:00:00Z",
    "period_end": "2026-09-18T00:00:00Z",
    "timezone": "UTC",
    "first_seen_at": null,
    "last_completed_poll_at": null,
    "last_message_at": null,
    "complete_polls": 0,
    "incomplete_polls": 0,
    "dropped_messages": 0,
    "storage_bytes": 0,
    "retention_days": 90,
    "collector_status": "not_started",
    "measurement_seconds": 0
  },
  "stream_languages": [
    {"language":"de","unique_streams":1,"unique_channels":1,"broadcast_hours":1.0,"viewer_hours":10.0,"avg_viewers":10.0}
  ],
  "message_languages": [
    {"language":"de","messages":30}
  ],
  "trend": [
    {"bucket_at":"2026-09-18T00:00:00Z","avg_streams":1.0,"avg_viewers":10.0,"poll_samples":1}
  ],
  "top_channels": [
    {"language":"de","user_id":"123","login":"example","display_name":"Example","broadcast_hours":1.0,"viewer_hours":10.0,"avg_viewers":10.0}
  ],
  "chat_heatmap": [
    {"language":"de","hour_utc":12,"messages":30}
  ]
}
```

Alle Listen bei noch fehlender Datensammlung leer. collector_status text: not_started/running/stale/disabled/error (kein Grün-Rot-Scheinbeweis nötig). Fehlende DB-Tabellen vor Migration sollen 503 liefern, nicht scheinbare Leerdaten. first_seen_at/last_completed_poll_at/last_message_at nullable. measurement_seconds entspricht echter Messdauer bis letzter Erfassung, nicht automatisch days*86400. incomplete_polls und complete_polls im ausgewählten Zeitraum; dropped_messages als bekannter kumulativer Zähler kennzeichnen, falls nicht zeitlich aggregiert. storage_bytes gemessene Relationengröße category_* inklusive Indizes, nicht Gesamt-DB-Schätzung. retention_days aus DB-Config.

## UI-Semantik
Die Seite heißt globaler Kategoriesammler oder Kategorie weltweit. Auswertung ausschließlich nach Sprache, nicht Region/Land/Flaggen. Stream-Sprache ist Dimension stream_languages, Nachrichtensprache ist unabhängig message_languages und chat_heatmap. Kernanzeige kann nach Sprachcode zusammenführen, MUSS aber die beiden Spaltenbereiche unterschiedlich beschriften. 'und' = nicht sicher erkannt. Top-Kanäle Filter je Stream-Sprache, Impact = Zuschauerstunden, keine Uniques über Stunden aufsummieren. Trend zeigt vorhandene Zeitpunkte mit poll_samples und lässt Datenlücken sichtbar, keine Offline-Nullen interpolieren. Tageszeit explizit UTC.

Bestehende Shell/Sidebar/Auth und Charts wiederverwenden. 7/30/90 Tage Auswahl. Messdauer/Datenstand/Ausfälle/Drops/Retention/Speicher anzeigen. Caddy deckt /twitch/dashboard-v2/*, /twitch/analyse/* und /twitch/api/v2/* bereits ab; innerhalb bestehender SPA routen, dann keine Proxy-Freigabe nötig. B benennt exakten UI-Pfad im Bericht.
