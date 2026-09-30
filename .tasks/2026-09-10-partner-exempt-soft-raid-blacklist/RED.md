# Roter Lauf (vor dem Fix)

Kommando:

```
SQLX_OFFLINE=1 TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:<port>/postgres \
cargo test -p tb-raid --test auto_raid_pipeline -- \
  partner_bleibt_ziel_trotz_weicher_raid_blacklist \
  partner_bleibt_ausgeschlossen_bei_hartem_global_ban \
  fallback_filtert_nicht_partner_weiter_gegen_weiche_blacklist
```

Ergebnis:

```
test fallback_filtert_nicht_partner_weiter_gegen_weiche_blacklist ... ok
test partner_bleibt_ziel_trotz_weicher_raid_blacklist ... FAILED
test partner_bleibt_ausgeschlossen_bei_hartem_global_ban ... ok

---- partner_bleibt_ziel_trotz_weicher_raid_blacklist stdout ----
thread 'partner_bleibt_ziel_trotz_weicher_raid_blacklist' panicked at crates/tb-raid/tests/auto_raid_pipeline.rs:1192:5:
assertion `left == right` failed: aktiver Partner darf nicht an der weichen Raid-Blacklist haengen
  left: NoTarget
 right: Started { target_login: "ziel", is_partner_raid: true }

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 19 filtered out
```

- Test A `partner_bleibt_ziel_trotz_weicher_raid_blacklist`: ROT. Der Partner steht in der weichen `twitch_raid_blacklist`, der Partnerpfad filtert ihn mit dem vollen Set, es faellt auf NoTarget statt den Partner zu raiden.
- Test B `partner_bleibt_ausgeschlossen_bei_hartem_global_ban`: gruen vor und nach dem Fix (harter Ban gilt immer).
- Test C `fallback_filtert_nicht_partner_weiter_gegen_weiche_blacklist`: gruen vor und nach dem Fix (Fallback unveraendert).
