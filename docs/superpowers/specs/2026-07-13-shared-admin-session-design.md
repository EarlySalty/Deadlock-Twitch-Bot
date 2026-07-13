# Gemeinsame Admin-Session für Discord und Twitch

Stand: 2026-07-13

## Problem

Discord- und Twitch-Admin-Dashboard setzen zwar dasselbe Domain-Cookie
`master_dash_session`, der live laufende Rust-Twitch-Dienst synchronisiert seine
Session aber nicht mit dem zentralen Discord-Dashboard. Der jeweils letzte Login
überschreibt deshalb den Cookie mit einer Session, die nur eines der beiden
Backends kennt.

Bei Twitch-Schreibaktionen kommt ein zweiter Cutover-Fehler hinzu: Der React-Client
erwartet einen CSRF-Token aus einer alten HTML-Seite. Der Rust-Dienst liefert dort
keinen passenden Token, obwohl jede lokale Admin-Session bereits einen
sessiongebundenen CSRF-Token speichert.

## Ziel

- Ein Login öffnet beide Admin-Dashboards mit demselben Cookie.
- Ein Logout in einem Dashboard meldet den Browser aus beiden Dashboards ab.
- Twitch-Schreibaktionen erhalten ihren CSRF-Token direkt aus dem Auth-API-Vertrag.
- Der zentrale Discord-Dienst bleibt die Autorität für gemeinsame Admin-Sessions.

## Nicht-Ziele

- Keine Änderung an Partner-/Twitch-Nutzer-Sessions.
- Kein Umbau der OAuth-Berechtigungsprüfung.
- Keine Caddy-Änderung und keine gemeinsame Datenbanktabelle beider Dienste.

## Architektur

### Twitch-Login

Der Twitch-Dienst erzeugt weiterhin seine lokale Admin-Session mit Fingerprint-
Bindung und CSRF-Token. Bevor er das Cookie ausstellt, importiert er dieselbe
Session-ID synchron über die bestehende interne Twitch-Schnittstelle in den
zentralen Discord-Dienst. Scheitert der Import, wird kein Cookie ausgestellt und
der Login antwortet mit 503.

### Discord-Login im Twitch-Dashboard

Kennt der Twitch-Dienst eine präsentierte `master_dash_session` nicht lokal,
validiert er sie über die bestehende zentrale Schnittstelle. Eine gültige Session
wird als lokaler Spiegel mit derselben ID und eigenem CSRF-Token gespeichert.
Ungültige oder nicht prüfbare Sessions gewähren keinen Zugriff.

Der zentrale Dienst bleibt maßgeblich: zentral ausgestellte beziehungsweise
importierte Sessions werden beim Twitch-Zugriff zentral validiert. Dadurch kann
ein zentraler Logout keinen dauerhaft gültigen Twitch-Spiegel zurücklassen.

### CSRF

Der Twitch-Auth-Status liefert für eine gültige Admin-Session deren lokalen
CSRF-Token. Der React-Client verwendet diesen Token für JSON- und Legacy-Form-
Aktionen und entfernt das HTML-Scraping der alten Announcement-Seite.

Die bestehende Origin-/SameSite-Prüfung und die serverseitige konstante
Token-Prüfung bleiben unverändert.

### Logout

Beide Logout-Wege löschen das Domain-Cookie mit identischer Domain und identischem
Pfad. Der Twitch-Logout entfernt zusätzlich den lokalen Spiegel und widerruft die
zentrale Session über eine kleine interne, token-geschützte Schnittstelle. Der
Discord-Logout entfernt die zentrale Session; ein noch vorhandener Twitch-Spiegel
wird wegen der zentralen Validierung nicht mehr akzeptiert.

## Fehlerverhalten

- Zentrale Session-API nicht erreichbar: fail-closed; kein neuer Login und keine
  unbekannte Session wird akzeptiert.
- Zentraler Import fehlgeschlagen: 503 ohne Cookie.
- CSRF-Token fehlt oder stimmt nicht: bestehende 403-/Fehlerantwort bleibt.
- Mehrere gleichnamige Alt-Cookies: weiterhin alle Kandidaten prüfen; ein
  ungültiger Kandidat darf einen gültigen nicht verdecken.

## Tests

1. Eine zentral erzeugte Session wird vom Twitch-Auth-Fluss akzeptiert und lokal
   mit CSRF gespiegelt.
2. Ein Twitch-Login importiert die Session zentral, bevor das Cookie ausgegeben
   wird; Importfehler ergibt 503 ohne Cookie.
3. Der Auth-Status liefert den gespeicherten Admin-CSRF-Token.
4. Der React-Client benötigt kein Legacy-HTML mehr für Schreibaktionen.
5. Logout löscht das gemeinsame Cookie und widerruft die zentrale Session.
6. Bestehende Auth-, CSRF-, Fingerprint- und Partner-Session-Tests bleiben grün.

## Rollout und Beweis

Nach grünen Tests werden beide Rust-Workspaces gebaut, die betroffenen User-
Services neu gestartet und PID, Binary-Pfad sowie fehlerfreies Journal geprüft.
Der Live-Beweis umfasst anschließend Login in einem Dashboard, Zugriff auf beide,
eine Twitch-Schreibaktion ohne CSRF-Fehler sowie Logout mit anschließendem 401/
Login-Redirect in beiden Dashboards.
