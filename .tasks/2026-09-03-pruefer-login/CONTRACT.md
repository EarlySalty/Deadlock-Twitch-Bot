# CONTRACT: Prüfer-Login für das Streamer-Dashboard

status: research abgeschlossen, wartet auf Umsetzungsfreigabe

## Ziel

Google-Prüfer (YouTube-API-Quota-Audit) ohne Twitch-Konto sollen sich per
Nutzername und Passwort an einem Pfad der bestehenden Domain anmelden und danach
im identischen Streamer-Dashboard landen wie ein normaler Twitch-Streamer. Der
Demo-Login prägt die normale Dashboard-Session, gebunden an die echte
Twitch-User-ID eines vom Betreiber angelegten Wegwerf-Twitch-Kontos.

## REQ (prüfbar)

- REQ-1: Ein GET auf den Demo-Login-Pfad liefert eine Login-Seite im
  Dashboard-Stil mit Nutzername- und Passwortfeld, solange die Demo-Secrets
  gesetzt sind.
- REQ-2: Ein POST mit korrektem Nutzernamen und korrektem Passwort prägt eine
  normale Partner-Session über `create_partner_session` und setzt das Cookie
  `twitch_dash_session`, gebunden an die konfigurierte Twitch-User-ID. Danach 302
  ins Dashboard.
- REQ-3: Nach erfolgreichem Login sieht der Prüfer `/twitch/dashboard`,
  `/twitch/verwaltung` und `/twitch/uplink` genau wie ein normaler
  freigeschalteter Streamer (Shell-Gate lässt ihn durch).
- REQ-4: Fehlt eines der Demo-Secrets in Infisical, liefert sowohl GET als auch
  POST des Pfads HTTP 404 (Feature existiert dann nicht sichtbar).
- REQ-5: Ein POST mit falschem Passwort oder unbekanntem Nutzernamen liefert HTTP
  401 und prägt keine Session.
- REQ-6: Der Demo-Login ist ratenbegrenzt über den bestehenden `RateLimiter`
  (eigener Bucket, IP-basiert), analog zu `/twitch/auth/login`.
- REQ-7: Passwort wird ausschließlich als Hash (argon2 PHC) aus Infisical gelesen
  und konstantzeitig verifiziert. Kein Klartext-Passwort im Code, in Logs oder in
  der Antwort.
- REQ-8: Die Session-Lebensdauer ist die normale (`SESSION_CREATE_TTL_SECS`, 6h),
  kein Sonderwert.
- REQ-9: Der Handler akzeptiert nur die eine konfigurierte Twitch-User-ID als
  Bindungsziel; er löst niemals einen beliebigen Account auf.

## INV (Invarianten)

- INV-1: Kein zweites Identitätsmodell. Es entsteht dieselbe Session wie beim
  Twitch-OAuth-Login (`SessionCreation` aus `create_partner_session`, Cookie
  `twitch_dash_session`). Downstream-Handler sehen eine normale
  `DashboardAuthLevel::Partner`-Session mit Plattform-ID.
- INV-2: Session identisch zur Twitch-Session. Es wird keine neue Session-Struct,
  kein neuer Cookie-Name und kein neuer Session-Typ eingeführt.
- INV-3: Ohne gesetztes Infisical-Secret liefert der Pfad 404.
- INV-4: Kein Admin-Zugang über diesen Weg. Das Ergebnis ist ausschließlich
  `DashboardAuthLevel::Partner`, niemals `Admin`. Kein Admin-Mode-Cookie.
- INV-5: Secrets nie im Klartext im Repo oder im Log. Passwort nur als Hash aus
  Infisical, Nutzername und User-ID ebenfalls aus Infisical.
- INV-6: Die gebundene Twitch-User-ID muss ein echtes, als aktiver Partner
  freigeschaltetes Twitch-Konto sein, sonst greift dasselbe Partner-Gate wie bei
  jedem Streamer (403).

## Nicht-Ziele

- Kein allgemeines Passwort-Login für normale Streamer. Ausschließlich das eine
  konfigurierte Prüfer-Konto.
- Keine Selbstregistrierung, kein Passwort-Zurücksetzen, keine Nutzerverwaltung.
- Kein Admin-Dashboard und kein Admin-Modus über diesen Pfad.
- Keine Änderung am bestehenden Twitch-OAuth-Login oder Discord-Admin-Login.
- Kein zweiter OAuth-Weg, kein zweiter Token-Speicher.

## Erlaubter Bereich (je Zeile genau ein Pfad)

rust/crates/tb-dashboard-api/src/handlers/demo_login.rs
rust/crates/tb-dashboard-api/src/handlers/mod.rs
rust/crates/tb-dashboard-api/src/lib.rs
rust/crates/tb-dashboard-api/Cargo.toml
/etc/caddy/Caddyfile

## Amendments

(keine)
