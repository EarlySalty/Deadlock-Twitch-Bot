# Partnerprofile mit Streamkalender

Auftrag: aktive Streamer-Partner erhalten ein eigenes öffentliches Profil unter
`/streamer/@user`, mit Infos, Socials, selbst gepflegtem Kalender und historischer
Live-Übersicht. Bearbeitung im Twitch-Dashboard; bei Bot-Deaktivierung oder
Austritt offline.

Branch: `feat/partner-profile-calendar`.
Worktree: `/home/nathanael/.worktrees/partner-profile-calendar`.
Basis: `679169ca`.

Implementierung und lokale Prüfungen abgeschlossen. Kein Merge in `main`, keine
Produktionsmigration, kein Dienstneustart, kein Live-Caddy-Reload.

Bedienung, API, Lebenszyklus, Grenzen, genaue Testergebnisse und Deployment:
[`docs/PARTNER_PROFILES.md`](../../docs/PARTNER_PROFILES.md).

Die öffentliche Aktivierung benötigt neben Backend und beiden Frontend-Builds
auch den mitgelieferten Caddy-Import. Ohne diesen würde der bisherige
Landing-Fallback für `/streamer/@...` weiter HTTP 200 mit der Landingpage liefern.
Stylesheet-Weiterleitung ist im Import ebenfalls enthalten und getestet.

Bekannte Restabnahme: visuelle Browserprüfung und Produktions-Smoke-Test.
Die vollständige Dashboard-Suite enthält sieben Fehler in unveränderten
Farb-/Social-Media-/OBS-Bereichen; Details stehen in der Dokumentation.
