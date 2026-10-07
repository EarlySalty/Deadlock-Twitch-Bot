# Twitch-Pfade seit 7. Oktober 2026

Neue Twitch-Bot-Seiten gehören ausschließlich unter `/twitch/<name>`. Die Analyse liegt unter `/twitch/analyse`, der Social-Media-Manager unter `/twitch/social-media`. Beide verwenden weiterhin dasselbe Dashboard-Bundle. Der Manager lädt Assets unter `/twitch/dashboard-v2/assets/`, die Analyse unter `/twitch/analyse/assets/`.

## Umstellung und Verträglichkeit

| Adresse | Verhalten |
| --- | --- |
| `/twitch/analyse` und Unterpfade | Analyse beziehungsweise deren Dateien; bestehende Zugriffsprüfung |
| `/twitch/social-media` und Unterpfade | Manager beziehungsweise dessen Dateien; bestehende Zugriffsprüfung |
| `/analyse`, `/social-media`, `/social-media-admin` samt Unterpfaden | 308 unmittelbar auf den entsprechenden neuen Pfad; Query und Kodierung bleiben erhalten |
| `/twitch/social-media/api/*` | Neue Adresse der Manager-API |
| `/social-media/api/*` | Vorübergehend derselbe Handler mit denselben Zugriffs-, Größen- und Schreibschutzregeln; keine HTML-Weiterleitung |
| `/social-media/oauth/start/{platform}`, `/social-media/oauth/disconnect/{platform}` | Bestehende offene Tabs bleiben verwendbar; neue Clients nutzen den `/twitch`-Präfix |
| `/social-media/oauth/callback`, `/social-media/oauth/callback/{platform}` | Direkter Callback ohne Pfadweiterleitung |
| `/twitch/social-media/oauth/callback`, `/twitch/social-media/oauth/callback/{platform}` | Zusätzliche Callback-Adressen; bestehende Starts verwenden unverändert die registrierten alten Rücksprünge |
| `/social-media/terms`, `/social-media/privacy`, `/privacy` | Direktes öffentliches HTML für Plattformprüfungen |
| `/twitch/social-media/terms`, `/twitch/social-media/privacy` | Zusätzliche direkte Rechtsseiten |

Die öffentliche Domain ist `https://deutsche-deadlock-community.de`. Auf der Admin-Subdomain bleiben Partnerseiten gesperrt. Navigation, Wiederverbindungsnachrichten und Login-/Logout-Ziele verweisen auf die neuen Adressen. Alte gespeicherte Login-Ziele und Rückmeldungslinks bleiben zulässig und gelangen über die Weiterleitung zur kanonischen Seite.

## Betreiber-Schritte für TikTok und Google

Die Plattformkonsolen wurden nicht verändert. Es ist für diese Auslieferung keine Änderung bestehender Rücksprünge erforderlich.

1. In TikTok for Developers die Website-/Produktadresse auf `https://deutsche-deadlock-community.de/twitch/social-media` aktualisieren. Die bestehenden Rechtsadressen `/social-media/terms` und `/privacy` bleiben erreichbar. Bei einer neuen Einreichung können zusätzlich `/twitch/social-media/terms` und `/twitch/social-media/privacy` angegeben werden.
2. Bei TikTok den vorhandenen Web-Rücksprung `https://deutsche-deadlock-community.de/social-media/oauth/callback/tiktok` beibehalten. Für eine spätere gesonderte Callback-Migration zuerst `https://deutsche-deadlock-community.de/twitch/social-media/oauth/callback/tiktok` zusätzlich registrieren und die Freigabe abwarten. Erst danach darf die Anwendung ihre Authorize- und Austauschadresse gemeinsam umstellen.
3. In Google Auth Platform die App-Startseite auf `https://deutsche-deadlock-community.de/twitch/social-media` aktualisieren; bestehende Nutzungsbedingungen und Datenschutzadressen weiter beibehalten oder die neuen Rechtsaliase zusätzlich verwenden. Domainprüfung und Freigabestatus prüfen.
4. Im betreffenden Google-OAuth-Webclient `https://deutsche-deadlock-community.de/social-media/oauth/callback/youtube` beibehalten. Einen neuen Rücksprung unter `/twitch/social-media/oauth/callback/youtube` gegebenenfalls zusätzlich registrieren, nicht den alten ersetzen. Andere vorhandene Callback-Registrierungen, insbesondere Drive, bleiben unverändert.
5. Nach Änderungen der Produktdarstellung die Plattformprüfung erneut abschließen und je Plattform eine Verbindung im Manager prüfen. Bestehende registrierte Callback-Adressen erst nach einer gesonderten, abgestimmten Umstellung entfernen.

## Zwischengespeicherte alte Analyseweiterleitungen

Vor der Umstellung antworteten `/twitch/analyse` und Unterpfade live mit 301 nach `/analyse`, ohne `Cache-Control` oder Ablaufdatum. Bereits gespeicherte Browserweiterleitungen sind deshalb möglich. Die neuen Weiterleitungen von `/analyse` tragen `Cache-Control: no-store` und `Clear-Site-Data: "cache"`, um den alten HTTP-Cache auf unterstützenden Browsern vor der Rückkehr zu leeren. Cookies und Website-Speicher werden dabei nicht gelöscht. Ein Browser ohne Unterstützung muss bei einer Schleife den Cache dieser Website leeren und die neue Analyseadresse erneut laden. Der Server selbst liefert auf der kanonischen Adresse keine Rückweiterleitung.

## Übrige Wurzelpfade: Bestand, nicht verschoben

Bestandsquelle: produktive Router in `rust/crates/tb-dashboard-api/src/lib.rs` und deren zusammengeführte Teilrouter. Die Proxy-Topologie steht im Caddyfile. Ein Backend-Pfad ist nicht automatisch auf jeder Domain öffentlich erreichbar.

| Pfad beziehungsweise Gruppe | Zweck | Abhängigkeiten und Grund für Beibehaltung |
| --- | --- | --- |
| `/` | Hostabhängiger Dashboard-Einstieg | Auf der Hauptdomain besitzt die Community-Website den Root; auf dem Admin-Host führt der Einstieg ins Admin-Panel. Keine neue Twitch-Seite |
| `/dashboards`, `/dashboads` | Historische Einstiegsaliase | Alte Lesezeichen und Links; bestehende Dashboard-Weiterleitung |
| `/streamer`, `/streamer/{*path}` | Öffentliche Landingpage, Partnerprofile und Website-Dateien | Suchmaschinen, öffentliche Twitch-/Discord-Links, Profil-Slugs, eingebettete Clips und Website-Assets. Caddy liefert überwiegend Website-Dateien direkt und proxied dynamische Profile |
| `/streamer/help`, `/streamer/commands`, `/streamer/faq` | Öffentliche Bot-Hilfe und Befehle | Chat-Befehlslinks und Discord-Hilfe; kein Login notwendig |
| `/website`, `/website/{*path}` | Historische Website-Weiterleitung nach `/streamer` | Alte öffentliche Links und Suchmaschinen |
| `/clips`, `/clips/` | Öffentlicher Clip-Wettbewerb | Community-/Discord-Links, Einbettungen, öffentlicher Wettbewerb |
| `/clips/api/current`, `/clips/api/archive`, `/clips/api/session` | Wettbewerb und Sitzungsstatus lesen | Website-Client und vorhandene Sitzungen |
| `/clips/api/submit`, `/clips/api/vote`, `/clips/api/admin/hide`, `/clips/api/admin/submissions` | Wettbewerbseinreichung, Abstimmung und Verwaltung | Bestehende Dashboard-/Discord-Sitzungen, eigener Schreibschutz und Ratenbegrenzung |
| `/clips/auth/discord/login`, `/clips/auth/discord/callback`, `/clips/auth/logout` | Discord-Anmeldung des Clip-Wettbewerbs | Registrierter Discord-Rücksprung und wettbewerbsspezifische Sitzung |
| `/callback/twitch` | Gemeinsamer Twitch-Rücksprung | Registrierte OAuth-Adresse für Dashboard-/Raid-Flows, Zustandsprüfung und interner Dispatch |
| `/callback/kick`, `/callback/youtube` | Uplink-Plattformverbindungen | Registrierte OAuth-Adressen und vorhandene Plattformzustände |
| `/callback/engagement-sender` | Verbindung des Engagement-Sendekontos | Registrierte Twitch-Adresse und interner Startflow |
| `/callback/discord` | Im Dashboard registrierter gemeinsamer Discord-Rücksprung | Öffentlich routet Caddy diesen gemeinsamen Pfad an den zentralen Besitzer auf Port 8766; Dashboard-Route und zentralen Dispatch nicht unabhängig verschieben |
| `/obs/ws` | Kanalbezogener OBS-WebSocket | Bestehende OBS-Docks, Sitzungsprüfung, WebSocket-Upgrade; kein Seitenpfad |
| `/healthz`, `/readyz`, `/health` | Zustand und Bereitschaft des Backends | Lokale Überwachung und Dienstprüfungen; kein Dashboard |
| `/robots.txt` | Ausschlussregeln für Suchmaschinen und Trainingscrawler | Hostabhängige Caddy-Auslieferung und native Backend-Regeln |
| `/social-media/api/*`, `/social-media/oauth/start/*`, `/social-media/oauth/disconnect/*` | Zeitweilige Manager-Kompatibilität | Offene Tabs und vorhandene Clients; derselbe Handler wie im neuen Namensraum |
| `/social-media/oauth/callback` und `/{platform}` | Geschützte Plattform-Rücksprünge | Exakte registrierte OAuth-Adressen, keine Pfadweiterleitung |
| `/social-media/terms`, `/social-media/privacy`, `/privacy` | Öffentliche Plattform-Rechtstexte | TikTok-/Google-Prüfung und bestehende Registrierungen |
| `/analyse`, `/analyse/{*path}`, `/social-media`, `/social-media/{*path}`, `/social-media-admin`, `/social-media-admin/{*path}` | Nur Weiterleitungsaliase nach dieser Umstellung | Lesezeichen, alte Links, Query-/Unterpfaderhalt mit 308 |

`/demo/*` ist ein Caddy-Publikationspräfix. Die native Demo selbst registriert `/twitch/demo` samt Assets und JSON-Endpunkten. Caddy entfernt bei der Demo den äußeren Präfix; eingebettete Demo-Seiten müssen weiter ohne die gewöhnliche Frame-Sperre funktionieren. Diese Veröffentlichung wird nicht verschoben.

`/terms` steht in der Caddy-Allowlist, hat derzeit aber keine native Registrierung in `tb-dashboard`; es ist kein zusätzlich verschobener Backend-Pfad. Die direkt registrierten Nutzungsbedingungen sind `/social-media/terms` und der neue Alias. `/uplink/dock/*` und `/uplink/v1/*` gehören zum Relay, nicht zu `tb-dashboard`. Andere Community-Routen wie `/coaching`, `/patch`, `/turnier` oder `/yoshi` werden ebenfalls nicht durch diesen Auftrag verschoben.

## Nachprüfung

Vor dem Merge gegen den aktuellen `origin/main` rebasen, damit parallel entwickelte VOD-Archiv-Routen und Manager-Ansichten erhalten bleiben. Alte und neue APIs müssen dieselben JSON-/Auth-Antworten liefern. Plattform-Callbacks und Rechtsseiten dürfen nicht in den Seiten-Wildcardredirect geraten. Canonical-Seiten und ihre Assets müssen durch die Dashboard-CSP und die öffentliche Caddy-Allowlist abgedeckt sein.
