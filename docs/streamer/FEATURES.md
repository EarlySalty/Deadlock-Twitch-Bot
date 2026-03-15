# Features und Workflows

Stand: `2026-03-13`

## Auth und Konto

### Login

Streamer kommen je nach Surface ueber diese Flows hinein:

- Twitch Login: `/twitch/auth/login?next=...`
- Discord Login: `/twitch/auth/discord/login?next=...`

### Kontoverwaltung

Die eigentliche Streamer-Ansicht dafuer ist `/twitch/verwaltung`.

Dort sichtbar:

- Twitch-Login
- Display Name
- User-ID
- aktive und fehlende Twitch-Scopes
- Discord-Verknuepfung
- Reconnect-Links fuer Twitch und Discord

### Wichtige Auth-Besonderheit

- Das Analytics-Dashboard akzeptiert Session oder Partner-Token.
- Das Social-Media-Dashboard verlangt fuer Partner effektiv eine echte Twitch-Session und blockt Token-only-Zugriffe ohne Session.
- Streamer duerfen in Streamer-Surfaces nur auf den eigenen Account zugreifen.

## Analyse-Dashboard

Die produktiven Streamer-Surfaces sind:

- `/twitch/dashboard` fuer Start/Home
- `/twitch/dashboard-v2` fuer Analytics
- `/twitch/pricing` fuer den Vergleich
- `/twitch/abbo` fuer die eigentliche Abo-Verwaltung

Die genaue Tab- und Seitenstruktur steht in [DASHBOARD.md](DASHBOARD.md).

## Raid-Bot

### Wofuer der Raid-Bot steht

Der aktuelle OAuth-/Raid-Flow ist nicht nur fuer Auto-Raids da, sondern auch fuer:

- Auto-Raids
- manuellen Raid per Chat
- Chat Guard / Moderationsnahe Features
- Raid- und Impact-Daten im Dashboard
- Zusatzfunktionen wie Clip-Erstellung und Chatter-Readiness

### Streamer-Self-Service Entry Points

- `/twitch/raid/auth`
- Discord `/traid`
- Discord `/check-auth`
- Discord `/check-scopes`
- Twitch Chat `!raid_enable`

### Aktuell benoetigte OAuth-Scopes

- `channel:manage:raids`
- `channel:read:subscriptions`
- `channel:manage:moderators`
- `channel:bot`
- `clips:edit`
- `channel:read:ads`
- `bits:read`
- `channel:read:hype_train`
- `channel:read:redemptions`

### Wichtige Hinweise

- Wenn nur eine aeltere Raid-Autorisierung mit wenigen Scopes existiert, markieren die Commands den Account als Re-Auth-faellig.
- Der OAuth-Callback landet standardmaessig wieder auf `/twitch/dashboard`.
- Die Seiten `/twitch/raid/requirements`, `/twitch/raid/history` und `/twitch/raid/analytics` sind aktuell eher Legacy-/Admin-Surfaces. Fuer Streamer selbst sind Commands und Dashboard der stabile Weg.
- Der Dashboard-Login selbst ist getrennt von diesen Bot-Scopes. Der reine Dashboard-Zugang und die Bot-/Analytics-Funktionen nutzen unterschiedliche Auth-Schichten.
- Bot-zentrierte Chat- und Moderator-Scopes wie `user:read:chat`, `user:write:chat` oder `moderator:read:chatters` laufen zentral ueber den Bot-Account und sind keine harte Streamer-Pflicht mehr.

## Live-Announcement Builder

### Surface

- Seite: `/twitch/live-announcement`
- Lesen: `GET /twitch/api/live-announcement/config`
- Speichern: `POST /twitch/api/live-announcement/config`
- Preview: `GET /twitch/api/live-announcement/preview`
- Testsendung: `POST /twitch/api/live-announcement/test`

### Was Streamer konfigurieren koennen

- Nachrichtentext
- Embed-Titel, Beschreibung, Felder, Bilder, Footer
- CTA-Button
- Mentions und Ping-Rolle
- erlaubte Editor-Rollen ueber `allowed_editor_role_ids`

### Platzhalter

- `{channel}`
- `{url}`
- `{rolle}`
- `{title}`
- `{viewer_count}`
- `{started_at}`
- `{language}`
- `{tags}`
- `{uptime}`
- `{game}`

### Wichtige Verhaltensdetails

- Die Streamer-Ping-Rolle wird bei Bedarf fuer den Streamer sichergestellt bzw. angelegt.
- Normale Partner duerfen nur ihre eigene Konfiguration bearbeiten.
- Die Konfiguration wird vor dem Speichern validiert.
- Mentions werden vor dem Speichern bzw. Rendern abgesichert.
- Speichern und Testsendung laufen mit CSRF-Schutz.

## Billing, Plaene und Lurker Steuer

### Reale Plan-IDs

| Plan-ID | Anzeigename | Effektives Tier |
| --- | --- | --- |
| `raid_free` | Free / Raid Free | `free` |
| `raid_boost` | Basic / Raid Boost | `basic` |
| `analysis_dashboard` | Erweitert / Analyse Dashboard | `extended` |
| `bundle_analysis_raid_boost` | Erweitert (Bundle) | `extended` |

### Relevante Seiten

- `/twitch/pricing`: moderner Vergleich
- `/twitch/abbo`: eigentliche Billing- und Settings-Seite

### Wichtige Billing-Routen

- `GET /twitch/abbo`
- `GET /twitch/abbo/bezahlen`
- `POST /twitch/abbo/rechnungsdaten`
- `GET|POST /twitch/abbo/kuendigen`
- `GET /twitch/abbo/rechnungen`
- `GET /twitch/abbo/rechnung`
- `GET /twitch/abbo/stripe-settings`
- `POST /twitch/abbo/promo-settings`
- `POST /twitch/abbo/lurker-tax-settings`
- `POST /twitch/abbo/promo-message`

### Was `/twitch/abbo` aktuell abdeckt

- Planwahl und Checkout-Einstieg
- Rechnungsdaten speichern
- Rechnungen anzeigen
- Stripe-Settings / Billing-Portal
- Abo kuendigen
- Promo-Settings fuer Bundle-Streamer
- eigene Promo-Message
- Lurker-Steuer-Toggle

### Streamer-Promo-Message

Die Streamer-spezifische Promo-Message ist von den Discord-Live-Announcements getrennt.

Aktuelle Regeln:

- speichern ueber `POST /twitch/abbo/promo-message`
- muss `{invite}` enthalten
- maximal 500 Zeichen
- wird im Chat-Promo-Flow vor Defaults genutzt, solange kein globaler Admin-Override aktiv ist

### Lurker Steuer

Die Lurker Steuer ist ein Paid-Plan-Feature.

Aktuelle Regeln:

- verfuegbar nur in `raid_boost`, `analysis_dashboard` und `bundle_analysis_raid_boost`
- Toggle auf `/twitch/abbo`
- Speichern ueber `POST /twitch/abbo/lurker-tax-settings`
- benoetigt den zentralen Bot-Zugriff fuer `moderator:read:chatters`
- im Chat dauerhaft abschaltbar ueber `!lurkersteuer_off`, `!lurkersteuer_aus`, `!lurker_tax_off`
- Reaktivierung aktuell ueber den Abo-Bereich
- teilt sich den 60-Minuten-Cooldown mit dem Promo-/Announcement-Loop

Mehr Details stehen in [../LURKER_TAX.md](../LURKER_TAX.md).

## Affiliate

### Aktuelle Streamer-Surfaces

- `/twitch/affiliate/portal`
- `GET /twitch/api/v2/affiliate/portal`

Das Portal zeigt:

- Referral-Link
- Gesamt-Claims
- Gesamt-Provision
- Claims dieses Monats
- ausstehende Auszahlung
- letzte Claims

### Aktueller Onboarding- und Verwaltungsflow

- `GET /twitch/auth/affiliate/login`
- `GET /twitch/auth/affiliate/callback`
- `GET /twitch/affiliate/signup`
- `POST /twitch/affiliate/signup/complete`
- `GET /twitch/affiliate/connect/stripe`
- `GET /twitch/affiliate/connect/stripe/callback`
- `POST /twitch/affiliate/claim`
- `GET /twitch/api/affiliate/me`
- `GET /twitch/api/affiliate/claims`
- `GET /twitch/api/affiliate/commissions`

Wichtig:

- Das Affiliate-System arbeitet mit eigenem Session-Cookie plus Twitch-OAuth und Stripe-Connect.
- Provisionen werden aktuell mit `30%` modelliert.
- Das alte Bild von "Affiliate-Dashboard + Linklisten + Stats-Endpunkten" in aelterer Doku passt nicht mehr vollstaendig.
- Fuer Streamer ist das Portal unter `/twitch/affiliate/portal` die aktuelle Uebersicht.

## Social Media / Clip Manager

### Surface

- `/social-media`

### Tabs und Kernfunktionen

- `Dashboard`
- `Clips`
- `Templates`
- `Settings`
- manuelles Clip-Fetching
- einzelne Uploads in die Queue legen
- Batch-Uploads starten
- Clips als hochgeladen markieren
- globale und streamer-spezifische Templates laden
- streamer-spezifische Templates speichern
- Templates auf Clips anwenden
- letzte Hashtags abrufen
- Plattformstatus abrufen

### Relevante Endpunkte

- `GET /social-media/api/stats`
- `GET /social-media/api/clips`
- `POST /social-media/api/upload`
- `GET /social-media/api/analytics`
- `GET /social-media/api/templates/global`
- `GET /social-media/api/templates/streamer`
- `POST /social-media/api/templates/streamer`
- `POST /social-media/api/templates/apply`
- `POST /social-media/api/batch-upload`
- `POST /social-media/api/mark-uploaded`
- `POST /social-media/api/fetch-clips`
- `GET /social-media/api/last-hashtags`
- `GET /social-media/oauth/start/{platform}`
- `GET /social-media/oauth/callback`
- `POST /social-media/oauth/disconnect/{platform}`
- `GET /social-media/api/platforms/status`

### Sicherheits- und Ownership-Regeln

- Partner duerfen nur ihren eigenen Account-Scope verwenden.
- Cross-Account-Zugriffe werden geblockt.
- Token-only-Partnerzugriffe ohne echte Session werden hier bewusst abgewiesen.
