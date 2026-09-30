# Dedizierter Admin-Modus (Streamer-Dashboard)

## Problem

Twitch-Adminrechte entstehen nur, wenn die ID aus einer verifizierten OAuth-Session mit der dauerhaft konfigurierten Betreiber-Twitch-ID übereinstimmt. Ein Twitch-Login reicht dafür nicht. `auth-status` lieferte früher für den Betreiber immer `admin_response`; dadurch schaltete das Frontend über `isAdmin` sämtliche Feature-Gates frei (Plan „Erweitert (Admin)", alle Entitlements).

Folge: Der Admin sah nie die echte Nutzer-Ansicht. Ein für reale Partner/Free-User
kaputtes oder gesperrtes Dashboard fiel ihm nicht auf, weil sein Override alles
entsperrte.

## Lösung

**Default = Nutzer-Ansicht. Admin-Vollzugriff ist opt-in pro Session.**

- Ohne aktiven Modus sieht der Admin sein eigenes Kanal-Dashboard mit seinem
  **echten** Plan (via `resolve_plan_snapshot`, i. d. R. `Free`), inkl. echter
  Sperren/Upgrade-Hinweise wie ein normaler Partner.
- Per Schalter aktiviert er den Admin-Vollzugriff. Das setzt ein **Session-Cookie**
  `tb_admin_mode=2` (kein Max-Age → stirbt beim Browser-Close; wird beim Logout
  gelöscht). „Pro Session" — nach Ablauf/Logout ist wieder Default aktiv.
  Der Wert ist versioniert: alte `=1`-Cookies aus den fehlerhaften Vorversionen
  gelten bewusst als inaktiv, damit ein bestehender Browser nicht ungefragt im
  Override startet.

Auf dem **öffentlichen Dashboard** gilt das Opt-in auch für eine vorhandene
Discord-Admin-Session (`master_dash_session`). Ohne Modus-Cookie zeigt sie die
echte Nutzeransicht des Owner-Kanals. Der **Admin-Host** (`admin.*`) und interne
Aufrufe bleiben unberührt voll-Admin. Ein `?streamer=`-Override auf einen
anderen Kanal wird in der öffentlichen Nutzeransicht mit `403` abgewiesen.

## Architektur — genau ein Hebel

Die Entscheidung fällt zentral im `DashboardAuthLevel`-Extractor. Im öffentlichen
Dashboard wird eine admin-berechtigte Twitch- oder Discord-Session ohne aktives
`tb_admin_mode=2` als Partner des Owner-Kanals aufgelöst. Damit greifen dieselben
Scope-, Plan- und Daten-Gates wie bei jedem anderen Partner; fremde
`?streamer=`-Overrides enden zentral mit `403`.

Mit aktivem Modus-Cookie sowie auf dem Admin-Host und bei internen Aufrufen bleibt
das Auth-Level `Admin`. Die übrigen Daten-Endpunkte brauchen dadurch keine eigenen
Admin-Modus-Sonderfälle.

### Backend (`tb-dashboard-api`)

`auth/level.rs` löst Session, Dashboard-Kontext und Modus-Cookie gemeinsam auf:

| Quelle | Kontext / Cookie | Effektives Level |
|--------|------------------|------------------|
| Twitch-Owner | Modus aktiv | `Admin { actor: Some }` |
| Twitch-Owner | Modus inaktiv | `Partner` (Owner) |
| Discord-Admin | öffentlich, Modus aktiv | `Admin { actor: None }` |
| Discord-Admin | öffentlich, Modus inaktiv | `Partner` (Owner) |
| Discord-Admin | Admin-Host / intern | `Admin { actor: None }` |
| normaler Streamer | — | `Partner` |
| keine gültige Session | kein Kontext | `None` |

Die drei Uplink-Wartelistenrouten (lesen, freischalten und ablehnen) erlauben zusätzlich die zentral verifizierte Twitch-Partner-Session, wenn ihre User-ID exakt der konfigurierten Betreiber-ID entspricht. Dafür ist kein Admin-Modus nötig. Gleicher Login oder ein Modus-Cookie ohne passende Session-ID reichen nicht.

`handlers/auth_status.rs` serialisiert dieses effektive Level. Seine Payload trägt
`adminEligible` und `adminMode`, damit das Frontend den Schalter darstellen kann.

`handlers/admin_mode.rs`: `POST /twitch/api/v2/admin-mode`, Body `{ "enabled": bool }`.
- Gate: zentrale Admin-Session oder Partner-Session, deren verifizierte Twitch-ID der konfigurierten Betreiber-ID entspricht. Login und Modus-Cookie sind kein Berechtigungsnachweis.
- `enabled:true` → Set-Cookie `tb_admin_mode=2` (HttpOnly, SameSite=Lax, Path=/,
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
  Beim Umschalten wird zuerst eine laufende Startseitenabfrage abgebrochen, dann
  das Session-Cookie geändert und anschließend ausschließlich `['auth-status']`
  synchron neu geladen. Der daraus folgende Wechsel von `isAdmin` ändert den
  Startseiten-Query-Key automatisch (`streamer` ↔ eigener Account). Eine
  parallele Invalidierung wäre falsch: Beim Aktivieren könnte sonst noch der
  Nutzer-Query ohne `streamer` im bereits aktiven Admin-Kontext laufen und durch
  seine korrekte `401`-Antwort zur Login-Seite navigieren.

## Verifikation

- Default: Admin loggt ein → Badge zeigt echten Plan (z. B. `Free`), Feature-Gates
  greifen wie bei Partnern; Streamer-Switcher ausgeblendet.
- Toggle an → voller Zugriff, Warn-Banner sichtbar, Switcher erscheint.
- Toggle aus und danach wieder an → kein Login-Redirect, kein leerer Root-Knoten,
  keine unbehandelte Browser-Exception (Firefox-WebDriver-Regressionstest).
- Logout/Browser-Close → wieder Default.
