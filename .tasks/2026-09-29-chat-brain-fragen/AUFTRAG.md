# Auftrag: chat-brain-fragen

status: aktiv (2026-09-29)

## Ziel

Nutzerwunsch: "Wenn wir in Twitch im Chat sind, pingen wir unseren Deadlock-Bot mit @Bot an, stellen eine Frage, und der Bot beantwortet sie im Chat über das Deadlock Brain. Man soll mit ihm auch so chatten können."

Am Ende: In Partnerkanälen, in denen der Bot sitzt, beantwortet er Nachrichten, die ihn per @-Mention ansprechen, mit einer kurzen Antwort aus dem Deadlock Brain, als Twitch-Reply an die fragende Person.

## Arbeitsschritte

1. Bestand prüfen (Graphify zuerst, Skill `code-suche`): wie tb-chat heute Nachrichten verarbeitet (`rust/crates/tb-chat/src/pipeline.rs`), ob es schon eine Mention-Erkennung für den Bot gibt (`mention_scoring.rs` bewertet fremde Mentions, nicht den Bot), wie das Smalltalk-Modul (`rust/bin/tb-bot/src/smalltalk_loop_wiring.rs`, `tb_engagement`) antwortet, und ob Twitch-Replies (reply-parent-msg-id bzw. Helix `reply_parent_message_id`) im Sendepfad schon gehen. Nichts doppelt bauen.
2. Brain-Anbindung wiederverwenden: `tb_knowledge::brain::BrainKnowledgeAdapter` (`rust/crates/tb-knowledge/src/brain.rs`, stateless, `answer()`/`answer_with_context()`, Ergebnis `KnowledgeReply::Answered{text,sources}` oder `NoEvidence`). Die Dashboard-Seite nutzt ihn schon über `BrainClientOptions` (`tb-config/src/dashboard_options.rs`, Modus Legacy/Shadow/Typed, Endpoint, public_scopes, timeout_ms) und `rust/bin/tb-dashboard/src/main.rs:427` (Token-Beschaffung). Für tb-bot dieselbe Konfigurationsform übernehmen bzw. teilen, Token aus Infisical wie beim Dashboard, keine ENV-Variablen.
3. Backend klären, bevor gebaut wird: Welcher Brain-Antwortdienst liefert heute unter dem konfigurierten Endpoint? Auf dem Host läuft aktuell kein Brain-HTTP-Antwortdienst (nur `deadlock-brain-postgresql` und `deadlock-brain-site` auf 8087). Der Brain-Cutover (G5) ist eine offene Betreiberentscheidung und läuft in anderen Threads ("Technische Fertigstellung vor G5"). Gibt es keinen live erreichbaren, auf main liegenden Antwortdienst, NICHT selbst einen Brain-Dienst deployen oder G5 umschalten, sondern Bump-up mit Befund (was fehlt, wo es liegt, welcher Branch).
4. Chat-Pfad bauen in tb-chat bzw. tb-bot-Wiring:
   - Auslöser: Nachricht enthält `@<Bot-Login>` (Groß/Klein egal, Login aus der Bot-Konfiguration, nicht hart codiert) und hat nach Entfernen der Mention echten Frageinhalt. Nachrichten des Bots selbst, reine Mentions ohne Inhalt und Nachrichten, die Moderation/Spam-Pipeline gelöscht oder bestraft hat, werden nicht beantwortet. Die Moderation läuft vorher.
   - Identität immer über Twitch-User-ID.
   - Antwort als Twitch-Reply auf die Nachricht, höchstens 450 Zeichen, an Satzgrenze kürzen, keine Links, keine Mentions außer der Reply, `@everyone`-artige Zeichenfolgen neutralisieren.
   - `NoEvidence` und Backend-Fehler: kurze ehrliche Antwort in mehreren rotierenden Varianten (etwa "Da bin ich mir nicht sicher, frag lieber im Discord nach"), nie erfundenes Spielwissen, nie Ich-Form über eigenes Spielen. Backend-Ausfall wird im Log einmal pro Zustandswechsel gemeldet, nicht pro Nachricht.
   - Grenzen als Konfigfelder mit Default (nicht als ENV, nicht als harte Konstante für das Timeout): je Twitch-User-ID eine Frage pro 60 s, je Kanal 20 pro Stunde, globaler Tagesdeckel 500. Über dem Limit wird still nicht geantwortet.
   - Schalter: globaler Config-Schalter `enabled` (Default an nach Deploy) plus Kanal-Schalter über den bestehenden Moderations-/Bot-Einstellungsweg, falls das ohne neuen Speicherweg geht; sonst nur global und im Report vermerken.
   - Sichtbarkeit für den Betreiber: jede beantwortete Frage (Kanal, Twitch-User-ID, Frage, Antwort, Status Answered/NoEvidence/Fehler, Dauer) in eine Log-Tabelle schreiben (neue Migration unter `rust/migrations/`, Rechte an `twitchbot`/`twitchdash` wie in `Docs/workspace/twitch-bot.md` beschrieben). Keine Discord-Review-Karte nötig.
5. Tests nach dem Stil der vorhandenen Suites für: Mention-Erkennung (mit/ohne Inhalt, fremde Mention, eigene Nachricht), Kürzung/Neutralisierung, Limits, NoEvidence-Varianten. `cargo fmt`, `cargo clippy`, `cargo test` für die berührten Crates; vorbestehende rote Tests als Baseline messen und nennen.
6. Die Akte `.tasks/2026-09-29-chat-brain-fragen/` mit auf den Branch committen.

## Fundstellen (aus dem Vorcheck)

- `rust/crates/tb-knowledge/src/brain.rs:30` `BrainKnowledgeAdapter::new(endpoint, token, timeout, public_scopes)`, `:101` `answer`, `:111` `answer_with_context`.
- `rust/crates/tb-knowledge/Cargo.toml:7` `brain-client` als Git-Abhängigkeit auf Deadlock-Brain Rev `3b86d3cb`.
- `rust/crates/tb-dashboard-api/src/handlers/self_explainer.rs:101` `from_config` als Vorlage für Modus und Adapteraufbau.
- `rust/crates/tb-config/src/dashboard_options.rs:34` Validierung der Brain-Client-Optionen.
- `rust/bin/tb-dashboard/src/main.rs:427` Token-Beschaffung.
- `rust/crates/tb-chat/src/pipeline.rs` Chat-Pipeline, `rust/bin/tb-bot/src/chat_wiring.rs` Wiring.

## Was nicht angefasst wird

- Deadlock-Brain-Repo, Brain-Datenbank, G5-Cutover und die laufenden Brain-Threads.
- Anlass-Pitch, Partner-Pitch, periodische Einladung, Spam-/Scam-Pfad (nur vorher laufen lassen, nicht ändern).
- Dashboard-Self-Explainer-Verhalten.
- Keine neuen LLM-Modelle oder Anbieter; falls eine Umformulierung per LLM nötig wäre, nur über `tb_llm::endpoint_for(<use_case>)` mit `denken_aus`, sonst Brain-Text direkt nutzen.

## Fertig-Kriterium

In einem Partnerkanal (Test mit Wegwerf- oder Nutzerkonto im Kanal earlysalty) beantwortet der Bot "@<Bot> wie viele Fähigkeiten hat Warden?" innerhalb weniger Sekunden als Reply mit einer Brain-Antwort; eine Frage ohne Beleg bekommt die ehrliche Kurzantwort; die Log-Tabelle zeigt beide Zeilen; zweite Frage innerhalb 60 s bleibt unbeantwortet.

## Deploy-Weg

Merge-Gate, Merge nach main, Prod-Migration von Hand als `postgres` samt Rechten und `_sqlx_migrations`-Eintrag, Release-Build im eigenen Worktree, `deploy-twitch-release <sha>` (Skill `deploy-restart-selbstdienst`), Live-Beweis wie oben. Merge und Deploy macht die Hauptsession nach Review, nicht der Worker.

## Nachtrag 2026-09-29 nach Bump-up

Befund: kein Brain-Antwortdienst auf main live, der typed Answer-Kernel liegt in Deadlock-Brain `migration/s08-answer-kernel-20260924` bzw. `integration/pre-g5-*`; G5 bleibt Betreiberentscheidung. `dl-knowledge` (8896) läuft retrieval-only und ist ein FAQ-Dienst, kein Ersatz.

Entscheidung: Die Twitch-Seite wird jetzt vollständig gegen den bestehenden typed Vertrag (`BrainKnowledgeAdapter`, `brain-client`) gebaut, kein Übergangs-Backend, kein zweiter Wissenspfad. Schritt 3 entfällt als Stopp-Grund. Die Chat-Brain-Konfiguration bekommt denselben Modus wie das Dashboard (Legacy = aus, Typed = an) mit Default Legacy; ist der Adapter nicht erreichbar, bleibt der Pfad stumm und meldet das einmal im Log. Tests laufen gegen einen Fake-Adapter bzw. lokalen Test-Server. Fertig-Kriterium für diesen Worker: Code, Migration und Tests auf dem Branch; der Live-Beweis folgt nach G5 durch die Hauptsession.

## Rahmen

- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.
- Keine Code-Kommentare schreiben, Code erklärt sich selbst.
- Nur den eigenen Branch pushen, nie main.
- Auftrag größer als beschrieben: Bump-up-Nachricht an den Intent-Thread, dann stoppen.
