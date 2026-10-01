# Selbstprüfung

- Freigaben: ausschließlich Twitch-ID; Pflichtfeld, Primärschlüssel und gültiges Zahlenformat in der Migration. Der Admin schreibt die ausgewählte ID. Ein Rename aktualisiert nur den Anzeigenamen derselben ID.
- Clips, Vorlagen, Layout, Zeitplan, Einstellungen, Hashtags, Reportzugriff und Credentials: Scope/Joins über IDs. Fremde Anfragen und fehlende IDs ergeben keinen Login-Fallback. Plattform-Refresh aktualisiert die konkrete Credential-Zeilen-ID; Login bleibt für bestehende verschlüsselte AAD erhalten.
- Globale Daten und Verbindungen erfordern sowohl NULL-ID als auch NULL-Anzeigenamen; eine vorhandene ID bleibt unabhängig vom Anzeigenamen kanalgebunden.
- Altdaten: Unauflösbare Altfreigaben werden ausdrücklich gemeldet und mit allen Feldern archiviert. Aktiver Zugriff wird dadurch nicht gewährt. Die beiden Produktivfreigaben sind eindeutig auflösbar.
- Kategorie-Fremdschlüssel bleibt erhalten; nur Login-gebundene Schlüssel fallen weg. Schema-Snapshot wird gegen echte vollständige Migrationen geprüft.
- Analyse und Social Media verwenden dieselbe DashboardHeader-Komponente. Admin-Kanalwahl und Freigabeschalter bleiben vorhanden; studio-brand und der rechte Partner-Chip entfallen.
- Keine neuen Code-Kommentare; Änderungen und Builds ausschließlich im eigenen Worktree/isolierten Release-Clone. Keine Unter-Agenten oder Unter-Threads.
- VOD-Warteschlange, Upload-Nachprüfung und Uploader-Zuordnung hängen ebenfalls an Twitch-IDs. Namen werden vom per ID verknüpften Streamer gelesen; wiedervergebene Namen übertragen keine gespeicherten VODs.
- Prod-Konfiguration bleibt erhalten. Der koordinierte Release muss auch den Bot laden, weil dort die Social-Media-Worker laufen. Kein Main-Push, keine Ref-Overrides, Prod-DDL oder Current-/Release-/Restart-Mutation bis zum expliziten Root-Handoff nach abgeschlossener Challenges63-Abnahme.
- Gate, Merge-SHA und abschließende Testergebnisse werden nach vollständiger Prüfung ergänzt.
