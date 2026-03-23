# Streamer-Dokumentation

> Hinweis: Die aktuelle, gegen den Code gepruefte Streamer-Doku liegt unter [docs/streamer/README.md](streamer/README.md).
>
> Empfohlene Einstiege:
> - [Streamer Guide](streamer/README.md)
> - [Dashboard und Seiten](streamer/DASHBOARD.md)
> - [Commands](streamer/COMMANDS.md)
> - [Features und Workflows](streamer/FEATURES.md)
>
> Der restliche Inhalt dieser Datei ist eine aeltere Uebersicht und kann in Details hinter dem aktuellen Stand zurueckliegen.

Alle Features fuer verifizierte Streamer-Partner. Zugang nach Discord-Link + Admin-Verifikation.

## Dashboard

### Dashboard V1
URL: `https://twitch.earlysalty.com/twitch/dashboard`
- Klassisches Server-Rendered-Dashboard
- Live-Status, Statistiken, Einstellungen
- **Datei**: `bot/analytics/api_overview.py` → `_serve_dashboard()`

### Dashboard V2
URL: `https://twitch.earlysalty.com/twitch/dashboard-v2`
- React 19 + TypeScript + Vite 7 SPA
- Alle Analytics in interaktiven Charts (Recharts, Framer Motion)
- Gebaut aus `bot/dashboard_v2/` → Output in `bot/analytics/dashboard_v2/dist/`
- Build: `build_dashboard.ps1` im Projektroot
- **Datei**: `bot/analytics/api_overview.py` → `_serve_dashboard_v2()`

### Demo-Dashboard (Public)
URL: `https://twitch.earlysalty.com/twitch/demo`
- Oeffentlich, mit Demo-Daten befuellt (`bot/analytics/demo_data.py`)
- Identisches UI wie Dashboard V2

---

## Analytics-API

Alle Endpunkte unter `/twitch/api/v2/` — vollstaendige Liste in [API.md](API.md).

### Wichtigste Endpunkte

| Endpunkt | Beschreibung |
|----------|--------------|
| `/overview` | Kernmetriken (Peak, Avg, Wachstum) |
| `/monthly-stats` | Monatliche Statistiken |
| `/viewer-timeline` | Viewer-Verlauf ueber Zeit |
| `/chat-analytics` | Chat-Aktivitaet und Engagement |
| `/coaching` | KI-basierte Coaching-Empfehlungen |
| `/raid-analytics` | Eigene Raid-Statistiken |
| `/viewer-directory` | Alle bekannten Viewer mit Profilen |
| `/audience-demographics` | Audience-Analyse |
| `/tag-analysis` | Welche Tags bringen mehr Viewer |
| `/title-performance` | Welche Titel-Typen performen besser |

**Frontend-Hooks**: `bot/dashboard_v2/src/hooks/useAnalytics.ts`
**API-Client**: `bot/dashboard_v2/src/api/client.ts`

---

## Live-Announcements

Konfiguriert wann und wie Go-Live-Posts im Discord erscheinen.

### Konfiguration
- URL: GET `/twitch/live-announcement`
- API: GET/POST `/twitch/api/live-announcement/config`
- Preview: GET `/twitch/api/live-announcement/preview`
- Test-Send: POST `/twitch/api/live-announcement/test`
- **Datei**: `bot/dashboard/live/live_announcement_mixin.py`
- **Tabelle**: `twitch_live_announcement_configs`

### Template-Engine
- **Datei**: `bot/live_announce/template.py`
- Variablen: `{streamer}`, `{title}`, `{game}`, `{viewers}`, `{url}` etc.

### Go-Live-Flow
1. Monitoring-Tick erkennt Stream-Start
2. `bot/monitoring/embeds_mixin.py` erstellt Discord-Embed
3. Embed wird in `TWITCH_NOTIFY_CHANNEL_ID` gepostet
4. Streamer-spezifische Announcement-Config aus `twitch_live_announcement_configs`
5. Optional: Live-Ping-Rolle aus `twitch_streamers.live_ping_role_id`

---

## Raid-System

### Streamer-Seite

| URL | Beschreibung |
|-----|--------------|
| `/twitch/raid/auth` | OAuth-Autorisierung fuer Raid-Bot |
| `/twitch/raid/go` | Raid starten |
| `/twitch/raid/requirements` | Raid-Anforderungen anzeigen |
| `/twitch/raid/history` | Raid-History des eigenen Channels |
| `/twitch/raid/analytics` | Raid-Performance-Metriken |
| `/twitch/raid/callback` | OAuth-Callback (nach Twitch-Auth) |

### Raid-OAuth
1. Streamer oeffnet `/twitch/raid/auth`
2. Wird zu Twitch OAuth weitergeleitet (Scope: `channel:manage:raids`)
3. Callback speichert Token AES-256-GCM verschluesselt in `twitch_raid_auth`
4. Raid-Bot kann jetzt im Namen des Streamers raiden

**Datei**: `bot/raid/auth.py`, `bot/dashboard/raids/raid_mixin.py`

### Auto-Raid-Logik
- `bot/raid/mixin.py` → `TwitchRaidMixin` — entscheidet wer geraided wird
- `bot/raid/executor.py` — fuehrt den Raid aus
- `bot/raid/manager.py` — Zustand, Cooldowns, Blacklist
- Auswahl-Kriterien: Sprache, Viewer-Count, nicht auf Blacklist, Raid-Bot aktiv

### Discord-Commands (Admin)
| Command | Beschreibung |
|---------|--------------|
| `!raid_enable [streamer]` | Aktiviert Raid-Bot fuer Streamer |
| `!raid_disable [streamer]` | Deaktiviert Raid-Bot fuer Streamer |

---

## Abo / Billing (Streamer-Seite)

| URL | Beschreibung |
|-----|--------------|
| `/twitch/abbo` | Abo-Uebersicht, aktueller Plan |
| `/twitch/abbo/bezahlen` | Checkout (Stripe) |
| `/twitch/abbo/rechnungsdaten` | Rechnungsadresse speichern |
| `/twitch/abbo/kuendigen` | Abo kuendigen |
| `/twitch/abbo/rechnungen` | Rechnungs-History |
| `/twitch/abbo/rechnung` | Einzelne Rechnung anzeigen |
| `/twitch/abbo/stripe-settings` | Stripe-Portal-Link |

**Datei**: `bot/dashboard/billing/billing_mixin.py`, `bot/dashboard/billing/billing_plans.py`
**Tabelle**: `streamer_plans`

### Promo-Mode (Streamer)
- POST `/twitch/abbo/promo-settings` — Promo-Einstellungen
- POST `/twitch/abbo/lurker-tax-settings` — Lurker Steuer aktivieren/deaktivieren
- POST `/twitch/abbo/promo-message` — Promo-Nachricht konfigurieren
- Validiert per `bot/promo_mode.py` → `validate_streamer_promo_message()`
- Siehe auch `docs/LURKER_TAX.md`

---

## Affiliate-System

Affiliate-Links generieren und tracken.

| URL | Beschreibung |
|-----|--------------|
| `/twitch/affiliate` | Affiliate-Dashboard |
| `/twitch/affiliate/links` | Link-Liste |
| `/twitch/affiliate/stats` | Klick-Statistiken |
| POST `/twitch/affiliate/links` | Neuen Link erstellen |
| GET `/twitch/affiliate/click/{id}` | Klick-Tracking + Redirect (public) |
| GET `/twitch/affiliate/api/summary` | API-Zusammenfassung |

**Datei**: `bot/dashboard/affiliate/affiliate_mixin.py`
**Tabelle**: `twitch_link_clicks`

---

## Social Media / Clip-Manager

Clips automatisch auf YouTube, Instagram und TikTok hochladen.

### Flow
1. `bot/social_media/clip_fetcher.py` — holt neue Clips von Twitch API
2. `bot/social_media/clip_manager.py` — entscheidet welche Clips hochgeladen werden
3. `bot/social_media/upload_worker.py` — Hintergrund-Worker, verarbeitet Queue
4. `bot/social_media/uploaders/` — plattformspezifische Upload-Implementierungen

### Tabellen
- `twitch_clips_social_media` — Clip-Metadaten
- `twitch_clips_upload_queue` — Upload-Warteschlange
- `twitch_clips_social_analytics` — Upload-Ergebnisse
- `clip_templates_streamer` — Eigene Templates fuer Titel/Beschreibung
- `clip_last_hashtags` — Letzte verwendete Hashtags

### OAuth
- `bot/social_media/oauth_manager.py` — OAuth fuer YouTube/Instagram/TikTok
- `bot/social_media/token_refresh_worker.py` — automatisches Token-Refresh
- Tokens verschluesselt mit AES-256-GCM in `social_media_platform_auth`

---

## Chat-Commands

| Command | Beschreibung | Channel |
|---------|--------------|---------|
| `!twl` | Zeigt aktuelle Live-Streams der Partner | `TWITCH_STATS_CHANNEL_IDS` |

Weitere Commands koennen in `bot/chat/commands.py` definiert werden.

---

## Leaderboard

Discord-Viewer-Leaderboard: `bot/community/leaderboard.py` → `TwitchLeaderboardMixin`
Zeigt welche Viewer am meisten Zeit mit Deadlock-Streams verbracht haben.

---

## Authentifizierung

### Twitch-Login
1. GET `/twitch/auth/login?next=...`
2. Redirect zu Twitch OAuth (Scope: `openid user:read:email`)
3. Callback: GET `/twitch/auth/callback`
4. Session wird in `dashboard_sessions` in PostgreSQL (Fernet-verschluesselt) gespeichert
5. Redirect zu `next` Parameter

### Discord-Login (alternativ)
1. GET `/twitch/auth/discord/login?next=...`
2. Redirect zu Discord OAuth
3. Callback: GET `/twitch/auth/discord/callback`
4. Session-Typ: `discord`

**Datei**: `bot/dashboard/auth/auth_mixin.py`
