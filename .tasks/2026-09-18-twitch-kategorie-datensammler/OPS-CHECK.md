# Betriebsprüfung vor Deployment

Read-only-Befunde am 18.09.2026 zwischen 11:18 und 11:25 Uhr Europe/Berlin.

- Livebaum: /opt/deadlock/twitch/releases/44cc2aa90e1d1b59269533b5d50b643e56bfbba8 über /opt/deadlock/twitch/current.
- Systemdienste: deadlock-twitch-bot-rust.service (twitchbot), deadlock-twitch-dashboard-rust.service (twitchdash), Port 8769 für Dashboard.
- Bestehender Deploy-Wrapper: ops/systemd/deploy-twitch-release bzw. installiert /usr/local/bin/deploy-twitch-release. Er verschiebt den gebauten Checkout, installiert das Release, führt deadlock-twitch-migrate.service als Migrationsschritt aus und startet explizit wählbare Bestandsdienste. Er kennt bisher KEIN Collector-Ziel. Feature-Worktree nicht als ungeprüften Release übergeben.
- Caddy hosts/v50671/Caddyfile:207 @dashboard_paths deckt /twitch/dashboard-v2/* und /twitch/analyse/* ab. :623 @public_twitch deckt /twitch/api/v2/* sowie bestehende SPA-Pfade ab. Eine neue Admin-API innerhalb /twitch/api/v2/admin/... und ein Tab innerhalb der bisherigen Shell brauchen daher keine neue breite Public-Freigabe. Server-Adminprüfung ist trotzdem zwingend.
- Postgres-Datenbank twitch_analytics: gemessene Gesamtgröße 1601 MB. Noch keine category_*-Tabellen angelegt.
- Letzte 24 Stunden der bestehenden twitch_stats_category: 16932 deutsche Snapshots aus 30 Kanälen, lediglich 2 englische Snapshots und 1 russischer. Das ist keine globale Vollerfassung und keine belastbare globale 24-Stunden-Vergleichsreihe.
- Dateisystem: 1,7 TB gesamt, 680 GB verwendet, 907 GB verfügbar (43 Prozent genutzt). Kein Datenträger-Engpass im aktuellen df-Snapshot, Retention bleibt trotzdem bindend.

Offen für Abnahme: neuer Code gebaut/getestet, eigenständige Credential-/DB-Rolle und Unit eingerichtet, Migration als postgres, Header/Auth-Tests gegen echte Route, anonyme IRC-Aufnahme anhand des tatsächlich neuen Dienstes, Zeilenzähler und Dashboard-Livebeweis. Keine dieser offenen Prüfungen darf als bereits bestanden berichtet werden.
