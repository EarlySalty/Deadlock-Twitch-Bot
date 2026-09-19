# Live-Abnahme am 18. September 2026

## Ausgeliefert

- Twitch-Release: `76d58382a775de8c01c893577263411ef7468b38`.
- Der Stand wurde per Fast-forward nach `origin/main` integriert und gepusht.
- Caddy: `e86ae6e` auf `origin/master` in `EarlySalty/caddy-config`.
- `/streamer/login` ist die öffentliche Profiladresse, ohne @. Die vorhandenen
  Website-Seiten bleiben ausgenommen. Alte @-Adressen antworten mit 404.
- Dashboard-Neustart: 20:32:05 Uhr Europe/Berlin; Bot-Neustart: 20:32:06 Uhr.
- Beide Dienste aktiv, `NRestarts=0`, `ExecMainStatus=0`. Die tatsächlichen
  `/proc/<pid>/exe`-Pfade zeigen auf das oben genannte Release, nicht nur der
  `current`-Symlink.

Der Release-Bau erfolgte in einem eigenständigen, sauberen Clone. Alle vier
Binaries (Bot, Dashboard, Stream-Audit, Category-Collector) tragen die geprüfte
Commit-ID in `.twitch_build`. Dashboard, Admin-Oberfläche und Website wurden
nach `npm ci` aus den unveränderten Frontend-Dateien dieses Releases gebaut.
Neu gestartet wurden nur die ausdrücklich benötigten Dienste Bot und Dashboard.

## Migration und Konfiguration

`deploy-twitch-release` aktivierte das Release, führte
`deadlock-twitch-migrate.service` aus und startete danach die beiden Dienste.
Migration `20260918210000_partner_profiles` ist mit `success=true` registriert.
Dashboard: SELECT/INSERT/UPDATE vorhanden. Bot: SELECT vorhanden, UPDATE nicht.
Der One-shot-Migrationsdienst endete erfolgreich mit Status 0.

Die installierten Release-Werkzeuge und Profil-Caddy-Fragmente wurden gegen die
versionierten Dateien verglichen. In der Live-Caddydatei wurden ausschließlich
die beiden Profil-Imports ergänzt. Abweichende bereits vorhandene Live-Änderungen
(insbesondere Connect- und T3-Pfade) blieben erhalten; der schmutzige kanonische
Caddy-Checkout wurde nicht bearbeitet. Die vollständige installierte Konfiguration
bestand `sudo -n caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile`.
Anschließend erfolgreicher `systemctl reload caddy`, kein Caddy-Neustart.
Sicherung: `/etc/caddy/Caddyfile.bak-partner-profiles-76d58382`.

## Prüfungen

- Neuer passiver Zugangscheck: rote Gegenprobe 0/1, danach grün.
- Sieben Profil-/Zugangstests erfolgreich, einschließlich isoliertem PostgreSQL,
  Besitzerschutz, CSRF, sieben Deaktivierungsvarianten, Namenswechsel und XSS.
- 28 gezielte Dashboard-/Community-/Verwaltungstests erfolgreich.
- Alle 48 Website-Tests erfolgreich.
- Clippy erfolgreich; bestehende Warnungen außerhalb der Änderung.
- ESLint für die drei neuen Profilmodule erfolgreich.
- Isolierter echter Caddy-Routingtest erfolgreich.
- Unabhängige Reviews nach Korrektur des passiven GET-Zugangs: ALLOW für
  Twitch-Änderung und Caddy-Konfiguration. Der zusätzliche vollständige
  Middleware-Routertest für passive Sitzungen bleibt eine nicht blockierende
  Testverbesserung; Methode und Pfad werden bereits separat regressionsgeprüft.

Chrome-Headless mit dem gebauten Produktionsbundle, ausschließlich gegen einen
lokalen Beispieldaten-Server: freier Verwaltungstab, Überschrift bearbeiten,
Kalendertermin übernehmen, veröffentlichen und mit CSRF speichern, Link ohne @,
Ansicht mit 390 Pixeln ohne Dokumentüberlauf, erneutes Laden mit gespeichertem
Stand und keine Browserfehler. Kein Produktivprofil und keine echte Sitzung
wurden dafür verändert. Browser-Skript lokal:
`/home/nathanael/.cache/partner-profile-browser-smoke.mjs`.

Die vollständige Dashboard-Suite hatte im vorherigen Lauf 367/374 erfolgreiche
Tests. Ihre sieben bekannten Fehler in Farb-/Social-Media-/OBS-Bereichen wurden
hier nicht repariert. Keine Behauptung eines vollständig grünen Gesamt-Repos.

## Live-HTTP und Bereitschaft

Alle folgenden Aufrufe gingen über die echte HTTPS-Domain
`deutsche-deadlock-community.de`, ohne Session-Cookies:

| Pfad | Ergebnis |
| --- | --- |
| `/streamer/` | 200, bestehende Landing |
| `/streamer/commands` | 200, bestehende Befehlsseite |
| `/streamer/vergleich/` | 200, bestehender Vergleich |
| `/twitch/profile-assets/profile.css` | 200, `text/css` |
| `/streamer/earlysalty` | 404, noch nicht veröffentlicht |
| `/streamer/earlysalty/?month=2024-03` | 404, gleiche Sichtbarkeitsprüfung |
| `/streamer/@earlysalty` | 404, alter Platzhalterpfad |
| `/twitch/api/v2/public/partner-profiles` | 200, leeres Verzeichnis |
| `/twitch/api/v2/streamer/profile` | 401, private Daten benötigen Anmeldung |

Verborgene Profile: `Cache-Control: no-store, max-age=0`,
`X-Robots-Tag: noindex, nofollow`, CSP vorhanden. Die gemessenen einzelnen
Antwortzeiten lagen bei 4 bis 67 ms vom Server aus; kein allgemeiner Benchmark.
`/healthz` meldete alive, `/readyz` ready, Datenbank und interne Bot-API ok,
keine unterschiedlichen Datenbank-Fingerprints.

Die Profiltabelle hatte bei der Abnahme null Zeilen. Es wurde bewusst kein
Partnerprofil automatisch veröffentlicht. Einstieg für echte Partner:
`/twitch/verwaltung#profil`, danach „Profil veröffentlichen“ und speichern.
Ein öffentliches echtes Partnerprofil mit Inhalt wurde daher nicht künstlich
als Produktions-End-to-End-Nachweis angelegt; positive Inhalts- und Schreibfälle
wurden isoliert getestet.

Beim Bot-Neustart meldete systemd einmal einen zurückgebliebenen yt-dlp-Prozess
vom vorherigen Lauf. Dieser Prozess war bei der anschließenden Prüfung bereits
beendet; kein fremder Download wurde manuell beendet. Keine Neustartschleife.

## Live-Abnahme des Profil-Redesigns am 19. September 2026

- Bei dieser Zwischenabnahme lief Release `bd5b888ff29b91d8b2ebfa0ef6f7410a525eae2c`; `/opt/deadlock/twitch/current` zeigte auf dieses Release, der Dashboard-Prozess lief als PID `3202277`, und der Binäranker `Streamplan ansehen` war im ausgelieferten `tb-dashboard` enthalten. `https://deutsche-deadlock-community.de/streamer/earlysalty?month=2026-09` antwortete mit HTTP 200, `text/html; charset=utf-8`, `no-store` und enthielt den Streamplan-CTA, den Twitch-CDN-Avatar sowie den komprimierten Kalendertag `+11 weitere`.
- Chrome Headless prüfte das öffentliche Profil bei 1440×1000 und 390×844 sowie den Profil-Editor aus demselben Produktionsbundle. Das echte Twitch-Bild lädt, Speichern liegt im sichtbaren Bereich und ist am Klickpunkt nicht überdeckt, die Live-Vorschau ist sichtbar, ein manuelles Profilbildfeld fehlt, und die Seite erzeugt keine horizontale Dokumentüberbreite; im öffentlichen Kalender sind pro Tag höchstens drei Einträge sichtbar und die Heatmap umfasst 336 Halb-Stunden-Zellen.
- `/twitch/verwaltung` verlangt ohne Sitzung erwartungsgemäß Twitch-Anmeldung und antwortet mit 303. Daher wurde der Editor ohne Produktionsschreibzugriff gegen eine kontrollierte lokale API-Sitzung mit dem aktuell ausgelieferten Bundle geprüft; die öffentliche Seite `earlysalty` wurde dagegen direkt live geprüft. Das dominante Farbbild der erzeugten Viewport-Screenshots ist Anthrazit mit Gold und warmen Neutralfarben, ohne fremde blau-violette Flächensprache.

## Abschlussabnahme mit Community-Logo am 19. September 2026

- Release `08c3afd9f71ab84a23fd2d9d59e82976c3400ff0` ist aktiv. Dashboard-PID wechselte von `3880304` auf `87240`, Bot-PID von `3202369` auf `87327`; `NRestarts=0` und `ExecMainStatus=0` bei beiden Diensten. Die `/proc/<pid>/exe`-Pfade zeigen auf die Binaries dieses Releases ohne `(deleted)`. Seit dem Deploy enthalten die Journale beider Dienste keine Einträge mit Priorität `err`.
- Der neue Binäranker `/streamer/brand/deadlock-d-logo.png` ist im ausgelieferten Dashboard enthalten. Das echte Profil `https://deutsche-deadlock-community.de/streamer/earlysalty?month=2026-09` antwortet mit HTTP 200 und erlaubt in der CSP neben dem Twitch-CDN auch Bilder von der eigenen Domain. Das Community-Logo und das Twitch-Profilbild laden im Browser.
- Chrome Headless bestätigte 1440×1000 und 390×844 Pixel ohne Dokument- oder Header-Overflow. Das Community-Logo misst 38 Pixel, der Kalender zeigt höchstens drei Einträge pro Tag plus `+11 weitere`, und die Heatmap enthält 336 Halb-Stunden-Zellen. Der sichtbare Stil bleibt Anthrazit mit Gold als Primärfarbe.
