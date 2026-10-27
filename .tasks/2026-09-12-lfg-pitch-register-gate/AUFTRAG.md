# Auftrag: lfg-pitch-register-gate

status: aktiv (2026-09-12)

## Ziel

Zwei Fehler in einem Pfad: der Bot hält harmlose Sätze wie "Drifter und noch
wer" für eine Mitspieler-Suche, und er lädt dann auch Leute ein, die längst in
der Community sind.

Der Bot antwortet auf Mitspieler-Suchen im Chat mit einer Discord-Einladung
("Schau gerne mal in unsere Community rein ..."), auch bei Leuten, die längst
in der Community sind. Am 2026-09-12 um 23:50 traf es den Betreiber selbst
(EarlySalty, Nachricht "Drifter und noch wer"). Nach dem Fix bekommt diese
Einladung nur, wer das Zuschauer-Register-Gate besteht: kein Broadcaster, Mod
oder Bot, keine bekannte Discord-Zuordnung, kein Partner, Streamer oder
Betreiber, kein Stammzuschauer.

## Ursache (aus dem Vorcheck)

Es gibt zwei Einladungspfade. Der Anlass-Pitch (`promos.rs::on_message_pitch`)
läuft seit Release 9c128ed6 über `ZuschauerRegister::gate`. Der LFG-Pitch
(`lfg_pitch.rs::LfgPitchResponder::decide`) ist ein eigener Pfad in der
Pipeline (`pipeline.rs:1289`, Schritt `lfg_pitch`) und prüft nur Regex,
Werbefrei-Plan, Invite-URL, Cooldowns und den LLM-Judge. Er kennt weder das
Register noch Mod/Broadcaster/Partner-Ausschlüsse. Das ist der Pfad aus dem
Screenshot (Text in `lfg_pitch.rs:24`).

## Arbeitsschritte

1. `LfgPitchResponder` bekommt einen optionalen Setter
   `set_zuschauer_register(Arc<ZuschauerRegister>)` analog zu
   `PromoEngine::set_zuschauer_register` (`promos.rs:666`).
2. In `decide` nach dem Regex-Match und vor dem Judge-Aufruf (keine
   LLM-Kosten für Ausgeschlossene):
   - Ist kein Register gesetzt: `Silent(SilentReason::RegisterFehlt)`,
     fail-closed wie im Anlass-Pitch (`promos.rs:897`).
   - `register.gate(event)` liefert `Reject(grund)`: `Silent` mit neuem
     `SilentReason::RegisterReject` und dem Grund im Log (Feld `grund`).
   - `Pass`: weiter wie bisher.
   Zusätzlich vor dem Register hart: `event.is_mod_or_broadcaster()` oder
   `chatter_user_id == broadcaster_user_id` oder leere `chatter_user_id`
   ergibt `Silent` (das Gate prüft das ebenfalls, der Kurzschluss spart den
   Register-Lookup).
3. Verdrahtung in `rust/bin/tb-bot/src/chat_wiring.rs`: dasselbe
   `ZuschauerRegister`-Objekt, das ab Zeile 759 in die PromoEngine geht, einmal
   bauen (`Arc`), an beide Stellen geben (PromoEngine und `LfgPitchResponder`
   ab Zeile 861). Die zweite `LfgPitchResponder::new`-Stelle bei Zeile 3143
   ebenso versorgen, falls sie einen echten Pfad bedient (prüfen, ob Test oder
   Prod).
4. Tests in `rust/crates/tb-chat/src/lfg_pitch.rs` (Modul `tests`) und
   `rust/crates/tb-chat/tests/lfg_pitch.rs`: ein Test "Broadcaster/Mod
   bekommt keinen LFG-Pitch", ein Test "Register-Reject bleibt still und ruft
   den Judge nicht auf", ein Test "Register-Pass sendet wie bisher". Bestehende
   Tests, die den Responder ohne Register bauen, müssen weiter grün sein: dort
   ein Test-Register setzen (Muster `promos.rs:4627` `test_register`) oder
   den Erwartungswert auf `RegisterFehlt` anpassen, je nachdem was der Test
   prüft.
5. Erkennung reparieren, damit "Drifter und noch wer" gar nicht erst als
   Mitspieler-Suche gilt (Journal 23:50: `verdict="yes" confidence=0.9`,
   der Judge sah nur diese vier Wörter ohne Chatverlauf):
   - `join_lfg_re()` (`lfg_pitch.rs:52`): `noch \s+ wer` allein ist kein
     Anschluss-Signal. `wer` aus der Gruppe `noch \s+ (platz|slot|einer|wer)`
     streichen; "noch wer" zählt nur noch über die Kombination Suche plus
     Objekt (`search_lfg_re` und `object_lfg_re`).
   - `LfgJudgeInput` bekommt `recent_chat: Vec<String>` (die letzten 8
     Nachrichten des Kanals vor der Nachricht, Format `login: text`). Quelle:
     neuer Port `RecentChatPort` in `lfg_pitch.rs`, den `PromoEngine` über
     `load_recent_channel_messages` (`promos.rs:3041`) implementiert, verdrahtet
     wie `PromoBlockCheck` (Option, `None` in Tests erlaubt).
   - Prompt (`lfg_pitch.rs:30` ff.): Chatverlauf als Kontext mitgeben und
     die Regel ergänzen: "no", wenn die Nachricht eine Antwort auf eine Frage
     im Verlauf ist, eine Aufzählung (Helden, Namen, Ergebnisse) oder ein
     Kommentar zum Stream ohne eigenen Wunsch mitzuspielen. "yes" nur, wenn
     die Person selbst und ausdrücklich Mitspieler sucht oder mitspielen will.
   - Tests: `classify_lfg("Drifter und noch wer")` ist `false`;
     `classify_lfg("noch wer bock auf ranked?")` bleibt `true`; ein Test, dass
     der Judge-Input den Chatverlauf enthält.
6. Formatieren nur die eigenen Dateien mit `rustfmt <datei>` (kein
   repoweiter Formatterlauf, der schreibt fremde Dateien um), dann
   `cargo clippy -p tb-chat -p tb-bot` und `cargo test -p tb-chat` (DB-Tests
   brauchen `TB_TEST_DATABASE_URL=postgres:///tb_bb_test?host=/var/run/postgresql`
   und `SQLX_OFFLINE=1`; Toolchain 1.97 aus `~/.rustup/toolchains`, nicht
   `/usr/bin/cargo`). Baseline vorher messen und im Bericht nennen.

## Fundstellen (aus dem Vorcheck)

- `rust/crates/tb-chat/src/lfg_pitch.rs:24`: Text der Einladung aus dem Screenshot.
- `rust/crates/tb-chat/src/lfg_pitch.rs:415`: `LfgPitchResponder` (Felder, kein Register).
- `rust/crates/tb-chat/src/lfg_pitch.rs:490`: `decide`, Gate-Kette ohne Ausschlüsse.
- `rust/crates/tb-chat/src/promos.rs:666`: Vorbild `set_zuschauer_register`.
- `rust/crates/tb-chat/src/promos.rs:897`: Vorbild Register-Gate im Anlass-Pitch (fail-closed).
- `rust/crates/tb-chat/src/zuschauer_register.rs:536`: `gate()` mit Gründen
  `broadcaster_mod_bot`, `register_fehlt`, `register_community`, `kein_neuling`, `partner_oder_streamer`.
- `rust/crates/tb-chat/src/types.rs:76`: `is_mod_or_broadcaster`.
- `rust/bin/tb-bot/src/chat_wiring.rs:759`: Register-Bau für die PromoEngine (nur wenn `member_relay` da ist).
- `rust/bin/tb-bot/src/chat_wiring.rs:861` und `:3143`: `LfgPitchResponder::new`.
- `rust/crates/tb-chat/src/pipeline.rs:1289`: Pipeline-Schritt `lfg_pitch`.
- `rust/crates/tb-chat/src/pipeline.rs:2508`, `:2788`, `:3570`: Test-Konstruktionen des Responders.

## Was nicht angefasst wird

- `ZuschauerRegister::gate` und seine Schwellen bleiben, wie sie sind.
- Anlass-Pitch, Partner-Pitch, periodische Einladung, `!invite` und
  `InviteQuestionResponder` (`commands.rs`) bleiben unverändert.
- Text der Einladung, Cooldowns und die Judge-Schwelle 0,7 bleiben.
- Keine Migration, keine neuen Tabellen, keine ENV-Schalter.

## Fertig-Kriterium

- `classify_lfg("Drifter und noch wer")` ist `false`, der Judge bekommt den
  Chatverlauf und antwortet auf Antworten und Aufzählungen mit "no".

- `LfgPitchResponder::decide` liefert für Broadcaster, Mods, Partner,
  Streamer, Betreiber und Zuschauer mit Discord-Zuordnung `Silent`, ohne den
  Judge aufzurufen; ohne gesetztes Register ebenfalls `Silent`.
- Prod-Verdrahtung gibt dem Responder dasselbe Register wie der PromoEngine.
- Neue Tests grün, bestehende tb-chat-Suite auf Baseline-Stand (vorbestehender
  roter Test "Invite-Antwort bei Doppelintent" ist Baseline).

## Deploy-Weg

Twitch-Release: nach Merge nach main Release aus dem eigenen Worktree bauen
(`rustc 1.97`), `deploy-twitch-release <sha>` (System-Units
`deadlock-twitch-bot-rust`, `deadlock-twitch-dashboard-rust`). Live-Beweis:
Betreiber-Konto schreibt in einem Partnerkanal eine Mitspieler-Suche, der Bot
bleibt still, Journal zeigt `lfg_pitch` mit Grund `broadcaster_mod_bot` oder
`partner_oder_streamer`. Den Merge macht der Intent-Thread, nicht der Worker.

## Rahmen

- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder
  Unter-Agenten spawnen.
- Keine Code-Kommentare schreiben, Code erklärt sich selbst.
- Nur den eigenen Branch pushen, nie main.
- Auftrag größer als beschrieben: Bump-up-Nachricht an den Intent-Thread, dann
  stoppen.
