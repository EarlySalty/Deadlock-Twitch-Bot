# Python-Cutover — Backlog / zurückgestellte Migrationen

Beim Abschluss des Python→Rust-Cutovers (2026-06-23) bewusst NICHT faithful nachgebaut, sondern
nur cutover-sicher umgelenkt (damit der Python-Stopp kein 502 erzeugt). Hier festgehalten, damit
die sinnvolle Migration später nicht vergessen wird.

## `/twitch/partners` — alte Stats-Seite (SSR, Partner-Modus)
- **Was:** Die alte serverseitig gerenderte Stats-Seite im Partner-Modus (`bot/dashboard/core/stats.py:1310`
  `partner_stats` → `_render_stats_page(partner_view=True)`, Partner-Token-geschützt).
- **Aktueller Use-Case: sehr gering.** Keine In-App-Links, keine SPA-Entsprechung; die reguläre
  Analytics-SPA (Overview/Growth/Audience) hat die Partner-Stats faktisch abgelöst.
- **Im Cutover:** 301-Redirect (auf die SPA) statt Neubau. Kein Datenverlust (reine Anzeige-Seite).
- **TODO später:** Falls die alte Partner-Stats-Ansicht doch noch gebraucht wird, sinnvoll als
  SPA-Sicht oder nativer Rust-Pfad neu aufsetzen — nicht als SSR-Altseite wiederbeleben.

## `/twitch/raid/analytics` — Raid-Netzwerk/Sankey (SSR)
- **Was:** SSR-Seite für Partner-Raid-Netzwerk/Sankey (`bot/dashboard/routes_mixin.py:625`).
- **Aktueller Use-Case: unklar / eher nutzlos**, wenn überhaupt eher Admin-relevant. Der Datenteil
  existiert in Rust (`raid_network_analytics.rs`, WIRING-TODO) und die SPA hat Raid-Analytics bereits
  unter `Growth.tsx` (`useRaidAnalytics`, `IncomingRaidsSection`).
- **Im Cutover:** Redirect zur SPA-Growth (Daten dort vorhanden) statt SSR-Neubau.
- **TODO später:** Falls ein dedizierter Admin-Raid-Analytics-View gewünscht wird, das WIRING-TODO in
  `raid_network_analytics.rs` schließen und an eine SPA-Admin-Sicht hängen.

## `/twitch/live-announcement` — Go-Live Builder / Discord Announcement Designer (ENTFERNT)
- **Entscheidung 2026-06-23 (User):** Der Custom-Builder (Embed-Designer) wird KOMPLETT entfernt — „unnötig".
  Der **automatische Go-Live-Discord-Post + Rollen-Ping BLEIBT** (Standard-Design); die 56 Streamer mit
  `live_ping_enabled=1` behalten ihren Live-Ping. Der Rust-Sender wird NICHT angefasst.
- **Im Cutover (jetzt):** `/twitch/live-announcement` + `/twitch/api/live-announcement/*` → nativer Rust-Redirect
  aufs Dashboard bzw. 410 (Builder unerreichbar, ohne Python zu editieren). Rust-OAuth-Whitelist-Eintrag
  `oauth_login.rs:40` entfernt.
- **BEHALTEN:** `tb-monitoring/src/announce/*` (BrokerAnnouncementSink) — Auto-Post läuft über Default-Config weiter.
- **DEFERRED bis Python-Stopp:** Python-Builder stirbt mit dem Cutover — `bot/dashboard/live/live_announcement_mixin.py`
  (+ Shim) + Routen (`routes_entry.py:33`, `routes_mixin.py:638-641`) + In-App-Tile (`pages.py:399-406`) + MRO
  (`server_v2.py:33/95`, `live/__init__.py`) + Python-OAuth-Whitelist (`auth_mixin.py:486`). DB NICHT jetzt droppen
  (`twitch_live_announcement_configs`, `twitch_partners.live_ping_*`) — Python liest beim Import noch (analog
  manual_verified). Internal-API `live/active-announcements` liest die Tabelle fürs Button-Label → vor einem
  späteren Tabellen-Drop umstellen.
