# Dedizierter Admin-Modus (Streamer-Dashboard)

## Problem

Der Owner-Login (`earlysalty`, aus `_TWITCH_ADMIN_LOGINS`) wird beim Dashboard-Login
zu `DashboardAuthLevel::Admin { actor: Some(..) }` promotet. `auth-status` lieferte
für ihn früher **immer** `admin_response` → das Frontend schaltete über `isAdmin`
sämtliche Feature-Gates frei (Plan „Erweitert (Admin)", alle Entitlements).

Folge: Der Admin sah nie die echte Nutzer-Ansicht. Ein für reale Partner/Free-User
kaputtes oder gesperrtes Dashboard fiel ihm nicht auf, weil sein Override alles
entsperrte.

## Lösung

**Default = Nutzer-Ansicht. Admin-Vollzugriff ist opt-in pro Session.**

- Ohne aktiven Modus sieht der Admin sein eigenes Kanal-Dashboard mit seinem
  **echten** Plan (via `resolve_plan_snapshot`, i. d. R. `Free`), inkl. echter
  Sperren/Upgrade-Hinweise wie ein normaler Partner.
- Per Schalter aktiviert er den Admin-Vollzugriff. Das setzt ein **Session-Cookie**
  `tb_admin_mode=1` (kein Max-Age → stirbt beim Browser-Close; wird beim Logout
  gelöscht). „Pro Session" — nach Ablauf/Logout ist wieder Default aktiv.

Der **Admin-Host** (`admin.*`, Discord-Admin via `master_dash_session`) und
**Localhost** bleiben unberührt voll-Admin: dort gibt es keinen Schalter
(`adminEligible = false`).

## Architektur — genau ein Hebel

Das Frontend bezieht `isAdmin` + `plan` ausschließlich aus der `auth-status`-Antwort
(`PlanContext`: `hasFullAccess = isAdmin || isLocalhost || isDemoMode`). Es genügt
daher, **nur die Präsentation** in `auth-status` umzuschalten — die ~364
Daten-Endpunkte und die Frontend-Gate-Logik bleiben unangetastet.

> **Bewusst NICHT angefasst:** der `DashboardAuthLevel`-Extractor (Security-Boundary).
> Ein Hard-Downgrade auf `Partner`-Level würde `partner_gate` triggern und den Admin
> aus seinen eigenen Daten aussperren (er steht evtl. nicht in `twitch_partners`).
> Das Auth-Level bleibt `Admin`; nur die ausgelieferte Payload wechselt.

### Backend (`tb-dashboard-api`)

`handlers/auth_status.rs` — Verzweigung nach Auth-Level **und** Cookie
`tb_admin_mode` (aktiv = vorhanden und `== "1"`). Jede Payload trägt zwei neue
Felder:

| Level | Cookie | Antwort | `adminEligible` | `adminMode` |
|-------|--------|---------|-----------------|-------------|
| `Admin { actor: Some }` | aktiv | `admin_response` | `true` | `true` |
| `Admin { actor: Some }` | inaktiv (Default) | `partner_response` (echter Plan) | `true` | `false` |
| `Admin { actor: None }` (Discord) | — | `admin_response` | `false` | `true` |
| `Localhost` | — | `admin_response` | `false` | `true` |
| `Partner` | — | `partner_response` | `false` | `false` |
| `None` | — | unauth | `false` | `false` |

`handlers/admin_mode.rs` — `POST /twitch/api/v2/admin-mode`, Body `{ "enabled": bool }`.
- Gate: nur `Admin { actor: Some(_) }`, sonst `403`.
- `enabled:true` → Set-Cookie `tb_admin_mode=1` (HttpOnly, SameSite=Lax, Path=/,
  Secure in prod, **kein Max-Age**). `enabled:false` → Cookie löschen.
- Antwort `{ "adminMode": bool }`.

`logout_handler` (`auth_login.rs`) löscht `tb_admin_mode` mit.

### Frontend (`dashboard_v2`)

- `api/auth.ts`: `AuthStatus` um `adminEligible?`/`adminMode?` erweitert;
  `setAdminMode(enabled, csrfToken?)` POSTet auf den Endpunkt (CSRF-Token nur wenn
  vorhanden — Parität zu `title`-/`admin`-Mutations, da `auth-status` `csrfToken:null`
  liefert).
- `pages/InternalHomeLanding.tsx` (Home-Sidebar): Sektion **Admin** mit Toggle-Button
  (sichtbar nur bei `adminEligible`) + Warn-Banner im Hauptbereich bei aktivem Modus.
  Nach dem Umschalten werden `['auth-status']`, `['internal-home']`, `['streamers']`
  invalidiert — der Streamer-Scope hängt an `isAdmin`.

## Verifikation

- Default: Admin loggt ein → Badge zeigt echten Plan (z. B. `Free`), Feature-Gates
  greifen wie bei Partnern; Streamer-Switcher ausgeblendet.
- Toggle an → voller Zugriff, Warn-Banner sichtbar, Switcher erscheint.
- Logout/Browser-Close → wieder Default.
