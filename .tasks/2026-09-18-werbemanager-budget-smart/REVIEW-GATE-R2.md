# Merge-Gate Runde 2: ein BLOCKER (Paket A)

Der Merge-Kritiker (claude-opus-4-8) hat den Push nach main verweigert. Ein
harter Blocker, an der Ursache zu beheben. Branch `feat/werbemanager-budget-backend`,
Worktree `/home/nathanael/.worktrees/tb-werbemanager-a`.

## BLOCKER (muss)
`rust/crates/tb-analytics/src/ad_manager.rs` in `last_first_chatter` (um Zeile
1166): Die Query lautet sinngemaess
`SELECT NULLIF(BTRIM(first_message_at),'')::timestamptz ... ORDER BY NULLIF(BTRIM(first_message_at),'')::timestamptz`.
`first_message_at` in `twitch_session_chatters` ist **timestamptz** (Schema-Snapshot
`rust/crates/tb-db/tests/fresh_schema_snapshot.txt`, Zeile ~1584:
`timestamp with time zone|NO`). `BTRIM` erwartet `text`; PostgreSQL hat keinen
impliziten Cast timestamptz->text bei Funktionsargumenten. Die Query wirft also
jedes Mal `function btrim(timestamp with time zone) does not exist`.

Aufgerufen wird sie in `ad_manager_wiring.rs` (`store.last_first_chatter(session)`)
auf **jedem** Smart-Tick. Das `?` bricht `process_channel` ab, bevor `claim_due`
die Werbung ausfuehrt. Ergebnis: der Manager tut in Prod fuer jeden aktivierten
Smart-Kanal nichts, also exakt der Ausgangsbug, den dieses Feature beheben soll.

Das defensive `BTRIM/NULLIF` stammt aus der Zeit, als die Spalte TEXT war; sie ist
jetzt timestamptz. Die Nachbarabfragen `last_incoming_raid`, `budget_used_this_hour`
und `record_match_transition` lesen ihre timestamptz-Spalten direkt ohne `BTRIM`.

**Fix (Ursache):** `first_message_at` direkt lesen, ohne `BTRIM`/`NULLIF`:
`SELECT first_message_at AS at, chatter_login ... ORDER BY first_message_at DESC NULLS LAST`
(Spalten- und Aliasnamen an den bestehenden Code anpassen, Rueckgabetyp gleich lassen).

## Nur wenn schnell und risikofrei (optional, NIT)
- Der `decide()`-Unittest ist DB-frei und der Schema-/Migrationstest skippt ohne
  `TB_TEST_DATABASE_URL`, deshalb faellt der Blocker in keinem Test auf. Wenn eine
  Test-DSN verfuegbar ist: den Migrations-/Schema-Test einmal mit DB laufen lassen.
  Sonst nur den Fix machen, keine neue Test-Infrastruktur bauen.

## Nicht anfassen
- Die bereits abgenommenen Fixes (Post-Match, GRANTs, plan_fit, blocks_in_window,
  Planer ohne Mindestabstand). Keine Code-Kommentare. Nur Branch A pushen, nichts
  Richtung main.

## Test / Fertig
Build gruen, `cargo test -p tb-analytics --test ad_manager_decision -- --include-ignored`
gruen (Baseline 16 passed). Fertigmeldung mit Commit und einem Satz zur Pruefung,
dann stoppen.
