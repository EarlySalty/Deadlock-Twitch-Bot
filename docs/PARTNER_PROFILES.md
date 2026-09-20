# Öffentliche Partnerprofile

Stand: 20. September 2026.

## Nutzung

Im Twitch-Dashboard unter **Verwaltung → Mein Profil** öffnen:
`/twitch/verwaltung#profil`. Zusätzlich gibt es im Analysebereich den Tab
`profile` bzw. den Alias `profil`. Für das Profil ist kein Analyse-Abo nötig.

Im Kopfbereich zeigt der Editor den Einrichtungsstand und bietet **Aus Twitch
importieren**. Der Import lädt einen frischen Twitch-Stand und übernimmt Bio,
Profilbild und Offline-Banner in den Profilentwurf. Gleichzeitig wird die
Streamplan-Synchronisierung aktiviert und der aktuelle Twitch-Streamplan neu
abgerufen. Zusätzliche Social-Links bleiben eigene Profilfelder, weil der
verwendete Twitch-Profilabruf dafür keine vergleichbare Linkliste liefert.

Für Deadlock stehen feste Auswahlfelder für bis zu drei Main Heroes, Rang,
Stream-Stil und typische Streamingzeiten bereit. Der Twitch Stream Schedule kann
aktiviert bleiben und wird dann für die kommenden Termine direkt gelesen. Eigene
Zusatztermine bleiben für Community-Abende und besondere Events verfügbar.
Zeiten gelten für `Europe/Berlin`; nicht existierende oder mehrdeutige Ortszeiten
bei der Zeitumstellung werden nicht stillschweigend verschoben.

Neue Profile sind private Entwürfe. **Profil veröffentlichen** aktivieren und
speichern, um `/streamer/<twitch_login>` freizugeben. Ausschalten und Speichern
nimmt die Seite wieder offline. Andere aktive und veröffentlichte Profile
erscheinen in der Partnerübersicht der Website; bis zu sechs können im eigenen
Profil unter „Aus meinem Umfeld“ empfohlen werden.

## Inhalt und Grenzen

- Bis zu 120 Zeichen Überschrift und 4000 Zeichen Über mich; Gold, Violett oder
  Petrol als persönlicher Akzent. Aktuelle Twitch-Daten können Bio, Profilbild,
  Offline-Banner, Live-Status und Live-Vorschaubild ergänzen. Gespeicherte Bildwerte
  dienen als Rückfall, wenn Twitch-Daten beim Abruf fehlen.
- Bis zu drei Main Heroes, ein Rangwert, sechs Stream-Stile und sechs typische
  Streamingzeiten. Die Werte erscheinen als kompakte Orientierung im öffentlichen
  Profilkopf.
- Bis zu zwölf benannte HTTPS-Links und sechs Empfehlungen. Veröffentlichte,
  aktive Partnerempfehlungen werden verlinkt.
- Der Twitch Stream Schedule wird bei aktivierter Synchronisierung für kommende
  Termine eingelesen. Zusätzlich sind bis zu 200 selbst gepflegte Einzeltermine
  mit Titel, Beschreibung, Beginn und Ende möglich, jeweils bis zu 48 Stunden.
- Die drei meistgesehenen Twitch-Clips des angefragten 30-Tage-Fensters werden
  als Twitch-Player eingebettet, wenn der Abruf Daten liefert. Die Clip-Auswahl
  stammt aus der von Twitch gelieferten Reihenfolge nach Aufrufen.
- Der Monatskalender zwischen 2000 und 2100 und das 90-Tage-Wochenraster aus
  erfassten Streams bleiben erhalten, liegen auf der öffentlichen Seite aber in
  einer nachgeordneten Detailansicht. Pro Kalendertag werden bis zu drei Einträge
  direkt dargestellt; weitere Einträge werden als Anzahl zusammengefasst.
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

Die Daten bleiben in `twitch_partner_profiles`, die ursprüngliche Tabelle stammt
aus Migration `20260918210000_partner_profiles.sql`. Für die zusätzlichen
Profilfelder ist keine weitere Datenbankmigration nötig, weil der Profilinhalt als
JSON gespeichert und beim Lesen mit Standardwerten ergänzt wird. Die Twitch-Daten
werden beim Abruf über einen kurzlebigen Prozesscache ergänzt und nicht als neue
Analysequelle in die Profiltabelle geschrieben.

## HTTP-Vertrag

| Pfad | Zugriff | Zweck |
| --- | --- | --- |
| `GET /streamer/<login>?month=YYYY-MM` | Öffentlich, aktive veröffentlichte Partner | HTML-Profil und Kalender |
| `GET /twitch/profile-assets/profile.css` | Öffentlich | Profil-Stylesheet |
| `GET /twitch/api/v2/public/partner-profiles` | Öffentlich | Nur Login und Überschrift sichtbarer Profile |
| `GET /twitch/api/v2/streamer/profile` | Angemeldeter Partner / Admin-Scope | Eigener gespeicherter Stand |
| `PUT /twitch/api/v2/streamer/profile` | Aktiver Partner / Admin-Scope + CSRF | Atomare Speicherung mit `revision` |

GET liefert zusätzlich zum gespeicherten Profil einen `twitch`-Block mit dem
aktuellen Profilbild, Banner, Live-Status, Clips und kommenden Schedule-Segmenten,
wenn Twitch erreichbar ist. `refresh_twitch=true` umgeht den kurzen Prozesscache
für einen bewusst ausgelösten Neuimport. PUT sendet weiterhin
`{ revision, published, profile }`; die Live-Daten werden nicht zurückgeschrieben.
Die vollständigen Feldtypen liegen in `bot/dashboard_v2/src/api/partnerProfile.ts`.
`streamer` ist für den bestehenden Admin-Scope frei wählbar. Maximale
Requestgröße: 512 KiB.

## Deployment

Der aktuelle Release- und Live-Prüfstand steht in der Task-Akte
`.tasks/2026-09-18-partner-profile-calendar/STATUS.md`.

1. Feature-Branch über den normalen Releaseprozess integrieren. Die neue
   Migration mit dem bestehenden Migrationsprozess anwenden, bevor die neuen
   Endpunkte genutzt werden; anschließend die Runtime-Rechte aktualisieren.
2. `tb-dashboard` sowie `bot/dashboard_v2` und `website` aus demselben
   Release-Stand bauen und mit dem vorhandenen Release-/Rollbackverfahren
   ausliefern. Der Deploy veröffentlicht keine privaten Profilentwürfe.
3. `ops/caddy/partner-profiles.caddy` **innerhalb** des bestehenden
   `handle /streamer*`-Blocks vor dessen Fallback importieren. Zusätzlich
   `ops/caddy/partner-profile-assets.caddy` auf Site-Ebene importieren.
   Die reservierten Website-Seiten (`commands`, `help`, `faq`, `vergleich`,
   Versionsseiten und Asset-Verzeichnisse) behalten ihre bisherigen Handler.
   Der gemeinsame Axum-Wildcard-Handler verteilt auf Profil und Website-Dateien;
   keine kollidierende zweite Parameterroute registrieren. Profile verwenden
   `/streamer/login`, auch mit abschließendem Slash. Alte `@`-Pfade liefern 404.
   Konfiguration vor dem Reload mit `caddy validate` prüfen. Upstream-Status,
   CSP und `no-store` nicht überschreiben; CDN-Regeln dürfen Profile nicht cachen.
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
  prüft echte HTTP-Aufrufe über isolierte Loopback-Listener für Profile,
  Monatsparameter, Stylesheet, 404 und bestehende Website-Pfade. Kein
  Produktions-Admin-Port, keine Zertifikatsanforderungen.
- Vollständige Dashboard-Suite: 367 von 374 Tests erfolgreich. Sieben Fehler
  betreffen unveränderte Bereiche: drei Farb-/Kontrastprüfungen, drei
  Social-Media-Vertragsprüfungen und eine OBS-Hilfe-Prüfung. Keine dieser sieben
  Fehlermeldungen nennt ein neues Profilmodul. Die Suite wird nicht als
  vollständig grün ausgewiesen.

Gezielter roter Gegenbeweis für den Verwaltungszugang: der neue Test erwartete
`#profil`, bekam vor der Verdrahtung `konto` und schlug fehl. Nach Einbau des
freien Verwaltungstabs und Anpassung der Tab-Vertragsprüfung ist er grün.
Live-Prüfergebnisse werden separat in der Task-Akte dokumentiert.
