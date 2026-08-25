# Research: Partneraufnahme im Twitch-Admin-Dashboard

status: geprüft
datum: 2026-08-25
contract: CONTRACT.md

## Ergebnis

Die Daten- und Geschäftslogik ist bereits produktiv vorhanden. Es fehlt nur
eine geschützte Dashboard-Verdrahtung und eine verständliche Bedienoberfläche.
Die Umsetzung soll direkt auf `tb_analytics::partner_signup_block` aufsetzen;
der Browser darf die bestehende interne Token-API nicht direkt aufrufen.

## Bestehender fachlicher Zustand

- Die Migration legt `public.twitch_partner_signup_denylist` mit stabiler
  `twitch_user_id`, Login, internem Grund, optionalem `public_message`,
  Bearbeiter, Zeitstempel und Herkunftsflag für eine Partner-Pause an.
- `add` schreibt den eigentlichen Block, zieht den Signup-Block in die
  Raid-Blacklist, löscht gespeicherte Raid-Credentials und pausiert einen noch
  aktiven Partner in einer Transaktion.
- `remove` entfernt nur die vom Signup-Block selbst erzeugten Folgewirkungen;
  fremde Raid-Gründe und ein unabhängig gesetzter Admin-Block bleiben erhalten.
- Die existierenden Internal-API-Routen liefern bereits Add-, Remove-, Check-
  und List-Verträge. Sie sind aber für privilegierte Serveraufrufe gedacht und
  erwarten den internen Auth-Level.

## Empfohlene Backend-Form

Neue Admin-Routen unter dem bestehenden Admin-Prefix:

```text
GET  /twitch/api/admin/partner-signup-blocks
POST /twitch/api/admin/partner-signup-blocks
POST /twitch/api/admin/partner-signup-blocks/remove
```

Der Handler bekommt `PgPool`, prüft `require_admin`, lässt die bestehenden
CSRF-Middleware des Admin-Streamers-Routers greifen und ruft die gemeinsame
Analytics-CRUD-Funktion auf. `added_by` kommt aus der authentifizierten
Dashboard-Session, nicht aus dem Request-Body.

Beim Add wird der Login serverseitig normalisiert. Zuerst wird der vorhandene
Resolver genutzt; wenn der Kanal noch nicht im lokalen Bestand ist, muss der
Backend-Handler ihn über den bestehenden serverseitigen Twitch-Helix-
`get_users`-Pfad auflösen. Bei einem nicht auflösbaren oder nicht existierenden
Kanal gibt es 4xx und keinen partiellen Eintrag. So bleibt die ID-Ankerregel
auch für neue Kandidaten erhalten.

Die Antwort sollte ein kleines, explizites Dashboard-Modell liefern: `items`
für die Liste sowie bei Add/Remove die fachlichen Outcome-Felder. Keine
interne API-URL, kein interner Token und keine Twitch-Credential-Daten werden
an den Browser durchgereicht.

## Empfohlene Frontend-Form

Eine eigene Community-Seite ist passender als ein Filter in
`StreamerList`: Die Ausschlussliste kann Kanäle enthalten, die noch gar keine
Partnerzeile haben. Die Seite enthält:

1. eine Add-Karte mit Login, internem Grund und optionalem Absagetext;
2. eine Warnung mit den drei möglichen Folgewirkungen;
3. eine Tabelle „Von Partneraufnahme ausgeschlossen“;
4. eine Remove-Aktion mit Bestätigungsdialog und Hinweis zur nicht erfolgenden
   Credential-Wiederherstellung.

Die vorhandenen `DataTable`, `ConfirmTypedDialog`, `Toast`, `useQuery`- und
`useMutation`-Muster werden wiederverwendet. Nach jeder Mutation werden
`partner-signup-blocks`, Streamer-Liste und gegebenenfalls das Streamer-Detail
invalidiert.

## Sicherheit und Betrieb

- Reads bleiben Admin-only; Writes laufen mit Session-Cookie, CSRF-Header und
  serverseitiger Admin-Prüfung.
- Der bestehende Admin-Audit-Middleware-Eintrag zeichnet Route, Actor und
  Status auf; die fachlichen Outcome-Felder bleiben zusätzlich im strukturierten
  Signup-Block-Log.
- Es gibt keine Migration und keinen Bot-Neustart wegen der UI allein. Nach
  dem Merge werden Dashboard-Bundle und `deadlock-twitch-dashboard-rust.service`
  neu gebaut, neu gestartet und die drei Live-Routen mit Admin-Session sowie
  ohne Session geprüft.

## Nicht wiederverwenden

- Nicht die Audio-Archiv-Ausschlussvariable.
- Nicht `manual_partner_opt_out` als Ersatz für den Signup-Block.
- Nicht die interne API direkt aus React.
- Nicht den frei eingegebenen Login als dauerhaften Primärschlüssel.
