# EVIDENCE: Prüfer-Login (Google YouTube-Quota-Audit) am Streamer-Dashboard

Bestandsaufnahme, jede Zeile eine echte Fundstelle. Alle Pfade relativ zu
`/home/nathanael/Documents/Deadlock-Twitch-Bot`.

## 1. Dashboard-Session nach dem Twitch-OAuth-Callback

Die Session, die der Demo-Login wiederverwenden muss, entsteht in genau einer Funktion.

- `rust/crates/tb-dashboard-api/src/auth/session.rs:468`: `create_partner_session(&self, twitch_login, twitch_user_id, display_name) -> Result<SessionCreation, sqlx::Error>`. Das ist die Prägefunktion. Sie legt session_id plus csrf_token an, verschlüsselt einen Payload per Fernet und persistiert ihn in `dashboard_sessions` (Typ `twitch`).
- `rust/crates/tb-dashboard-api/src/auth/session.rs:460-467`: dokumentierter Payload mit `twitch_login`, `twitch_user_id`, `display_name`, `is_partner`, `csrf_token`, `created_at`, `expires_at = now + 6h`. TTL hartkodiert, kein Env-Override.
- `rust/crates/tb-dashboard-api/src/auth/session.rs:176-180`: `pub struct SessionCreation { session_id: String, csrf_token: String }` (Rückgabe der Prägefunktion).
- `rust/crates/tb-dashboard-api/src/auth/session.rs:350`: `pub const PARTNER_COOKIE_NAME: &str = "twitch_dash_session"` (Cookie-Name).
- `rust/crates/tb-dashboard-api/src/auth/session.rs:347`: `pub const SESSION_CREATE_TTL_SECS: u64 = 6 * 3600` (Session-Lebensdauer).
- `rust/crates/tb-dashboard-api/src/auth/session.rs:241`: `pub fn build_session_cookie(...)` baut den Set-Cookie-Header (HttpOnly, SameSite=Lax, Secure in Prod).
- `rust/crates/tb-dashboard-api/src/auth/session.rs:53`: `pub struct PartnerSession` (geladene Session-Sicht bei jedem Request).

Der OAuth-Callback ruft diese Prägefunktion so auf, dieses Muster kopiert der Demo-Login:

- `rust/crates/tb-dashboard-api/src/handlers/auth_login.rs:395-402`: `state.create_partner_session(&partner.twitch_login, &partner.twitch_user_id, &identity.display_name)`.
- `rust/crates/tb-dashboard-api/src/handlers/auth_login.rs:419-425`: Cookie via `build_session_cookie(PARTNER_COOKIE_NAME, &session.session_id, config.cookie_secure, SameSite::Lax, SESSION_CREATE_TTL_SECS)`.
- `rust/crates/tb-dashboard-api/src/handlers/auth_login.rs:758`: `fn redirect_with_cookie(location, cookie) -> Response` (302 plus Set-Cookie).

Session-Validierung bei jedem Request, kein Sonderfall für Demo nötig:

- `rust/crates/tb-dashboard-api/src/auth/level.rs`: `impl FromRequestParts for DashboardAuthLevel` liest `twitch_dash_session` und löst über `DashboardAuthState` zu `DashboardAuthLevel::Partner { twitch_login, twitch_user_id, display_name }` auf. Fehlt Extension oder Session, wird es `None` (fail-closed).
- `rust/crates/tb-dashboard-api/src/auth/session.rs:1-8` (Modul-Doku): "wir prüfen das Partner-Gate bei JEDEM Request (mit 5s-Cache)". Ein departnerter oder geblockter Account verliert sofort Zugriff.
- `rust/crates/tb-dashboard-api/src/auth/session.rs`: `fernet_key_from_env()` liest den Fernet-Key aus Env `SESSIONS_ENCRYPTION_KEY`, identisch für Session-Verschlüsselung und Rate-Limit.

Partner-Gate beim Login, relevant für die Bindung (siehe Punkt 4):

- `rust/crates/tb-dashboard-api/src/handlers/auth_login.rs:344-368`: `find_partner_for_login(login, user_id)`. Kein Partner ergibt HTTP 403 "nicht als Streamer-Partner freigegeben", KEINE Session.
- `rust/crates/tb-dashboard-api/src/handlers/auth_login.rs:384-393`: `reactivate_partner(...)` (Self-Heal von departnered/archived auf active).

## 2. Routenstruktur des Dashboards

Login- und Callback-Routen liegen alle im Auth-Router (`build_auth_router`, `tb-dashboard-api/src/lib.rs`):

- `rust/crates/tb-dashboard-api/src/lib.rs:1028-1031`: `route("/twitch/auth/login", get(auth_login::login_handler))` mit Rate-Limit-Layer.
- `rust/crates/tb-dashboard-api/src/lib.rs:1035-1038`: `route("/twitch/auth/callback", get(auth_login::callback_handler))`.
- `rust/crates/tb-dashboard-api/src/lib.rs:1042-1045`: `route("/callback/twitch", get(auth_login::shared_callback_handler))`.
- `rust/crates/tb-dashboard-api/src/lib.rs:1048`: `route("/twitch/auth/logout", get(auth_login::logout_handler))`.
- `rust/crates/tb-dashboard-api/src/lib.rs:1050-1086`: Discord-Admin-Login-Familie (`/twitch/auth/discord/login`, `/callback/discord`, Fingerprint-Seite). Das ist das nächste Vorbild für einen zusätzlichen Passwort-Login.
- `rust/crates/tb-dashboard-api/src/lib.rs:1019-1024`: Rate-Limit-Buckets `RateLimitLayerConfig::new(rate_limiter, "auth_login", 30, 60)`, `"auth_callback"` 30/60, `"discord_admin_login"` 10/60. Muster für einen `"demo_login"`-Bucket.

Frontend-Auslieferung ist eine server-seitig gegatete SPA-Shell (dashboard_v2), gerendert vom Rust-Dienst, kein separater Node-Server:

- `rust/crates/tb-dashboard-api/src/handlers/spa.rs:81`: `serve_dashboard_v2_index()`; `:219` `serve_dashboard_v2_index_with_asset_prefix(...)`.
- `rust/crates/tb-dashboard-api/src/handlers/spa.rs:602-603`: `dist_root()` liest Env `DASHBOARD_V2_DIST_PATH` (Default `bot/analytics/dashboard_v2/dist`).
- `rust/crates/tb-dashboard-api/src/handlers/spa.rs:111-125`: `main_domain_spa_shell_gated_handler` (die eingeloggte Shell).
- `rust/crates/tb-dashboard-api/src/lib.rs:1529-1540`: Routen `/twitch/dashboard`, `/twitch/verwaltung`, `/twitch/uplink` auf `spa::main_domain_spa_shell_gated_handler`.

Vorbild für eine server-gerenderte Login-Seite mit Formular (statt Twitch-Redirect):

- `rust/crates/tb-dashboard-api/src/auth/discord_admin_login.rs:647`: `fingerprint_page_handler` (HTML-Seite).
- `rust/crates/tb-dashboard-api/src/auth/discord_admin_login.rs:678`: `fingerprint_submit_handler` (POST-Verarbeitung).

## 3. Wie das Frontend "eingeloggt" erkennt

Die Entscheidung ist server-seitig, nicht clientseitig, deshalb bricht ein Demo-Streamer nichts:

- `rust/crates/tb-dashboard-api/src/handlers/spa.rs:168-190`: `shell_gate_decision(auth, landing_allowed, path)`. `DashboardAuthLevel::None` gibt 303 Redirect auf `shell_login_url(path)` (`/twitch/auth/login?next=...`); `Partner` mit `landing_allowed` gibt die Shell; sonst 403.
- `rust/crates/tb-dashboard-api/src/handlers/spa.rs:193-218`: `check_shell_auth(...)` lädt `tb_analytics::partner_access::load_partner_access_state` und liest nur `twitch_login`, `twitch_user_id`, `display_name` aus der Session (keine Twitch-Live-Abfrage pro Request).
- `rust/crates/tb-dashboard-api/src/lib.rs:177-180`: `route("/twitch/api/v2/auth-status", get(auth_status::auth_status_handler))` (Client-Statusabfrage).

Belegt: da die Session an eine echte Twitch-User-ID gebunden ist und die Shell nur Session-Felder (login, user_id, display_name) nutzt, gibt es keine Twitch-spezifische Live-Annahme, die bei einem Wegwerf-Konto bricht.

## 4. Streamer-Freischaltung

Voll bedienbar bedeutet aktiver Partner mit Landing-Freigabe:

- `rust/crates/tb-dashboard-api/src/handlers/auth_login.rs:344-368`: Partner-Gate. Konto muss in `twitch_partners` liegen, sonst 403.
- `rust/crates/tb-dashboard-api/src/handlers/auth_login.rs:1210-1214`: Tabellenschema `twitch_partners (twitch_login, twitch_user_id, status TEXT NOT NULL DEFAULT 'active', ...)`.
- `rust/crates/tb-dashboard-api/src/handlers/spa.rs:193-212`: Shell-Gate braucht `access.landing_access_allowed == true` (`tb_analytics::partner_access::load_partner_access_state`).
- `rust/crates/tb-dashboard-api/src/handlers/uplink.rs:636-676`: `freischaltung_antwort` und `admin_freischalten_handler(auth, Json{streamer_id})` schalten Uplink-Wartelisteneinträge über den Relay-Admin-Pfad frei (admin-gegated per `admin_pruefen`). Das betrifft die Uplink- und YouTube-Verbinden-Warteliste, nicht den Basis-Dashboard-Zugang.

Konsequenz für das Wegwerf-Konto: einmalig als aktiver Partner mit `landing_access_allowed=true` eintragen. YouTube-Verbinden und VOD-Archiv laufen dann über die normalen Uplink-Knöpfe (interaktiv durch den Prüfer).

## 5. Infisical-Leseweg im Bot

Secrets kommen als Prozess-Env über einen Infisical-Wrapper und werden per `std::env::var` gelesen:

- `rust/scripts/run_tb_dashboard_service.sh`: sourcet Config, holt `INFISICAL_SERVICE_TOKEN` aus systemd-Credentials, startet den Dienst unter Infisical.
- `scripts/run_with_infisical.sh`: generischer Wrapper, lädt Infisical-Secrets in den übergebenen Befehl (Config `$HOME/.config/deadlock-twitch-bot/infisical.conf`).
- `rust/crates/tb-dashboard-api/src/handlers/auth_login.rs:782-808`: `oauth_login_config_from_env()` liest `TWITCH_CLIENT_ID`, `TWITCH_CLIENT_SECRET`, `TWITCH_DASHBOARD_AUTH_REDIRECT_URI` via `non_empty_env`. Fehlt eins, wird die Config `None` und der Login bleibt aus. Genau dieses Muster gilt für D2.
- `rust/crates/tb-dashboard-api/src/handlers/auth_login.rs:841-846`: `non_empty_env(key)` (trim plus Leer-Filter).
- Namens-Konvention: UPPER_SNAKE mit Präfix `TWITCH_`, `TB_DASHBOARD_` oder `SESSIONS_`. Fehlendes Secret ergibt Config-`None` und damit keine Route. Für den Demo-Login: `None` ergibt Handler-Antwort 404 (D2).

## 6. Rate-Limit- und Passwort-Hash-Bestand

Rate-Limit vorhanden und wiederverwendbar:

- `rust/crates/tb-dashboard-api/src/auth/security.rs:35`: `pub struct RateLimiter { pool, fernet_key }`.
- `rust/crates/tb-dashboard-api/src/auth/security.rs`: `allow(key, max, window)` (atomares Sliding-Window in `dashboard_sessions`, fail-open bei DB-Fehler), `allow_login(key)` mit Defaults `LOGIN_MAX_REQUESTS=10` und `LOGIN_WINDOW_SECS=60`.
- `rust/crates/tb-dashboard-api/src/auth/security.rs:152`: `bucket_prefix(key, window)` gleich `rl:{window}:{sha256(key)}`.
- `rust/crates/tb-dashboard-api/src/lib.rs:1019`: `RateLimitLayerConfig::new(...)` als Axum-Middleware-Layer pro Route.

Passwort-Hash: nicht im Workspace vorhanden.

- `rust/Cargo.toml`: kein `argon2`, kein `bcrypt`.
- `rust/Cargo.lock:1738`: nur `pbkdf2` transitiv, nicht als direkte Bot-Abhängigkeit für Passwörter genutzt.
- `rust/crates/tb-crypto/src/` (`aad.rs`, `field.rs`, `lib.rs`, `token.rs`): bietet Fernet, `random_urlsafe_token`, `constant_time_eq`, aber kein Passwort-Hashing.
- Folge: `argon2` (mit `password-hash`) als direkte Abhängigkeit in `tb-dashboard-api` ergänzen. Passwort als PHC-Hash in Infisical, Verifikation konstantzeitig.

## 7. Caddy-Routing

`/twitch/*` ist kein Wildcard-Proxy, sondern eine explizite Pfad-Allowlist:

- `/etc/caddy/Caddyfile:612-614`: `@public_twitch { path /analyse ... /twitch/auth/login /twitch/auth/callback /twitch/auth/logout ... }` (jeder Pfad einzeln gelistet).
- `/etc/caddy/Caddyfile:615-624`: `handle @public_twitch { reverse_proxy 127.0.0.1:8769 ... X-Dashboard-Context public }` (Dashboard-Dienst).
- `/etc/caddy/Caddyfile:120-122`: Callbacks laufen über eigene `@callback_*`-Handles (`/callback/twitch` auf 8769).
- `/etc/caddy/Caddyfile:194-195`: `@non_demo_embed`-CSP-Liste (strikte CSP für alle Pfade, die dort nicht ausgenommen sind).
- `/etc/caddy/Caddyfile:207-211`: `@dashboard_paths`-CSP für die SPA-Seiten.

Folge: ein neuer Pfad unter `/twitch/...` erreicht 8769 nur, wenn er in `@public_twitch` aufgenommen wird. Caddy-Edit ist Pflicht. Eine reine Formular- und POST-Seite kommt mit der Default-CSP aus, solange kein Inline-Script nötig ist.
