# Plan: Lurker-Steuer mit Kanalpunkte-Belohnung

Klasse mittel. Nur Bauanleitung, keine Implementierung. Alle Pfade relativ zu `~/repos/Deadlock-Twitch-Bot`.

## Bauordnung (Kurzfassung)

1. REQ-1 Titel-Erkennung als reine Funktion (Basis fuer REQ-3, REQ-4, REQ-5, REQ-6)
2. REQ-2 Erinnerungstext neu bauen
3. REQ-4 Kandidaten-SQL: bereits Eingeloeste dieser Session ausschliessen
4. REQ-5 Helix custom_rewards Abfrage plus Laufzeit-Gate vor dem Senden
5. REQ-3 Redemption-Hook aus dispatch bis in den Chat-Sendepfad, Dankes-Antwort
6. REQ-7 Bot-Scope-Kapabilitaet persistieren, Dashboard-Pruefung angleichen
7. REQ-6 Dashboard-Karte mit Vorlage und Belohnungs-Status
8. REQ-8 Textpruefung am Ende (durchgaengig, kein eigener Schritt)

Tests je REQ zuerst rot (siehe Testplan), dann bauen.

---

## REQ-1 Standardname und Titel-Erkennung

Neue freie Funktion in `rust/crates/tb-chat/src/promos.rs`:

- `pub fn lurker_tax_title_matches(title: &str) -> bool`
  - Normalisierung: trimmen, `to_lowercase`, Mehrfach-Whitespace auf ein Leerzeichen zusammenziehen.
  - Praefixvergleich: normalisierter Titel beginnt mit `"lurker steuer"`. Damit greifen `Lurker Steuer`, `lurker steuern`, `Lurker  Steuer` und Varianten mit Zusatz.
- Konstante `const LURKER_TAX_REWARD_TITLE: &str = "Lurker Steuer";` fuer Vorlage und Dashboard.
- Keine Kostenlogik, kein Kosten-Text.

Diese Funktion ist die einzige Wahrheitsquelle fuer die Titel-Erkennung und wird von REQ-3, REQ-4, REQ-5 und (als Spiegel) vom Dashboard genutzt.

## REQ-2 Erinnerungstext

`rust/crates/tb-chat/src/promos.rs`, `build_lurker_tax_text` (heute :1946) neu schreiben:

- Ein Name: `Hey @xy, schoen dass du da bist! Vergiss nicht, deine Lurker Steuer zu zahlen: Belohnung 'Lurker Steuer' einloesen.`
- Zwei Namen: beide Erwaehnungen in einem Satz, z. B. `Hey @a und @b, schoen dass ihr da seid! Vergesst nicht, eure Lurker Steuer zu zahlen: Belohnung 'Lurker Steuer' einloesen.`
- Echte Umlaute, keine Gedankenstriche, keine Kostenangabe.
- `LURKER_TAX_MAX_MENTIONS`, 60-Minuten-Promo-Slot, ein Viewer je Session unveraendert.

## REQ-4 Bereits Eingeloeste dieser Session nicht mehr erinnern

`rust/crates/tb-chat/src/promos.rs`, `get_lurker_tax_candidates` (heute :1852):

- Im finalen SELECT einen Ausschluss ergaenzen: Logins, die in `twitch_channel_points_events` mit `session_id = <aktive Session>` und `lurker_tax_title_matches(reward_title)` stehen, fallen raus.
- Da `lurker_tax_title_matches` Rust ist, im SQL als `LOWER(reward_title) LIKE 'lurker steuer%'` abbilden (identische Normalisierung: Titel sind bereits `str_lower`-nah gespeichert; Whitespace-Kollaps ist bei realen Twitch-Titeln praktisch nie noetig, im Zweifel `regexp_replace` fuer Mehrfach-Whitespace).
- Aktive Session-ID kommt wie beim Versand aus `twitch_live_state.active_session_id` (bereits im Sende-Pfad geladen). Sauberste Loesung: die Session-ID vor dem Kandidatenaufruf ermitteln und als Parameter durchreichen, damit Kandidaten- und Dedupe-Sicht dieselbe Session sehen.
- `NOT IN`/`NOT EXISTS` Subquery statt Nachfilter im Rust.
- Aenderung an `rust/.sqlx/` und `rust/crates/tb-db/tests/fresh_schema_snapshot.txt` faellt an (neue Query).

## REQ-5 Belohnung existiert und ist aktiv (Helix)

### Transport (kleinste Ergaenzung)

Es gibt heute keinen custom_rewards-Aufruf in `tb-transport-twitch` (geprueft: `streams.rs`, `chat.rs`, `client.rs` haben keinen `channel_points`-Pfad; `get_with_user_token` existiert seit `client.rs:296`).

Neue Datei `rust/crates/tb-transport-twitch/src/channel_points.rs` oder Methode in `streams.rs`:

- `pub async fn get_custom_rewards(&self, broadcaster_id: &str, user_token: &str) -> Result<Vec<HelixCustomReward>, HelixError>`
  - `self.get_with_user_token("/channel_points/custom_rewards", user_token).query(&[("broadcaster_id", broadcaster_id)]).send()`, dann `check_status_and_json`.
  - Response-Struct `HelixCustomReward { id: String, title: String, is_enabled: bool }`, Wrapper `{ data: Vec<..> }`.
- Modul in `rust/crates/tb-transport-twitch/src/lib.rs` exportieren.
- Nutzt den Streamer-User-Token; Scope `channel:read:redemptions` liegt im Basisprofil (`tb-raid/src/scope_profiles.rs:39,70`). Kein neuer Scope, kein neuer OAuth-Weg (INV-1).

### Laufzeit-Gate in promos

`rust/crates/tb-chat/src/promos.rs`:

- Neuer optionaler Port nach dem Muster von `BotScopeProvider`/`set_bot_scope_provider`:
  - `pub trait LurkerRewardChecker: Send + Sync { async fn active_lurker_reward_exists(&self, broadcaster_id: &str) -> bool; }`
  - Feld `reward_checker: Option<Arc<dyn LurkerRewardChecker>>`, Setter `set_lurker_reward_checker`.
- In `maybe_send_lurker_tax_reminder` (heute :1679) vor dem Textbau und Versand pruefen: ist ein Checker verdrahtet und liefert `false`, kein Chat-Text (return). Ist keiner verdrahtet, bleibt das Verhalten wie bisher (gleiche Konvention wie die uebrigen optionalen Ports); produktiv wird er immer verdrahtet.
- Broadcaster-ID kommt aus dem bereits geladenen `plan_user_id`/`user_id`.

### Wiring

`rust/bin/tb-bot/src/chat_wiring.rs`:

- Adapter, der `LurkerRewardChecker` implementiert: haelt `HelixClient` und einen Streamer-Token-Zugriff (`tb_raid::TokenProvider`, wie in `chatters_wiring.rs` fuer `moderator:read:chatters`; hier `channel:read:redemptions`), fragt `get_custom_rewards` und prueft `is_enabled && lurker_tax_title_matches(title)`.
- Kurzer In-Memory-Cache (z. B. 60 s je Broadcaster) gegen einen Helix-Call pro Promo-Tick.
- Per `.set_lurker_reward_checker(...)` an die `PromoEngine` haengen, dort wo schon `.set_bot_scope_provider(...)` (chat_wiring.rs:734) steht.

## REQ-3 Redemption bis in den Chat-Sendepfad

Bestehender Weg (kein neuer Bus): Der `EventSubDispatcher` haelt `hooks: Arc<dyn EventSubHooks>` und ruft in `store_telemetry` je Sub-Typ Trait-Methoden auf (z. B. `on_chat_subscription_notification`). `EventSubHooks` hat Default-Methoden. Die aeussere Implementierung ist `ChatHooks` in `rust/bin/tb-bot/src/chat_wiring.rs:1271/1525`; sie delegiert an `inner` und feuert eigene Reaktionen (`maybe_send_golive_tip`, Werbefrei-Pitch) und haelt `promos: Arc<PromoEngine>` und `api: Arc<dyn ChatApi>`. Genau dieser Pfad wird genutzt, analog zur Raid-Begruessung, die ueber `ChatApi` in den Chat schreibt.

Schritte:

1. `rust/crates/tb-monitoring/src/dispatch.rs`: neue Default-Methode im Trait `EventSubHooks`:
   `async fn on_channel_points_redemption(&self, _broadcaster_id: &str, _broadcaster_login: &str, _event: &Value) {}`
   Ausserdem die dekorierende Weiterleitung im `LoggingEventSubHooks`-Wrapper (dispatch.rs:267ff) ergaenzen.
2. `rust/crates/tb-monitoring/src/dispatch.rs`, `store_telemetry` Arm `channel.channel_points_custom_reward_redemption.add`: nach dem Telemetrie-Insert `self.hooks.on_channel_points_redemption(&context.broadcaster_id, &context.broadcaster_login, event).await` aufrufen. Nur der custom-Arm, nicht der automatic-Arm.
3. `rust/bin/tb-bot/src/chat_wiring.rs`, `impl EventSubHooks for ChatHooks`: `on_channel_points_redemption` implementieren.
   - `inner` weiterreichen.
   - Reward-Titel aus `event.reward.title` (Fallback `event.reward_title`) lesen; `lurker_tax_title_matches` pruefen, sonst return.
   - Redeemer-Login aus `event.user_login`/`event.user_name`.
   - `self.promos.thank_lurker_tax_redeemer(broadcaster_id, broadcaster_login, redeemer_login).await` aufrufen.
4. `rust/crates/tb-chat/src/promos.rs`: neue Methode `thank_lurker_tax_redeemer`.
   - Aktive Session-ID aus `twitch_live_state` laden.
   - Per-Session-Dedupe: neues Feld in `ChannelState`, z. B. `thanked_redeemers: (i64, HashSet<String>)`, bei Session-Wechsel leeren (gleiche Mechanik wie `lurker_mentions`). Ist der Login schon drin, return (INV-4, hoechstens ein Dank je Zuschauer und Session).
   - Text ohne Kosten, echte Umlaute, keine Gedankenstriche, z. B. `@xy hat die Lurker Steuer bezahlt. Vorbildlich, danke!`
   - Versand ueber `self.api.send_message(channel, text)` (normale Chat-Antwort); bei Erfolg Login ins Dedupe-Set aufnehmen.
   - Kein LLM (INV-2), kein Promo-Slot-Verbrauch (Dank ist keine Promo).

## REQ-7 Warnhinweis korrigieren

Befund: Die Laufzeit akzeptiert den Bot-Token-Scope bereits (`promos.rs has_chatters_scope`, :1827). Das Dashboard prueft nur `twitch_raid_auth` des Streamers (`lurker_tax_settings.rs:91-121`), und kein Streamer-OAuth-Profil fragt `moderator:read:chatters` ab (`scope_profiles.rs`), daher ist Neu-Verbinden wirkungslos. Der Scope ist eine bot-globale Betreiber-Kapabilitaet, nicht Sache des Streamers.

Da Dashboard (`tb-dashboard`) und Bot (`tb-bot`) getrennte Prozesse sind und die Bot-Scopes nur zur Laufzeit aus Twitch `/oauth2/validate` kommen (keine DB-Ablage), wird die Kapabilitaet persistiert:

1. Migration in `rust/migrations/`: kleine Statustabelle, ein Zeilenwert, z. B.
   `twitch_bot_capabilities(id smallint primary key default 1, has_chatters_scope boolean not null, updated_at timestamptz not null default now())`.
   Rechte an `twitchbot` (write) und `twitchdash` (read) wie in der Twitch-Infra ueblich; Version in `_sqlx_migrations` von Hand als `postgres` nachziehen (siehe CLAUDE.md Twitch-Infra).
2. Schreiber: der Chatters-Collect-Loop kennt die Bot-Scope-Lage bereits (`BotChatterAuth::has_chatters_scope`, `chatters_wiring.rs:68-72`). In `rust/bin/tb-bot/src/chatters_wiring.rs` je Tick (oder gedrosselt) den Wert in die Statuszeile upserten.
3. Dashboard: `rust/crates/tb-dashboard-api/src/handlers/lurker_tax_settings.rs`, `has_moderator_read_chatters` erweitern auf die Laufzeitbedingung: `Streamer-Scope ODER Bot-Kapabilitaet` (Statuszeile lesen). Feldname im JSON bleibt `has_moderator_read_chatters`.
4. Frontend: `bot/dashboard_v2/src/components/verwaltung/LurkerTaxSection.tsx`, Warntext ersetzen. Bei echtem Fehlen (weder Streamer-Scope noch Bot-Kapabilitaet) Betreiber-Hinweis in Nutzersprache statt Neu-Verbinden, z. B. `Der Community-Bot braucht noch die Leseberechtigung fuer die Zuschauerliste. Wir kuemmern uns darum, du musst nichts tun.` Kein internes Vokabular, keine Gedankenstriche.

## REQ-6 Dashboard-Karte mit Vorlage und Belohnungs-Status

1. Backend `rust/crates/tb-dashboard-api/src/handlers/lurker_tax_settings.rs`, `get_handler`:
   - Zusaetzlich pruefen, ob die Belohnung existiert und aktiv ist: Streamer-Token (aus dem bestehenden Token-Pfad, `channel:read:redemptions`) plus `HelixClient::get_custom_rewards`, dann `is_enabled && lurker_tax_title_matches(title)`.
   - Setzt voraus, dass die dashboard-api Zugriff auf einen `HelixClient` hat. Falls nicht im State, `HelixClient` in `rust/crates/tb-dashboard-api/src/lib.rs` in den State aufnehmen (Wiring-Erweiterung, innerhalb des erlaubten Bereichs).
   - JSON um `reward_present: bool` erweitern.
2. Frontend `bot/dashboard_v2/src/components/verwaltung/LurkerTaxSection.tsx` und `bot/dashboard_v2/src/api/lurkerTax.ts`:
   - Vorlage anzeigen: Name `Lurker Steuer`, Vorschlag `10 Punkte`, ein Beschreibungstext, Kurzanleitung wo man die Belohnung in Twitch anlegt (Kanalpunkte-Verwaltung, neue Belohnung, exakt diesen Namen).
   - Status-Badge `Belohnung gefunden` (gruen) oder `Belohnung fehlt` (neutral/warnend) aus `reward_present`.
   - `LurkerTaxSettingsResponse` um `reward_present` erweitern.

## REQ-8 Texte

Alle sichtbaren Strings (Erinnerung, Dank, Warnhinweis, Vorlage, Anleitung) in Nutzersprache, echte Umlaute, keine Gedankenstriche, kein internes Vokabular. Doku `docs/LURKER_TAX.md` auf den neuen Reminder-Text und die Belohnungs-Vorlage anpassen (bisher bewusst ohne Belohnungsname).

---

## Testplan (zuerst rot)

Vor dem Bau je einen fehlschlagenden Test schreiben und den roten Lauf mit Name und Meldung festhalten. Rust-Tests der betroffenen Crates gegen den Docker-Test-Container bzw. `TB_TEST_DATABASE_URL` (siehe CLAUDE.md), `--` mit `SQLX_OFFLINE=1`.

1. Textbau (`tb-chat`, ohne DB): `build_lurker_tax_text` bei einem und bei zwei Namen liefert exakt den REQ-2-Wortlaut mit Belohnungsname, echten Umlauten, ohne Gedankenstriche.
2. Titel-Erkennung (`tb-chat`): `lurker_tax_title_matches` ist wahr fuer `Lurker Steuer`, `lurker steuern`, `LURKER STEUER 10`, `Lurker  Steuer`; falsch fuer `Steuer`, `Lurk`, leerer String.
3. Dank nur einmal je Session (`tb-chat` mit Fake-`ChatApi`): zweimaliges `thank_lurker_tax_redeemer` fuer denselben Login und dieselbe Session sendet genau einmal; nach simuliertem Session-Wechsel wieder einmal.
4. Keine Erinnerung nach Einloesung (`tb-chat`/DB): Kandidat, der in `twitch_channel_points_events` der aktiven Session mit passendem Reward-Titel steht, taucht in `get_lurker_tax_candidates` nicht auf; ohne solchen Eintrag schon.
5. Reward-Gate (`tb-chat` mit Fake-`LurkerRewardChecker`): Checker `false` -> `maybe_send_lurker_tax_reminder` sendet nicht; Checker `true` -> Textbau und Versand laufen.
6. Redemption-Hook (`tb-monitoring`): Dispatch von `channel.channel_points_custom_reward_redemption.add` ruft `on_channel_points_redemption` mit korrektem Broadcaster und Event auf (Fake-Hooks zaehlt); automatic-Reward loest den Hook nicht aus.
7. Dashboard-Readiness gegen Bot-Scope (`tb-dashboard-api`): bei fehlendem Streamer-Scope, aber gesetzter Bot-Kapabilitaet liefert `get_handler` `has_moderator_read_chatters: true`; bei beidem fehlend `false`.
8. Transport (`tb-transport-twitch`, wiremock wie in `streams.rs`): `get_custom_rewards` parst `data[].{id,title,is_enabled}` und schickt `broadcaster_id` sowie den User-Token-Bearer.

Baseline: vor dem Bau `cargo test` der vier Crates laufen lassen und die bekannte rote Baseline (siehe Memory `tb-bot Build-Toolchain`) festhalten, damit neue Rote von Altlasten unterscheidbar sind. Toolchain rustc 1.97 via `~/.rustup`, `/home/nathanael/.cargo/bin/cargo`.

---

## Punkt 4: Bot-Token-Scope live

Es gibt keine DB-Quelle fuer die Bot-Token-Scopes. `BotTokenManager` (`tb-chat/src/token.rs`) laedt den Seed-Token aus Infisical/Env und ermittelt die Scopes zur Laufzeit ueber Twitch `/oauth2/validate` (`ValidateResponse.scopes`); persistiert wird nur Access/Refresh (`secret_sink.rs::persist_bot_tokens`), nicht die Scope-Liste. `DEADLOCK_CENTRAL_DSN` zeigt auf die Datenbank `deadlock` (zentrale Bots); die Twitch-Analytics-Tabellen liegen in einer eigenen Twitch-DB und sind ueber diese DSN nicht erreichbar, also auch kein Umweg-Beleg dort. Der Scope kann nur am laufenden Bot bestaetigt werden (Log der Validate-Antwort oder ein Laufzeit-Probe). Funktional muss der Bot-Token `moderator:read:chatters` tragen, weil der gesamte Chatters-Poller (Quelle der Lurker-Kandidaten) global davon abhaengt. Genau deshalb persistiert REQ-7 diese Kapabilitaet aus dem laufenden Bot, damit das Dashboard dieselbe Bedingung wie die Laufzeit sehen kann.

---

## Risiken und Scope-Konflikte

- Kein Bau ausserhalb des erlaubten Bereichs noetig, ABER zwei Stellen liegen ausserhalb der in `EVIDENCE.md` benannten Zeilen und muessen ueber den erlaubten Ordner abgedeckt sein:
  - `rust/bin/tb-bot/src/chat_wiring.rs` (Redemption-Hook, Reward-Checker-Wiring) ist im Contract-Scope, gut.
  - `rust/crates/tb-transport-twitch/src/` als ganzer Ordner ist im Scope (neue Datei `channel_points.rs` und `lib.rs`-Export dort erlaubt).
- `rust/bin/tb-bot/src/eventsub_hooks.rs` und `wiring.rs`/`obs_dock.rs` implementieren ebenfalls `EventSubHooks`. Die neue Trait-Methode hat einen leeren Default, daher brauchen diese Impls keine Aenderung; falls Clippy/Trait-Vollstaendigkeit doch anschlaegt, waeren diese Dateien NICHT im erlaubten Bereich. Vor dem Bau die Default-Methode bewusst nur in dispatch.rs und ChatHooks anfassen.
- `rust/crates/tb-chat/src/token.rs` und `secret_sink.rs` liegen ausserhalb des Scopes: die Loesung darf sie nicht anfassen. Die Bot-Kapabilitaet wird deshalb aus `chatters_wiring.rs` (im Scope) geschrieben, nicht aus dem Token-Manager.
- `HelixClient` im Dashboard-State: falls `tb-dashboard-api/src/lib.rs` bisher keinen `HelixClient` haelt, ist die State-Erweiterung eine echte Wiring-Aenderung (im Scope, aber Aufwand und Startpfad pruefen). Alternative: der GET-Handler baut einen `HelixClient` aus vorhandener Config; das kann die Route verlangsamen und braucht Config-Zugang.
- Neue Migration und Statustabelle: erfordern das Twitch-Rechtemodell (postgres legt an, twitchbot/twitchdash bekommen Rechte, `_sqlx_migrations` von Hand). Fehlt der Schritt, bricht der Dashboard-Read.
- `.sqlx` und `fresh_schema_snapshot.txt` muessen mit jeder neuen Query mitwandern (sind im Scope), sonst blockt die Merge-Schleuse.
- Groesstes Risiko: REQ-7. Die Kapabilitaets-Persistenz koppelt zwei Prozesse ueber eine neue Tabelle. Schreibt der Bot sie nie (Loop startet nicht, Rechte fehlen), zeigt das Dashboard dauerhaft die Warnung, obwohl die Laufzeit sendet. Gegenmassnahme: Default-Zeile bei Migration setzen und den Schreibpfad mit einem Integrationstest gegen die Test-DB absichern.
