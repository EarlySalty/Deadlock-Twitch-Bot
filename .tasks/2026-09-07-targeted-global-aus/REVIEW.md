# Review: Timer-Targeted-Pitch (targeted_global) abschalten

## TESTNACHWEIS

TESTNACHWEIS[TW-1]: 786 passed, 4 ignored | Rot-Gegenprobe: Sabotage (periodic
loggt als targeted_global) lässt `send_promo_if_due_laeuft_ohne_targeted_global`
rot werden (0 passed, 1 failed); ohne Sabotage 2 passed. Befehl: `PATH` mit
Toolchain 1.97.1, `SQLX_OFFLINE=1 TB_TEST_DATABASE_URL=postgres:///tb_bb_test?host=/var/run/postgresql
TB_TEST_REQUIRE_DB=1 cargo test -p tb-chat -j 2` (TB_TEST_REQUIRE_DB zwingt echte
DB-Läufe, kein stiller Skip).

## Vorbestehendes Rot (Baseline, Zahl gegen Zahl)

- `pipeline::tests::invite_antwort_ueberspringt_lfg_pitch_bei_doppelintent`
  rot auf dem unveränderten Baum (git stash, gleicher Befehl): 0 passed,
  1 failed — identisch mit meinem Lauf (786 passed, 1 failed, 4 ignored).
  Liegt in pipeline.rs, das dieser Diff nicht anfasst; wahrscheinlich Folge von
  a8d14ebc (Anfänger-Discord-Anlass-Zuordnung). Nicht Teil dieses Auftrags.

## Clippy und fmt (Baseline)

- `cargo clippy -p tb-chat --all-targets -- -D warnings` bricht in tb-crypto
  (deprecated `Nonce::from_slice`, 3 Funde in field.rs) — vorbestehend, mein
  Diff enthält tb-crypto und dessen Abhängigkeiten nicht. Betroffen ist nur
  der Warn-Level-Scan; `cargo build --release` (Deploy-Weg) bleibt von
  Deprecation-Warnungen unberührt. Gehört in einen eigenen tb-crypto-Fix.
- fmt: 28 Abweichungen in promos.rs vor und nach dem Diff (gleiche Anzahl,
  nur Zeilennummern verschoben), keine im neuen Code; repo-weite rote
  Formatter-Baseline (verify-change stuft fmt als Bericht, nicht fatal).

## Eigene Prüfung des Fixers (gate_hook --review)

Wird nach dem Push-Gate ergänzt.
