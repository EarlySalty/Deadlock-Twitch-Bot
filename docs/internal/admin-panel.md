# Admin-Panel

## Zweck und Abgrenzung

Das Repo hat zwei Admin-Richtungen:

- serverseitige Legacy-/Operations-Surfaces unter `/twitch/admin`
- neuere React-Admin-Module unter `bot/admin_dashboard/`

Die zentrale Regel ist: Admin und User-Surface bleiben getrennt. Auf dem Admin-Host duerfen User-Dashboards nicht ausgespielt werden; das wurde zusaetzlich auditiert und regressionsgetestet.

## Berechtigungen

Die eigentlichen Admin-Routen erwarten eine Admin- oder Localhost-Session. Wichtige Guards:

- Discord-Admin-Login fuer `/twitch/admin`
- Token-/Session-Pruefungen fuer schreibende Admin-Aktionen
- Same-Origin-Pruefung bei Partner-Link-Issuing aus einer Admin-Session
- CSRF-Pruefung bei Formular-Posts

Normale Partner-Sessions duerfen die Streamer-Surfaces nutzen, aber keine Admin-Aktionen gegen fremde Kanaele ausfuehren.

## Wichtige `/twitch/admin`-Routen

Im Legacy-Route-Set sind vor allem diese Endpunkte relevant:

- `GET /twitch/admin`
- `GET /twitch/admin/legacy`
- `GET/POST /twitch/admin/announcements`
- `GET /twitch/admin/roadmap`
- `POST /twitch/admin/chat_action`
- `POST /twitch/admin/manual-plan`
- `POST /twitch/admin/manual-plan/clear`

Dazu kommen die allgemeinen Partner-Verwaltungsaktionen:

- `POST /twitch/add_any`
- `POST /twitch/add_url`
- `POST /twitch/add_login/{login}`
- `POST /twitch/add_streamer`
- `POST /twitch/remove`
- `POST /twitch/verify`
- `POST /twitch/archive`
- `POST /twitch/discord_flag`

## Typische Admin-Workflows

### 1. Streamer aufnehmen, verifizieren, archivieren

Der Standardfall laeuft ueber Add-/Verify-/Archive-Endpunkte. Aufnahme und Verifikation schreiben den Streamer in den Partnerbestand; Archivierung und Remove sind fuer inaktive oder falsche Eintraege da. Das ist die Grundpflege fuer alles weitere: Billing, Raids, Analytics und Social Media bauen auf einem sauberen Partner-State auf.

### 2. Manuellen Plan setzen oder loeschen

Im Admin-Panel gibt es eine manuelle Planverwaltung. Sie zeigt:

- effektiven Plan
- Billing-Plan aus Stripe-Sync
- manuellen Override

`manual-plan` ist fuer Support-Faelle, Kulanz, Bonusmonate oder Reparaturen nach Billing-Problemen gedacht. `manual-plan/clear` entfernt den Override und laesst wieder Stripe beziehungsweise den Default-Fallback greifen.

### 3. Announcement- und Promo-Steuerung

Die Announcement-Surfaces steuern globale Modi und Zeitfenster. Dazu gehoeren Chat-/Announcement-Overrides, die absichtlich nicht in die Streamer-Self-Service-Seiten ausgelagert sind. Schreibzugriffe bleiben hier admin-only.

### 4. Monitoring, Billing, Affiliates

Das React-Admin-Paket bildet zusaetzlich diese Bereiche ab:

- Streamer-Liste und Streamer-Detail
- Monitoring (`SystemOverview`, `EventSubStatus`, `DatabaseStats`, `ErrorLogs`)
- Konfiguration (`BotConfig`, `RaidConfig`, `ChatConfig`)
- Billing (`Subscriptions`, `Affiliates`, `Gutschriften`)

Die Dateistruktur lebt schon in `bot/admin_dashboard/`. Beim Review immer unterscheiden, ob eine Funktion bereits an die produktiven Server-Routen verdrahtet ist oder nur als Admin-Frontend-Modul existiert.

## Operative Hinweise

- Schreibende Admin-Routen nicht ohne Host-/Session-Kontext erweitern.
- Fremde Streamer-Daten nur ueber echte Admin-Level oeffnen, nie ueber Partner-Session plus URL-Parameter.
- Plan-Overrides sparsam einsetzen; Billing-Sync bleibt die primaere Quelle.
- Social-Media-Admin-Endpunkte leben separat unter `/social-media/api/admin/*` und sind kein Ersatz fuer das Haupt-Admin-Panel.
