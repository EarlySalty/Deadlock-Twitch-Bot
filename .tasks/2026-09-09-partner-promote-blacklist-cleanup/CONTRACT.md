# Contract: Recruitment-Blacklist beim Partnerwerden aufräumen

## Ziel

Wird ein Streamer zum aktiven Partner befördert, muss ein Altlast-Eintrag aus der
Werbephase in `twitch_raid_blacklist` gelöscht werden. Sonst bleibt der frische
Partner dauerhaft vom Auto-Raid ausgeschlossen (Symptom: earlysalty geht offline,
der einzige eligible Partner ismile_e fällt am Blacklist-Filter raus, der Bot
raidet einen Nicht-Partner aus der Kategorie).

## REQ

- REQ-1 `promote_streamer_to_partner` löscht in derselben Transaktion alle Zeilen aus `twitch_raid_blacklist`, deren `target_id` gleich der `twitch_user_id` des Streamers ist UND deren `reason` mit `confirmed_external_recruitment_limit` beginnt.
- REQ-2 Andere Blacklist-Gründe (Bot-Bann, Raid-API-Ablehnung, manuelle Bans) bleiben unangetastet.
- REQ-3 Gelöscht wird nur, wenn die Promotion tatsächlich die Guards passiert und die Partner-Zeile geschrieben wird, nicht bei No-op-Abweisung (Signup-Block, Hard-Pause, inaktive Sperre).

## INV

- INV-1 Kein anderer Blacklist-Eintrag als der recruitment-limit-Eintrag des betroffenen `target_id` wird verändert.
- INV-2 Der Raid-Auswahlpfad (target_resolution, candidate_selection, Score-Cache) bleibt unverändert.

## Nicht-Ziele

- Kein Umbau des Score-Caches oder des `is_live`-Gates in `resolve_partner_target`.
- Keine Änderung am Auto-Raid-Trigger oder an der Kandidaten-Assemblierung.
- Keine UI-, Dashboard- oder Migrationsänderung (kein Schema-Wechsel, nur ein neuer DELETE).
- Kein Backfill im Code (die bestehende Altzeile ismile_e ist per Einmal-SQL schon entfernt).

## Erlaubter Bereich

rust/crates/tb-raid/src/partner_setup.rs
rust/.sqlx
