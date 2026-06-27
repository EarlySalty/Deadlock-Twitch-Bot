# C2 Not-Wired Verification

Datum: 2026-06-27

Rolle: adversarialer Verifizierer, refute-by-default. Scope war read-only fuer Code: `rg`, `sed`, `nl`; kein Git, keine Secrets, keine Tests/Services.

Gepruefte Quell-Findings:

- `findings/B2-raid.md` B2-RAID-10
- `findings/B5b-dashboard-analytics.md` B5b-05
- `findings/B8-social-highlight.md` B8-009 und B8-019
- `findings/B6-internal-transport.md` B6-P3-002

## Summary

| Item | Verdict | Kurzurteil |
|---|---|---|
| B2-RAID-10 | CONFIRMED-DEAD | `OrphanReplay`/`with_orphan_replay` existiert, aber kein Production-Impl und kein Pipeline-Builder-Aufruf. |
| B5b-05 | CONFIRMED-DEAD | `raid_network_analytics_handler` existiert, aber keine Route registriert ihn; `/twitch/raid/analytics` redirectet zur SPA. |
| B8-009 | CONFIRMED-DEAD | `TokenRefreshWorker` existiert, aber `tb-bot` spawnt ihn nicht; andere Social-Worker werden gespawnt. |
| B8-019 | CONFIRMED-DEAD | echter Social-Admin-SPA-Handler existiert, Live-Router bindet aber nur den Stub-Redirect. |
| B6-P3-002 | PARTIAL | Route ist gemountet; Funktions-Port ist live `None`, Handler gibt deshalb 503. |

Keine `WIRED-ELSEWHERE=FALSE`-Faelle gefunden.

## B2-RAID-10 OrphanReplay / with_orphan_replay

Verdict: **CONFIRMED-DEAD**

Impl-Stelle:

- `rust/crates/tb-raid/src/auto_raid_pipeline.rs:284` definiert `trait OrphanReplay`.
- `rust/crates/tb-raid/src/auto_raid_pipeline.rs:332-351` haelt `orphan_replay: Option<Arc<dyn OrphanReplay>>`, Default `None` in `new` bei `:383`.
- `rust/crates/tb-raid/src/auto_raid_pipeline.rs:392-394` setzt den Hook via `with_orphan_replay`.
- `rust/crates/tb-raid/src/auto_raid_pipeline.rs:971-981` nutzt den Hook nur, falls `Some`.

Production-Wiring:

- `rust/bin/tb-bot/src/main.rs:692-726` baut `AutoRaidPipeline::new(...).with_observability(...)`; dort fehlt `.with_orphan_replay(...)`.
- `rg -n "with_orphan_replay|OrphanReplay|orphan_replay|replay_orphan|orphan.*replay"` ueber `rust/bin/tb-bot rust/crates` findet Production-Hits nur in `tb-raid`-Impl/Re-Export und Tests.
- `rust/bin/tb-bot/src/raid_arrival_wiring.rs:212` hat eine eigene `fn pop_orphan(...)`; das ist keine `impl OrphanReplay` und setzt den Pipeline-Hook nicht.

Fehlende Verdrahtung: Composition-Root `rust/bin/tb-bot/src/main.rs:692-726` muesste eine Production-Implementierung von `OrphanReplay` bauen und per `.with_orphan_replay(...)` an die Pipeline haengen.

## B5b-05 raid_network_analytics_handler

Verdict: **CONFIRMED-DEAD**

Impl-Stelle:

- `rust/crates/tb-dashboard-api/src/handlers/raid_network_analytics.rs:33-47` definiert `raid_network_analytics_handler`.
- Der Handler liefert die alte Datensicht mit `partner_stats`, `leechers`, `manual_raids`, `date_min`, `date_max`, `total` bei `:177-185`.

Production-Wiring:

- `rust/crates/tb-dashboard-api/src/handlers/mod.rs:66` exportiert das Modul.
- `rust/crates/tb-dashboard-api/src/lib.rs:523-524` registriert nur `/twitch/api/v2/raid-analytics` auf `raid_analytics::raid_analytics_handler`, nicht den Network-Handler.
- `rust/crates/tb-dashboard-api/src/lib.rs:1184-1185` registriert `/twitch/raid/analytics` auf `spa::analyse_root_redirect_handler`.
- Harte Suche nach `raid_network_analytics_handler`, `raid.*network`, `partner_stats`, `leechers`, `manual_raids`, `/twitch/raid/analytics/network` findet ausser Modul/Tests keine Route.

Fehlende Verdrahtung: In `rust/crates/tb-dashboard-api/src/lib.rs` gibt es keinen `.route(..., get(raid_network_analytics::raid_network_analytics_handler))` fuer die implementierte Network-Datensicht.

## B8-009 TokenRefreshWorker / refresh_worker

Verdict: **CONFIRMED-DEAD**

Impl-Stelle:

- `rust/crates/tb-social-media/src/refresh_worker.rs:24-32` definiert `TokenRefreshWorker`.
- `rust/crates/tb-social-media/src/refresh_worker.rs:35-41` definiert den periodischen `run`-Loop.
- `rust/crates/tb-social-media/src/refresh_worker.rs:46-64` implementiert `run_once`.
- Der Dateikopf sagt selbst, der periodische `TokenRefreshWorker::run` sei "noch nicht verdrahtet" (`:1-5`).

Production-Wiring:

- `rust/bin/tb-bot/src/main.rs:1193-1210` spawnt Retention, Approval, Reports und Enrichment.
- `rust/bin/tb-bot/src/main.rs:1215-1236` spawnt bei vorhandenem FieldCipher Upload und Insights.
- `rust/bin/tb-bot/src/main.rs:1244` loggt "6 Loops"; diese sechs sind Retention, Approval, Reports, Enrichment, Upload, Insights.
- `rg -n "TokenRefreshWorker::new|TokenRefreshWorker|refresh_worker::|tb_social_media::refresh_worker"` ueber `rust/bin/tb-bot rust/crates` findet ausser `tb-social-media`-Modul/Tests keinen Production-Aufruf.
- `OAuthManager::new` wird im Dashboard-Handler fuer OAuth-Flows gebaut (`rust/crates/tb-dashboard-api/src/handlers/social_media.rs:2279-2281`), aber nicht in `tb-bot` fuer einen Refresh-Worker-Spawn.

Fehlende Verdrahtung: Im Social-Media-Worker-Block `rust/bin/tb-bot/src/main.rs:1215-1236` muesste zusaetzlich `TokenRefreshWorker::new(pool.clone(), cipher.clone(), OAuthManager::new(...))` gespawnt werden.

## B8-019 Social-Admin-SPA

Verdict: **CONFIRMED-DEAD**

Impl-Stelle:

- `rust/crates/tb-dashboard-api/src/handlers/spa.rs:214-249` definiert `social_media_admin_handler`.
- `rust/crates/tb-dashboard-api/src/handlers/spa.rs:254-267` definiert `social_media_admin_assets_handler`.

Production-Wiring:

- `rust/crates/tb-dashboard-api/src/lib.rs:1139-1151` baut `build_social_media_admin_router`, bindet aber `/social-media-admin` und `/social-media-admin/*path` auf `obsolete_routes::social_media_admin_stub_redirect_handler`.
- `rust/crates/tb-dashboard-api/src/handlers/obsolete_routes.rs:42-48` ist der Stub und redirectet nach `/twitch/dashboard`.
- `rust/crates/tb-dashboard-api/src/lib.rs:1238` merged diesen Stub-Router in den Live-Router.
- Harte Suche nach `social_media_admin_handler`, `social_media_admin_assets_handler`, `social_media_admin_stub_redirect_handler`, `/social-media-admin` findet keine Route auf die echten SPA-Handler.

Fehlende Verdrahtung: `build_social_media_admin_router` muesste die echten `spa::social_media_admin_handler`/`spa::social_media_admin_assets_handler` mit `PgPool`/Auth-State verwenden; aktuell ist nur der Stub live.

## B6-P3-002 /raid/reauth-all

Verdict: **PARTIAL**

Warum partial: Die Route ist produktiv gemountet. Der Befund "nicht injizierter Port -> live 503" ist aber bestaetigt.

Impl-Stelle:

- `rust/crates/tb-internal-api/src/handlers/reauth_all.rs:24-33` definiert den internen `BulkReauthPort` und `BulkReauthExt`.
- `rust/crates/tb-internal-api/src/handlers/reauth_all.rs:45-63` implementiert `reauth_all_handler`.
- `rust/crates/tb-internal-api/src/handlers/reauth_all.rs:52-54` gibt `ApiError::unavailable()` zurueck, wenn `BulkReauthExt(None)` anliegt.
- Die SQL-Primitive existiert in `rust/crates/tb-raid/src/reauth_admin.rs:18-33` als `ReauthAdminStore::snapshot_and_flag_reauth`.

Production-Wiring:

- `rust/crates/tb-internal-api/src/lib.rs:50-63` akzeptiert `bulk_reauth: Option<Arc<dyn handlers::reauth_all::BulkReauthPort>>`.
- `rust/crates/tb-internal-api/src/lib.rs:197-198` mounted `POST /internal/twitch/v1/raid/reauth-all`.
- `rust/crates/tb-internal-api/src/lib.rs:308` legt `BulkReauthExt(bulk_reauth)` als Extension.
- `rust/bin/tb-bot/src/main.rs:1501-1517` ruft `build_internal_router(...)` auf und uebergibt an der `bulk_reauth`-Position explizit `None`; der Kommentar `:1513-1514` nennt genau das fehlende `Some(Arc::new(tb_raid::ReauthAdminStore::new(pool.clone())))`.
- Harte Suche nach `build_internal_router(`, `ReauthAdminStore`, `BulkReauthExt(Some`, `BulkReauthPort for` findet keinen Production-Adapter fuer den `tb-internal-api`-Port; `ReauthAdminStore` implementiert nur den `tb-raid`-eigenen Trait (`rust/crates/tb-raid/src/reauth_admin.rs:68-72`).

Fehlende Verdrahtung: In `rust/bin/tb-bot/src/main.rs:1513-1515` muss statt `None` ein Adapter/Port auf `tb_raid::ReauthAdminStore` injiziert werden. Bis dahin ist der Endpunkt erreichbar, aber funktional 503.

## Verifikation

Statisch geprueft mit `rg`, `sed`, `nl` in:

- `rust/bin/tb-bot`
- `rust/crates`

Keine Tests ausgefuehrt, keine Services gestartet, kein Git verwendet.
