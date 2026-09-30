# Evidence

## Vorfall

- Raid 1607: earlysalty geht offline 2026-09-09 22:47:57 UTC, Auto-Raid an 1337cammy (Nicht-Partner, Kategorie-Fallback), ismile_e (aktiver Partner, live in Deadlock) uebersprungen. Quelle: twitch_raid_history, twitch_observability_events flow_id=raid-1788994074494-1 (is_partner_raid=false, candidates_count=1).
- Journal deadlock-twitch-bot-rust 22:47:49 UTC: `Auto-Raid-Pipeline gestartet ... online_partners=1 eligible_partners=1 filtered_out=0`. ismile_e war via Helix live und in Deadlock, also eligible.
- Journal 22:47:54 UTC: `Hole Deadlock-DE-Kategorie-Streams (Boost/Fallback)`, danach `Raid-Versuch to=1337cammy partner=false`.
- Die Zeile `Partner-Auswahl: Score-Cache unvollstaendig` (auto_raid_pipeline.rs:813-820) feuerte NICHT. Also cache_misses=0 und stale_not_live=0. Der Partner wurde schon vor dem Score-Join gefiltert (considered=0).

## Ursache

- rust/crates/tb-raid/src/target_resolution.rs:176. Partner faellt raus bei `!raid_enabled || is_filtered(...)`, bevor `considered` hochzaehlt.
- rust/crates/tb-raid/src/target_resolution.rs:69. `is_filtered` schliesst `blacklist_ids` und `blacklist_logins` ein.
- rust/crates/tb-raid/src/auto_raid_pipeline.rs:443. Auto-Raid laedt die weiche Blacklist (`load_all()`), da `respect_soft_raid_blacklist=true`.
- DB twitch_raid_blacklist: `target_id=58819840` (ismile_e), `reason=confirmed_external_recruitment_limit_grace_expired: count=4 limit=4 ...`, `added_at=2026-04-24`.
- ismile_e ist aktiver Partner seit `twitch_streamers_partner_state.created_at=2026-06-19`. Der Blacklist-Eintrag stammt aus der Werbephase davor und wurde beim Partnerwerden nie geloescht.
- Nur ein aktiver Partner betroffen (JOIN blacklist gegen partner_state WHERE is_partner_active=1): ismile_e.

## Fix-Ort

- rust/crates/tb-raid/src/partner_setup.rs:638 `promote_streamer_to_partner`. Laeuft in einer Transaktion, nach den Guards (Signup-Block, Hard-Pause, inaktive Sperre) und vor bzw. bei dem Partner-State-Upsert. Hier den recruitment-limit-Blacklist-Eintrag des `target_id` loeschen.

## Sofortmassnahme (bereits erledigt)

- `DELETE FROM twitch_raid_blacklist WHERE target_id='58819840' AND reason LIKE 'confirmed_external_recruitment_limit%'`. Eine Zeile entfernt, ismile_e wieder raidbar. Geloeschte Zeile fuer Rollback: target_id=58819840, target_login=ismile_e, reason wie oben, added_at=2026-04-24 13:14:33.245159+00.
