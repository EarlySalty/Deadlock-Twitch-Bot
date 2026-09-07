# Review: Spam-Lernen für Angebot-plus-Domain und generische Einzelwörter

Reviewer: frischer Agent (Opus 4.8), nicht der Implementierer.
Stand: 2026-09-07, Worktree `~/.worktrees/tb-spam-lernen`, Branch `fix/spam-lernen-angebot-domain`, rebased auf origin/main 9c128ed6.
Geprüft: `git diff origin/main..HEAD` (spam_filter.rs, scam_pitch.rs, spam_learning.rs, .tasks/) plus Contract inkl. Amendment A1.

## Ergebnis: FREIGABE

Alle REQ erfüllt, alle INV gehalten, Scope sauber, Suite grün (790 passed, 0 failed, 4 ignored; die sieben neuen Regressionstests laufen und sind grün). Kein blockierender Mangel. Zwei Hinweise (nit) am Ende, keiner davon merge-relevant.

## Prüfpunkt 1: dritter Gate-Zweig (ist_angebot_plus_domain / ist_domainform / TLD / Zusatz-Entfernung)

Korrekt und eng genug. Der Zweig macht ein Muster nur *lernbar* (Gate-true); tatsächlich gelernt wird nur, wenn der Richter die Nachricht als Spam einstuft, das Muster wortwörtlich in der Quelle steht (`belegt`) und das Safe-Wording-Gate passiert (scam_pitch.rs:1881 bis 1908). Die Speicherung ist Ganzphrase (`phrase`), der Treffer damit ganzphrasen-eng (REQ-02).

Zwölf Alltagssätze durchgespielt (Gate-Sicht):
- NICHT distinktiv (safe): "danke an alle viewer. ad hoc" (kein space-dot, "viewer." ist ein Label, "ad" ohne Punkt), "follower.io ist cool" (Domain, aber kein SEPARATES Angebotswort, denn "followerio" ist nicht "follower"), "danke für die subs leute" (keine Domain), "boost your aim, gg" ("gg" ohne Punkt), "thanks for the follows everyone" (keine Domain), "viewer.com is down" (Domain, aber kein separates Angebotswort; deckt INV-02), "sub goal reached, ty all" (keine Domain).
- distinktiv, also lernbar (nur judge-gated und ganzphrasen-eng): "subs bis morgen.de", "check twitch.tv for subs", "my growth on youtube.com", "get more subs at boost.gg now", "promotion at shop.store today".

Entscheidend: Ein Angebotswort, das nur IN der Domain steckt (etwa `follower.io`), zählt nicht, es braucht ein zweites, getrenntes Angebots-Token. Das schließt die naheliegendsten Fehl-Lerner aus. Selbst bei den lernbaren Sätzen bleibt der Schaden klein, weil ein Fehl-Lernen eine Richter-Fehlklassifikation voraussetzt und danach nur die kanonisierte Ganzphrase trifft (Kompaktform meist ab 12 Zeichen). Das entspricht exakt der Contract-Absicht (REQ-01/REQ-02). Kein Defekt.

Zusatz-Entfernung ("(no space)"/"(remove the space)"): `kanonische_angebot_domain` (spam_filter.rs) entfernt jede Klammer, die "space" enthält, per Regex und zieht " ." zu ".". Test `gate_lernt_angebot_plus_domain` und `angebot_plus_domain_wird_vom_richter_gelernt` belegen "ai viewers twitch .ad (no space)" wird zu "ai viewers twitch.ad".

## Prüfpunkt 2: Kompakt-Matching-Falle

Kein Treffer auf harmlose Nachrichten. Die Phrase-Kompaktprüfung (`pc.len() >= 4 && compact_str.contains(pc)`, spam_filter.rs calculate_spam_score) läuft über die KOMPLETTE kanonisierte Phrase, nicht über ein Domain-Fragment. Für "ai viewers twitch.ad" ist `pc = "aiviewerstwitchad"` (17 Zeichen); "die twitch ads nerven" (kompakt "dietwitchadsnerven") enthält das nicht. Genau das ist der Unterschied zur alten Fragment-Falle "twitchad". Test `gelerntes_angebot_muster_trifft_nur_ganzphrase` prüft "die twitch ads nerven", "twitch ad break", "twitch.ad", jeweils Score 0. Kein Gegenbeispiel gefunden.

## Prüfpunkt 3: REQ-04 (zehn Plattformwörter)

Überall abgelehnt:
- Richter-Pfad: `learn_pattern_from_judge` ruft `is_distinctive_spam_pattern` (scam_pitch.rs:1902). Einzelwort: `ist_dienstwort` liefert None über `gate_token` (Wort steht jetzt in `GENERIC_PATTERN_TOKENS`), keine Domain, `ist_angebot_plus_domain` false, also abgelehnt.
- Mensch-Pfad: `learn_handler` ruft `is_distinctive_spam_pattern_vom_menschen` (spam_learning.rs:210). Einzelwort: Phrasen-Ausnahme greift nicht (ein Wort), fällt auf `is_distinctive_spam_pattern`, false.
- Ladepfad: `LearnedPatterns::load` (spam_filter.rs:642) nutzt `is_distinctive_spam_pattern_vom_menschen`, verwirft Altbestand mit Warn-Log.

Test `gate_lehnt_generische_plattformwoerter_ab` deckt alle zehn Wörter über beide Gates; `gelerntes_plattformwort_wird_beim_laden_verworfen` deckt den Ladepfad ("bei Discord erlauben links zu öffnen" ergibt keinen `Learned-`-Grund, Score 0). Der Test-Konstruktor `mit_gelernten_spam_mustern` filtert mit demselben `vom_menschen`-Gate wie der Prod-Ladepfad, reproduziert also ehrlich.

Prod-Liste einzeln durchgerechnet (Ladepfad `vom_menschen`):
- streamboo.com: `ist_dienstdomain` (Label "streamboo" distinktiv), BLEIBT.
- eballo.com: `ist_dienstdomain` (Label "eballo"), BLEIBT.
- "boost viewers on the stream - promotion. ru": Phrasen-Ausnahme (8 Wörter, 43 Zeichen), BLEIBT.
- stream_promotion_bot: `ist_dienstwort` (kompakt "streampromotionbot", kein Punkt, ab 6), BLEIBT.
- twitchstar: `ist_dienstwort` ("twitchstar" nicht generisch, ab 6), BLEIBT.
- twitchmax: `ist_dienstwort`, BLEIBT.
- streamerbeat: `ist_dienstwort`, BLEIBT.
- nyvexx2: `ist_dienstwort` (kompakt "nyvexx2", ab 6), BLEIBT.
- discord: keine Domain, `gate_token` liefert None (jetzt generisch), `ist_dienstwort` false, `ist_angebot_plus_domain` false, FÄLLT WEG. Einzig discord fällt weg, wie gefordert.

## Prüfpunkt 4: Amendment A1 (Additivität)

Wiederhergestellt und beweisbar. `calculate_spam_score` ist gegen origin/main zeilenweise identisch (Diff nach Entfernen der Kommentarzeilen leer). Das Viewer-Muster läuft unbedingt (`if viewer_pattern_re()...`), die gelernte Phrase addiert danach; keine Unterdrückung. Die Scoring-Reihenfolge und die Eskalatoren-Bedingung (`hits < SPAM_MIN_MATCHES && has_hard_spam_signal`) sind unverändert. Die zwischenzeitliche Viewer-Unterdrückung aus 560f5962 wurde durch bcb64a32 zurückgenommen. Tests `gelerntes_angebot_muster_bannt_erstnachricht_ohne_richter` (Score 3, Ban, beide Gründe) und `additivitaet_viewer_bleibt_neben_gelernter_phrase` (eballo.com plus Viewer, Score ab 3, Ban) grün.

## Prüfpunkt 5: Scope

Nur erlaubte Dateien: `git diff --stat` zeigt genau spam_filter.rs, scam_pitch.rs, spam_learning.rs und drei Dateien unter `.tasks/2026-09-07-.../`. pipeline.rs (erlaubt) nicht angefasst. Keine bestehende Assertion gelöscht oder abgeschwächt; einzige Löschungen im Diff sind Kommentarzeilen in `calculate_spam_score`. Kein neuer `sqlx::query!`-Aufruf, also keine `.sqlx`- oder Schema-Snapshot-Änderung nötig (INV-07); Suite läuft mit `SQLX_OFFLINE=1` grün.

## Prüfpunkt 6: Invarianten

- INV-01 (SPAM_MIN_MATCHES=3, Eskalatoren, Bedingung): unverändert (calculate_spam_score und evaluate-Eskalatorblock byte-identisch). OK.
- INV-02 (Domain- und Dienstwort-Muster weiter lernbar; best/hello viewers, viewer.com, view.ers, ai viewers, cheap viewers... weiter nicht): `ist_dienstdomain`/`ist_dienstwort` unverändert; Test `gate_lernt_angebot_plus_domain` deckt alle Negativfälle. OK.
- INV-03 (Phrasen-Ausnahme vom_menschen ab 5 Wörtern und 30 Zeichen, nicht für Richter): unverändert (PHRASE_MIN_WORDS=5, PHRASE_MIN_CHARS=30). OK.
- INV-04 (Broadcaster/Mods/safe_list, Safe-Wording, LEARN_MIN_CONFIDENCE): pipeline.rs/safe_list.rs unangetastet. OK.
- INV-05 (kein neues LLM, kein neuer Richter-Aufruf, kein Prompt-Change): erfüllt. OK.
- INV-06 (keine Tests gelöscht/abgeschwächt): erfüllt. OK.
- INV-07 (keine Migration/Tabelle/ENV): erfüllt; INSERT in spam_learning.rs unverändert. OK.

## Prüfpunkt 7: Tests

`cargo test -p tb-chat --lib` (Toolchain 1.97, TB_TEST_REQUIRE_DB=1): 790 passed; 0 failed; 4 ignored in 270,85 s. Die sieben neuen Tests laufen und sind grün. `cargo test -p tb-internal-api`: kompiliert und läuft (0 Lib-Tests, EXIT 0), die neue `angebot_domain_speicherform`-Nutzung baut also.

## Hinweise (nit, nicht blockierend)

- spam_filter.rs `DOMAIN_TLDS` enthält sehr kurze Alltagsendungen ("ad", "me", "gg", "co", "de", "cc", "tv", "bio"). Das weitet die Domain-Form-Fläche. Durch die Kombination aus drei Tokens, separatem Angebotswort, Ganzphrasen-Speicherung und Richter-Gate bleibt der Effekt eingegrenzt; kein Handlungsbedarf für den Merge.
- Auf dem Richter-Pfad wird `kanonische_angebot_domain`/`ist_angebot_plus_domain` doppelt gerechnet (im Gate und in `angebot_domain_speicherform`). Reine Redundanz, korrekt, nicht merge-relevant.

## Nachtrag Gate-Runde 1

Fund (blockierend): Die Aussage in Prüfpunkt 1 und der Freigabesatz oben deckten die Trennform mit Leerzeichen auf beiden Seiten des Punkts nicht ab. `kanonische_angebot_domain` zog mit `.replace(" .", ".")` nur das Leerzeichen vor dem Punkt zusammen. `kanonische_angebot_domain("ai viewers streamboo . com")` ergab "ai viewers streamboo. com", Tokens `["ai","viewers","streamboo.","com"]`, `ist_domainform` false, das Muster wurde nicht gelernt. REQ-01 nennt "streamboo . com" wörtlich als Pflichtfall (40 von 65 Live-Fällen).

Fix (Commit 3d52da8e): Ein Regex `\s+\.\s*` zieht Leerraum vor und nach dem Punkt zusammen. "streamboo . com" und "streamboo .com" werden zu "streamboo.com". "promotion. ru" (kein Leerzeichen vor dem Punkt) bleibt unverändert, INV-02 gilt weiter. Der Fix sitzt im gemeinsamen Helfer und wirkt damit auf beiden Speicherpfaden (Richter in `scam_pitch.rs`, Mensch in `spam_learning.rs`).

Grenze (INV-02 hat Vorrang): Die Form "streamboo. com" (Punkt am Wort, Leerzeichen danach, 7 Live-Fälle) bleibt außen vor, weil sie strukturell identisch zu "promotion. ru" ist und nur über die TLD-Liste unterscheidbar wäre; "ru" ist eine echte TLD, ein Zusammenziehen träfe "promotion. ru" mit und verletzte INV-02.

Testnachweis: Neue Tests `gate_lernt_angebot_domain_leerzeichen_beidseits` (Distinktivität und Kanonform für "ai viewers streamboo . com", "best viewers eballo . com (remove the space)", Negativfall "boost viewers on the stream - promotion. ru") und `gelernte_phrase_trifft_kompaktform_mit_leerzeichen` (Learned-Phrase-Treffer auf "Ai viewers streamboo . com"). Der erste war vor dem Fix rot (`assertion failed: is_distinctive_spam_pattern("ai viewers streamboo . com")` an spam_filter.rs:1775), nach dem Fix grün. `cargo test -p tb-chat --lib`: 792 passed, 0 failed, 4 ignored. `cargo test -p tb-internal-api`: 303 passed, 0 failed.

Nit 6 mit erledigt: die verbliebenen Kommentare "Schritt 1" bis "Schritt 4" in `calculate_spam_score` entfernt (die Kommentare "Schritt 5" und "Schritt 6+7" fehlten bereits), reiner Kommentar-Diff, Repo-Regel keine Code-Kommentare.
