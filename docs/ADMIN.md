# Admin-Dokumentation

Alle Features die Admin-Rechte erfordern. Admin = Discord-Owner oder konfigurierter Admin-Account.

## Admin-Panel

URL: `https://admin.earlysalty.de/twitch/admin` (separates Caddy-Zertifikat)

Inhalt des Admin-Panels:
- Alle registrierten Streamer mit Status
- Streamer hinzufuegen / entfernen / verifizieren / archivieren
- Partner-Chat-Aktionen
- Manuelle Plan-Ueberschreibungen (Override Abo-Plan fuer Streamer)
- Link zu Announcement-Verwaltung

**Datei**: `bot/dashboard/routes_mixin.py` → `admin()` Handler

---

## Streamer-Verwaltung

### Streamer hinzufuegen
| Aktion | Route | Beschreibung |
|--------|-------|--------------|
| Per Twitch-URL | POST `/twitch/add_url` | Parst Twitch-URL, fuegt Streamer hinzu |
| Per Login | POST `/twitch/add_login/{login}` | Fuegt Streamer per Login-Name hinzu |
| Beliebig | POST `/twitch/add_any` | Erkennt URL oder Login automatisch |
| Formular | POST `/twitch/add_streamer` | Ausfuehrliches Formular mit Optionen |

### Streamer bearbeiten
| Aktion | Route | Beschreibung |
|--------|-------|--------------|
| Entfernen | POST `/twitch/remove` | Entfernt Streamer aus dem System |
| Verifizieren | POST `/twitch/verify` | Manuell verifizieren (Partner-Status) |
| Archivieren | POST `/twitch/archive` | Streamer archivieren (kein Monitoring mehr) |
| Discord-Flag | POST `/twitch/discord_flag` | Discord-Status manuell setzen |
| Discord-Link | POST `/twitch/discord_link` | Discord-Account verknuepfen |

**Datei**: `bot/community/admin.py` → `TwitchAdminMixin`

---

## Billing & Stripe

### Plan-Verwaltung
Streamer-Abos werden ueber Stripe verwaltet. Plan-Definitionen in `bot/dashboard/billing/billing_plans.py`.

| Aktion | Datei |
|--------|-------|
| Plan-Katalog anzeigen | GET `/twitch/api/billing/catalog` |
| Stripe-Produkte synchronisieren | POST `/twitch/api/billing/stripe/sync-products` |
| Manuelle Plan-Ueberschreibung | POST `/twitch/admin/manual-plan` |
| Plan-Override loeschen | POST `/twitch/admin/manual-plan/clear` |
| Stripe-Readiness pruefen | GET `/twitch/api/billing/readiness` |

### Stripe-Webhook
Empfaengt Stripe-Events unter POST `/twitch/api/billing/stripe/webhook`.
Verarbeitet: `checkout.session.completed`, `invoice.paid`, `customer.subscription.*`

**Datei**: `bot/dashboard/billing/billing_mixin.py`

### Stripe-Konfiguration
Stripe-Keys werden aus dem Windows Credential Manager gelesen:
- Service: `DeadlockBot` — Keys: `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`
- Alternativ via ENV: `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`

---

## Promo-Mode

Globaler Promo-Mode-Status der steuert ob Promo-Nachrichten im Chat gesendet werden.

- **Datei**: `bot/promo_mode.py`
- **Tabelle**: `twitch_global_promo_modes`
- Validierungslogik: `validate_streamer_promo_message()` prueft Inhalte

---

## Announcements

### Live-Announcement-Verwaltung
Globale Announcement-Einstellungen fuer alle Streamer:

- URL: GET/POST `/twitch/admin/announcements`
- **Datei**: `bot/dashboard/live/announcement_mode_mixin.py`
- Konfiguriert ob/wie Discord-Announcements gesendet werden

### Chat-Aktionen
Moderations-Aktionen per Chat fuer Partner-Streamer:
- POST `/twitch/admin/chat_action`
- **Datei**: `bot/chat/moderation.py`

---

## Discord-Commands (Admin)

| Command | Beschreibung | Datei |
|---------|--------------|-------|
| `!reload` / Discord Slash | Cog-Module neu laden | bot/reload_mixin.py |
| `!raid_enable` | Raid-Bot fuer Streamer aktivieren | bot/raid/commands.py |
| `!raid_disable` | Raid-Bot fuer Streamer deaktivieren | bot/raid/commands.py |

**Reload-Manager**: `bot/reload_manager.py` — verwaltet welche Module geladen sind und kann einzelne Mixins neu laden.

---

## Stats & Monitoring (Admin-only Pages)

| URL | Inhalt |
|-----|--------|
| `/twitch/stats` | Uebersicht aller Streamer (raw Stats) |
| `/twitch/partners` | Partner-Status-Uebersicht |
| `/twitch/market` | Market-Research-Daten |
| `/twitch/admin/roadmap` | Roadmap-Verwaltung (Admin-Ansicht) |

---

## Rechtliche Seiten (Admin editierbar)

| URL | Inhalt | Datei |
|-----|--------|-------|
| `/twitch/legal/access` | Human-Gate mit Cloudflare Turnstile | admin/legal_mixin.py |
| `/twitch/legal/verify` | Validiert Turnstile und setzt Gate-Cookie | admin/legal_mixin.py |
| `/twitch/impressum` | Impressum | admin/legal_mixin.py |
| `/twitch/datenschutz` | Datenschutzerklaerung | admin/legal_mixin.py |
| `/twitch/agb` | AGB / ToS | admin/legal_mixin.py |

`/twitch/impressum` und `/twitch/datenschutz` werden ueber ein Human-Gate vor Bots und KI-Crawlern geschuetzt. `/twitch/agb` ist aktuell oeffentlich ohne Gate.

Fuer den produktiven Betrieb sind drei Secrets noetig:
- `TWITCH_LEGAL_TURNSTILE_SITE_KEY`
- `TWITCH_LEGAL_TURNSTILE_SECRET_KEY`
- `TWITCH_LEGAL_GATE_COOKIE_SECRET`

Die vollstaendige Betriebsdoku mit Routing-, CSP- und Troubleshooting-Hinweisen steht in [`docs/LEGAL_ACCESS_GATE.md`](LEGAL_ACCESS_GATE.md).

---

## Konfiguration

Alle wichtigen Konstanten in `bot/core/constants.py`:

| Konstante | Wert | Beschreibung |
|-----------|------|--------------|
| `TWITCH_DASHBOARD_PORT` | 8765 | Dashboard-Service Port |
| `TWITCH_INTERNAL_API_PORT` | 8776 | Interner API-Port |
| `POLL_INTERVAL_SECONDS` | 15 | Monitoring-Tick |
| `TWITCH_NOTIFY_CHANNEL_ID` | 1304169815505637458 | Live-Posting-Channel |
| `TWITCH_ALERT_CHANNEL_ID` | 1374364800817303632 | Warn-Channel |
| `TWITCH_STATS_CHANNEL_IDS` | [1428...] | !twl-Command reagiert hier |
| `TWITCH_TARGET_GAME_NAME` | "Deadlock" | Ueberwachtes Spiel |
| `TWITCH_CATEGORY_SAMPLE_LIMIT` | 400 | Max Streamer pro Kategorie-Tick |
| `TWITCH_LOG_EVERY_N_TICKS` | 1 | Stats jeden Tick loggen (=15s bei Default-Polling) |
