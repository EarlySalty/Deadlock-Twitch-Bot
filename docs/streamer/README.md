# Streamer Guide

Stand: `2026-03-13`

Dieser Guide wurde gegen den aktuellen Codebestand geprueft. Er beschreibt die Streamer-Surfaces, Commands und Features so, wie sie aktuell im Repo implementiert sind.

## Navigation

- [Dashboard und Seiten](DASHBOARD.md)
- [Commands](COMMANDS.md)
- [Features und Workflows](FEATURES.md)
- [Werbemanager](WERBEMANAGER.md)
- [Chat-Netzwerk deaktiviert](CHAT_NETWORK_DISABLED.md)
- [Chat Penetration deaktiviert](CHAT_PENETRATION_DISABLED.md)

## Schnellstart fuer Streamer

1. Login ueber Twitch oder Discord herstellen.
2. `/twitch/dashboard` als Startseite oeffnen.
3. In `/twitch/verwaltung` OAuth-Scopes und Discord-Verknuepfung pruefen.
4. In `/twitch/dashboard-v2` die Analytics nutzen.
5. Ueber `/twitch/raid/auth` oder Discord `/traid` den Raid-Bot autorisieren.
6. In `/twitch/abbo` Plan, Rechnungsdaten und Zusatzfeatures verwalten.
7. Optional `/twitch/live-announcement`, `/social-media` und `/twitch/affiliate/portal` nutzen.

## Wichtige Realitaeten

- `/twitch/dashboard` ist aktuell die interne Startseite fuer Streamer, nicht mehr das alte klassische Statistik-Dashboard.
- `/twitch/dashboard-v2` ist das eigentliche Analytics-Dashboard mit Tabs und Plan-Gating.
- Die KI-Seite ist im Frontend als Extended-Tab sichtbar, aber die API unter `/twitch/api/v2/ai/*` ist derzeit admin-only.
- Die Legacy-Seiten `/twitch/raid/requirements`, `/twitch/raid/history` und `/twitch/raid/analytics` sind aktuell effektiv Admin-/Legacy-Surfaces und kein sauberer Streamer-Self-Service.
- Die aelteren Affiliate-Eintraege in [../API.md](../API.md) sind nicht mehr vollstaendig aktuell. Fuer Streamer gilt in erster Linie die Doku in diesem Ordner.
- Auch einzelne Route-Angaben in [../API.md](../API.md) driften vom Code ab, zum Beispiel bei `internal-home`, Affiliate und den neueren React-Seiten.
