# Plan: Pitch-Stimme und Lernschleife

status: aktiv
datum: 2026-09-09
contract: CONTRACT.md (Ziel, REQ, INV dort)
evidence: EVIDENCE.md

Reihenfolge ist verbindlich. Nach jedem Milestone Validierung; rot heißt stehen bleiben und fixen, nie weiter.

## M1 Adressat (REQ-02, REQ-09 Teil 2)

Änderungen:
- `tb-chat/src/types.rs`: `ChatMessageEvent.reply: Option<ChatReply>` mit `parent_user_id`, `parent_user_login` (EventSub-Feld `reply`, serde default None); `MessageFragment.mention: Option<MentionRef>` mit `user_id`, `user_login`.
- `tb-chat/src/promos.rs`: Funktion `adressat_fremd(event: &ChatMessageEvent) -> bool`: true bei `reply` vorhanden, bei Mention-Fragment auf einen anderen als den Chatter selbst, oder wenn der Text (lowercase, Wortgrenze) `broadcaster_user_login` oder `broadcaster_user_name` enthält. In `on_message_pitch` direkt nach den Kanal-Gates: bei true `log_zuschauer_reject(..., "adressat_fremd", ...)` und return, also vor Partner- und Anlass-Pfad.
- Tests: drei synthetische Events (Reply, Mention, "na marcy, schön eingeranked?") ergeben Reject; ein normales Event läuft durch.

Zwischenzustand: Nachrichten an den Streamer lösen keinen Pitch mehr aus.
Validierung: `SQLX_OFFLINE=1 cargo test -p tb-chat --lib adressat` grün; Deserialisierung eines echten EventSub-JSON mit `reply`-Block im Test.
Stop-Regel: EventSub-Feldnamen unklar → Twitch-Doku `channel.chat.message` v1 prüfen, nicht raten.

Status: erledigt (Commit 4fad946f). `cargo test -p tb-chat --lib adressat` 5 passed, 0 failed (780 filtered). Amendment zum Scope: 3 Test-Helfer-Literale ausserhalb des erlaubten Bereichs mussten ein Feld ergaenzen (siehe CONTRACT Amendments).

## M2 Stilvertrag, Judge-Feld, harte Filter (REQ-01, REQ-03, REQ-04, REQ-08, REQ-09 Teil 1)

Änderungen in `tb-chat/src/promo_pitch.rs`:
- `pub const STILVERTRAG: &str` (Bot-Identität, Ich-Form-Verbot, Faktenbremse, Humorregel, Ton aus `community-ankuendigung`: Umgangssprache in Dosen, Selbstironie, Smiley nur `:)`, keine Werbesprache, keine Gedankenstriche, echte Umlaute), per `concat!` oder `format!` in `PITCH_SYSTEM_PROMPT`, `PARTNER_PITCH_SYSTEM_PROMPT`, `CHANNEL_PROMO_SYSTEM_PROMPT` eingebettet; `TARGETED_PITCH_SYSTEM_PROMPT` und alles Targeted-Only löschen (toter Rest, siehe Review vom 2026-09-08).
- Judge-JSON um `"ernst_gemeint": bool`; `parse_pitch_response` liest es (Default false); Anlass zählt nur bei `ernst_gemeint == true`. Prompt: Scherz, Trollen, Sarkasmus, Zugangsfragen → occasion null.
- `ich_form_reject(reply) -> Option<&'static str>` (Formen aus REQ-03, Wortgrenzen, case-insensitiv) und `beleidigung_reject(reply)` (Sperrliste als `const [&str]`, gängige deutsche Beleidigungen und Fäkal-/Sexualwörter, Wortgrenzen) in die Kette von `pitch_filter_reject` vor dem Smiley-Check; Reject-Gründe `ich_form`, `beleidigung`.
- `build_partner_pitch_text` und `build_channel_promo_text`: `.denken_aus()` und Token-Budget setzen (Judge 300, Texte 220; Werte als Konstanten).
- Fixture `tests/fixtures/pitch_log_2026-09-09.txt` oder inline: die 17 gesendeten Antworten aus dem Log; Test erwartet Reject für die vier Ich-Form-Texte aus dem Contract-Ziel und Durchlass für den Rest (die Spielwissen-Texte fallen nicht über Filter, das ist Prompt-Sache; im Test dokumentieren).

Zwischenzustand: Prompts tragen den Stilvertrag, Filter verwerfen Ich-Form und Beleidigungen.
Validierung: `cargo test -p tb-chat --lib promo_pitch` grün inkl. Fixture-Test; bestehende Filter-Tests unverändert grün.
Stop-Regel: ein bestehender Test wird rot, weil sein Fixture jetzt als Ich-Form gilt → Fixture-Text minimal ändern, Eigenschaft des Tests bleibt (INV-04), im Commit begründen.

Status: erledigt (Commit 0b7fae0c). `cargo test -p tb-chat --lib promo_pitch` 31 passed, 0 failed (756 filtered). Targeted-Only entfernt; pitch_response-Testhelfer bekam ernst_gemeint: true (Eigenschaft bleibt).

## M3 Beispielblock (REQ-05)

Änderungen:
- Neues Modul `tb-chat/src/pitch_beispiele.rs` (in lib.rs registrieren): `enum PitchPfad { Anlass, Partner, Periodic }`, `START_BEISPIELE` je Pfad (mindestens sechs, Stil siehe Stilvertrag; Startbeispiele im Anhang dieses Plans), `struct Beispiel { ausloeser: Option<String>, antwort: String }`, `async fn lade_gelernte(pool, pfad) -> (gut: Vec<Beispiel>, schlecht: Vec<Beispiel>)` aus `twitch_promo_pitch_log` (bewertung, jüngste zuerst, gut max 8, schlecht max 3), `fn baue_block(start, gut, schlecht) -> String` (ab vier gelernten guten ersetzen sie die Startbeispiele, sonst Startbeispiele; schlecht als "So nicht:"-Liste), Muster nach `tb-engagement/src/style_examples.rs`, kein Import aus tb-engagement.
- `promos.rs`: Block je Aufruf laden und als Teil der User-Message (Feld `beispiele`) an Judge, Partner und periodische Promo geben; bei DB-Fehler nur Startbeispiele.
- Tests: Ersetzungsschwelle, Deckel, Reihenfolge; DB-Test mit drei gut-Zeilen (Startbeispiele bleiben) und vier gut-Zeilen (gelernte ersetzen).

Zwischenzustand: Prompts enthalten Beispiele, gelernte greifen ab vier.
Validierung: `cargo test -p tb-chat --lib pitch_beispiele` grün; DB-Test mit `TB_TEST_DATABASE_URL`.
Stop-Regel: Prompt über 6000 Zeichen → Beispielzahl senken, nicht Stilvertrag kürzen.

Status: erledigt (Commit folgt in dieser Runde). pitch_beispiele-Tests grün im vollen tb-chat-Lauf (793 passed, DB-Tests liefen). Migration und Schema-Snapshot der drei Spalten sind schon in diesem Commit (Schema-Fundament fuer M3 bis M5). Prompts nutzen echte Umlaute (Korrektur der ae/oe/ue-Fassung aus M2).

## M4 Karte mit Message-ID und Migration (REQ-06)

Änderungen:
- Migration `rust/migrations/20260909150000_twitch_promo_pitch_log_bewertung.sql`: `review_message_id BIGINT`, `bewertung TEXT CHECK (bewertung IN ('gut','schlecht'))`, `bewertet_at TIMESTAMPTZ`, Teilindex auf `(sent_at)` WHERE `review_message_id IS NOT NULL AND bewertung IS NULL`.
- `PitchReviewSink::send_card` liefert `Option<i64>` (Message-ID); `DiscordPitchReviewSink` liest sie aus `SendResult.result.message_id`; Kartentext bekommt eine Zeile "Daumen hoch oder Daumen runter als Reaktion, der Bot lernt daraus".
- `promos.rs`: nach `send_card` `UPDATE twitch_promo_pitch_log SET review_message_id = $1 WHERE id = $2`.
- `rust/.sqlx` per `cargo sqlx prepare` gegen die Test-DB nachziehen, Schema-Snapshot `tb-db/tests/fresh_schema_snapshot.txt` aktualisieren.

Zwischenzustand: jede neue Karte ist mit ihrer Log-Zeile verknüpft.
Validierung: `cargo test -p tb-chat --lib` und `cargo test -p tb-bot pitch_review` grün; `cargo sqlx prepare --check` sauber; tb-db-Snapshot-Test grün (Docker-Container aus `rust/scripts/test_db.sh`).
Stop-Regel: Snapshot-Test verlangt Handarbeit außerhalb des Scopes → stoppen und melden.

Status: erledigt (Commit folgt). send_card liefert Option<i64>, DiscordPitchReviewSink liest SendResult.result.message_id, Karte traegt den Daumen-Hinweis, on_message_pitch und run_partner_pitch schreiben review_message_id. tb-bot pitch_review 1 passed; Migration/Snapshot in M3-Commit. Hinweis: promos.rs traegt hier auch die M3-Beispielverdrahtung (Dateien nicht per Hunk trennbar, git add -p gesperrt).

## M5 Bewertungs-Timer (REQ-07)

Änderungen:
- `tb-transport-discord/src/backend.rs`: Trait-Methode `fetch_message_reactions(channel_id, message_id) -> Result<Vec<Reaktion { emoji: String, count: u32 }>, DiscordError>`; `relay.rs`: GET `/internal/master/v1/discord/message-reactions?channel_id=&message_id=`, Antwort `{found, reactions: [{emoji, count}]}`; `found == false` heißt Karte gelöscht → Zeile als `bewertet_at = now()` ohne Bewertung abschließen.
- Neues Modul `tb-chat/src/pitch_bewertung.rs`: `trait ReaktionsQuelle` (damit testbar), `async fn bewerte_offene_karten(pool, quelle, kanal_id, jetzt)`: Zeilen mit `review_message_id` und `bewertung IS NULL` und `sent_at > jetzt - 14 Tage`; Daumen hoch `👍` (auch Hautton-Varianten per Prefix) → gut, Daumen runter `👎` → schlecht, beides → schlecht, keine Reaktion → unverändert; Broker-Fehler einmal je Lauf per `tracing::warn!`.
- `tb-bot/src/main.rs`: Timer alle 10 Minuten (Konstante), Start nur wenn `review_relay` vorhanden.
- Tests: Fake-Quelle mit Fällen gut, schlecht, beides, leer, gelöscht; DB-Test schreibt und liest `bewertung`.

Zwischenzustand: Reaktionen landen als Bewertung in der DB und wirken über M3.
Validierung: `cargo test -p tb-chat --lib pitch_bewertung` grün; `cargo check --workspace` sauber; `cargo test -p tb-bot` grün gegen die bekannte Baseline.
Stop-Regel: Emoji-Format des Brokers weicht ab (Name statt Unicode) → an der Live-Antwort messen (Python-Helfer, kein curl), Mapping anpassen, nicht raten.

## M6 Abschluss

- Migration auf Prod von Hand als `postgres` anwenden, Rechte an `twitchbot` und `twitchdash`, Eintrag in `_sqlx_migrations` mit sha384-Checksumme (Memory `twitch-release-deploy-weg`, Infrastruktur-Fakten).
- Review durch frischen Agenten gegen Contract und Diff, dann Gate, Merge, Release in eigenem Worktree, Deploy über `install-twitch-release`, Neustart `deadlock-twitch-bot-rust`.
- Live-Beweis: Karte eines echten Pitches trägt den Hinweis; Testreaktion des Nutzers auf eine Karte erscheint innerhalb von 10 Minuten als `bewertung` in der DB; Journal `-p err` leer.

## Anhang: Startbeispiele (Entwurf, Nutzerbeispiele ersetzen sie wortgleich)

Anlass (Auslöser → Antwort):
- "hab keine leute zum zocken" → "@{login} solo in deadlock ist wie solo im lane, geht, macht aber keinen spaß. im discord der community hängen abends immer welche rum, die genau dich suchen :)"
- "spiel ist tot" → "@{login} totgesagte spiele leben länger, sagt der bot, der davon lebt. die deutsche community ist jedenfalls quicklebendig."
- "warum sind alle so tryhard" → "@{login} weil deadlock leute anzieht, die drei stunden patchnotes lesen. gibt aber auch die entspannte fraktion, die trifft sich im community discord."
- "solo queue ist die hölle" → "@{login} solo queue ist nicht die hölle, die hölle hat wenigstens feste teams. die community kennt das leid, da findet man leute für stack."
- "hab gestern angefangen, versteh nix" → "@{login} willkommen, in den ersten tagen versteht niemand was, das gehört zum spiel. wenn du fragen hast, im community discord beißt keiner :)"
- "welchen hero soll ich lernen" → "@{login} den, der dir am wenigsten aufs maul gibt. ehrliche antwort gibts von den leuten im community discord, die spielen das länger als der bot existiert."

Partner (Auslöser → Antwort):
- "nice, graves läuft bei dir" → "@{login} graves läuft, der chat läuft, nur der bot hier läuft im kreis. wenn du öfter deadlock streamst: die deutsche deadlock community hat ein partner-netzwerk, wer offline geht, schickt seine zuschauer zum nächsten deutschen streamer, und bekommt selbst raids zurück. dazu chat-schutz gegen spam und scam."
- "bin auch am streamen, kommt keiner" → "@{login} kennt jeder, der mit drei zuschauern startet, zwei davon der bot. bei der deutschen deadlock community gibt es ein partner-netzwerk: raids gehen automatisch an deutsche deadlock-streamer, die gerade live sind, und kommen genauso zurück."
- (vier weitere im selben Muster, vom Implementierer nach dem Stilvertrag, Mechanik immer ehrlich und in dritter Person)

Periodische Einladung (Kontext → Satz vor dem Link):
- Chat redet über builds → "wer hier gerade über builds streitet: im community discord streiten sie seit monaten, mit besseren argumenten"
- ruhiger Chat → "still hier. im discord der community ist es lauter, und man findet leute zum zocken"
- (vier weitere)
