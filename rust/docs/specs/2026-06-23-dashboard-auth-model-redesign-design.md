# Dashboard-Auth-Modell: Redesign (Partner-Default + Twitch-Vorrang + CSRF-Fix)

**Datum:** 2026-06-23
**Status:** Design freigegeben (User), Umsetzung offen
**Crate:** `tb-dashboard-api` (Service `deadlock-twitch-dashboard-rust`, Port 8769)

## Problem

Im Streamer-Verwaltungs- und Analyse-Dashboard (`/twitch/verwaltung`, `/twitch/analyse`)
scheitern fast alle Widgets. Drei unabhängige Wurzelursachen:

- **RC1 — Admin-Promotion ist rein präsentativ.** `TWITCH_ADMIN_LOGINS = ["earlysalty"]`
  promotet die Twitch-OAuth-Session beim Login **immer** zu `Admin`
  (`level.rs::partner_or_admin`). Das `tb_admin_mode`-Cookie steuert nur die
  *Präsentation* in `auth-status`, nicht den echten Auth-Level. Folge:
  - `resolve_partner`-Handler (onboarding, tip) lehnen `Admin` ab → **„partner required"** (403).
  - `resolve_login`-Handler (silent-settings, scam-guard, …) verlangen von `Admin` ein
    `?streamer=` → **„streamer required"** (400), das das Frontend nicht mitschickt.
- **RC2 — `Localhost`-Level + Discord-Admin können den Streamer-Pfad kapern.** Die Kaskade
  prüft `Localhost` → `X-Admin-Token` → `master_dash_session` (Discord-Admin → `Admin{None}`)
  **vor** der Twitch-Session. Liegt ein Discord-Admin-Cookie vor, gewinnt es → `Admin{None}`
  ohne Twitch-Identität → Streamer-Dashboard nicht scopebar → buggt.
- **RC3 — alle v2-Schreib-POSTs hängen hinter hartem `csrf_protect`** (`lib.rs:556`,
  umschließt `build_authed_router` inkl. Engagement-Toggle). `auth-status` liefert aber
  `csrfToken: null` → das Frontend kann kein `X-CSRF-Token` senden → **„csrf_failed"** (403).
  Bekannter Vorfall #235.

## Ziele

1. Nach Twitch-OAuth-Login gilt earlysalty **standardmäßig als Partner**; Admin nur, wenn der
   Admin-Mode-Toggle aktiv ist. Selbst-Hoch-/Runterstufung ohne erneuten Login.
2. **Admin ohne Twitch-Login** ist auf den Streamer-Dashboards **nicht** möglich.
3. **Kein `Localhost`-Zugang** mehr — außer dem internen Changelog-Endpoint.
4. v2-Schreib-Aktionen (Engagement-Toggle etc.) funktionieren wieder (CSRF-Fix).
5. Das **separate** Admin-Dashboard (`admin.deutsche-deadlock-community.de/twitch/admin`,
   forward_auth + `is_privileged`) bleibt über Discord-Admin erreichbar (unverändert).

## Nicht-Ziele

- Keine Änderung am separaten Admin-Dashboard-Funktionsumfang.
- Kein interner Ersatz-Token für Localhost (bewusst verworfen).
- `resolve_partner`-Handler (onboarding/tip) im *Admin-Mode* mit `?streamer=` bedienbar zu
  machen ist **Follow-up**, nicht Teil dieses Tickets (Default-Partner-Pfad ist der Fix).

## Design

### 1. Auth-Kaskade umbauen (`auth/level.rs`)

Neue Reihenfolge im `FromRequestParts`-Extractor:

1. **Twitch-Session** (`twitch_dash_session`, dann `twitch_dash_session_partner`) →
   `partner_or_admin(partner, admin_mode_aktiv)`:
   - Login in `TWITCH_ADMIN_LOGINS` **und** `tb_admin_mode=2`-Cookie → `Admin{ actor: Some(..) }`
   - sonst → `Partner{ .. }` **(Default, auch für earlysalty ohne Cookie)**
2. **`master_dash_session`** (Discord-Admin) → `Admin{ actor: None }` — **nur erreicht, wenn
   keine gültige Twitch-Session vorlag** (Twitch hat Vorrang).
3. `None`.

Entfällt:
- Der `is_local_request`-Kurzschluss (kein `Localhost`-Level mehr aus der Kaskade).
- Der `X-Admin-Token`-Header-Pfad (`admin_token_matches`) — dormant, nicht in Infisical.

`is_admin_login(login: &str) -> bool` wird als `pub(crate)`-Helfer exportiert
(Membership in `TWITCH_ADMIN_LOGINS`), genutzt von `auth-status` und dem Admin-Toggle.

`is_local_request` **bleibt als Helfer erhalten**, wird aber nur noch vom Changelog-Handler
genutzt (siehe 5).

### 2. `Localhost`-Variante entfernen

`DashboardAuthLevel::Localhost` wird aus dem Enum entfernt. Mechanische Folgen
(~60 Stellen, inkl. Tests):
- `Localhost | Admin{..}`-Arme → nur noch `Admin{..}`.
- `is_privileged()` = `matches!(self, Admin{..})`.
- `auth_status.rs` Localhost-Arm raus; Tests, die `DashboardAuthLevel::Localhost` als
  Fixture bauen, auf `DashboardAuthLevel::admin()` umstellen.
- Reine Localhost-Spezialfälle (`stream_report` writer-key `"localhost"`,
  `auth_status` `level:"localhost"`) entfallen.

### 3. Admin-Toggle-Gate lockern (`handlers/admin_mode.rs`)

Heute: `matches!(auth, Admin{ actor: Some(_) })`. Problem: nach RC1 ist earlysalty bei
ausgeschaltetem Admin-Mode `Partner` und könnte den Toggle nie *einschalten* (Deadlock).

Neu: Gate = „die Session trägt einen admin-eligiblen Twitch-Login". D. h. aus `auth` einen
Twitch-Login extrahieren (`Partner{twitch_login}` **oder** `Admin{actor:Some}`) und gegen
`is_admin_login()` prüfen. Discord-`Admin{None}` und `None` → 403. Router bleibt csrf-frei.

### 4. `auth-status` (`handlers/auth_status.rs`)

- Localhost-Arm entfernen.
- `Admin{ actor: Some(actor) }` → immer Admin-Präsentation (die Kaskade liefert diese
  Variante künftig nur noch *mit* aktivem `tb_admin_mode`-Cookie; die Cookie-Prüfung im
  Handler wird damit redundant und entfällt).
- `Partner{login,..}`-Arm: `adminEligible = is_admin_login(login)` setzen, damit das Frontend
  den „Admin aktivieren"-Toggle für earlysalty weiter anzeigt. `adminMode=false`.
- `Admin{ actor: None }` (Discord-Admin) → unverändert Admin-Präsentation.

### 5. Changelog behält Loopback-Bypass (`handlers/internal_home.rs`)

`changelog_handler` matcht heute `DashboardAuthLevel::Localhost`. Nach Wegfall der Variante
prüft der Handler selbst per `is_local_request(&parts)` (zusätzlicher Extractor für `Parts`).
Loopback **oder** `Admin{..}` → erlaubt; sonst 401/403. Same-Origin-Guard für Browser-Admins
bleibt. Optional: Loopback-Check zusätzlich auf `X-Forwarded-For=Loopback` härten
(Caddy reicht `X-Forwarded-For {remote_host}` bereits durch) — Defense-in-Depth.

### 6. CSRF-Fix (`auth/csrf.rs`)

`csrf_protect` akzeptiert künftig als gültig:
- ein korrektes `X-CSRF-Token` (bisheriger Pfad, für Clients die eins haben), **ODER**
- einen **same-origin** Request (`is_allowed_origin(headers)` == nicht `CrossOrigin`) **mit**
  gültiger Session (Admin- oder Partner-Cookie vorhanden).

Cross-Origin-POSTs bleiben 403. Damit gehen alle v2-Schreib-POSTs (csrfToken null) wieder
durch, ohne den Token-Pfad für token-fähige Clients zu entfernen. Konsistent zur #235-Doktrin
(SameSite=Lax-Cookies + Origin-Check). Der bisherige `is_local_request`-Bypass in
`csrf_protect` bleibt für den internen Changelog-Loopback bestehen.

## Betroffene Dateien (Blast-Radius)

- `auth/level.rs` — Kaskade, `Localhost`-Removal, `is_admin_login`, Cookie-Threading.
- `auth/csrf.rs` — Origin-Fallback im `csrf_protect`.
- `handlers/auth_status.rs` — Localhost-Arm raus, `adminEligible` für Partner.
- `handlers/admin_mode.rs` — Toggle-Gate.
- `handlers/internal_home.rs` — Changelog-Loopback-Eigenprüfung.
- ~60 Handler/Tests mit `DashboardAuthLevel::Localhost`-Arm oder -Fixture (mechanisch).

## Tests (TDD — Red zuerst)

- `level.rs`: earlysalty + Twitch-Session **ohne** Cookie → `Partner`; **mit** `tb_admin_mode=2`
  → `Admin{Some}`. Twitch-Session **schlägt** vorhandenes `master_dash_session`. Reines
  `master_dash_session` ohne Twitch → `Admin{None}`. Kein Loopback-Pfad mehr (Loopback-Host
  + Loopback-Peer → `None`, nicht `Localhost`).
- `admin_mode.rs`: `Partner{earlysalty}` darf togglen; `Partner{andererLogin}` → 403;
  `Admin{None}` → 403.
- `auth_status.rs`: `Partner{earlysalty}` → `adminEligible:true, isAdmin:false`;
  `Partner{anderer}` → `adminEligible:false`.
- `csrf.rs`: same-origin POST ohne Token, mit Session-Cookie → durch; cross-origin POST → 403;
  fehlende Session → 403.
- Regressionsschutz: alle bisherigen `Localhost`-Tests auf `admin()` migriert, bleiben grün.

## Verifikation (live, nach Build+Restart)

1. **Artefakt:** im neuen Binary nach den Auth-Strings greppen (kein stale Cache).
2. `cargo build --release --bin tb-dashboard` → `systemctl --user restart
   deadlock-twitch-dashboard-rust`.
3. Mit **echter** Twitch-Session (earlysalty) prüfen: Verwaltung/Analyse laden ohne
   „partner/streamer required"; Engagement-Toggle ohne „csrf_failed"; Admin-Toggle schaltet
   hoch/runter.
4. Separates Admin-Dashboard (`/twitch/admin`) via Discord-Admin weiter erreichbar.
5. Changelog-Spiegelung (Loopback) funktioniert weiter.
6. Journal sauber (0 Errors).

## Risiken / Caveats

- **Localhost-Removal trifft Admin-Routen:** interne Loopback-Aufrufe auf Admin-APIs (außer
  Changelog) verlieren den Zugang. Vor Merge prüfen, ob reale interne Caller existieren
  (Erstbefund: nur Changelog); falls doch → melden, nicht stillschweigend brechen.
- **Admin-Mode-Vollständigkeit:** onboarding/tip (`resolve_partner`) liefern im *Admin*-Mode
  weiter „partner required". Bewusst Follow-up — Default-Partner-Pfad ist gefixt.
- **CSRF-Posture:** Wechsel von Token- auf Origin-basiert für v2-POSTs. Code hat das nach #235
  bereits so entschieden; wir ziehen es konsequent durch.
