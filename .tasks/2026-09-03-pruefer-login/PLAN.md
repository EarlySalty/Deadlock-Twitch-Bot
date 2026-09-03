# PLAN: Prüfer-Login für das Streamer-Dashboard

Minimaler Umsetzungsplan. Klasse hoch (Auth-Pfad), Stufe 4 (fremder Review)
Pflicht.

## Empfohlener Pfad

`/twitch/auth/google` mit zwei Methoden:
- `GET /twitch/auth/google`: server-gerenderte Login-Seite (Formular).
- `POST /twitch/auth/google`: Nutzername plus Passwort, prägt Session, 302 ins
  Dashboard.

Begründung: der Pfad sitzt in der bestehenden Login-Familie `build_auth_router`
(`auth_login.rs`, Geschwister `/twitch/auth/login`, `/twitch/auth/discord/login`),
nutzt denselben `RateLimitLayerConfig`-Layer und ist für Google-Prüfer
selbsterklärend. `/twitch/google` wäre auch möglich, läge aber außerhalb der
Auth-Familie und bräuchte einen eigenen Router-Zweig. Endgültige Wahl trifft die
Implementierung, der Contract-Bereich deckt beide ab.

## Backend

1. Neues Modul `rust/crates/tb-dashboard-api/src/handlers/demo_login.rs`.
   - Config `DemoLoginConfig { username, password_hash (PHC), twitch_user_id,
     display_name, cookie_secure }` plus `demo_login_config_from_env() -> Option`.
     Liest Infisical-Env `TWITCH_DEMO_LOGIN_USER`,
     `TWITCH_DEMO_LOGIN_PASSWORD_HASH`, `TWITCH_DEMO_LOGIN_TWITCH_USER_ID`,
     optional `TWITCH_DEMO_LOGIN_DISPLAY_NAME`, per `non_empty_env`-Muster aus
     `auth_login.rs:841`. Fehlt eines, wird die Config `None`.
   - `get_handler`: ist die Config `None`, 404. Sonst HTML-Login-Seite im
     Dashboard-Stil (Vorbild `discord_admin_login::fingerprint_page_handler`,
     `discord_admin_login.rs:647`). Kein Inline-Script, CSP-konform.
   - `post_handler`: ist die Config `None`, 404. Sonst:
     a) Rate-Limit über den vorhandenen `RateLimiter` (IP-Key, eigener Bucket).
        Bucket voll ergibt 429.
     b) Nutzername konstantzeitig vergleichen (`tb_crypto::constant_time_eq`).
     c) Passwort gegen `password_hash` via `argon2`/`password-hash` verifizieren.
        Fehlschlag ergibt 401, keine Session.
     d) Erfolg: `state.create_partner_session(&login_der_user_id, &user_id,
        &display_name)` (Prägefunktion `session.rs:468`), Cookie
        `build_session_cookie(PARTNER_COOKIE_NAME, &session.session_id,
        cookie_secure, SameSite::Lax, SESSION_CREATE_TTL_SECS)`, dann
        `redirect_with_cookie("/twitch/dashboard", &cookie)`.
        Für den Login-Namen die kanonische `twitch_login`-Spalte des gebundenen
        Partners bevorzugen (wie der OAuth-Callback, `auth_login.rs:395`), damit
        das Partner-Gate greift.
   - Kein Admin-Mode-Cookie, kein Promotion-Pfad. Ergebnis ist reiner Partner.
2. `handlers/mod.rs`: Modul registrieren.
3. `lib.rs`: im `build_auth_router` (bei `lib.rs:1028`) die zwei Routen mit einem
   `RateLimitLayerConfig::new(rate_limiter.clone(), "demo_login", 10, 60)`
   ergänzen und die `DemoLoginConfig` als Extension injizieren (analog
   `OAuthLoginConfig`).
4. `Cargo.toml` von `tb-dashboard-api`: `argon2` (mit `password-hash`) als direkte
   Abhängigkeit ergänzen. Keine Default-Features, die ungenutzt sind.

## Frontend (Login-Seite)

- Server-gerenderte HTML-Seite aus dem Rust-Handler, kein SPA-Umbau (die SPA ist
  gegatet und hätte kein Passwortfeld). Gold-Look an der Twitch-UI orientiert,
  externe/erlaubte Styles, kein Inline-Script wegen CSP.
- Nach Login übernimmt die bestehende dashboard_v2-Shell
  (`main_domain_spa_shell_gated_handler`), keine Änderung nötig.

## Infisical

- Secrets im Twitch-Projekt: `TWITCH_DEMO_LOGIN_USER`,
  `TWITCH_DEMO_LOGIN_PASSWORD_HASH` (argon2 PHC-String),
  `TWITCH_DEMO_LOGIN_TWITCH_USER_ID`, optional `TWITCH_DEMO_LOGIN_DISPLAY_NAME`.
- Der Dienst liest sie als Prozess-Env (Wrapper `run_tb_dashboard_service.sh` plus
  `run_with_infisical.sh`). Kein neuer Secret-Pfad, kein ENV-File.

## Caddy

- `@public_twitch`-Allowlist (`/etc/caddy/Caddyfile:612`) um den neuen Pfad
  erweitern (`/twitch/auth/google`), sonst erreicht der Request 8769 nicht.
- Prüfen, ob die Default-CSP (`@non_demo_embed`, Zeile 194) die Login-Seite ohne
  Inline-Script trägt. Falls die Seite eine eigene CSS-/JS-Datei braucht, den Pfad
  zusätzlich in `@dashboard_paths` (Zeile 207) aufnehmen. Ziel: kein Inline-Script,
  damit die strikte Default-CSP reicht.
- `caddy reload` nach dem Edit.

## Tests (Regression zuerst rot)

- T1: `demo_login_config_from_env` fehlend ergibt GET und POST je 404.
- T2: POST mit falschem Passwort ergibt 401, keine Session-Row, kein Set-Cookie.
- T3: POST über dem Rate-Limit ergibt 429 (Bucket `demo_login`).
- T4: POST mit korrektem Nutzernamen und Passwort prägt eine `dashboard_sessions`-
  Row vom Typ `twitch`, setzt `twitch_dash_session` und redirectet ins Dashboard;
  die Session trägt die konfigurierte `twitch_user_id`.
- T5: Der Handler prägt niemals `Admin`; ein anschließender Auth-Level-Load ergibt
  `Partner`, nicht `Admin`.
- T6 (Integration, DB): mit einer aktiven `twitch_partners`-Zeile plus
  `landing_access_allowed=true` lässt das Shell-Gate den Prüfer auf
  `/twitch/dashboard` durch.

Tests laufen mit den repo-üblichen Flags (Rolle test-waechter), Baseline gegen den
bekannten roten Stand messen.

## Betreiber-Schritte (nicht Teil des Codes)

1. Wegwerf-Twitch-Konto anlegen (echtes Twitch-Konto mit eigener User-ID). Nie ein
   echtes Streamer-Konto verwenden.
2. Das Konto als aktiven Partner mit `landing_access_allowed=true` freischalten
   (bestehender Admin-/Partner-Freischaltweg, `twitch_partners` status active).
   Optional YouTube/Uplink vor dem Prüftermin interaktiv verbinden.
3. Passwort-Hash erzeugen (argon2 PHC) und die vier Secrets in Infisical setzen.
   Fehlt eines, bleibt der Pfad 404.
4. Caddyfile-Pfad ergänzen und `caddy reload`.
5. Dashboard-Dienst neu starten und den Login live prüfen (GET zeigt Formular,
   korrektes Passwort landet im Dashboard, falsches ergibt 401).

## Risiken

- R1: Wegwerf-Konto nicht als aktiver Partner mit Landing-Freigabe eingetragen,
  dann 403 am Shell-Gate trotz erfolgreichem Login. Betreiber-Schritt 2 ist
  Pflicht.
- R2: CSP. Inline-Script auf der Login-Seite würde blockiert. Seite ohne
  Inline-Script bauen.
- R3: Caddy-Allowlist vergessen ergibt 404 oder falsches Backend. Schritt 4 ist
  Pflicht.
- R4: `argon2` ist eine neue Abhängigkeit. Minimal halten, Dependabot-Scan
  danach prüfen.
- R5: Der Handler darf nur die eine konfigurierte User-ID binden, sonst wird der
  Weg ein allgemeines Passwort-Login. INV-9 im Test absichern.
- R6: `SESSIONS_ENCRYPTION_KEY` muss gesetzt sein (bereits Prod-Voraussetzung für
  das ganze Dashboard), sonst kann keine Session geprägt werden.
