# Partnerprofile mit Streamkalender

Auftrag: aktive Streamer-Partner erhalten ein öffentliches Profil unter
`/streamer/user`, mit Infos, Socials, eigenem Kalender und historischer
Live-Übersicht. Bearbeitung im Twitch-Dashboard; bei Bot-Deaktivierung oder
Austritt offline. Das zuvor verwendete @ war nur ein Platzhalter im Auftrag
und gehört ausdrücklich nicht in die Adresse.

Branch: `feat/partner-profile-calendar`.
Worktree: `/home/nathanael/.worktrees/partner-profile-calendar`.
Aktueller Produktionsstand `5bc791c1` ist in den Feature-Branch integriert.

## Abnahme der Pfadkorrektur

- Backend-Gegenprobe: 4/6 erfolgreich, zwei Fehler wegen alter @-Pfade.
- Vollständige Router-Komposition deckte eine zusätzliche Axum-Kollision auf.
  Behebung: ein gemeinsamer Wildcard-Dispatcher für Profile und Website-Dateien.
- Backend danach: 6/6 erfolgreich. Echte isolierte PostgreSQL-Instanzen, keine
  Produktionsprofile angelegt oder verändert.
- Dashboard: 28/28 gezielte Profil-/Verwaltungs-/Community-Tests erfolgreich.
- Caddy: echter isolierter Loopback-Test ohne Admin-Port, Profile und Monate mit
  und ohne abschließenden Slash; reservierte Website-Seiten/Assets unverändert.
  Alte @-Adressen und deaktivierte Profile liefern 404 ohne Cache.
- Profiltabellen-Snapshot aus einer isolierten PostgreSQL-Instanz erzeugt.

Deployment-Auftrag: nach Review nach main integrieren, pushen, das komplette
Release bauen, Migration ausführen, beide Caddy-Imports setzen sowie Dashboard
und Twitch-Bot neu starten. Live-Nachweis wird nach der Ausführung ergänzt.

Bedienung und HTTP-Vertrag: `docs/PARTNER_PROFILES.md`.
Die vollständige Dashboard-Suite hatte vor diesem Folgeauftrag sieben Fehler
in unveränderten Farb-/Social-Media-/OBS-Bereichen. Keine pauschale Behauptung,
dass die gesamte Suite fehlerfrei sei.
