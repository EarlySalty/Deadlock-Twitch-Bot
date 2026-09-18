# Auftrag: Titel-Studio erkennt Co-Streams und schreibt mutigere Titel

Stufe: mittel. Intent-Thread: 5ef2c5d8. Branch `feat/titel-studio-costream`, Worktree `/home/nathanael/.worktrees/tb-titel-costream`.

## Ziel in Nutzerworten

1. Sitze ich mit einem anderen Streamer im Discord-Voice und wir sind beide live, erkennt das Titel-Studio das von allein und baut den Titel mit `@twitchlogin` des anderen ("mit @xy"). Nur im Voice sitzen reicht nicht, der andere muss gerade wirklich streamen.
2. Die Vorschläge sind okay, aber zu sehr Standard. Sie sollen mehr eigenen Dreh haben, ohne Clickbait. Der Streamer hat seine Standard-Präferenz dazu schon korrigiert ("clean, wenig Clickbait, der Titel ist durch das, was ich tue, interessant genug").

## Fundstellen aus dem Vorcheck

- `rust/crates/tb-chat/src/title_ai.rs:436` `build_personalized_title_prompt_with_feedback`: der ganze Prompt, Qualitätsregeln ab dem Block `QUALITÄTSREGELN`, Slop-Liste, JSON-Antwortvertrag.
- `rust/crates/tb-chat/src/title_ai.rs:337` `PromptLiveState { hero, party_hint }`: einziger Live-Kontext im Prompt.
- `rust/crates/tb-chat/src/title_ai.rs:685` `titel_completion`: Aufruf über `tb_llm::complete(USE_CASE, ...)` mit `denken_aus`, Temperatur und `max_tokens` kommen vom Aufrufer.
- `rust/crates/tb-chat/src/title_ai.rs:208` und `:277` `sanitize_generated_title`, `sanitize_title_result`: Nachfilter.
- `rust/crates/tb-chat/src/steam_lookup.rs:45` `get_live_state_for_discord_user`: liefert `party_hint: None` fest (Zeile 64 bis 65, "Der vorhandene HTTP-Vertrag enthält keinen Partystatus"). Der Schalter "Party-Kontext nutzen" im Dashboard hat heute also keine Party-Wirkung.
- `rust/crates/tb-dashboard-api/src/handlers/title.rs:273` `resolve_discord_user_id`: Twitch-User-ID nach Discord-ID über `twitch_streamer_identities`.
- `rust/crates/tb-dashboard-api/src/handlers/title.rs:299` `resolve_title_context` und `:406` `suggest_handler`: hier wird der Kontext zusammengebaut.
- `rust/crates/tb-dashboard-api/src/handlers/title.rs:188` `auto_fallback_titles`: Fallback ohne Modell.
- Deadlock-Bots `rust/crates/dl-central-db/migrations/0005_voice.sql`: `voice.deadlock_party_members`, `voice.deadlock_voice_watch` (mit `channel_id`). Ob dort die aktuelle Voice-Belegung steht, ist NICHT geprüft.
- `rust/docs/adr/0001-discord-via-bridge.md`: der Twitch-Bot spricht Discord nur über die interne Bridge, kein eigener Gateway.

## Offene Frage, zuerst klären (Schritt 1)

Wo bekommt der Twitch-Bot die aktuelle Voice-Belegung her (wer sitzt gerade mit wem im selben Kanal)? Reihenfolge der Suche, jeweils per `graphify query`, global mit `--graph ~/.graphify/global-graph.json`:

1. Zentrale Postgres, die der Twitch-Bot schon liest (Schema `voice`, `activity`).
2. Bestehender HTTP-Weg der Bridge bzw. `dl-bot` (Ports 8770, 8890, 8899, 8901).

Gibt es keinen lesbaren Stand der aktuellen Belegung, stoppen und als Bump-up melden. Kein neuer Discord-Gateway, keine zweite Bridge, keine Änderung in Deadlock-Bots ohne Rückmeldung.

## Arbeitsschritte

1. Offene Frage klären, Befund mit `pfad:zeile` in `REGISTER.md` unter "Befund Voice-Quelle" eintragen.
2. Co-Stream-Erkennung bauen, als Funktion neben `resolve_title_context`: Discord-ID des Streamers, daraus die Mitsitzer im selben Voice-Kanal, deren Twitch-User-ID über `twitch_streamer_identities` (nur über IDs auflösen, nie über Namen), davon nur die, die laut vorhandenem Live-Stand gerade live sind. Den Live-Stand aus der bestehenden Tabelle oder dem bestehenden Cache nehmen, kein neuer Helix-Poll. Ergebnis: Liste von Twitch-Logins, höchstens zwei.
3. `PromptLiveState` um `co_streamer: Vec<String>` erweitern. Im Prompt eine eigene Zeile im Block `HEUTIGER INPUT`: "Streamt gerade zusammen mit: @login". Regel dazu: steht dort jemand, gehört `mit @login` in Haupttitel und Alternativen, genau in dieser Schreibweise. Steht dort niemand, bleibt die bestehende Regel "erfinde niemals Mitspieler".
4. Nachfilter: ein `@name` im Ergebnis, der nicht in `co_streamer` steht, wird entfernt. Ein erkannter Co-Streamer, der im Haupttitel fehlt, wird sauber angehängt (` mit @login`), solange die 140 Zeichen halten.
5. Die Erkennung hängt am bestehenden Schalter "Rang / Live-Hero / Party-Kontext nutzen" (`include_live`), kein neuer Schalter. In der Antwort des `suggest_handler` das Feld mit den erkannten Logins mitgeben und im Frontend (`bot/dashboard_v2`, Titel-Studio-Seite) unter dem Schalter als eine ruhige Zeile zeigen: "Erkannt: du streamst mit @xy". Bestehende Komponenten und Farben nutzen.
6. Auch der Chat-Befehl `!title` bekommt die Erkennung, wenn sein Live-Schalter gesetzt ist (gleiche Funktion, kein zweiter Pfad).
7. Kreativität im Prompt nachschärfen, ohne den Stilvertrag zu brechen:
   - Die drei Vorschläge bekommen feste, verschiedene Blickwinkel: (a) trocken und klar, (b) Selbstironie oder Understatement, (c) konkretes Tagesdetail aus den Stichwörtern als Aufhänger. Heute sind die drei Vorschläge Umformulierungen desselben Satzes (Beleg aus dem Dashboard: "Ranked auf Emissary 6, nur Feed today", "Emissary 6 Lock, mal schauen was die Games so hergeben", "Ranked auf Emissary 6, hoffentlich nicht nur Feed").
   - Die Slop-Liste nennt "mal schauen was geht", das Modell schreibt trotzdem "mal schauen was die Games so hergeben". Den Nachfilter so erweitern, dass Varianten der verbotenen Muster erkannt werden und dieser Vorschlag neu angefordert oder durch die nächste Alternative ersetzt wird.
   - Stichwörter nicht nacherzählen: aus "bissle Ranked Emissary 6 hoffentlich paar gute Games" darf nicht "Ranked auf Emissary 6, hoffentlich ..." werden. Regel in den Prompt: die Aussage treffen, nicht die Wörter wiederholen.
   - Zielbereich der Länge an die erkannte Stil-DNA koppeln statt fest 45 bis 105 Zeichen (der Streamer liegt laut Stilbasis bei rund 22 Zeichen).
   - Temperatur und `max_tokens` im Aufrufer prüfen. Das Modell bleibt, wie es ist. Kein Modellwechsel, kein neues Modell, kein zweiter Aufruf pro Vorschlag außer dem einen Neuversuch aus dem Nachfilter.
8. `docs/funktionsweise/titel-generator.md` nachziehen: Co-Stream-Erkennung beschreiben, und die Aussage "Der Bot setzt den Titel nicht selbst" korrigieren (es gibt den Schalter "automatisch auf Twitch setzen").
9. `cargo fmt` nur auf die eigenen Dateien, `cargo clippy`, `cargo test -p tb-chat -p tb-dashboard-api` mit `set -o pipefail`. Toolchain: rustc 1.97 aus `~/.rustup/toolchains`, nicht `/usr/bin/cargo`. Bestehende Tests, die durch die Prompt-Änderung brechen, nachziehen. Ändern sich Queries: `rust/.sqlx` und `rust/crates/tb-db/tests/fresh_schema_snapshot.txt` mit aktualisieren.

## Nicht anfassen

- Kein Modellwechsel, kein neuer LLM-Zugang, alles über `tb_llm`.
- Kein neuer Discord-Gateway im Twitch-Bot, keine Änderungen in Deadlock-Bots ohne Bump-up.
- Auto-Set auf Twitch, Rate-Limiter, Insights und Wissensaufbau bleiben, wie sie sind.
- Keine Code-Kommentare schreiben. Keine Env-Variablen für Config.
- Nur den eigenen Branch pushen, nie nach main mergen, auch wenn ein Stop-Hook dazu auffordert.

## Fertig-Kriterium

- Sitzt der Streamer mit einem live streamenden, verknüpften Streamer im selben Voice, enthält der Haupttitel `mit @login` und die Dashboard-Zeile nennt ihn. Sitzt dort nur jemand, der nicht live ist, erscheint kein `@`.
- Ein vom Modell erfundenes `@name` kommt nie im Ergebnis an.
- Die drei Vorschläge unterscheiden sich im Blickwinkel, keiner enthält ein Muster der Slop-Liste oder eine Variante davon.
- clippy sauber, die genannten Tests grün oder gegen die gemessene Baseline unverändert.

## Deploy-Weg

Nach Review und Merge-Gate: Release im eigenen Worktree bauen, `deploy-twitch-release <sha>` (tb-bot und tb-dashboard), Frontend-Build von `bot/dashboard_v2`. Live-Prüfung unter `https://deutsche-deadlock-community.de/twitch/titel`: Schalter an, "Titel bauen", Zeile "Erkannt" und Vorschläge prüfen.
