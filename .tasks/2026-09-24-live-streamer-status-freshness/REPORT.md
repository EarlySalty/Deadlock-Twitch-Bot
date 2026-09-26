# Frischer Live-Nachweis für temporäre Voice-Rechte

## Auftrag und Vertrag
Der Discord-Bot soll Mitgliedern mit Streamer-Rolle während eines beliebigen Twitch-Livestreams Owner-ähnliche Panel-Rechte im aktuellen Voice-Call erteilen, ohne Kick-/Ban-Recht gegen den echten Owner. Der zugehörige Discord-Branch ist `EarlySalty/Deadlock-Bots:feat/live-streamer-voice-rights-20260924`.

Der vorhandene authentifizierte, rein lesende Diagnose-Endpunkt wird um `last_seen_at: Option<String>` erweitert. Für den Live-Nachweis löst er die Discord-ID über `twitch_streamer_identities` zur Twitch-User-ID auf und liest `twitch_live_state` nur über diese ID. Ein recycelter Login darf keine fremde Beobachtung liefern. Auch ein verknüpfter Nichtpartner erhält `found=true` und seinen eigenen Live-Status; Partner-/OAuth-Diagnosedaten werden nur ergänzt, wenn der Partner-State dieselbe Twitch-ID trägt. Der Zeitstempel bleibt der gespeicherte Originalwert, keine neu erfundene Abrufzeit. Ohne Beobachtung oder ohne verknüpften Streamer bleibt das Feld null. `is_live` ist spielunabhängig; es wird nicht an den Partnerstatus oder eine Deadlock-Kategorie gebunden. Keine neue Authentifizierung, keine neuen Geheimnisse, keine Migration und kein zusätzlicher Poller.

Der Discord-Verbraucher fordert einen frischen positiven Live-Nachweis, gültige Streamer-Rolle, gesunden Voice-Cache und Anwesenheit im verwalteten Call. Fehlt das neue Feld auf einem alten Server, gibt es keine zusätzliche Berechtigung. Für einen später ausdrücklich freigegebenen Rollout zuerst diesen Twitch-Endpunkt und danach die Discord-Änderung ausliefern.

**Offene Intent-Lücke:** Der vorhandene Poller beobachtet aktive Partner und `twitch_streamers`-Einträge. Eine Discord-Streamer-Rolle oder eine verknüpfte Twitch-Identität allein trägt den Kanal bisher nicht in diese Tracking-Menge ein. Der ID-sichere Diagnosepfad kann daher für einen nicht getrackten Rolleninhaber keinen frischen Live-Nachweis liefern. Vor Merge muss eine gezielte Aufnahme dieser Kanäle in den bestehenden Poller samt Prüfung seiner Nebenwirkungen und Last geklärt werden; das Versprechen „jeder Streamer-Rolleninhaber“ ist mit dieser Änderung allein nicht erfüllt.

## Prüfnachweise
Basis: `947e1344b2b4249b2b8ac48f521863cd0abe1d34`.

`cargo test -p tb-internal-api handlers::diagnose::tests --lib`: nach der Nachbesserung **6 bestanden, 0 Fehler, 0 ausgeschlossen**. Der zusätzliche Test prüft den Live-Nachweis für verknüpfte Nichtpartner. Der neue DB-Test in `tb-analytics::admin_streamers` kompiliert, wurde lokal ohne isolierte Test-DB aber übersprungen; ein grüner Datenbank-Nachweis wird deshalb nicht behauptet.

Zwei vorhandene Clippy-Befunde in `partner_signup_tag_block.rs` wurden ohne Verhaltensänderung behoben (`contains` und direkte Result-Rückgabe). Die **15 zuvor zugehörigen Tests bestanden vollständig** auf einem eigenen Timescale-Wegwerfcontainer mit `TB_TEST_REQUIRE_DB=1`. Der Container wurde anschließend entfernt. Diese frühere Prüfung deckt die neue ID-Abfrage noch nicht ab; deren DB-Test muss vor Merge mit einer isolierten Test-DB tatsächlich laufen. Kein vollständiger Workspace-Testlauf behauptet.

`rustfmt --edition 2024 --check crates/tb-internal-api/src/handlers/diagnose.rs` und `git diff --check`: erfolgreich.

## Bekannte Lint-Grenze
Die zusätzlich versuchte umfassende strenge Prüfung `cargo clippy -p tb-internal-api --all-targets -- -D warnings` ist **nicht vollständig grün**. Nach Behebung der beiden Analytics-Befunde meldet sie bestehende, hier unveränderte Befunde in `tb-chat/src/scam_pitch.rs:1443` (unnötige Referenz) und `tb-chat/src/title_ai.rs:436` (acht Argumente). Eine gesonderte Prüfung der betroffenen Packages mit `--no-deps` erreicht zusätzlich vorhandene Befunde in `tb-internal-api/src/handlers/streamers.rs:1241` (acht Handler-Argumente) und `session_detail.rs:702` (ungenutzte Testvariable). Diese vier Dateien sind gegenüber der genannten Basis unverändert. Keine Allow-Attribute, Skip-Flags oder Gate-Abschwächung wurden eingebaut. Compiler und die aufgeführten ausgeführten Tests sind erfolgreich; daraus folgt kein grüner gesamter Lint-/CI-Status.

Lokale Protokolle: `/tmp/tb-live-streamer-final-tests-20260924.log`, `/tmp/tb-live-streamer-clippy-final-20260924.log`, `/tmp/tb-live-streamer-clippy-scoped-20260924.log`.

## Auslieferungsgrenze
Offener Draft-PR entsprechend PR-first-Testbetrieb. Kein Merge, kein Auto-Merge, kein Deploy, kein Produktionsdatenbankzugriff und kein Neustart. Der Draft verhindert die vorhandene automatische Release-Freigabe, ohne ihre Checks zu verändern. PR-Nummer, veröffentlichter Head und tatsächliche Actions-Ergebnisse werden im PR nachgetragen.
