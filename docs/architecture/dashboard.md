# dashboard/ (Backend) — Architektur & Funktionsreferenz

> Pfad: `bot/dashboard/` · Stand: 2026-06-08 · 59 Dateien, ~25.800 Zeilen
>
> Teil der [Architektur-Doku](README.md). **Feature-Map:** [dashboard/README.md](dashboard/README.md), [dashboard/ADMIN.md](dashboard/ADMIN.md), [dashboard/STREAMER.md](dashboard/STREAMER.md). Routen-Liste: [../API.md](../API.md). Frontend: [frontend-streamer-dashboard.md](frontend-streamer-dashboard.md). Verwandt: [analytics.md](analytics.md), [bot-core.md](bot-core.md) (`DashboardBotService`), [internal-api.md](internal-api.md).

## 1. Zweck & Abgrenzung

`bot/dashboard/` ist das **serverseitige Dashboard**: die `aiohttp`-Web-App, die Streamer- und Admin-Oberflächen bedient — Auth (Discord/Twitch-OAuth), Live-Announcement-Konfig, Abo/Billing (Stripe), Affiliate-Programm (inkl. Gutschriften), Raid-Dashboard, rechtliche Seiten. Sie rendert HTML und stellt JSON-APIs bereit, die das React-Frontend ([dashboard_v2](frontend-streamer-dashboard.md)) konsumiert.

Zwei Einstiegspunkte, die man auseinanderhalten muss:
- **`server_v2.py::build_v2_app(...)`** — die eigenständige App-Factory, die der `dashboard_service` startet (Split-Runtime).
- **`mixin.py::TwitchDashboardMixin`** — die **bot-seitige Kompat-Brücke**: Methoden, die der Cog aufruft und die über `DashboardBotService`/interne API an Bot-Zustand kommen.

Abgrenzung: Die **Analytics-Queries/-API v2** (`/twitch/api/v2/*`) liegen in [analytics.md](analytics.md); die **Embed-Render-Engine** in [live-announce.md](live-announce.md). `dashboard/` ist das Web-/Auth-/Billing-Gerüst drumherum.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Eintritt** | `dashboard_service/app.py` (standalone) bzw. der Cog (Kompat-Mixin). Frontend ruft die JSON-APIs. |
| **Nutzt** | `storage/` (Sessions, Streamer, Billing-Entitlements), `analytics/` (eingebundene API-v2-Routen), `internal_api`-Client (Bot-Zustand), `live_announce/` (Embed-Render), Stripe-SDK, Discord-OAuth. |
| **DB-Tabellen** | `dashboard_sessions`, `oauth_state_tokens`, `streamer_plans`, Billing-/Entitlement-Tabellen, Affiliate-/Gutschrift-Tabellen, Streamer-Tabellen. |
| **Externe Dienste** | Discord-OAuth (Admin-Login + Streamer-Link), Twitch-OAuth (Raid-Scopes), Stripe (Checkout/Connect/Invoices), E-Mail (Affiliate). |
| **Secret-Namen** | OAuth-Client-ID/-Secret, `stripe_secret_key`, Legal-Gate-Cookie-Secret, interne API-Tokens. |

## 3. Dateien im Überblick (nach Feature)

| Bereich | Schlüssel-Dateien (Zeilen) | Rolle |
|---------|----------------------------|-------|
| **App-Factory** | `server_v2.py` (1165) | `build_v2_app(...)`, Security-Middleware, Routen-Registrierung. |
| **Bot-Brücke** | `mixin.py` (937) | `TwitchDashboardMixin` — Cog-seitige Helfer über `DashboardBotService`. |
| **Routen** | `routes_mixin.py` (651), `routes_entry.py` (348), `routes_billing.py` (837), `routes_market.py` (269), `routes_self_explainer.py` (251), `routes_title.py` (191), `routes_settings.py` (31), `pages.py` (766), `route_deps.py` (51) | Route-Gruppen + Seiten. |
| **Auth** | `auth/auth_mixin.py` (1906), `auth/services.py` (654), `auth/state_store.py` (428), `auth/partner_auth_mixin.py` (205), `auth/fingerprint_mixin.py` (186) | Discord-Admin-/Streamer-/Partner-Auth, Sessions, Rate-Limit. |
| **Billing/Abo** | `billing/billing_mixin.py` (1934), `billing/billing_plans.py` (457), `abbo_routes.py` (739), `abbo_billing_routes.py` (557), `core/abbo_html.py` (403) | Stripe-Billing + Abo-Selfservice. |
| **Affiliate** | `affiliate/affiliate_mixin.py` (1513), `affiliate/gutschrift.py` (1052), `affiliate/affiliate_pii.py` (351), `affiliate/affiliate_email.py` (149) | Affiliate-Programm + Gutschriften. |
| **Live** | `live/live.py` (2081), `live/live_announcement_mixin.py` (2064) | Live-Status + Go-Live-Announcement-Konfig. |
| **Admin** | `admin/legal_mixin.py` (735), `admin/announcement_mode_mixin.py` (387), `streamer_admin_mixin.py` (569) | Rechtliche Seiten, Announcement-Mode, Streamer-Verwaltung. |
| **Raids** | `raids/raid_mixin.py` (600), `raids/pages.py` (337), `raids/oauth_callback.py` (282) | Raid-Dashboard + OAuth-Callback. |
| **Core** | `core/stats.py` (1316), `core/templates.py` (680), `dashboard_metrics_mixin.py` (424) | Stats-Seite, HTML-Templates, Metriken. |
| **Compat** | `_compat.py` (74) + viele 6-Zeilen-Shims (`auth_mixin.py`, `billing_mixin.py`, …) | Lazy-Re-Export für alte Importpfade. |

## 4. Datenfluss / Lebenszyklus

**App-Start:** `dashboard_service` ruft `build_v2_app(*, noauth, token, partner_token, oauth_client_id/secret/redirect_uri, session_ttl_seconds=6h, legacy_stats_url, dashboard_services, …)`. Die Factory hängt `_security_headers_middleware` ein, registriert alle Route-Gruppen (Mixins) und die Analytics-v2-Routen und gibt die `web.Application` zurück.

**Admin-Login (Discord-OAuth):** `auth_login`/`auth_callback` bzw. der delegierte Discord-Flow (`discord_auth_login`/`discord_auth_complete`) tauschen den Code, prüfen die **Discord-Gilden-Mitgliedschaft** (`_check_discord_admin_membership`) und legen eine verschlüsselte Session (`dashboard_sessions`) an. Optional sammelt der Fingerprint-Flow (`fingerprint_page`/`fingerprint_submit`) nach dem Login einen JS-Fingerprint.

**Streamer-/Partner-Zugang:** `partner_auth_mixin` tauscht einen One-Time-Login-Token gegen eine Cookie-Session (`PartnerLoginTokenService`/`PartnerAccessService`), gebunden an Request-Kontext (`_partner_access_binding_matches`).

**Abo/Billing:** Über `abbo_*`-Routen bezahlt ein Streamer (`abbo_pay` → Stripe-Checkout), pflegt Profil (`abbo_profile_save`), kündigt (`abbo_cancel`), sieht Rechnungen (`abbo_invoices`/`abbo_invoice`). Stripe-Webhooks (siehe [internal-api.md](internal-api.md)/[stripe-webhooks-internal.md](../internal/stripe-webhooks-internal.md)) aktualisieren `streamer_plans`/Entitlements; `billing_mixin` triggert bei Planänderung u. a. einen Partner-Raid-Score-Refresh.

**Affiliate:** Affiliate-Signup (`_affiliate_auth_login/callback`), Stripe-**Connect** (`_affiliate_connect_stripe[_callback]`), Provisionen (`_affiliate_load_commissions_sync`, `_affiliate_process_commission`) und **Gutschriften** (Credit Notes) als PDF (`gutschrift.py`, `_affiliate_run_gutschrift_job`, `_affiliate_api_gutschrift_pdf`). PII liegt separat (`affiliate_pii.py`).

**Live-Konfig:** `DashboardLiveAnnouncementMixin` liefert die Konfig-Seite + JSON-API (`api_live_announcement_config`/`_save_config`/`_test_send`/`_preview`), validiert mit `_validate_config_dict` und rendert über [live_announce/](live-announce.md).

## 5. Funktionsreferenz pro Bereich

### server_v2.py
- `build_v2_app(*, noauth, token, partner_token=None, oauth_client_id=None, oauth_client_secret=None, oauth_redirect_uri=None, session_ttl_seconds=21600, legacy_stats_url=None, dashboard_services=None, …) -> web.Application` — die App-Factory; baut die App, registriert Routen + Middleware.
- `_security_headers_middleware(request, handler)` — setzt Security-Header auf jede Antwort.

### mixin.py — `TwitchDashboardMixin` (Bot-seitig)
Brücke vom Cog zum Dashboard/Bot-Zustand über `_dashboard_bot_service()` (= `DashboardBotService`). Wichtige Methoden:
- Streamer-Verwaltung: `_dashboard_add(login, require_link)`, `_dashboard_remove(login)`, `_dashboard_list()`/`_dashboard_list_sync()`, `_dashboard_partner_chat_action(login, mode, color, message)`.
- Live: `_dashboard_live_active_announcements()`, `_dashboard_live_link_click(*, streamer_login, tracking_token, discord_user_id, …)`, `_dashboard_live_button_label(login)`, `_dashboard_build_referral_url(login)`.
- Raid: `_dashboard_raid_auth_url(login, discord_user_id=None, scope_profile=None)`, `_dashboard_raid_go_url(state)`, `_dashboard_raid_requirements(login)`, `_dashboard_raid_blacklist_add/remove(login, …)`.

### auth/
- `auth_mixin.py` — Discord-Admin-OAuth (`auth_login`, `auth_callback`, `discord_auth_login`, `discord_auth_complete`, `discord_link_auth_login/complete`), Gilden-Mitgliedschaftsprüfung (`_check_discord_admin_membership`), Code-Tausch (`_exchange_discord_admin_code`), Session-Registrierung; Request-Klassifizierung (`_is_discord_admin_request`, `_is_secure_request`), delegierter Discord-Flow über die interne API (`_post_discord_oauth_internal_api`, `_fetch_delegated_discord_authorize_url`, `_fetch_delegated_discord_session`).
- `services.py` — `PartnerAccessService`, `PartnerLoginTokenService`, Session-/State-Helfer.
- `state_store.py` — `DashboardAuthStateStore` (`_save_state`/`_consume_state`/`_load_session`), `DashboardAuthRateLimitStore` (`allow_request(*, key, max_requests, window_seconds)`).
- `partner_auth_mixin.py` — `_DashboardPartnerAuthMixin`: One-Time-Login → Cookie-Session (`_create_partner_access_session`, `_get_partner_access_session`, `_partner_access_binding_matches`, `_delete_partner_access_session`).
- `fingerprint_mixin.py` — `fingerprint_page`, `fingerprint_submit` (Post-Login-JS-Fingerprint).

### billing/ + abbo
- `billing/billing_mixin.py` — `_DashboardBillingMixin`: Stripe-Checkout/Status, Plan-Gating, `_billing_refresh_partner_raid_score_cache(...)` bei Planänderung.
- `billing/billing_plans.py` — Plan-Katalog (Preise, Features, IDs).
- `abbo_routes.py` — `abbo_entry`, `_abbo_auth_redirect_or_none`, `_abbo_scope_state`, `_load_abbo_saved_settings`, `_abbo_upsert_lurker_tax_setting`.
- `abbo_billing_routes.py` — `abbo_pay`, `abbo_profile_save`, `abbo_cancel`, `abbo_invoices`, `abbo_invoice`, `abbo_stripe_settings`.
- `core/abbo_html.py` — HTML der Abo-Seiten.

### affiliate/
- `affiliate_mixin.py` — Signup (`_affiliate_auth_login`, `_affiliate_auth_callback`, `_affiliate_signup_page/_complete`), Stripe-Connect (`_affiliate_connect_stripe[_callback]`), Claims (`_affiliate_claim`, `_affiliate_load_claims_sync`), Provisionen (`_affiliate_load_commissions_sync`, `_affiliate_process_commission`, Lock-Key `_affiliate_commission_lock_key`), Gutschriften (`_affiliate_run_gutschrift_job`, `_affiliate_load_gutschriften_sync`, `_affiliate_api_gutschrift_pdf`), Sessions (`_create_affiliate_session`, `_set_affiliate_cookie`), JSON-API (`_affiliate_api_me/_profile_update/_claims/_commissions/_gutschriften`).
- `gutschrift.py` — Gutschrift-(Credit-Note-)Erzeugung, Nummerierung, PDF-Render.
- `affiliate_pii.py` — getrennte PII-Ablage. `affiliate_email.py` — `AffiliateEmailSender`.

### live/
- `live/live.py` — Live-Status-Seite + Go-Live-Embed-Konfiguration (streamerseitig).
- `live/live_announcement_mixin.py` — `DashboardLiveAnnouncementMixin`: `live_announcement_page`, `api_live_announcement_config`, `api_live_announcement_save_config`, `api_live_announcement_test_send`, `api_live_announcement_preview`; Konfig-Helfer `_default_live_announcement_config`, `_deep_merge`, `_parse_config_json`, `_to_template_config`, `_validate_config_dict`; Auth-Gates `_la_require_auth`/`_la_auth_level`/`_la_session`.

### admin/ + streamer_admin
- `admin/legal_mixin.py` — `_DashboardLegalMixin`: Impressum/Datenschutz/AGB/Sicherheitskonzept (`_render_legal_page`), Legal-Gate (`_legal_gate_cookie_secret`, `_legal_gate_configuration_state`). Siehe [LEGAL_ACCESS_GATE.md](../LEGAL_ACCESS_GATE.md). **Achtung Cutover 12.6.2026:** Der Live-Traffic für `/twitch/{impressum,datenschutz,agb,sicherheit}` + `/twitch/legal/*` läuft via Caddy über die Rust-Portierung (`rust/crates/tb-dashboard-api/src/handlers/legal.rs`, Service `deadlock-twitch-dashboard-rust`, Port 8769). Dieses Python-Modul bleibt als synchron gehaltener Fallback/Rollback-Pfad bestehen — Inhaltsänderungen müssen in BEIDEN Defaults nachgezogen werden (oder via `data/admin_dashboard/legal_pages.json`, das beide Seiten lesen). `/twitch/sicherheit` ist bewusst ohne Gate und ohne noindex.
- `admin/announcement_mode_mixin.py` — Announcement-Mode-Steuerung + Admin-Section-Nav.
- `streamer_admin_mixin.py` — Streamer-Verwaltung/Verifizierung (`add`, `remove`, `verify`, `archive`, `_do_add`).

### raids/
- `raids/raid_mixin.py` — Raid-Dashboard-Seite + API.
- `raids/pages.py` — Seiten-Rendering.
- `raids/oauth_callback.py` — der Twitch-OAuth-Callback-Handler für den Raid-Scope-Flow.

### core/ + metrics
- `core/stats.py` — Stats-Seiten-Rendering (Legacy-Stats-Surface).
- `core/templates.py` — HTML-/Render-Helfer, Footer-Links.
- `dashboard_metrics_mixin.py` — Dashboard-Metriken.

### routes_* / pages.py
Registrieren die Route-Gruppen (Entry/Market/Billing/Title/Settings/Self-Explainer) und liefern die HTML-Seiten. `route_deps.py` bündelt geteilte Abhängigkeiten; `routes_self_explainer.py` bindet den [Self-Explainer](chat.md) als öffentlichen Frage-Endpoint ein. `routes_market.py` enthält neben der Legacy-Market-Research-Seite auch `GET /twitch/api/v2/market-share` (Admin-Gate via `_require_v2_admin_api`): ein dünner Proxy auf den Rust-Worker (`/internal/twitch/v1/market-share`, 8776), der Marktanteils-Zeitreihen aus `twitch_stats_category` berechnet — Datengrundlage der Admin-Seite „Markt-Dominanz".

### _compat.py
- `export_lazy(globals_dict, target, *, public=None)` / `export_name_map(globals_dict, exports)` — machen die 6-Zeilen-Shim-Module zu Lazy-Re-Exports der Feature-Pakete (alte Importpfade bleiben gültig).

## 6. Datenbank & externe Schnittstellen

- **DB:** `dashboard_sessions`, `oauth_state_tokens`, `streamer_plans` + Billing-/Entitlement-Tabellen, Affiliate-/Gutschrift-/PII-Tabellen, Streamer-Tabellen.
- **HTTP-Routen:** vollständige Liste in [../API.md](../API.md) (`/twitch/dashboard`, `/analyse`, `/twitch/admin`, `/twitch/abo`, Affiliate-Portal, `/twitch/impressum|datenschutz|agb`).
- **Extern:** Discord-OAuth (Admin + Link), Twitch-OAuth (Raid), Stripe (Checkout/Connect/Invoices/Webhooks), E-Mail (Affiliate).

## 7. Stolperfallen / Besonderheiten

- **Bot-Mixin ≠ App-Factory:** `TwitchDashboardMixin` (Cog) und `build_v2_app` (Service) sind getrennte Welten. Im reinen Dashboard-Prozess gibt es **keinen** Bot — Bot-Zustand kommt über `DashboardBotService`/interne API (siehe [bot-core.md](bot-core.md)).
- **Compat-Shims nicht „aufräumen“:** Die 6-Zeilen-Module (`auth_mixin.py`, `billing_mixin.py`, `legal_mixin.py`, …) sehen wie Dead-Code aus, sind aber Lazy-Re-Exports für alte Importpfade — Löschen bricht Importe.
- **Admin-Auth = Discord-Gilden-Mitgliedschaft:** Admin-Zugang hängt an `_check_discord_admin_membership`, nicht an einem statischen User. No-Auth nur per ENV + Loopback (siehe [bot-core.md](bot-core.md) `require_noauth_loopback_guard`).
- **Provisions-Verarbeitung ist gelockt:** `_affiliate_commission_lock_key` (Advisory-Lock) verhindert doppelte Provisions-/Gutschrift-Verarbeitung bei parallelen Webhooks.
- **Caddy-Reload-Falle:** Bei Änderungen an der Legal-Gate-/Caddy-Konfiguration muss Caddy ggf. **neu gestartet** werden (`docker restart caddy`) — siehe [Caddy-Setup-Memory] / [LEGAL_ACCESS_GATE.md](../LEGAL_ACCESS_GATE.md).
- **In-App-Changelog ist create-only:** Die `internal_home_changelog`-Spiegelung kennt nur POST (Loopback=Admin), kein Edit/Delete — vgl. Memory.
