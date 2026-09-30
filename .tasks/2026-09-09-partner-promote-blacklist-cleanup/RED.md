# Roter Ausgangslauf

Worktree: /home/nathanael/.worktrees/tb-partner-bl-cleanup
Branch: fix/partner-promote-blacklist-cleanup

## Befehl

```
cd rust
PATH="$HOME/.cargo/bin:$PATH" SQLX_OFFLINE=1 \
  TB_TEST_DATABASE_URL='postgres://postgres:tbtest@127.0.0.1:32769/postgres' \
  cargo test -p tb-raid --lib blacklist
```

Toolchain: cargo 1.97.1. Test-DB: Docker-Container tb-test-postgres (scripts/test_db.sh).

## Ergebnis (vor dem Fix)

```
test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 333 filtered out; finished in 0.31s
```

Roter Test (der Regressionstest):
`partner_setup::tests::promote_loescht_recruitment_blacklist_und_bewahrt_hard_grund`

Fehlermeldung:
```
thread 'partner_setup::tests::promote_loescht_recruitment_blacklist_und_bewahrt_hard_grund' panicked at crates/tb-raid/src/partner_setup.rs:1363:9:
assertion `left == right` failed: recruitment-Zeile des Partners geloescht, Hard-Grund und fremde ID bleiben
  left: ["fremd", "ismile_e", "ismile_e_hard"]
 right: ["fremd", "ismile_e_hard"]
```

Deutung: ohne Fix loescht `promote_streamer_to_partner` die recruitment-Blacklist-Zeile
des frischen Partners nicht (`ismile_e` bleibt stehen).

Der zweite Test `signup_block_noop_loescht_recruitment_blacklist_nicht` ist bereits
gruen: er sichert nur ab, dass die No-op-Abweisung nichts loescht, und muss vor wie
nach dem Fix gruen bleiben.
