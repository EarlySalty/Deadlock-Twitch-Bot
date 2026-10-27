# Auftrag: Social-Media-Dashboard, Go-Live-Prüfung

Auftraggeber: Haupt-Orchestrator (Claude-Hauptsession, Thread d3a1741e-82bc-4a48-865b-2845c663dca7)
Rolle: Blatt-Worker, nur lesen und prüfen. Kein Code ändern, nichts committen außer dem Bericht, keine Threads starten.
Repo: Deadlock-Twitch-Bot. Eigener detached Worktree `~/.worktrees/tb-social-golive` von origin/main.

## Ziel

Der Nutzer will das Social-Media-Dashboard bald für Partner freischalten. Die TikTok-App ist inzwischen von TikTok zugelassen. Prüfe jede Funktion des Dashboards Ende zu Ende gegen Code, Prod-DB (nur lesend) und Live-Laufzeit und urteile je Funktion: funktioniert / teilweise / kaputt / nur UI ohne Backend.

Umfang mindestens:
- Verbinden und Trennen je Plattform (TikTok, YouTube, Instagram und weitere im Code): OAuth-Weg, Speicherung, Refresh, Trennen. Hinweis: Ablaufanzeige und Neu-Verbinden-DM werden parallel im Auftrag `2026-10-07-social-media-token-ablauf` gefixt, hier nur vermerken.
- Clip-Pipeline: Clip-Erfassung, Download, Aufbereitung (Facecam, Untertitel, Vorschau), Freigabe, Zeitplan und Reihenfolge, Upload je Plattform, Status-Rückmeldung (TikTok-Inbox/Publish-Status), Retry und Backoff.
- Analytics/Insights je Plattform.
- Rechte: Partner sieht nur eigene Daten (Twitch-User-ID aus Session), Admin-Pfade serverseitig geprüft, Caddy-Allowlist und CSP für die Routen.
- Leer- und Fehlerzustände, Texte (keine Versprechen, die der Code nicht hält), Sammelverbindung vs. eigene Verbindung.
- Plattform-Auflagen für den Live-Betrieb: TikTok Content Posting API (Direct Post vs. Inbox, Pflicht-UI wie Creator-Info, Privatsphäre-Auswahl, Hinweistexte laut TikTok-Richtlinien), YouTube-Verifizierung und Quota, Instagram-App-Review.
- Laufende Worker: ob sie im tb-bot gespawnt sind und im Journal sauber laufen (nur lesen).

Sichtprüfung im Browser, wo möglich (Skill `impeccable` nicht nötig, nur Funktion).

## Bericht

`.tasks/2026-10-07-social-media-golive-check/BERICHT.md`: Tabelle Funktion / Urteil / Beleg (Datei:Zeile, DB-Abfrage oder Log) / Blocker für Go-Live ja/nein / Fixvorschlag mit Größe. Am Ende eine priorisierte Liste "muss vor Go-Live", "kann nach Go-Live". Bericht in diesem Worktree auf einen Branch `docs/social-golive-check` committen und pushen (nicht nach main), dann an den Haupt-Orchestrator melden und `t3-thread.py settle --selbst`.

Secrets nie im Klartext lesen. Echte Streamer-Zugänge nicht trennen oder entwerten. Nutzer- und Community-Daten nicht an externe Dienste geben.
