# Öffentliche Partnerprofile

Stand: 18. September 2026. Branch: `feat/partner-profile-calendar`.

## Nutzung

Im Twitch-Dashboard unter **Verwaltung → Mein Profil** öffnen:
`/twitch/verwaltung#profil`. Zusätzlich gibt es im Analysebereich den Tab
`profile` bzw. den Alias `profil`. Für das Profil ist kein Analyse-Abo nötig.

Überschrift, Über-mich-Text, optionale Twitch-Profilbildadresse, Akzentfarbe,
Social-Links und empfohlene Partner bearbeiten. Kalender: Monat wählen, Tag
anklicken, Beginn/Ende und Beschreibung eintragen, **Termin übernehmen** und
anschließend **Profil speichern**. Zeiten gelten für `Europe/Berlin`, unabhängig
von der Gerätezeitzone. Nicht existierende oder mehrdeutige Ortszeiten bei der
Zeitumstellung werden nicht stillschweigend verschoben.

Neue Profile sind private Entwürfe. **Profil veröffentlichen** aktivieren und
speichern, um `/streamer/@<twitch_login>` freizugeben. Ausschalten und Speichern
nimmt die Seite wieder offline. Andere aktive und veröffentlichte Profile
erscheinen in der Partnerübersicht der Website; bis zu sechs können im eigenen
Profil unter „Aus meinem Umfeld“ empfohlen werden.

## Inhalt und Grenzen

- Bis zu 120 Zeichen Überschrift und 4000 Zeichen Über mich; Gold, Violett oder
  Petrol als Akzent; optional ein Twitch-CDN-Profilbild, ansonsten Initiale.
- Bis zu zwölf benannte HTTPS-Links und sechs Empfehlungen. Es werden nur
  veröffentlichte, aktive Partnerempfehlungen verlinkt.
- Bis zu 200 selbst gepflegte Einzeltermine mit Titel, Beschreibung, Beginn und
  Ende. Höchstens 48 Stunden pro Termin. Keine Serienautomatik oder externe
  Kalender-Synchronisierung in dieser Version.
- Öffentlicher Monatskalender zwischen 2000 und 2100, getrennte Darstellung von
  geplanten Terminen und tatsächlich abgeschlossenen Streams. Vergangene Monate
  sind nicht auf das letzte Jahr begrenzt. Zusätzlich 90-Tage-Wochenraster aus
  tatsächlich erfassten Streams, mit vorhandener Gewichtung neuerer Daten.
- Historie folgt `twitch_stream_sessions.twitch_user_id`, nicht einem wieder
  vergebenen Twitch-Namen. Alte Zeilen ohne eindeutige Kanal-ID werden nicht
  öffentlich zugerechnet. Erfassungslücken, offene Streams und offensichtlich
  defekte Zeiträume werden nicht als sicherer Sendeplan ausgegeben.
- Je Historienfenster höchstens 2001 Zeilen mit sichtbarem Begrenzungshinweis;
  Partnerverzeichnis höchstens 500 veröffentlichte Profile. Diese Grenzen
  begrenzen Abfragen, löschen aber keine Daten.
- Keine öffentlichen Zuschauerzahlen, Chatter-Listen, internen Discord-IDs,
  Token oder privaten Analysen. Die öffentliche Seite ist servergerendert und
  benötigt kein JavaScript. Benutzertexte werden ausschließlich als Text
  ausgegeben, keine HTML-/Script-Einbettung.

## Lebenszyklus und Sicherheit

Jeder öffentliche Profilabruf und jede Verzeichnisabfrage prüft unmittelbar
`status='active'`, kein `departnered_at`, kein `admin_archived_at`, keinen
`manual_partner_opt_out`, `raid_bot_enabled=1` und keinen
`technical_pause_reason`. Austritt, Archivierung, Bot-Deaktivierung, Bot-Bann
oder technischer Pausezustand liefern keine Profilinhalte mehr. Öffentliche
Profilantworten verwenden `Cache-Control: no-store, max-age=0`; verborgene
Profile antworten mit echtem HTTP 404 und `noindex, nofollow`.

Die Profilzeile bleibt erhalten. Bei erneuter Aktivierung wird ein zuvor
veröffentlichtes Profil wieder sichtbar. Es gibt keine automatische Löschung
von Texten oder Terminen. Ein Profil, dessen Veröffentlichung abgeschaltet
wurde, bleibt auch nach erneuter Aktivierung privat.

Die Bearbeitung bindet sich an die angemeldete Twitch-ID. Fremde Kanalparameter
werden für Partner mit 403 abgelehnt; bestehende Admin-Scope-Regeln bleiben
bestehen. Schreibzugriffe verwenden die vorhandene CSRF-Middleware und ein
atomares Versionsvergleichsverfahren. Gleichzeitige Änderungen überschreiben
sich nicht unbemerkt: der zweite veraltete Stand erhält 409, der Editor behält
seine Eingaben. Profil und alle Termine werden gemeinsam gespeichert.

Tabelle: `twitch_partner_profiles`, neue Migration
`20260918210000_partner_profiles.sql`. Das Dashboard erhält SELECT/INSERT/UPDATE,
der Bot nur SELECT. Die vorhandene Runtime-Rechtematrix wurde entsprechend
angepasst. Die bestehenden Sammler werden nicht verändert.

## HTTP-Vertrag

| Pfad | Zugriff | Zweck |
| --- | --- | --- |
| `GET /streamer/@<login>?month=YYYY-MM` | Öffentlich, aktive veröffentlichte Partner | HTML-Profil und Kalender |
| `GET /twitch/profile-assets/profile.css` | Öffentlich | Profil-Stylesheet |
| `GET /twitch/api/v2/public/partner-profiles` | Öffentlich | Nur Login und Überschrift sichtbarer Profile |
| `GET /twitch/api/v2/streamer/profile` | Angemeldeter Partner / Admin-Scope | Eigener gespeicherter Stand |
| `PUT /twitch/api/v2/streamer/profile` | Aktiver Partner / Admin-Scope + CSRF | Atomare Speicherung mit `revision` |

PUT sendet `{ revision, published, profile }`. Die vollständigen Feldtypen liegen
in `bot/dashboard_v2/src/api/partnerProfile.ts`. `streamer` ist nur für den
bestehenden Admin-Scope frei wählbar. Maximale Requestgröße: 512 KiB.

## Deployment

**Implementiert und lokal getestet, noch nicht auf Produktion umgeschaltet.**

1. Feature-Branch über den normalen Releaseprozess integrieren. Die neue
   Migration mit dem bestehenden Migrationsprozess anwenden, bevor die neuen
   Endpunkte genutzt werden; anschließend die Runtime-Rechte aktualisieren.
2. `tb-dashboard` sowie `bot/dashboard_v2` und `website` aus demselben
   Release-Stand bauen und mit dem vorhandenen Release-/Rollbackverfahren
   ausliefern. In diesem Arbeitsstand wurden keine Produktionsdienste neu
   gestartet und keine Produktionsdaten geändert.
3. `ops/caddy/partner-profiles.caddy` im Site-Block von
   `deutsche-deadlock-community.de` importieren, **auf derselben Ebene wie** der
   bestehende `handle /streamer*`-Block. Die spezifischeren Profilpfade müssen
   vor dessen SPA-Fallback greifen. Auch `/twitch/profile-assets/*` braucht die
   enthaltene eigene Weiterleitung, da der vorhandene Twitch-Matcher Pfade
   aufzählt und kein allgemeines `/twitch/*` ist. Konfiguration vor dem Reload
   mit `caddy validate` prüfen. Upstream-Status, CSP und `no-store` nicht
   überschreiben; eventuelle CDN-Regeln dürfen diese Pfade nicht cachen.
4. Mit einem Testpartner Entwurf speichern, veröffentlichten Stand im privaten
   Browserfenster prüfen und einen Termin anlegen/ändern/entfernen. Zweiter
   Partner darf das Profil nicht bearbeiten. Bot-Deaktivierung muss beim
   nächsten Abruf 404 liefern; Verzeichnis und Empfehlungen dürfen nicht mehr
   auf das Profil verweisen. Ein bereits geöffnetes Verzeichnis aktualisiert
   sich spätestens beim nächsten sichtbaren 60-Sekunden-Refresh.

Rollback: vorheriges Release und vorherige Caddy-Konfiguration wiederherstellen.
Die additive Profiltabelle kann stehen bleiben; keine Inhalte löschen. Für eine
gezielte Abschaltung vor einem Rollback kann der Betreiber die spezifischen
öffentlichen Profilpfade vorübergehend mit 404 beantworten lassen.

## Prüfungen

- Rust: sechs Profiltests mit isoliertem PostgreSQL, einschließlich Opt-in,
  sieben Austritts-/Deaktivierungsvarianten, Namenswechsel, gleichzeitiger
  Speicherung, XSS, Besitzerbindung, CSRF und Zeitumstellung erfolgreich.
- Rust: 20 relevante Community-/Zeitfenster-Regressionstests erfolgreich.
- `cargo clippy -p tb-dashboard-api --lib`: erfolgreich, keine Diagnose in den
  neuen Profilmodulen. Bestehende Warnungen außerhalb dieser Änderung bleiben.
- Dashboard: 28 gezielte Profil-/Verwaltungs-/Tab-/Community-Tests erfolgreich;
  TypeScript- und Produktionsbuild erfolgreich.
- Website: vollständige Suite mit 48 Tests und Produktionsbuild erfolgreich.
- Caddy: `node --test ops/tests/partner_profiles_caddy.test.mjs` erfolgreich;
  prüft tatsächliche Adapter-Reihenfolge für Profilpfad und Stylesheet ohne
  Listener, Zertifikatsanforderung oder Produktionsänderung.
- Vollständige Dashboard-Suite: 367 von 374 Tests erfolgreich. Sieben Fehler
  betreffen unveränderte Bereiche: drei Farb-/Kontrastprüfungen, drei
  Social-Media-Vertragsprüfungen und eine OBS-Hilfe-Prüfung. Keine dieser sieben
  Fehlermeldungen nennt ein neues Profilmodul. Die Suite wird nicht als
  vollständig grün ausgewiesen.

Gezielter roter Gegenbeweis für den Verwaltungszugang: der neue Test erwartete
`#profil`, bekam vor der Verdrahtung `konto` und schlug fehl. Nach Einbau des
freien Verwaltungstabs und Anpassung der Tab-Vertragsprüfung ist er grün.
Eine visuelle Browser-Abnahme der öffentlichen Profilseite und der echte
Produktions-Smoke-Test sind noch nicht erfolgt.
