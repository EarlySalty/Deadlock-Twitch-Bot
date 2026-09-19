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

## Nachprüfung des passiven Dashboard-Zugangs

Der unabhängige Review fand einen echten Fehler in der vorgeschalteten
`partner_status_gate`: Der eigene Profilabruf war für passive Partner gesperrt,
bevor der Handler seinen gespeicherten, nicht öffentlichen Stand zeigen konnte.
Korrigiert wird ausschließlich GET/HEAD auf dem exakten Eigentümer-Endpunkt.
PUT/POST/PATCH/DELETE bleiben aktivitätspflichtig; Login, Besitzerbindung,
CSRF und die öffentliche Abschaltung bleiben unverändert.

Rote Gegenprobe: neuer Test 0/1 erfolgreich (GET wurde nicht zugelassen).
Danach 7/7 Profiltests erfolgreich; alle sieben Deaktivierungsvarianten prüfen
zusätzlich den privaten Lesezugang und die erhaltene Veröffentlichungsabsicht.
Clippy erfolgreich, nur bestehende Warnungen außerhalb der Änderungen.

Abgeschlossen: `76d58382` ist auf main, gepusht und live installiert. Migration
und beide Caddy-Imports sind aktiv; Dashboard und Twitch-Bot wurden am
18. September 2026 um 20:32 Uhr (Europe/Berlin) neu gestartet. Live-Nachweis,
HTTP-Antworten, Browser-Abnahme und verbleibende nicht blockierende Hinweise
stehen in `LIVE.md` neben dieser Datei.

Bedienung und HTTP-Vertrag: `docs/PARTNER_PROFILES.md`.

## Nachtrag: Profiloberfläche am 19. September 2026

Der Profil-Editor folgt jetzt der gemeinsamen Industrial-Gold-Oberfläche, stellt den Streamer
mit einer direkten Vorschau vor und hat Veröffentlichen sowie Speichern oben im Profilkopf.
Das Twitch-Profilbild wird aus der vorhandenen Twitch-Anbindung übernommen. Die öffentliche
Profilseite nutzt dieselbe Gestaltung und begrenzt sehr volle Kalendertage auf drei sichtbare
Einträge plus Restanzahl, damit einzelne Tage die Monatsansicht nicht mehr aufziehen.

Gezielte Dashboard-Tests: 14 erfolgreich. Partnerprofil-Backend: 8 erfolgreich. Caddy-Routing:
1 erfolgreich. Der Dashboard-Produktionsbuild ist erfolgreich. Die vollständige Dashboard-Suite
hat weiterhin bekannte Fehler außerhalb dieser Änderung und wird nicht als vollständig grün bezeichnet.
