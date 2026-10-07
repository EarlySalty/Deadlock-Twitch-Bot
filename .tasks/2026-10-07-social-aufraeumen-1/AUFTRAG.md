# Social Media aufräumen, Runde 1

Auftraggeber: Haupt-Orchestrator, Thread `d3a1741e-82bc-4a48-865b-2845c663dca7`.
Rolle: Blatt-Worker. Branch: `fix/social-aufraeumen-1`. Ausgang: `9315b3cf7e4a6feeffc32b603db26eca78b03a2c` von frisch geholtem `origin/main`.

1. Admin-Freigabe über Twitch-ID; veraltete Python- und Startaussagen entfernen.
2. Unaufgerufenen Redirectstub und doppelten Schema-Bootstrap entfernen; Migrationen erhalten.
3. Uploaddownload auf atomaren Prep-/Vorschaukern umstellen; Retention für abgeleitete Dateien über Clipzustand und Frist.
4. Fehler des Clip-Fetchdiensts in der HTTP-Antwort weiterreichen.

Kein Python, keine neuen Code-Kommentare, keine Alt-API-Bereinigung, keine Änderung am Detektor oder VOD-Archiv. Prüfungen: Formatierung, Clippy, betroffene Tests, beide Frontend-Builds, Merge-Gate. Abschluss: ALLOW, HEAD nach main, Release-Wrapper, Neustart, Liveprüfung, eigener Branch und Worktree entfernen.
