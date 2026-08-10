# Offene Defekte nach dem ID-Umbau

Status: offen
Herkunft: Merge `fcf39153` (ID-first-Umbau, Viewbot-Werkzeuge, Spam-Judge-Gate)

Drei Punkte, die beim Abschluss des ID-Umbaus aufgefallen sind und dort bewusst
nicht mitgefixt wurden. Keiner davon stammt aus dem Umbau — die beiden Defekte
liegen seit Wochen auf `main`, der dritte Punkt ist ein liegengebliebener Branch.

## 1. `tb-crypto/tests/interop.rs` sucht eine gelöschte Datei

**Wirkung:** drei Tests dauerhaft rot, seit dem 2026-07-21.

`interop.rs` ruft `tests/py_oracle.py`. Die Datei wurde mit `cbcfeca2` gelöscht,
der Test blieb stehen. Betroffen:

- `identical_nonce_yields_identical_blob`
- `python_encrypts_rust_decrypts`
- `rust_encrypts_python_decrypts`

Sie zählen seither zur roten Grundlast jedes `cargo test --workspace` (11 von
3905) und verdecken damit echte Regressionen.

**Zwei Wege:** das Oracle-Skript aus der History zurückholen (`git show
cbcfeca2^:rust/crates/tb-crypto/tests/py_oracle.py`) oder den Interop-Test
löschen, wenn die Python-Seite endgültig weg ist. Der Test prüft die
Kompatibilität mit dem Legacy-Python-Pfad — solange der Patchnotes-Bot noch
Python ist, spricht mehr für das Zurückholen.

## 2. Migration `20260806120000_social_media_partner_access` scheitert auf frischer DB

**Wirkung:** jede neu aufgesetzte Datenbank ist unbaubar. Live unkritisch, weil
die Migration auf Prod bereits gelaufen ist.

```
Key (streamer_login)=(earlysalty) is not present in table "twitch_streamers"
```

Die Migration setzt eine Zeile in `twitch_streamers` voraus, die kein Seed
anlegt. Folgen:

- 8 tb-db-Tests rot (`fresh_migrations_schema`, `hermetic`,
  `betriebstabellen_id_trigger`, …)
- `rust/scripts/sqlx-prepare.sh` läuft nicht durch; der sqlx-Cache lässt sich
  nur über einen Workaround erneuern, der die fehlende Zeile vorher anlegt

**Haken:** die Migration ist auf Prod gelaufen und damit eingefroren — jede
Änderung an der Datei, auch am Kommentar, ändert die Prüfsumme und bricht den
nächsten Start. Ein Fix braucht deshalb entweder einen Prod-Eingriff an
`_sqlx_migrations.checksum` oder eine Folge-Migration, die den Seed nachholt und
die alte unverändert lässt. Der zweite Weg ist ohne Prod-Eingriff machbar und
sollte zuerst geprüft werden.

## 3. Branch `fix/codeql-action-4372` trägt einen ungemergten Bump

Der Branch hängt am Worktree `/home/naniadm/.worktrees/twitch-bot-codescan2`
(`25b37b28`) und ist kein Vorfahre von `main`:

- codeql-action 4.37.2
- brace-expansion 5.0.9

Beim Aufräumen nach dem Merge stehen geblieben, weil er als einziger der
Alt-Branches echte Änderungen trägt. Der praktische Wert ist gering — das
Actions-Budget ist erschöpft, CodeQL läuft ohnehin nicht —, aber ohne
Entscheidung wird der Branch weder gemergt noch gelöscht.

## Nicht zu tun

- `feature/pricing-premium-umbau` (`e5f335d5`) nicht anfassen: fremde aktive
  Arbeit, als WIP gesichert und gepusht.
- `claude/deadlock-twitch-bot-strategy-m3pbrh` bleibt liegen (entschieden).
