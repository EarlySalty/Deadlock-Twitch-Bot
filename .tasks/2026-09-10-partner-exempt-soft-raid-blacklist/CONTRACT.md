# Contract: Aktive Partner sind im Auto-Raid von der weichen Blacklist ausgenommen

## Ziel

Ein aktiver Partner soll als Auto-Raid-Ziel nie an der weichen
`twitch_raid_blacklist` haengenbleiben, unabhaengig davon, ob und warum ein
Eintrag dort steht. Nur der harte globale Ban (`twitch_chatter_global_ban`)
gilt weiter fuer alle. Damit ist die eigentliche Regel in der Zielauswahl
verankert, nicht in einer Datenbereinigung beim Partnerwerden.

## REQ

- REQ-1 Im Auto-Raid-Partnerpfad (`resolve_partner_target` in der Pipeline) werden aktive Partner NICHT gegen `twitch_raid_blacklist` gefiltert; fuer Partner gilt als Sperre nur der harte globale Ban aus `twitch_chatter_global_ban`.
- REQ-2 Der Kategorie-Fallback-Pfad (Nicht-Partner) filtert unveraendert gegen die volle Liste: `load_all` wenn `respect_soft_raid_blacklist`, sonst nur die harten Bans.
- REQ-3 Der harte globale Ban (`twitch_chatter_global_ban`) gilt weiterhin fuer alle Ziele, auch Partner.

## INV

- INV-1 Der manuelle Raid (`start_manual_raid`) bleibt unveraendert; er nutzt bereits nur die harten Bans.
- INV-2 Boost-Pfad, Exclude-Logik und die Score-Auswahl bleiben unveraendert.

## Nicht-Ziele

- Keine Aenderung an der Blacklist-Schreibseite (`add`) oder am promote-Cleanup aus 5e3bf545 (bleibt als Tabellenhygiene).
- Kein Schema-Wechsel, keine Migration.
- Keine Aenderung an der Kandidaten-Assemblierung oder der Deadlock-Eligibility.

## Erlaubter Bereich

rust/crates/tb-raid/src/auto_raid_pipeline.rs
rust/.sqlx
