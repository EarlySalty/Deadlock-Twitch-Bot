# Zusammen spielen & streamen

## Umfang und Einstieg

Neuer kostenloser Tab **Zusammen spielen** im Analyse-Dashboard. Direkte Tab-Auswahl nach dem Rollout: `/analyse?tab=community`; auch `tab=lobbys` und `tab=zusammenspielen` funktionieren. Dies ist eine Implementierung in getrennten Feature-Worktrees, keine bereits erfolgte Live-Freischaltung.

Die Oberfläche verbindet einen visuellen Wochenvergleich, nachvollziehbare Streamer-Vorschläge und einen personenbezogen berechtigungsgeprüften Discord-Lobbyüberblick. Der Streamer-Treffpunkt ist fest auf Guild `1289721245281292288`, VC `1326984426906714236` begrenzt. Der Link öffnet Discord; er betritt keinen Sprachkanal automatisch und überträgt kein Audio in den Browser.

Die Hinweise zu Streamer-Rechten beziehen sich auf Verschieben und Zugriffssperren in den vorgesehenen Sprachkanälen, nicht auf serverweite Banns. Bei Problemen verweist der Tab auf Mod-Team/Serverleitung. Es werden keine Rollen oder Berechtigungen verändert und keine Nachrichten verschickt.

## Daten und Matching

- Nur aktive Partner, ohne Archivierung, Entpartnerung oder vorhandenen manuellen Opt-out. Keine Selbstvorschläge. Partner können nur ihr eigenes Matching abfragen; Admins dürfen einen anderen Partner betrachten, erhalten aber weiterhin nur die Discord-Rechte ihres **eigenen** Accounts.
- Zeitraum 7–90 Tage; maximal 180 abgeschlossene Sessions je Kanal, maximal 500 Partner. Offene, zukünftige oder mehr als 48 Stunden lange Sessions werden nicht als abgeschlossene Historie verwendet. Doppelte/überlappende Intervalle erhöhen die Häufigkeit nicht.
- 336 halbstündige Wochen-Slots in `Europe/Berlin`, mit tatsächlicher Zeitumrechnung einschließlich Sommerzeit. Neuere Streams zählen mit 21 Tagen Halbwertszeit stärker. Eine separate Intervallberechnung liefert die tatsächlich gleichzeitig gestreamten Minuten, nicht vermeintlich gemeinsam gespielte Matches.
- Mindestens drei unterschiedliche Sessions je Person für einen Score. Zeitprofil: bis 65 Punkte; gemeinsame Spiele: 15; ähnliche bestätigte Steam-Ränge: bis 15; gleicher erkannter Modus: 5 aus Spielhistorie, bei Titelhinweisen höchstens 2. Punkte sind **keine Wahrscheinlichkeit**. Fehlende Angaben ergeben keine Bonuspunkte.
- Ab mehr als zwei Rangstufen Unterschied oder bei unterschiedlichen erkannten Modi wird ein Konflikt gezeigt. Unbekannte Ränge/Modi bleiben ausdrücklich unbekannt. Zeitlich passende Kandidaten werden auf maximal 16 vorausgewählt, bevor die begrenzte Steam-Anreicherung erfolgt.
- Steam-Zuordnung über die bereits gespeicherte Twitch→Discord-Identität und einen bestätigten Steam-Freundschaftslink. Ränge brauchen den tatsächlichen Quellzeitpunkt `rank_updated_at` und dürfen höchstens 14 Tage alt sein. Ohne den kleinen Steam-API-Patch erscheinen Ränge vorsichtshalber als unbekannt.
- Explizite Steam-Spielmodi aus den letzten 14 Tagen: mindestens drei bekannte Matches und mindestens 60 % Übereinstimmung innerhalb der erkannten Modi. `not_scored` ist **kein** Spielmodus. Ein frischer aktueller Deadlock-Titel kann eine andere heutige Spielabsicht signalisieren; Titel werden als unbestätigte Hinweise ausgewiesen.
- Ein Twitch-Liveflag benötigt einen höchstens fünf Minuten alten Zeitstempel. Gleichzeitig im selben Spiel live zu sein ist keine Zusage zum Mitspielen.

## Discord-Lobbys

Der Master liefert nur aggregierte Informationen zu belegten Community-Sprachkanälen. Keine Mitgliedernamen oder Mitglieder-IDs, kein Audio, keine Steam-IDs. Kategorien entsprechen den vorhandenen Casual-/Ranked-/Street-Brawl-Kategorien. Staging- und Chill-Kanäle sind ausgeschlossen. Der Streamer-VC ist zusätzlich als Treffpunkt enthalten.

Die tatsächlichen effektiven Discord-Rechte des Betrachters müssen `VIEW_CHANNEL` und `CONNECT` erlauben. Kanal-Overwrites, Zugriffssperren und Timeouts werden berücksichtigt; zusätzlich gelten die bestehenden Broker-Allowlisten. Ein unbekanntes Discord-Konto oder unbestätigte Mitgliedschaft führt nicht zu einem öffentlichen Fallback-Verzeichnis.

Die neue, separate Lobby-Gateway-Bereitschaft wird beim Disconnect des zuständigen Shards geschlossen und erst nach Ready/Resume dieses Shards wieder geöffnet. Sie ändert nicht die bisherige globale Bot-Bereitschaft. So wird ein eingefrorener Cache beim Reconnect nicht als gerade frisch abgerufen beworben.

Mindestens ein bekannter menschlicher Teilnehmer macht eine Lobby aktiv. Bei der Kapazität zählen auch Bots als belegte Plätze. Rangmittelwerte beruhen auf erkannten Discord-Rollen und sind ausdrücklich nur eine grobe Einschätzung; der Modus ist eine Kanalkategorie, kein Nachweis eines laufenden Matches. Freie VC-Plätze sind keine garantierten Ingame-Plätze.

Die Anzeige fragt alle 30 Sekunden nach. Ein Snapshot älter als 60 Sekunden, ein Quellfehler oder eine fehlende Rechtebestätigung deaktiviert Lobby-Einstiege. Discord überprüft beim Öffnen weiterhin die aktuellen Rechte. Der feste Streamer-VC-Link bleibt als Treffpunkt verfügbar, ohne eine aktive Belegung zu behaupten.

## Schnittstellen und Laufzeit

`GET /twitch/api/v2/community?streamer=<login>&days=56`

Antwort: eigenes Profil und Wochenprofil, begründete Vorschläge, frische Lobby-Aggregate oder ein expliziter Status (`link_required`, `membership_unconfirmed`, `stale`, `unavailable`). `Cache-Control: private, no-store`. Nicht angemeldet: 401; fremder Kanal als Partner: 403; inaktiver/unbekannter Partner: 404; Historie nicht verfügbar: 503. Fremde Query-Felder werden abgelehnt. Eigenes Rate-Limit: 30 Requests pro Minute.

Der Twitch-Server ruft intern `POST /internal/master/v1/discord/community-lobbies` auf. Discord-ID ausschließlich aus der **Betrachteridentität im Backend**, nie aus Browserparametern. Der Broker prüft das vorhandene interne Token, Loopback-/Netzwerkregeln und Allowlisten.

Verwendete bestehende Konfiguration: `STEAM_BOT_RANK_URL` (Standard localhost:8783/rank); `MASTER_BROKER_BASE_URL` oder `MASTER_BROKER_HOST`/`MASTER_BROKER_PORT` (Standard localhost:8770); internes Token aus `MASTER_BROKER_TOKEN`, `MAIN_BOT_INTERNAL_TOKEN` oder `TWITCH_INTERNAL_API_TOKEN`. Keine Werte oder Zugangsdaten im Frontend.

HTTP-Timeout 2,2 Sekunden je Quelle, höchstens vier gleichzeitige Profilanreicherungen, absolut sechs Sekunden Anreicherungsbudget. Erfolgreiche Steam-Profile fünf Minuten Cache, unvollständige eine Minute; begrenzter Single-Flight-Cache pro Identität. Unfertige Anreicherungen werden abgebrochen, nicht im Hintergrund fortgesetzt. Datenbankabfragen sind ebenfalls zeitlich begrenzt. Demo-/Preview-Modus startet keine persönlichen Live-Abfragen.

## Repositoryübergreifender Rollout

Benötigt werden die Änderungen in **Deadlock-Steam-Bot** (additives `rank_updated_at`), **Deadlock-Bots** (geschütztes Lobby-Verzeichnis und Gateway-Bereitschaft) und **Deadlock-Twitch-Bot** (API/Tab). Keine neue Datenbankmigration erforderlich. Erst die Quellschnittstellen, dann Twitch-API und Frontend ausrollen. Bestehende Secrets, Discord-Zuordnung, Gateway-Intents und Broker-Allowlisten müssen zur Installation passen.

Nicht blind eine der bereits anderweitig veränderten Haupt-Worktrees deployen. Die Implementierung entstand getrennt in `feature/community-coplay-dashboard-20260918`, `feature/community-lobby-directory-20260918` und `feature/community-rank-freshness-20260918`.

Live-Abnahme nach einem bewussten Rollout: eigener Partner-Login, bestätigter Discord-/Steam-Link, belegter zugänglicher VC, voller VC, gesperrter VC, fehlende Verbindung, Discord-Reconnect und veralteter Snapshot. Ein Live-Rollout oder Test mit echten Teilnehmern wurde hier nicht vorgenommen.

## Tests

Twitch: `cargo test -p tb-dashboard-api handlers::community --lib` — 16 Tests inklusive isoliertem PostgreSQL, Authentifizierung, Scope, Historie, Sommerzeit, Quellenalter und Kapazität.

Discord: `cargo test -p dl-broker -p dl-discord community --lib` — 5 Tests einschließlich Token, Mitgliedschaft, Allowlist, Rechtefilter, Rollennamen und Gateway-Reconnect.

Frontend: `node --import tsx --test tests/community.test.tsx` — 11 Tests. `npm run build` erstellt den Produktionsbuild. Die Gesamtsuite hat dieselben sieben Fehler wie der unveränderte Ausgangscommit `59ae37db` (Palette, Social-Media-Verträge und OBS-Hilfe): Baseline 345/352, Feature 356/363. Kein neuer Fehler in diesem Vergleich.

Browser-Abnahme mit synthetischen Daten: Vite auf Loopback-Port 4199 starten und `npm exec --yes --package=playwright -- node tests/community-browser.cjs` ausführen. Geprüft werden Desktop und 390px-Mobilbreite ohne Seitenüberlauf, Filter, Vergleichsauswahl, volle VCs, exakter Streamer-Link, abgelaufene Snapshots und JavaScript-Laufzeitfehler. Die Fixture unter `tests/community-visual.html` ist kein Produktions-Einstiegspunkt und enthält ausschließlich gekennzeichnete Testdaten.

Steam-Web kompiliert. Sein vollständiger bestehender Testlauf benötigt `CENTRAL_TEST_DSN`: ohne diese Testdatenbank-Konfiguration bestehen 22 Tests und 35 datenbankabhängige Tests brechen an der Testharness-Voraussetzung ab. Es wurde absichtlich keine Produktions-Datenbank dafür verwendet. Der additive Timestamp-Vertrag besteht zusätzlich als eigener Test ohne Datenbank oder externe Dienste. Insgesamt bestehen 33 gezielte Feature-Tests (16 Twitch, 11 Frontend, 5 Discord, 1 Steam) sowie die Browser-Abnahme.
