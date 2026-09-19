# Abschlussprüfung: Werbetexte neu und Raid-Dank

Stand: 19. September 2026, Europe/Berlin

## Auftrag und Git-Stand

Ziel ist ausschließlich der Remote-Feature-Branch `feat/werbetexte-neu-raid-dank`. Kein Merge nach `main`, keine Produktionsmigration, kein Deploy und kein Bot-Neustart.

Gearbeitet wurde im detached Rettungs-Worktree `/home/nathanael/.worktrees/tb-werbetexte-neu-salvage`. Der ursprüngliche Worktree `tb-werbetexte-neu` wurde nicht verändert.

Ausgangsstand: `bca7e4d4f0ab605266ce730311dbee4077d7e502`.

Eigener Implementierungscommit: `6925d294` (`feat(promos): Werbetexte erneuern und Raid-Dank sicher einmalig senden`). Er enthält elf Auftragsdateien.

In den Feature-Stand integrierter Remote-Main: `0c053e4628dc0b45184fbbf32dae25542eed49a5`. Die Integration war konfliktfrei. Dieser Stand wurde vor Abschluss nochmals mit `git ls-remote` abgeglichen. Es wurde nicht auf dem lokalen Main gearbeitet.

Der Rettungsbaum enthält außerdem zwölf vorgefundene Formatierungsänderungen außerhalb dieses Auftrags. Sie wurden nicht gestaged, nicht zurückgesetzt und nicht committed. Ein SHA256-Abgleich bestätigte ihren bytegleichen Erhalt nach der Main-Integration. Der Rettungsbaum muss deshalb erhalten bleiben.

## Umgesetzt und nachgeprüft

Die Werbeprompts und neuen festen Texte bewerben ausschließlich Mitspieler/Voice-Lanes, kostenloses Coaching, deutsche Patchnotes, Turniere/Scrims und Community-Events. Das alte Standardsatzmuster ist in den Prompts ausgeschlossen. Das Scam-Thema ist aus den Werbeprompts und neuen Werbetexten entfernt. Moderations- und Spam-/Scam-Filter bleiben unverändert.

LFG, bestätigter Invite-Bedarf, Invite-Rückfrage und `!invite` haben jeweils vier Varianten. `TEXTE.md` wurde auf den endgültigen Code abgeglichen: Alle 33 nummerierten Varianten stimmen mit Code oder SQL überein. Die zuvor nicht dokumentierte allgemeine Voice-Lane-Fallback-Variante ist ergänzt.

Die Migration `rust/migrations/20260918233000_werbetexte_neu.sql` deaktiviert bestehende Community-Ansagen, erhält sie als Historie und ergänzt fünf aktive Texte. Jeder neue Text enthält `{invite}` genau einmal. Die Migration wurde nur in der Wegwerf-Testdatenbank geprüft, nicht in Produktion.

Der Raid-Dank verwendet feste Texte ohne Modellaufruf, die Streamer-Seite, den eigenen Log-Pfad `raid_dank` und Review-Karten vom Typ `RaidDank`. Kandidatenprüfung, Live-/Deadlock-/Partner-Gates und Werbefrei-Prüfung sind getestet. Fehlende Raider-ID, fehlende oder zu alte Deadlock-Session, Partner/Ex-Partner, Blacklist, Outreach und vorhandener Ledger-Eintrag verhindern die Ansprache.

`PARTNER_STREAMER_PITCH_ENABLED` bleibt `false`. `on_message_pitch` wurde nicht neu in die Pipeline eingebunden. Kein Modellwechsel.

## Zusätzlich behobenes Parallelitätsproblem

Die übernommene Reservierung bestand aus `INSERT ... WHERE NOT EXISTS` und einem instanzlokalen Mutex. Ohne passenden Unique-Key konnten zwei getrennte Engines gleichzeitig eine Sendefreigabe erhalten.

Dafür wurde zuerst der Test `promos::db_tests::raid_dank_ledger_reservierung_ist_instanzuebergreifend_atomar` ergänzt. Zwei Engines konkurrieren um denselben Raider. Ein ausschließlich im Test angelegter verzögernder INSERT-Trigger macht das Rennen sichtbar.

Rot-Gegenprobe vor der Korrektur: **0 bestanden, 1 fehlgeschlagen**. Beide Reservierungen meldeten Erfolg, obwohl genau eine erlaubt ist (`left: 2`, `right: 1`).

Die Korrektur reserviert innerhalb einer Postgres-Transaktion mit einem kurzen `SHARE ROW EXCLUSIVE`-Schreiblock auf der vorhandenen Ledger-Tabelle. Prüfung, Eintrag und Commit erfolgen vor dem Senden. Kein Netzwerkaufruf liegt innerhalb der Transaktion. Ein DB-Fehler gibt keine Sendefreigabe.

Grün nach der Korrektur: **3 Raid-Dank-Tests bestanden, 0 fehlgeschlagen**. Die vorhandenen Tests wurden zusätzlich um offline/inaktives Ziel, fehlende Plan-Tabelle, fehlende Raider-ID und fehlende beziehungsweise 61 Tage alte Session erweitert. Auch diese Fälle sind im späteren fokussierten Lauf grün.

## Verifikation

Alle DB-Tests verwendeten ausschließlich den Wegwerfcontainer `tb-test-werbetexte-neu` auf einem von Docker vergebenen Loopback-Port. Compiler: `/home/nathanael/.cargo/bin/cargo`, zwei Build-Jobs. Das ältere `/usr/bin/cargo` konnte die Lockfile-Version 4 nicht lesen und wurde nicht verwendet.

### Compiler und Formatierung

`cargo check -j 2 -p tb-chat -p tb-raid -p tb-bot`: erfolgreich, auch nach Integration von `origin/main` (Exit 0).

`rustfmt --edition 2021 --config skip_children=true --check` auf den acht eigenen Rust-Dateien: erfolgreich. `skip_children=true` verhindert Änderungen an anderen Modulen durch die Formatierung von `main.rs`.

`git diff --check`: erfolgreich.

### Fokussierte Tests

```text
cargo test -p tb-chat --lib -- \
  promo_pitch:: invite_question:: lfg_pitch:: raid_dank_ \
  community_timer_persistenz_farbauswahl_und_sendpfad --test-threads=8
```

**107 bestanden, 0 fehlgeschlagen, 0 ignoriert.** Enthalten sind die neuen Text-/Invite-/LFG-Tests, alle drei Raid-Dank-Tests und der korrigierte Community-Migrations-/Rotationstest.

### Vollständiger Paketlauf nach Main-Integration

Mit `set -o pipefail`, Test-DSN, zwei Build-Jobs und folgendem Befehl vollständig bis einschließlich Integrations- und Dokumentationstests ausgeführt:

```text
cargo test -p tb-chat -p tb-raid -p tb-bot --no-fail-fast -- --test-threads=8
```

Gesamt über 34 abgeschlossene Testtargets: **1.737 bestanden, 10 fehlgeschlagen, 6 ignoriert**, Exit 101. Das ist ausdrücklich kein vollständig grüner Paketlauf.

Die Unit-Testtargets:

| Target | Bestanden | Fehlgeschlagen | Ignoriert |
| --- | ---: | ---: | ---: |
| `tb-bot` | 282 | 6 | 0 |
| `tb-chat` | 877 | 4 | 3 |
| `tb-raid` | 364 | 0 | 0 |

Alle ausgeführten Integrationstesttargets sind grün. Weitere drei ignorierte Fälle liegen in den Dokumentationstests.

Ein erster Paketlauf mit nur zwei Testthreads wurde vom Werkzeug nach 900 Sekunden beendet (Exit 143). Er ist kein Vollnachweis. Die obigen Zahlen stammen ausschließlich aus dem danach vollständig beendeten Lauf mit acht Testthreads.

### Sechs bekannte OAuth-Baselinefehler

Der aktuelle `tb-bot`-Stand entspricht exakt der übergebenen unveränderten Baseline von **282 bestanden / 6 fehlgeschlagen**. Die sechs Fehler liegen weiterhin in `raid_oauth_impl::callback_tests`:

- `discord_login_erzeugt_keine_login_erwartung`
- `erfolg_speichert_verschluesselte_tokens_und_liefert_redirect`
- `reauth_ohne_discord_state_fuehrt_partner_sync_aus`
- `reauth_ohne_discord_state_holt_getrennten_partner_zurueck`
- `reauth_triggert_sofortigen_chat_subscription_reconcile`
- `uplink_callback_does_not_enable_raids_or_create_a_partner`

### Vier zusätzlich sichtbar gewordene Fehler in unveränderten Chat-Tests

Diese vier Fälle scheitern auch bei isolierter serieller Ausführung: **0 bestanden / 4 fehlgeschlagen**. Sie wurden nicht als Parallelitätsflackern abgetan.

1. `pipeline::tests::invite_antwort_ueberspringt_lfg_pitch_bei_doppelintent`: Der Testsatz „Wie kann ich mitspielen, suche noch Leute für die Lobby?“ enthält kein konkretes Zugangswort. Der bestehende `classify_invite_question`-Vorfilter lehnt ihn deshalb bereits vor Judge und Texterzeugung ab. Die Testannahme von genau einer Invite-Antwort widerspricht diesem unveränderten Vorfilter.
2. `standard_replies::tests::gruss_aus_dem_vorigen_stream_blockiert_nicht`
3. `standard_replies::tests::unterdrueckter_doppelgruss_sperrt_den_naechsten_chatter_nicht`
4. `standard_replies::tests::zweiter_gruss_desselben_chatters_bleibt_aus`

Die drei Grußtests erzeugen zwar Tabellen und eine offene Session, aber keine Plan-Zeile mit aktiviertem `greeting_reply_enabled`. Das unveränderte `greeting_enabled` liefert ohne diese Zeile ausdrücklich `false`. Andere Tests in derselben Datei aktivieren den Schalter mit `set_greeting_flag` und bestehen.

**Nachweisgrenze:** Ein zusätzlicher ausführbarer Baseline-Lauf aus einem unveränderten Git-Export wurde von den Tool-Sicherheitschecks blockiert. Er wird hier nicht als ausgeführt oder als empirisch rot behauptet. Stattdessen wurde der tatsächlich ausgeführte Feature-Test mit einem byteweisen Quellcodevergleich gegen `bca7e4d4` ergänzt: `pipeline.rs`, `standard_replies.rs` und sämtliche Invite-Vorfilter sind identisch. Damit liegen die beschriebenen Testwidersprüche bereits im Ausgangscode. Die betreffenden Tests und Produktiv-Gates wurden nicht verändert, nur um die Suite grün zu bekommen.

SHA256 der vollständig identischen Dateien:

```text
pipeline.rs
013db9ad2278df0a111dfcc35bbab8009bd31b5b9596bb4a3e589d4b14b048fb
standard_replies.rs
4a27da9c74730bb2169ae84176180d0175d47b829d8c8cbe9439a68f853db8b8
```

Die Reparatur dieser alten Test-Fixtures gehört in einen gesonderten Auftrag. Sie ist kein Grund, Invite-Erkennung oder Begrüßungs-Opt-in für diesen Werbetext-Auftrag aufzuweichen.

### Clippy

`cargo clippy -p tb-chat -p tb-raid -p tb-bot --all-targets --no-deps`: nach Main-Integration mit Exit 0 beendet, mit bestehenden Warnungen.

Der strikte Lauf mit zusätzlichem `-- -D warnings` scheitert an vorhandenen Befunden: `title_ai.rs` (`too_many_arguments`), der vorhandenen Teststruktur `FixedSuppression` (`dead_code`), dem bestehenden Review-Testtyp (`type_complexity`) und `zuschauer_register.rs` (`items_after_test_module`). Reguläres Clippy zeigt außerdem vorhandene Testwarnungen in `tb-bot`. Es wurden keine Warnungen per globalem Allow oder Codeänderung außerhalb des Auftrags unterdrückt.

## Evidenzdateien auf dem Arbeitsrechner

Präfix: `/tmp/tb-werbetexte-session-20260919-`.

- `ledger-red.log`, `ledger-green.log`: echte Rot-/Grün-Gegenprobe.
- `focused.log`: 107 fokussierte Tests.
- `final-check.log`, `final-clippy.log`, `clippy-strict.log`: Compiler und Linter.
- `final-packages.log`: vollständiger Paketlauf und echter Exit-Status.
- `isolated-chat-failures.log`: vier Chat-Fehler seriell reproduziert.
- `unowned-hashes.json`: Fingerabdrücke der zwölf nicht zum Auftrag gehörenden Dateien.

Diese Akte dokumentiert die Eigenverifikation des Executors, keine unabhängige Review-Freigabe. Produktionsfreigabe und eine vollständig grüne Gesamtsuite werden nicht behauptet.
