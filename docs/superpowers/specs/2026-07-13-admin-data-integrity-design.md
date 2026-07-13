# Admin-Datenintegrität: Design

## Ziel

Das Twitch-Admin-Dashboard soll belastbare statt nur optisch plausible Daten zeigen. Der Scope umfasst vier bestehende Bereiche: Partnerliste, Research, EventSub-Status und Audit-Log.

## Entscheidungen

### Partner seit

`Partner seit` bedeutet das Datum der ersten erfolgreichen Bot-Autorisierung. Autoritative Quelle ist `twitch_raid_auth.created_at`, weil eine erneute Autorisierung nur `authorized_at` aktualisiert. Die bestehende Streamer-Tabelle erhält eine sortierbare Spalte sowie Von-/Bis-Filter. Ohne Autorisierung wird `Nie autorisiert` gezeigt.

### Research-Vorschläge

Die Research-Seite erhält zusätzlich zur Einzelanalyse eine Rangliste. Sie verwendet dieselben 7-/30-/90-Tage-Metriken und dieselbe Bewertungslogik, schließt vorhandene Partner aus und begrenzt die Ausgabe. Damit bleiben Einzelbewertung und Vorschläge fachlich konsistent.

### EventSub

Ein alter Capacity-Snapshot darf nicht als aktive Verbindung erscheinen. Der Rust-Bot schreibt seinen tatsächlich aufgebauten Subscription-Zustand in den vorhandenen Snapshot-Speicher; das Dashboard kennzeichnet veraltete Snapshots als offline und zeigt gespeicherte Subscriptions statt einer künstlich leeren Liste.

### Audit-Log

Mutierende Admin-Requests werden serverseitig nach Abschluss in einer eigenen Append-only-Tabelle gespeichert. Erfasst werden Zeitpunkt, Actor, HTTP-Methode, Route und Ergebnisstatus. Das bestehende Audit-Log liest diese Quelle zusammen mit den vorhandenen Fachquellen; reine Lesezugriffe und Auth-Secrets werden nicht gespeichert.

## Grenzen

- Keine neue Abhängigkeit und keine zweite Partner-Datumsdefinition.
- Keine rückwirkend erfundene Historie: fehlende erste Autorisierungen bleiben leer.
- Keine Tokens, Request-Bodies oder Query-Parameter im Audit-Log.
- Vorhandene API-Verträge werden nur additiv erweitert.

## Abnahme

- Backend-Vertragstests decken Quelle, Sortierung/Filterung, Ranking, Snapshot-Aktualität und Audit-Persistenz ab.
- Admin-Frontend baut erfolgreich und zeigt alle neuen Zustände verständlich.
- Rust-Workspace baut, testet und lintet ohne neue Fehler.
- Nach Merge werden Bot und Dashboard neu gebaut, neu gestartet und über PID, Binary-Pfad, Journallog und Live-Endpoints verifiziert.
