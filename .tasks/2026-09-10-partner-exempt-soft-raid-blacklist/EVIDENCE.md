# Evidence

## Ausgangslage / Hart-Weich-Trennung

- rust/crates/tb-raid/src/raid_blacklist.rs `load_all`: UNION aus der gesamten `twitch_raid_blacklist` UND `twitch_chatter_global_ban`. Das ist die weiche Liste (nur Auto-Raid).
- rust/crates/tb-raid/src/raid_blacklist.rs `load_hard_bans`: nur `twitch_chatter_global_ban`. Das ist die harte Liste (auch manuell).
- Deshalb ging das manuelle `!raid ismile_e` immer: der manuelle Pfad nutzt nur `load_hard_bans`, der recruitment-Eintrag steht aber in `twitch_raid_blacklist` (weich).

## Aktueller Filterpfad

- rust/crates/tb-raid/src/auto_raid_pipeline.rs:443 `run` laedt `blacklist_sets`: `load_all()` bei `respect_soft_raid_blacklist`, sonst `load_hard_bans()`. EIN Set-Paar wird sowohl an den Partnerpfad als auch an den Fallback durchgereicht.
- rust/crates/tb-raid/src/auto_raid_pipeline.rs:805 ruft `resolve_partner_target(&req.partners, scores, blacklist_ids, blacklist_logins, exclude_ids, raider_class)`.
- rust/crates/tb-raid/src/target_resolution.rs:176 filtert Partner via `is_filtered(...)` gegen genau diese Blacklist-Sets, bevor `considered` zaehlt. Ein weicher Eintrag schliesst den Partner also aus, und der Fallback greift.

## Aenderung

- In `run`: beide Set-Paare getrennt laden. Hartes Set (`load_hard_bans`) an den Partnerpfad. Volles Set (`load_all` bei `respect_soft_raid_blacklist`, sonst hart) an den Fallback- und Boost-Pfad. So bleibt der Fallback unveraendert und der Partner haengt nur noch am harten globalen Ban.

## Bezug

- Vorfall und Ursache: siehe `.tasks/2026-09-09-partner-promote-blacklist-cleanup/EVIDENCE.md` (ismile_e, recruitment-Eintrag, Auto-Raid an 1337cammy statt an den Partner).
- Der promote-Cleanup 5e3bf545 loescht recruitment-Zeilen beim Partnerwerden; diese Regel hier deckt zusaetzlich alle weichen Gruende und Eintraege ab, die nach dem Partnerwerden entstehen.
