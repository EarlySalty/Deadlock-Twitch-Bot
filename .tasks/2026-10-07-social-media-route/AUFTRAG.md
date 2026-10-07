# Social-Media-Manager

Intent-Thread: d3a1741e-82bc-4a48-865b-2845c663dca7. Rolle: Blatt-Worker, keine T3-Unterthreads.

Ziel: Manager unter `/social-media` mit SPA-Unterpfaden. Alter Pfad mit 308 und erhaltenem Unterpfad sowie Query. API, OAuth und Rechtstexte unverändert. Sichtbarer Name: Social-Media-Manager. Neu-Verbinden-DM auf neuen Pfad setzen. Keine neuen Code-Kommentare.

Repos: Deadlock-Twitch-Bot und caddy-config. Branch: feat/social-media-route. Eigene Worktrees von origin/main beziehungsweise origin/master. Fremde Änderungen bleiben unangetastet.

Abnahme: fmt, clippy, Tests, Dashboard-Build, Sichtprüfung, lokaler Merge-Gate bis ALLOW. Danach main/master pushen, aktuellen origin/main-Stand deployen, Bot und Dashboard neu starten, Caddy prüfen und reloaden, live prüfen, eigene Branches und Worktrees entfernen.
