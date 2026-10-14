# Zwei öffentliche Domains und OAuth pro Domain (Paket O)

Stand 2026-10-12. Gehört zum [Multi-Server-Plan](https://github.com/EarlySalty/Deadlock-Twitch-Bot/blob/docs/multiserver-plan-20261012/.tasks/2026-10-12-multiserver-englisch/PLAN.md), Abschnitte 3.5, 4.3 und Entscheidung E2.

## Kurzfassung

- `deutsche-deadlock-community.de` bleibt unverändert die Hauptdomain.
- `dead-lock.de` leitet nicht mehr weiter. Sie bekommt einen eigenen Caddy-Block und geht an dasselbe Backend (`tb-dashboard`, `127.0.0.1:8769`).
- Jeder OAuth-Ablauf wählt die Redirect-URL nach der Domain, auf der er startet. Erlaubt sind nur die zwei Domains (Allowlist). Ein unbekannter Host fällt auf die Hauptdomain zurück.
- Die gewählte Redirect-URL wird im OAuth-State gespeichert und beim Code-Tausch wiederverwendet. Der Rücksprung nach dem Login landet auf derselben Domain.
- Session-Cookies sind host-only (kein `Domain=`-Attribut) und bleiben damit auf der Startdomain.
- Die Hostnamen stehen nur noch an einer Stelle: `rust/crates/tb-config/src/public_site.rs`.

## Zentrale Basisadresse

`tb_config::PublicSite` kennt die zwei Domains:

| Funktion | Zweck |
|---|---|
| `PublicSite::from_host(host)` | ordnet einen `Host`-Header der Allowlist zu, sonst `None` |
| `PublicSite::url(path)` | absolute URL auf dieser Domain |
| `PublicSite::rebase(url)` | überträgt eine auf die Hauptdomain konfigurierte URL auf diese Domain; andere Hosts (lokal, Admin, fremd) bleiben unverändert |
| `tb_config::public_site::redirect_variants(url)` | alle Domain-Varianten einer Redirect-URL, Grundlage der Liste unten |
| `primary_url!("/pfad")`, `admin_url!()`, `primary_host!()` | Konstanten zur Compile-Zeit, z. B. für Bot-Texte |

Im Dashboard liefert `tb_dashboard_api::public_site::request_site(&headers)` die Domain einer Anfrage.

**Links in Bot-Nachrichten** (Twitch-Chat, Discord, DMs) zeigen weiter auf die Hauptdomain (`primary_url!`). Sobald es Heimat-Server gibt (Paket G), werden diese Stellen auf die Domain des Heimat-Servers des Streamers umgestellt.

## Welche Abläufe die Domain mitnehmen

| Ablauf | Start | Callback | Domain-bewusst |
|---|---|---|---|
| Dashboard-Login (Twitch) | `/twitch/auth/login` | `TWITCH_DASHBOARD_AUTH_REDIRECT_URI` | ja |
| Raid-, Uplink- und Titel-Freigabe, Website-Onboarding (Twitch) | `/twitch/raid/auth` → interne API `raid/auth-url?public_site=…` → `tb-bot` | `bot.raid_redirect_uri` (`/callback/twitch`) | ja, inkl. `go-url` und Rücksprung |
| Spieler-Verknüpfung `/twitch/connect` (Twitch) | `/twitch/connect/twitch` | wie Dashboard-Login | ja |
| Affiliate-Login (Twitch) | `/twitch/auth/affiliate/login` | `/callback/twitch` | ja |
| Affiliate Stripe Connect | `/twitch/affiliate/connect/stripe` | `/twitch/affiliate/connect/stripe/callback` | ja |
| Kick-Verbindung | `/twitch/uplink/connect/kick` | `/callback/kick` | ja |
| YouTube-Verbindung (Google) | `/twitch/uplink/connect/youtube` | `/callback/youtube` | ja |
| Discord-Verknüpfung im Dashboard | `/twitch/auth/discord/link` | Broker des Haupt-Bots | **nein**, bleibt Hauptdomain: der Broker in Deadlock-Bots lässt nur Rücksprünge auf die Hauptdomain zu, und Deadlock-Bots bleibt laut Plan unangetastet. Internationale Streamer bekommen ihren Discord-Login über den Hub (Paket G). |
| Clip-Contest-Login (Discord) | `/clips/...` | Broker | nein, deutsches Angebot |
| Admin-Login (Discord) | `admin.deutsche-deadlock-community.de` | unverändert | nein, internes Admin-Panel |
| Engagement-Sender | Admin | `/callback/engagement-sender` | nein, intern |

Der Clip-Raid-Token (`bot.clip_raid_redirect_uri`) wird nur erneuert, nie neu autorisiert. Dafür ist keine zusätzliche Redirect-URL nötig.

## Redirect-URLs, die in den Apps einzutragen sind

Die bestehenden Einträge der Hauptdomain bleiben. Zusätzlich:

### Twitch-App (`TWITCH_CLIENT_ID`)

| URL | Für |
|---|---|
| `https://dead-lock.de/callback/twitch` | Raid-, Uplink- und Titel-Freigabe, Website-Onboarding, Affiliate-Login; Dashboard-Login, falls `TWITCH_DASHBOARD_AUTH_REDIRECT_URI` auf `/callback/twitch` zeigt |
| `https://dead-lock.de/twitch/auth/callback` | nur falls `TWITCH_DASHBOARD_AUTH_REDIRECT_URI` auf `/twitch/auth/callback` zeigt |
| Variante von `TWITCH_AFFILIATE_AUTH_REDIRECT_URI` | nur falls diese Variable gesetzt ist und auf die Hauptdomain zeigt |

Twitch erlaubt bis zu zehn Redirect-URLs pro App.

### Google (YouTube-Uplink)

- `https://dead-lock.de/callback/youtube`

### Kick (Uplink)

- `https://dead-lock.de/callback/kick`

### Stripe Connect (Affiliate)

- `https://dead-lock.de/twitch/affiliate/connect/stripe/callback`

### Neue Discord-App (Hub, Paket H/G)

Die Routen entstehen erst mit Paket G (Onboarding) und A (Server-Admin). Vorgesehen sind:

| URL | Für |
|---|---|
| `https://dead-lock.de/twitch/auth/discord/hub/callback` | Onboarding mit Discord-Login (`identify`, `guilds`) |
| `https://deutsche-deadlock-community.de/twitch/auth/discord/hub/callback` | dasselbe auf der Hauptdomain |
| `https://dead-lock.de/twitch/server-admin/auth/callback` | Server-Admin-Login |

Einladungslink des Hubs (Kanäle sehen, Nachrichten senden, Links einbetten, Rollen verwalten; kein Admin):

```
https://discord.com/oauth2/authorize?client_id=<HUB_CLIENT_ID>&scope=bot%20applications.commands&permissions=268454912
```

`268454912` = View Channels (1024) + Send Messages (2048) + Embed Links (16384) + Manage Roles (268435456).

## Caddy

Der Block liegt in `ops/caddy/dead-lock-domain.caddy` und ersetzt das bisherige Snippet `dead_lock_domain_alias` (308 auf die Hauptdomain):

```
dead-lock.de {
    import /pfad/zum/repo/ops/caddy/dead-lock-domain.caddy
}
```

- `/twitch*`, `/streamer*`, `/callback/twitch`, `/callback/kick`, `/callback/youtube` und `/demo*` gehen an `127.0.0.1:8769`.
- `/` leitet vorerst auf `/streamer` (bis die englische Seite aus Paket W steht).
- Alles andere (Patchnotes, Steam-`/link`, Clip-Contest, Admin) gibt 404. Es gibt keine Weiterleitung mehr auf die Hauptdomain.
- Der `Host`-Header wird unverändert durchgereicht. **Kein `header_up Host …` in diesem Block**, sonst wählt das Backend die falsche Domain.
- Ein `www.dead-lock.de` ist nicht in der Allowlist. Falls DNS dafür existiert: eigener Block mit `redir https://dead-lock.de{uri} 308`.

Test: `node --test ops/tests/dead_lock_domain_caddy.test.mjs` (braucht ein lokales `caddy`).

## Reihenfolge beim Live-Schalten

1. Redirect-URLs oben in Twitch, Google, Kick und Stripe eintragen.
2. Backend mit diesem Stand ausrollen. Auf der Hauptdomain ändert sich nichts: dort ergibt `rebase` exakt die konfigurierte URL.
3. Caddy-Block tauschen, `caddy validate`, dann `caddy reload`.
4. Auf beiden Domains durchspielen: Dashboard-Login, Raid-Freigabe über `/twitch/raid/auth`, Website-Onboarding, Kick/YouTube-Verbindung. Prüfen: Rücksprung und Cookie bleiben auf der Startdomain.

Rollback: alten Caddy-Block wiederherstellen. Die zusätzlichen Redirect-URLs in den Apps stören nicht.
