status: erledigt
datum: 2026-09-05

# Adversariales Review: Discord-Pitches mit Qualität statt Preset-Sprüchen

Rolle Stufe 4, read-only. Gegenstand: Worktree `/home/nathanael/.worktrees/tb-discord-pitch`, Branch `feat/discord-pitch-qualitaet`, HEAD `ec74eaad`, gegen `origin/main` `f91d07f3` (merge-base `ec1ba600`).

## TLDR

Kein FREIGABE. Die Funktion ist fachlich korrekt umgesetzt, alle acht REQ und alle neun INV sind erfüllt, Scope sauber, 19 Unit-Tests grün, Rebase konfliktfrei. Ein Punkt sollte vor dem Deploy fallen: der LLM-Judge läuft für jede qualifizierende Chat-Nachricht in jedem Live-Partnerkanal ohne Versuchs-Drossel, die im Plan zugesagte Kostendeckelung greift nicht (nur gesendete Pitches werden gezählt, nicht die Modellaufrufe). Dazu flutet die Log-Tabelle entgegen der im STATUS behaupteten Politik.

## Testlauf

`cd rust && SQLX_OFFLINE=true cargo test -p tb-chat --lib promo_pitch::` EXIT=0, 19 passed, 0 failed (Parser, JSON-Extraktion aus Rohtext, alle harten Filter inkl. Gedankenstrich/Emoji/Join/400 Zeichen, Guard-Test Nur-Fireworks-Liste, finalize_channel_promo/targeted). Die DB-Tests (`db_tests::anlass_*`) brauchen `TB_TEST_DATABASE_URL` (hier nicht gesetzt) und konnten nicht laufen; ihr grüner Stand ist nur aus PLAN/STATUS belegt, nicht nachgeprüft.

## REQ-Erfüllung

- REQ-01 erfüllt: `on_message_pitch` antwortet als `@login`-Nachricht (Amendment), Judge-Timeout 20 s, gesendet in `promos.rs:788-792`.
- REQ-02 erfüllt: `FireworksPitchJudge::decide` ruft `tb_llm::complete(USE_CASE, ..)` mit JSON-Objekt, Parse in `promo_pitch.rs:296-304`; ohne Anlass kein Send (`promos.rs:770-774`).
- REQ-03 erfüllt: harte Filter über `pitch_filter_reject` vor dem Senden, `promos.rs:776-786`; kein Fallback-Text.
- REQ-04 erfüllt: alle fünf Preset-Listen plus `PresetPicker`/`RandomPresetPicker`/`PromoPreset` gelöscht (Diff `promos.rs`, `chat_wiring.rs`); `build_promo_text` nutzt LLM nur bei fehlenden Overrides und hängt den Invite an (`promos.rs:1275-1289` via `finalize_channel_promo` `promo_pitch.rs:328-337`), Targeted-Pfade tragen nie einen Link (`promos.rs:1959-2130`). Kein fester Text bei `None`.
- REQ-05 erfüllt: User-Limit 7 Tage über alle Kanäle (`pitch_user_limit_ok` `promos.rs:852-866`, kein Kanal-Filter), Kanal-Limit 3/Stream mit 600 s Abstand (`pitch_channel_limit_ok` `promos.rs:868-895`), Cooldown-Kopplung `mark_promo_sent("anlass_pitch")` gleicher `save_promo_cooldown(login, "sent", ..)`-Pfad wie `send_timeout_pitch` (`promos.rs:818-824`, `1354-1375`).
- REQ-06 erfüllt: Gate-Reihenfolge Partner, Allowlist, `promo_disabled`, Suppression, Startverzögerung, Doppelsend-Lock in `promos.rs:712-754`.
- REQ-07 erfüllt (mit Nebenwirkung, siehe Mangel 2): `record_pitch_log` auf allen Pfaden, Review-Karte in `promos.rs:826-828`.
- REQ-08 erfüllt: `faq-werbung.md` beschreibt Anlass-Antwort ohne Link, Link nur in periodischer Einladung, korrekt und ohne Werbefloskeln.

## INV-Erfüllung

- INV-01 erfüllt: `promo_blocked_by_plan_or_flag`-Gate greift auch im Anlass-Pfad (`promos.rs:726-730`), Test `anlass_pitch_bei_promo_disabled_sendet_nichts`.
- INV-02 erfüllt: ausschließlich `tb_llm::complete`, `promo_pitch` in `FIREWORKS_ONLY_USE_CASES` (`selection.rs:14`), kein Modellname oder HTTP-Client im neuen Code; Guard-Test grün.
- INV-03 erfüllt: `build_promo_text` prüft globalen und Streamer-Override vor dem LLM-Text (`promos.rs:1276-1282`).
- INV-04 erfüllt: `send_timeout_pitch`, `twitch_promo_cooldowns` und Doppelsend-Lock unverändert.
- INV-05 erfüllt: `commands.rs` nicht angefasst (nicht im Diff).
- INV-06 erfüllt: acht gelöschte Preset-/Pool-Tests (`jeder_promo_text_traegt_einen_link_und_ist_einzigartig`, `promo_texte_haben_keine_gedankenstriche`, `partner_texte_liegen_im_chat_activity_pool`, `coaching_texte_liegen_im_chat_activity_pool`, `promo_pools_sind_nicht_leer`, `community_texte_liegen_im_chat_activity_pool`, `promo_text_rotiert_anti_repeat` plus Helfer `alle_sichtbaren_promo_texte`) ersetzt durch `periodischer_promo_text_traegt_invite_am_ende_ohne_strich`, `periodischer_promo_text_leer_gibt_keinen_text`, die vier `anlass_pitch_*`-DB-Tests und 19 `promo_pitch::tests`. Die Gedankenstrich-Prüfung lebt weiter (Filtertest plus `hat_gedankenstrich`).
- INV-07 erfüllt: keine ENV-Config, kein Secret, keine Änderung an `streamer_plans`/Preisplänen.
- INV-08 erfüllt: `git diff --stat origin/main...HEAD -- rust/crates/tb-engagement` ist leer; `outreach_shadow.rs` und Smalltalk unberührt. In `chat_wiring.rs` fällt nur ein ungenutzter `ChatMessage`-Import aus `tb_engagement::minimax_chat` weg, keine Crate-Änderung.
- INV-09 erfüllt: Migration `20260905090000_twitch_promo_pitch_log.sql` additiv (`CREATE TABLE IF NOT EXISTS`), zwei Indizes `(target_user_id, sent_at)` und `(channel_login, sent_at)` decken die Limit-Abfragen; keine GRANTs im File (bewusst, Deploy-Schritt als `postgres`).

## Scope

`git diff --stat origin/main...HEAD` berührt nur erlaubte Pfade: `promos.rs`, `promo_pitch.rs`, `lib.rs`, `pipeline.rs`, `chat_wiring.rs`, `main.rs`, `selection.rs`, `faq-werbung.md`, `migrations/`, `.tasks/` sowie sechs `rust/.sqlx/*.json` (mechanisch, bekannt). Kein verbotener Pfad (`tb-engagement/`, `tb-dashboard-api/`, `commands.rs`, `website/`, Lint/CI, bestehende Migrationen) angefasst.

## Mängelliste

1. sollte (hoch, vor Deploy beheben). LLM-Judge ohne Versuchs-Drossel, die Kostendeckelung des Plans greift nicht. `promos.rs:756-765`. Fehlbild: In einem Live-Partnerkanal, der alle Gates passiert (nicht werbefrei, nicht gemutet, älter als 10 min, erlaubt), ruft `on_message_pitch` `pitch_judge.decide` (Deepseek) für JEDE Nachricht mit mindestens 25 Zeichen ohne Befehlspräfix auf. Die Limits `pitch_user_limit_ok`/`pitch_channel_limit_ok` zählen nur Zeilen mit `sent_at IS NOT NULL`, also gesendete Pitches. Solange kein Pitch gesendet wurde (der Normalfall, weil die meisten Nachrichten keinen Anlass treffen), bleibt `count = 0` und es gibt keine Drossel vor dem Modell; der einzige Vorfilter sind die 25 Zeichen. Ein einzelner Chatter mit zehn Nachrichten löst zehn Modellaufrufe aus. Das widerspricht direkt der im PLAN unter "Risiken" zugesagten Gegenmaßnahme ("Kanal-Limit 3 pro Stream mit 10 min Abstand"), die den Modellverbrauch drosseln sollte, aber als reines Sende-Limit implementiert ist. Bei einem Partnernetz mit mehreren Live-Kanälen sind das dauerhaft viele Deepseek-Aufrufe pro Minute. Fix: den Judge hinter eine leichte Versuchs-Drossel setzen (z. B. In-Memory pro Kanal höchstens ein Judge-Aufruf alle N Sekunden und pro Chatter-ID ein Dedup-Fenster), bevor `decide` aufgerufen wird, ohne dafür DB-Zeilen zu schreiben.

2. sollte. Log-Tabelle wird geflutet und widerspricht der dokumentierten Politik. `promos.rs:717,722,732,737,766,771`. Fehlbild: Der STATUS M5 (PLAN) behauptet "Vorfilter/Partner/Allowlist/Suppression/Startverzögerung schweigen, um die Log-Tabelle nicht zu fluten", und beruft sich auf ein "Contract-Amendment 2026-09-05", das im CONTRACT gar nicht existiert (dort steht nur das REQ-01-Amendment vom 2026-09-04). Der Code tut das Gegenteil: `not_partner`, `not_allowed`, `suppressed`, `start_delay` und vor allem `kein_anlass` erzeugen je eine Log-Zeile pro Nachricht. In einem steady-state Live-Partnerkanal schreibt damit fast jede substanzielle Chat-Nachricht eine `kein_anlass`-Zeile in `twitch_promo_pitch_log`, obwohl gar kein Pitch erzeugt wurde. REQ-07 verlangt nur das Protokollieren "erzeugter Pitches (gesendet oder verworfen)"; `kein_anlass` und die frühen Gate-Rejects sind keine erzeugten Pitches. Folge: unbegrenztes Tabellenwachstum. Fix: die frühen Gate-Rejects (Partner, Allowlist, Suppression, Startverzögerung) und `kein_anlass` nicht loggen, wie im STATUS beschrieben; nur Werbefrei-Block, Limit-Ausgänge und erzeugte/gesendete Texte protokollieren. Alternativ die Politik im CONTRACT als echtes Amendment festhalten und einen periodischen Retention-Cleanup ergänzen.

3. Hinweis. Review-Kanal hartkodiert, ignoriert den Env-Override des Smalltalk-Moduls. `chat_wiring.rs:1875`. Die Konstante `PITCH_REVIEW_CHANNEL_ID = 1_374_364_800_817_303_632` stimmt mit `DEFAULT_REVIEW_CHANNEL_ID` in `smalltalk_loop_wiring.rs:37` überein, also greift REQ-07 im Normalfall. Aber Smalltalk erlaubt eine Env-Überschreibung (`review_channel_id_from_env()` `shadow_review_wiring.rs:42`); setzt der Betreiber den Smalltalk-Review-Kanal um, landen die Pitch-Karten weiter im hartkodierten Default. Fix: denselben Env-Weg wie Smalltalk nutzen.

4. Hinweis. Neue Code-Kommentare hinzugefügt. `promos.rs:3872-3874` (Banner "Anlass-Pitch (REQ-01 bis REQ-07)" im Testmodul). Verstößt gegen die No-Kommentar-Regel; drei Zeilen entfernen. Die vielen Bestandskommentare im Rest von `promos.rs` bleiben unberührt, korrektes Vorgehen, weil Löschen den Diff sinnlos aufblähte.

5. Hinweis. `send_lock` wird über den bis zu 20 s langen LLM-Aufruf gehalten. `promos.rs:753-765`. Der Doppelsend-Lock des Kanals wird vor `pitch_judge.decide` genommen und erst am Funktionsende freigegeben. Das serialisiert zwar korrekt gegen Doppel-Sends, blockiert aber den periodischen `on_message`-Promo-Pfad desselben Kanals für die Dauer des Modellaufrufs, und in sehr aktiven Kanälen stauen sich detached `tokio::spawn`-Tasks am Lock auf (unbegrenzte Task- und Speichermenge). Hängt eng mit Mangel 1 zusammen; eine Versuchs-Drossel entschärft beides.

6. Hinweis. Bestands-Gedankenstriche in `faq-werbung.md` bleiben. `faq-werbung.md:34,45,48` enthalten weiter Em-Dashes in nicht geänderten Absätzen, obwohl die Datei angefasst wurde. Keine der hinzugefügten Zeilen enthält einen Gedankenstrich (geprüft). Bei Gelegenheit die drei Bestandsstellen mitziehen.

## Rebase auf origin/main

Ja, konfliktfrei. `git merge-tree --write-tree origin/main HEAD` liefert einen sauberen Baum (`db2169cf...`, EXIT=0, keine Konfliktmarker). Der Branch berührt nur die Promo-Dateien, `origin/main` hat seit der merge-base andere Bereiche geändert (Task-Artefakte, Chat-Klassifikation), es gibt keine Zeilenüberlappung.

Anmerkung zur Vorgabe: Der Auftrag nennt `origin/main = 9c278c87`, nach `git fetch` ist `origin/main` aber bereits `f91d07f3` (9c278c87 ist dessen Vorfahre). Der Rebase auf den tatsächlichen aktuellen Stand `f91d07f3` ist wie beschrieben konfliktfrei.

## Runde 2

Gegenstand: Fix-Commit `a54987dc` plus die aktuelle Fassung von `on_message_pitch` (`rust/crates/tb-chat/src/promos.rs:705-863`). Kein FREIGABE. Ein blockierender Mangel bleibt, sonst sind die vier Prüfpunkte erfüllt.

Erfüllt:

- Punkt 1 Drossel-Wirkung: `pitch_judge_throttle_ok` (`promos.rs:841-863`) sitzt vor `pitch_judge.decide` und damit vor `tb_llm::complete`; je (Kanal, Chatter) ein Aufruf pro 15 min, je Kanal Deckel 30 pro Stunde mit gleitendem `retain`-Fenster. Die Pre-Judge-Prüfungen `pitch_user_limit_ok`/`pitch_channel_limit_ok` (`promos.rs:744-751`) laufen davor, also kein Modellaufruf bei erschöpften Limits. Stub-Test `anlass_pitch_judge_drossel_pro_chatter` deckt den Chatter-Deckel.
- Punkt 2 Log-Politik und REQ-07: Gate-Blocks, Pre-Judge-Limits, Judge-Drossel und kein_anlass gehen auf `tracing::debug` und schreiben keine Zeile (`promos.rs:723-756,768-775`). Eine Zeile in `twitch_promo_pitch_log` entsteht nur bei erzeugtem Modelltext: harter Filter (`promos.rs:777-787`), Post-Judge-Kanal-Limit unter dem Lock (`promos.rs:791-795`), Send-Drop (`promos.rs:806-814`) und gesendeter Pitch (`promos.rs:816-828`). Ein API-Fehler des Modells liefert `decide` = None (`promo_pitch.rs:303`) und ist kein erzeugter Pitch, das Nicht-Loggen ist mit REQ-07 konsistent. REQ-07 ("jeder erzeugte Pitch, gesendet oder verworfen") bleibt erfüllt.
- Punkt 3 send_lock: Der Lock (`promos.rs:790-791`) umschließt nur die wiederholte Kanal-Limit-Prüfung, den Send, das Log und `mark_promo_sent`, `drop(_guard)` in `promos.rs:833`; der bis zu 20 s lange `decide` liegt außerhalb. tokio-`Mutex`, gleiche `get_send_lock`-Instanz wie `on_message` (`promos.rs:696-697`), keine Re-Entranz und kein Halten über einen Aufruf der jeweils anderen Funktion, also kein Deadlock. Die Limit-Prüfung wird unter dem Lock wiederholt (`promos.rs:793`) und die 600-s-Sperre greift, weil Task A vor `drop(_guard)` per `record_pitch_log` mit `sent_at` schreibt; ein zweiter Anlass-Pitch im selben Kanal wird korrekt serialisiert weggeworfen.
- Punkt 4: Kein neuer Kommentar im Fix, das Banner im Testmodul ist entfernt. Keine Gedankenstriche in nutzersichtbarem Text (die neuen Strings sind Log-Zeilen).

Blockierender Mangel:

1. Unbegrenztes Speicherwachstum der Chatter-Drossel-Map. `rust/crates/tb-chat/src/promos.rs:519,860` und `rust/crates/tb-chat/src/promos.rs:2592-2600`. Fehlbild: `pitch_judge_last` (`DashMap<String, Instant>`, Schlüssel `"{login}|{chatter_id}"`) bekommt in `promos.rs:860` für jede (Kanal, Chatter)-Kombination, die die Drossel passiert, einen Eintrag und verliert ihn nie wieder. `prune_promo_runtime_state` (`promos.rs:2592`) räumt ausschließlich `channel_states` auf und fasst `pitch_judge_last` nicht an; auch der Prune-Takt alle 60 s (`promos.rs:988`) erreicht die Map nicht. Über die Laufzeit des Bots wächst sie mit der Zahl der jemals aktiven Zuschauer je Partnerkanal monoton, obwohl jeder Eintrag nach dem 15-min-Cooldown wertlos ist. Das ist dieselbe Klasse unbegrenzten Wachstums, die der Fix beim Kanal-Vektor per `retain` (`promos.rs:854`) bereits vermeidet, für die Chatter-Map aber offen lässt. Fix: `pitch_judge_last` (und die im Prinzip beschränkte, aber ebenfalls nie geräumte Vektor-Map `pitch_judge_channel`) in `prune_promo_runtime_state` mitprunen, Einträge älter als `PITCH_JUDGE_CHATTER_COOLDOWN` bzw. mit leerem/veraltetem Fenster entfernen.

## Runde 3 (Behebung)

Der blockierende Mangel aus Runde 2 ist behoben. `prune_promo_runtime_state` (`rust/crates/tb-chat/src/promos.rs:2592`) prunt jetzt zusätzlich beide Judge-Maps im bestehenden 60-s-Takt: `pitch_judge_last` verliert Einträge, deren letzter Aufruf länger als `PITCH_JUDGE_CHATTER_COOLDOWN` (15 min) zurückliegt; `pitch_judge_channel` behält je Kanal nur Zeitstempel innerhalb von `PITCH_JUDGE_CHANNEL_WINDOW` (1 h) und entfernt den Kanal-Eintrag, sobald sein Vektor leer ist. Damit wachsen beide Maps nicht mehr monoton über die Bot-Laufzeit.

Regressionstest `prune_raeumt_judge_maps_nach_cooldown` (`promos.rs`, DB-los über `dummy_pool()`): setzt je Map einen abgelaufenen und einen frischen Eintrag, ruft `prune_promo_runtime_state` mit vorgerückter Zeit und prüft, dass der alte (Kanal, Chatter)-Eintrag weg ist, der frische bleibt und ein gemischter Kanal-Vektor nur den frischen Zeitstempel behält. Ohne den Fix rot (Assertion an `pitch_judge_last`), mit Fix grün.
