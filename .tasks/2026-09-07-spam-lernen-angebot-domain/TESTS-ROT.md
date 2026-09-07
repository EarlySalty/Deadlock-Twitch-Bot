# Rote Baseline der Regressionstests

Stand 2026-09-07, Worktree `~/.worktrees/tb-spam-lernen`, Branch `fix/spam-lernen-angebot-domain` auf origin/main 51ed50b1.

Befehl (je Test):

```
cd ~/.worktrees/tb-spam-lernen/rust
export PATH="$HOME/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin:$PATH"
SQLX_OFFLINE=1 TB_TEST_DATABASE_URL='postgres:///tb_bb_test?host=/var/run/postgresql' TB_TEST_REQUIRE_DB=1 \
  cargo test -p tb-chat --lib <testname>
```

Alle sechs Tests sind ohne den Fix rot. Der DB-Test lief in 0,07 s gegen die echte Testdatenbank (kein 0,00-s-Skip).

## REQ-01 Angebot-plus-Domain wird distinktiv (Gate)

- Test: `spam_filter::tests::gate_lernt_angebot_plus_domain`
- Rot: `assertion failed: is_distinctive_spam_pattern("ai viewers twitch .ad")` an `spam_filter.rs:1653`
- Soll nach Fix: true fuer die vier Angebot-plus-Domain-Muster, false fuer generische Reste und Domain- bzw. Angebotswort allein.

## REQ-04 Plattformnamen sind generisch (Gate)

- Test: `spam_filter::tests::gate_lehnt_generische_plattformwoerter_ab`
- Rot: `Plattformwort darf nie allein distinktiv sein: discord` an `spam_filter.rs:1685`
- Soll nach Fix: `is_distinctive_spam_pattern` und `is_distinctive_spam_pattern_vom_menschen` false fuer alle zehn Plattformwoerter.

## REQ-02 Ganzphrasen-Treffer

- Test: `spam_filter::tests::gelerntes_angebot_muster_trifft_nur_ganzphrase`
- Rot: erwartet `Learned-Phrase`, gematcht wird nur `["Muster: viewer + name"]` (Muster wird heute vom Gate im Test-Konstruktor verworfen) an `spam_filter.rs:1703`
- Soll nach Fix: gelerntes Muster ergibt `Learned-Phrase` und hartes Signal; "die twitch ads nerven", "twitch ad break", "twitch.ad" ergeben Score 0.

## REQ-03 Regelpfad statt KI

- Test: `spam_filter::tests::gelerntes_angebot_muster_bannt_erstnachricht_ohne_richter`
- Rot: `assertion left == right failed ... left: None, right: Ban` an `spam_filter.rs:1730`
- Soll nach Fix: mit Kontoalter 0 und Erstnachricht `SpamAction::Ban`, ohne Kontext `SpamAction::DeleteOnly`.

## REQ-04 Altbestand-Plattformwort wird beim Laden verworfen (Filter)

- Test: `spam_filter::tests::gelerntes_plattformwort_wird_beim_laden_verworfen`
- Rot: `generisches Plattformwort darf kein gelerntes Signal sein: ["Learned-Fragment: discord"]` an `spam_filter.rs:1752`
- Soll nach Fix: "bei Discord erlauben links zu oeffnen" ergibt keinen `Learned-`-Grund und Score 0.

## REQ-01 auf dem Richter-Pfad (DB)

- Test: `scam_pitch::tests::angebot_plus_domain_wird_vom_richter_gelernt`
- Rot: `Angebot plus Domain muss gelernt werden` an `scam_pitch.rs:2935` (heute liefert `learn_pattern_from_judge` `LearnOutcome::Rejected`)
- Soll nach Fix: `LearnOutcome::Saved` mit Muster "ai viewers twitch.ad".

## Volllauf tb-chat (Bestand kompiliert und bleibt gruen)

```
SQLX_OFFLINE=1 TB_TEST_DATABASE_URL='postgres:///tb_bb_test?host=/var/run/postgresql' TB_TEST_REQUIRE_DB=1 \
  cargo test -p tb-chat --lib
```

Ergebnis: `760 passed; 6 failed; 4 ignored; 0 filtered out` in 271,18 s. Die sechs Fehlschlaege sind genau die neuen Regressionstests, kein Bestandstest bricht.

TESTNACHWEIS[TW-1]: 760 passed, 4 ignored | Rot-Gegenprobe: 6 failed statt 0 (genau die sechs neuen Tests, ohne Fix rot)

## Runde 2 (Amendment A1)

Nachbesserung gegen HEAD bc6567b0 (Fix schon drin, aber mit falscher Viewer-Unterdrückung bei gelernter Phrase). Beide Tests sind gegen diesen HEAD rot. Befehl und Env wie oben.

### REQ-03 additiv: gelernte Phrase plus Viewer-Muster ergibt Ban

- Test: `spam_filter::tests::gelerntes_angebot_muster_bannt_erstnachricht_ohne_richter` (umgebaut)
- Rot: `assertion left == right failed: gelernte Phrase +2 und Viewer-Muster +1 bleiben additiv ... left: 2, right: 3` an `spam_filter.rs:1831` (Viewer-Muster wird unterdrückt, sobald die gelernte Phrase greift)
- Soll nach Fix: "Ai viewers twitch .ad (no space)" ohne Kontext Score 3 und `SpamAction::Ban`, Gründe enthalten `Learned-Phrase` und `Muster: viewer + name`; mit Kontext ebenfalls `Ban`; die gelernte Phrase "boost promotion twitch.ad" ohne Viewer-Wort bleibt `DeleteOnly`.

### Additivität mit Prod-Muster eballo.com

- Test: `spam_filter::tests::additivitaet_viewer_bleibt_neben_gelernter_phrase`
- Rot: `Viewer-Muster muss additiv neben der gelernten Phrase stehen: ["Phrase(Exact): (remove the space)", "Learned-Phrase: eballo.com"]` an `spam_filter.rs:1904` (Viewer-Grund fehlt wegen der Unterdrückung)
- Soll nach Fix: "Best Viewers Eballo .com (remove the space)" mit gelernter Phrase "eballo.com" ergibt Score mindestens 3, `Ban`, Gründe enthalten `Learned-Phrase` und `Muster: viewer + name`.
