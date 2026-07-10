# Host- & Pfad-Routing-Vertrag (Dashboard)

> **Zweck:** Wiederkehrende „falscher Pfad / Not Found"-Bugs an der Caddy-↔-Backend-Grenze
> ein für alle Mal nachschlagbar machen. Wer einen Redirect, eine OAuth-Redirect-URI oder
> einen Caddy-Block anfasst, liest **zuerst hier**.

## TL;DR

Es gibt **zwei Hosts**, die auf **dasselbe** Backend (`tb-dashboard`, `127.0.0.1:8769`) zeigen:

| Host | Rolle | `/analyse`, `/social-media`, generisches `/twitch/*` |
|------|-------|------------------------------------------------------|
| `deutsche-deadlock-community.de` | öffentlich / Partner | **erlaubt** |
| `admin.deutsche-deadlock-community.de` | Admin-Dashboard | **bewusst 404** |

Caddy unterscheidet die beiden nur über den weitergereichten `Host`-Header
(`header_up Host …`) plus `X-Dashboard-Context: admin` für Admin-Flows. **Derselbe
Pfad verhält sich je nach Host unterschiedlich.** Darum gilt die eine goldene Regel:

> **Jeder Redirect, den ein Handler erzeugt, der vom Admin-Host erreichbar ist, muss
> host-bewusst sein. Ein nackter relativer Pfad (`/analyse`) wird vom Browser gegen den
> *aktuellen* Host aufgelöst — auf `admin.*` ist das ein 404.**

## Warum `/analyse` auf dem Admin-Host 404 ist (kein Pfusch, Absicht)

User-sichtbare Dashboard-Seiten dürfen nicht unter der Admin-Subdomain leben. Das ist an
**zwei** Stellen abgesichert (defense in depth):

1. **Caddy** — `admin.deutsche-deadlock-community.de`-Block, Matcher `@admin_block_nonadmin`:
   `path /analyse /analyse/* /twitch/* /social-media* /demo*` → `respond "Not Found" 404`.
2. **Backend** — `handlers/spa.rs::admin_dashboard_host_page_gate()` → liefert `404` für
   user-facing Seiten, wenn `is_admin_dashboard_host_request(headers)` wahr ist.

Was der Admin-Host **erlaubt** (Reihenfolge = Caddy-`handle`-Auswertung, erster Treffer gewinnt):

- `/twitch` , `/twitch/` → `redir /twitch/admin`
- `/twitch/admin`, `/twitch/admin/*` → `forward_auth` (Session-Gate) → Backend.
  Antwortet `/twitch/auth/validate` mit **401/403**, redirectet Caddy auf den Discord-Login
  (`handle_response @unauthorized`). Das Gate darf **niemals** an der bloßen *Präsenz* des
  `master_dash_session`-Cookies hängen, nur an dessen *Gültigkeit*: ein abgelaufenes oder per
  Device-Bindung (IP/Passive-FP, `forward_auth.rs`) verworfenes Cookie bleibt im Browser stehen
  und sperrte den Admin sonst dauerhaft aus — nackter 401, Login unerreichbar (Vorfall 2026-07-10).
- `/twitch/auth/*` → Login-/Logout-/Fingerprint-Flows → Backend
- `@twitch_admin_support` → explizite Admin-API-Allowlist (`/twitch/api/admin/*`,
  `/twitch/auth/logout`, `/twitch/auth/discord/logout`, …) → Backend
- alles andere user-facing → **404** (siehe oben)

## Die wiederkehrende Bug-Klasse

> **„1:1 aus Python portierter Relativ-Redirect."** Das Python-Dashboard lief auf **einem**
> Host, dort war `/analyse` immer gültig. Beim Rust-Cutover wurden Redirect-Ziele wörtlich
> übernommen (`LOGOUT_REDIRECT = "/analyse"`). Sobald derselbe Handler über die
> Admin-Subdomain erreichbar ist, zeigt der Relativpfad ins Leere (404).

**Gegenmittel beim Anfassen eines Redirects:**

1. Kann dieser Handler vom **Admin-Host** erreicht werden? (Caddy-Allowlist prüfen:
   `/twitch/auth/*` und `@twitch_admin_support` → **ja**.)
2. Wenn ja und das Ziel ist eine **öffentliche** Seite (`/analyse` etc.): **absolute URL**
   auf `https://deutsche-deadlock-community.de` ausgeben, nicht relativ.
3. Wenn das Ziel auf dem Admin-Host gültig ist (`/twitch/admin`, `/twitch/auth/discord/login`):
   Relativpfad ist ok.
4. Host-Erkennung **nie neu erfinden** → `handlers::spa::is_admin_dashboard_host_request(&HeaderMap)`
   nutzen (vergleicht `Host` gegen `configured_admin_dashboard_host()`).

## Wo Redirect-Ziele wohnen (Audit-Checkliste)

| Stelle | Datei | Host-bewusst? |
|--------|-------|---------------|
| Partner-Logout `/twitch/auth/logout` | `handlers/auth_login.rs::logout_handler` | **ja** (host-aware: öffentliche `/analyse` absolut, sonst relativ) |
| Discord-Admin-Logout `/twitch/auth/discord/logout` | `auth/discord_admin_login.rs::logout_handler` | ja (`admin_route_url(base, ADMIN_LOGIN_PATH)`) |
| Admin-Login-Redirects | `auth/discord_admin_login.rs` | ja (`DEFAULT_ADMIN_BASE_URL` + ENV) |
| OAuth-Redirect-URIs | siehe Memory `dashboard_oauth_redirect_migration` | ENV-gesteuert, pro Host getrennt |
| user-facing SPA-Gate | `handlers/spa.rs::admin_dashboard_host_page_gate` | ja |

## Relevante ENV / Konstanten

- `TWITCH_ADMIN_PUBLIC_URL` / `MASTER_DASHBOARD_PUBLIC_URL` → Admin-Host-Erkennung & Base-URL
  (`configured_admin_dashboard_host`, `admin_base_url_from_env`). Default
  `https://admin.deutsche-deadlock-community.de`.
- `TWITCH_PUBLIC_URL` / `DASHBOARD_PUBLIC_URL` → öffentliche Base-URL für host-aware Redirects.
  Default `https://deutsche-deadlock-community.de`.

## Verwandte Doku

- Caddy-Topologie & Deploy: Memory `reference_caddy_reverse_proxy_setup`,
  `reference_twitch_dashboard_deploy_runbook`.
- OAuth-Redirect-Migration nach Cutover: Memory `dashboard_oauth_redirect_migration`.
- Caddyfile: `/home/naniadm/Documents/Caddy/conf/Caddyfile`, Block
  `admin.deutsche-deadlock-community.de` — am `@admin_block_nonadmin`-Matcher steht ein
  Verweis zurück auf dieses Dokument.
