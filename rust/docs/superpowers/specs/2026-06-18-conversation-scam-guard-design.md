# Conversation-Scam-Guard — Design-Spec

**Datum:** 2026-06-18
**Branch:** `worktree-conversation-scam-guard`
**Status:** Entwurf zur Umsetzung (Implementierung → Codex)

---

## 1. Ziel

Streamer im Partner-Netzwerk — besonders neue, untrainierte — vor **Social-Engineering-Scammern** schützen, die **Keyword-Filter überleben**, indem ein LLM (MiniMax) den Chat mitliest, das Muster semantisch erkennt und bei hoher Sicherheit **kanal-lokal bannt** — plus eine **verständliche Begründung**, damit der Streamer lernt, die Masche selbst zu erkennen.

Es geht **nicht** um den plumpen Keyword-Spam („add him on Discord lirikk_1") — den fängt `scam_pitch` schon. Es geht um die getarnte Variante: ein Erstschreiber, der eine **aufgesetzte Kennenlern-Konversation** fährt.

### Kanonische Beispiele (Fake-Accounts)

`sophiaa_star` (30+ Nachrichten, einseitig): generisches Dauerlob, Recon-Fragen (PC/PS5, Ort, Job, Uhrzeit), Mitleids-Haken („hab grad kein Geld"), dann Pivot „can we connect with each other?".

`minniepearl19`: „Heya @… / How's it going? / If possible can we talk on chat now?" … „I'm back / How's your day been?".

Gemeinsam: **Erstschreiber**, **Englisch** in deutschem Kanal, generischer Beziehungsaufbau statt Spielbezug, Pivot zu Off-Platform-Kontakt.

---

## 2. Verifizierte Fakten (Codebase, mit Belegen)

Diese Fakten sind geprüft — **nicht** neu herleiten, darauf aufbauen:

- **Andockpunkt:** Nach dem Partner-Gate in `tb-chat/src/pipeline.rs:432` läuft für **jeden Partner-Kanal** alles unbedingt (scam_pitch, spam, tracker …). Der neue Detektor hängt sich **genau dort** ein.
- **Ban-Pfad:** `tb-chat/src/moderation.rs` → `ModerationEngine::auto_ban_and_cleanup(AutoBanRequest{ reason_text, … })` bannt kanal-lokal und schreibt `twitch_ban_events.reason` (≤300 Zeichen). Ban-Token = **Bot-User-Token** aus `token_mgr`, `moderator_user_id = token_mgr.bot_user_id()` (`moderation.rs:184,203,312–341`). Unban: `ChatApi::unban_user`.
- **Auto-Ban ist NICHT garantiert:** `add_channel_moderator` läuft nur **einmalig** beim Onboarding (`partner_setup.rs:921`), kein Auto-Repair. Verliert der Bot Mod → Ban schlägt fehl (`BanOutcome::Forbidden`). **Muss abgefangen werden** → Fallback `alert_only`.
- **„Erstschreiber"-Daten existieren:** `twitch_chatter_rollup` → `is_first_global` (noch nie irgendwo im Netzwerk, `chatter_tracking.rs:310`); pro Session `twitch_session_chatters.is_first_time_streamer` (erstmals in DIESEM Kanal). first-seen ist **session-gated** (`chatter_tracking.rs:135` verlangt offene Session; Target-Game-Gate nur wenn `persist_all_games=false`).
- **MiniMax-Plumbing existiert:** `tb-engagement` `EngagementMinimaxClient` (`messages_completion`, `raw_completion`) bzw. `call_minimax` in `tb-chat/src/scam_pitch.rs`. **Wiederverwenden**, keinen neuen HTTP-Client erfinden.
- **Trust-Signale am Event:** `ChatMessageEvent.badges` (Mod/VIP/Sub), `KNOWN_CHAT_BOTS`-Liste (legit Bots wie `deutschedeadlockcommunity` bereits ausgeschlossen).

### Offener Blocker (ZUERST klären — Ticket T0)

Kein **Prod**-Aufrufer von `ensure_chat_subscriptions` gefunden (nur Tests); Altbefund „`channel.chat.message` kam nie in Rust an" (`webhook_receiver.rs:8`). **Wenn der EventSub-Chat-Stream auf Partner-Kanälen nicht fließt, ist das Feature tot.** Codex verifiziert das als **ersten** Schritt; fehlt der Stream → als Blocker melden, nicht blind weiterbauen.

---

## 3. Architektur

Neues, **getrenntes** Modul `tb-chat/src/conversation_scam.rs` (bei Bedarf Submodul-Ordner), **parallel** zu `scam_pitch`. Der bestehende `SpamAiReviewer` / das Pattern-Learning bleibt **unangetastet**.

```
channel.chat.message (EventSub, Partner-Kanal)
        │  pipeline.rs (nach :432)
        ▼
  ConversationScamGuard::observe(event)
        │
        ├─ Trigger? (Erstschreiber in DIESEM Kanal, kein Mod/VIP/Sub/Known-Bot)
        │       └─ nein → return (Detektor ignoriert)
        ▼
  Pro (Kanal, Chatter): wachsender MiniMax-Dialog (DialogState)
        │  Nachricht anhängen → MiniMax-Urteil
        ▼
  Verdict { verdict, confidence, category, reasoning }
        ├─ unsure / scam<Schwelle → weiter sammeln
        ├─ clean → als erledigt markieren, Dialog beenden
        └─ scam & confidence ≥ Schwelle (default 0.90)
                 ▼  Aktion je Modus (Dashboard, pro Kanal)
              auto_ban → ModerationEngine::auto_ban_and_cleanup (reason = reasoning)
                          └─ Forbidden (kein Mod) → alert_only + Dashboard-Flag
              timeout  → timeout_user (gleiche Begründung)
              alert_only → nur Dashboard-Alert + Verdict-Persistenz
        ▼
  Persistenz: twitch_scam_guard_verdicts (+ twitch_ban_events via Ban-Pfad)
```

**Tiefes Modul:** schmale Außenschnittstelle `observe(&ChatMessageEvent) -> ()` (fire-and-forget, wie `scam_pitch`); interne Tiefe (Trigger, Dialog-State, Judge, Decision-Engine, Persistenz) gekapselt. Der LLM-Judge hinter einem **Trait** (mockbar für Tests).

---

## 4. Komponenten

### 4.1 Trigger / Gate (kein Keyword-Verdikt)
Geprüft wird nur, wer **erstmals in diesem Partner-Kanal** schreibt (`is_first_time_streamer`). Ausgeschlossen: Mods, VIPs, Subs (badges), Known-Bots (`KNOWN_CHAT_BOTS`), der Bot selbst. `is_first_global` ist **kein** Trigger, wird aber als **Zusatz-Hinweis** an den Judge gegeben (brandneu im Netzwerk = verdächtiger). Account-Alter spielt **keine** Rolle. Kein Token-Spar-Gate — der Erstschreiber-Trigger ist scharf genug.

### 4.2 Dialog-State (pro Kanal+Chatter)
In-Memory `HashMap<(channel, chatter), DialogState>` (Verlust bei Neustart unkritisch — Persistenz liegt in DB). `DialogState` hält das **wachsende MiniMax-`messages`-Array**: `[system(Judge-Prompt), user(msgs…), assistant(letztes JSON), user(neue msgs), …]`. Bewertung startet erst, wenn **genug Daten** da sind (Default: ab der **3.** substanziellen Nachricht des Chatters), danach bei **jeder** weiteren Nachricht erneut. `clean` → State auf „erledigt", keine weiteren Calls. Triviale Nachrichten (Emote-only, 1 Wort) zählen nicht als „substanziell".

### 4.3 MiniMax-Judge
Trait `ScamJudge { async fn judge(&self, dialog: &mut DialogState) -> Verdict }`. Prod-Impl nutzt das vorhandene MiniMax-Plumbing, **ohne Token-Limit** (voller Verlauf + volle Begründung, kein `max_answer_len`-Truncate). **Output-Vertrag** — strikt JSON:

```json
{ "verdict": "scam|clean|unsure", "confidence": 0.0, "category": "…", "reasoning": "…" }
```

Robustes Parsing (JSON ggf. aus Fließtext extrahieren; bei Parse-Fehler → `unsure`, nie crashen, nie bannen). Der **System-Prompt** (§6) ist user-sichtbarer deutscher Text → **Claude liefert ihn**, Codex setzt eine `const SCAM_JUDGE_SYSTEM_PROMPT: &str = "<<PLATZHALTER>>";` und meldet Datei:Zeile.

### 4.4 Decision-Engine (Stufen-Modell)
Konfidenz-Bänder bei `verdict = scam`:
- **`confidence ≥ threshold`** (default 0.90) → **Auto-Aktion** je `mode`:
  - `mode = auto_ban` → `auto_ban_and_cleanup`, `reason_text = reasoning`. Bei `Forbidden`/Fehler → **kein** stiller Ausfall: zu `suggested` herabstufen + Verdict `action_taken='ban_failed_no_mod'`.
  - `mode = timeout` → `timeout_user`, gleiche Begründung.
  - `mode = alert_only` → kein Eingriff, als **Moderationsvorschlag** (`suggested`, „starker Vorschlag") ins Dashboard.
- **`suggestion_floor ≤ confidence < threshold`** (default Floor 0.70) → **Moderationsvorschlag** (`suggested`), **unabhängig vom Modus**: Dashboard-Eintrag mit **Ban-Button + Begründung + Ignorieren**, **kein** Auto-Eingriff. Klick auf Ban → derselbe `auto_ban_and_cleanup`-Pfad (reason = reasoning); Klick auf Ignorieren → `dismissed`.
- **`confidence < suggestion_floor`** oder `verdict = unsure` → kein Eingriff, **weiter sammeln**.
- `verdict = clean` → erledigt, Dialog beenden.

Jeder Übergang + Verdict → `twitch_scam_guard_verdicts` (Status siehe §5).

### 4.5 Chat-Commands (Mod/Broadcaster-only)
- **`!unban [@user]`** — Override: `unban_user` + Verdict als `overturned` markieren (False-Positive-Spur). Ohne `@user` → letzter Scam-Guard-Ban im Kanal.
- **`!explain [@user]`** — MiniMax erklärt den Fall **ausführlich** (aus gespeichertem Verdict + Verlauf), Antwort in **beliebig viele** ≤500-Zeichen-Chat-Häppchen gesplittet (**kein** Mengen-Cap), Twitch-Rate-Limit beachten (sequentiell senden). Ohne `@user` → letzter Fall.
- Command-Antworttexte = user-sichtbar → **Platzhalter** durch Codex, Claude füllt.

### 4.6 Dashboard
- **Moderationsvorschläge-Queue:** offene `suggested`-Verdicts (verdict, confidence, category, reasoning, Verlaufs-Snapshot) mit **Ban**-Button (→ `auto_ban_and_cleanup`) und **Ignorieren**-Button (→ `dismissed`).
- **Verdict-Detail/-Historie:** alle Verdicts inkl. `action_taken`, Zeit; bei gebannten ein **Override**-Button (→ `unban` + `overturned`).
- Modus-/Schwellen-/Floor-/Enable-Umschalter pro Kanal.
- Alle UI-Strings = **Platzhalter** durch Codex, Claude füllt.

---

## 5. Datenmodell (Migration, TDD)

**`twitch_scam_guard_settings`** (pro Kanal):
| Spalte | Typ | Default |
|---|---|---|
| `channel_login` | TEXT PK | |
| `enabled` | BOOLEAN | `TRUE` |
| `mode` | TEXT | `'auto_ban'` (`auto_ban`\|`timeout`\|`alert_only`) |
| `threshold` | REAL | `0.90` (Auto-Aktion ab hier) |
| `suggestion_floor` | REAL | `0.70` (Moderationsvorschlag ab hier bis < threshold) |

**`twitch_scam_guard_verdicts`** (Audit + Override):
| Spalte | Typ |
|---|---|
| `id` | BIGSERIAL PK |
| `channel_login` | TEXT |
| `chatter_login` | TEXT |
| `chatter_id` | TEXT NULL |
| `verdict` | TEXT |
| `confidence` | REAL |
| `category` | TEXT |
| `reasoning` | TEXT |
| `transcript_snapshot` | TEXT |
| `action_taken` | TEXT (`banned`\|`timed_out`\|`suggested`\|`dismissed`\|`ban_failed_no_mod`\|`overturned`) |
| `created_at` | TIMESTAMPTZ DEFAULT NOW() |

Migration nach `rust/migrations/`. Settings-Default `enabled=TRUE` setzt das Opt-out-Modell um (default an).

---

## 6. MiniMax-Judge-System-Prompt (von Claude — Codex setzt Platzhalter)

> Du bist ein Sicherheits-Wächter für den Twitch-Chat eines **deutschsprachigen** Deadlock-Streamers. Deine Aufgabe: erkennen, ob ein **Erstschreiber** (jemand, der zum ersten Mal in diesem Kanal schreibt) eine **aufgesetzte Social-Engineering-Konversation** führt — eine Masche, bei der ein Fake-Account den Streamer mit übertrieben freundlichem, skriptartigem Smalltalk umgarnt, um Vertrauen aufzubauen und ihn am Ende auf Discord / eine andere Plattform zu locken oder auszunutzen.
>
> Du bekommst nach und nach die Nachrichten **eines** Chatters (chronologisch; im Verlauf kommen weitere dazu). Bewerte den **gesamten bisherigen** Verlauf.
>
> **Typische Indizien:** generischer Beziehungsaufbau statt Spielbezug („Heya", „How's it going?", „How's your day been?", „Welcome back <3"); übertrieben schleimiges, unnatürliches Dauerlob ohne Anlass; einseitiges, vorgefertigt wirkendes Skript unabhängig von den Antworten des Streamers; Recon-Fragen ohne Spielbezug (Wohnort, Job, Alter, Uhrzeit bei dir, PC/PS5, „wie lange streamst du schon"); der **Pivot** zu Off-Platform-Kontakt („can we talk on chat now?", „can we connect?", Discord, Freundschaftsanfrage); Mitleids-Haken („hab grad kein Geld, probier's aber").
>
> **Sprache als Indiz (kein Alleinkriterium):** Diese Scammer schreiben praktisch immer **Englisch**, obwohl der Kanal deutschsprachig ist. Englischer Erstschreiber mit sofortigem Beziehungs-Smalltalk = deutlich verdächtiger. **Deutschsprachige** Erstschreiber sind selten diese Masche — im Zweifel „clean" oder „unsure".
>
> **Echte neue Zuschauer** unterscheiden sich klar: konkrete Spiel-/Stream-Fragen („lohnt sich Haze?", „welcher Rang?"), echte Reaktion aufs Geschehen, kein Beziehungs-Skript.
>
> Sei **zurückhaltend**: „scam" nur bei klar erkennbarem Muster. Reicht der Verlauf noch nicht → „unsure". Echte Zuschauer → „clean".
>
> Antworte **ausschließlich** mit einem JSON-Objekt, kein weiterer Text:
> `{"verdict":"scam|clean|unsure","confidence":0.0-1.0,"category":"<kurzes Label>","reasoning":"<2–4 Sätze Deutsch, allgemeinverständlich für einen unerfahrenen Streamer: WARUM verdächtig/unverdächtig, mit konkreten Indizien aus dem Verlauf, kein Fachjargon>"}`

---

## 6a. Roh-Korpus (echte Fälle — Few-Shot + Test-Fixtures)

Diese Transkripte sind **echte** Chat-Verläufe. Verwendung: (a) verdichtet als Few-Shot-Beispiele im Judge-Prompt (von Claude beim Füllen eingearbeitet), (b) als **Test-Fixtures** — der Judge MUSS Fall 1 & 2 als `scam` einstufen, die Kontrast-Fälle als `clean`. Sprache der Scammer: durchgehend Englisch im deutschsprachigen Kanal.

### Fall 1 — `sophiaa_star` (POSITIV/scam, Kanal `cheazycrust`)
Erstschreiberin, einseitiges Kennenlern-Skript über ~50 Nachrichten, Pivot am Ende.
```
sophiaa_star: Howdy Howdy
sophiaa_star: how's the day going?
[Streamer] cheazycrust: still on lunch break, brb
sophiaa_star: Okay Okay no problem
sophiaa_star: i'm waiting <3
sophiaa_star: welcome back <3
sophiaa_star: Wow this game is awesome
sophiaa_star: i'm doing great
sophiaa_star: thanks for asking <3
sophiaa_star: What do you think of this game? Since you're playing it I'd love to know I really like this game and I hope I get a chance to play it someday
sophiaa_star: so what other games are you play into?
sophiaa_star: wow you have good taste in games
sophiaa_star: i play mostly story games
sophiaa_star: but this game is my most fav game
sophiaa_star: just drop you a followeew
sophiaa_star: no problem you deserve it
sophiaa_star: No I haven't played it but I really feel like playing this game
sophiaa_star: For one I really like this game's graphics and secondly its smoothness is amazing I mean this is a very good game in every respect
sophiaa_star: Right now I don't have money to buy this game but I will definitely try it
sophiaa_star: By the way are you playing it on PS5 or on PC?
sophiaa_star: By the way PC has its own fun. Once someone plays on PC they don't really enjoy it on PS5 anymore
sophiaa_star: By the way i'm from USA
sophiaa_star: and you?
sophiaa_star: i'm from Chicago
sophiaa_star: So do you stream daily?
sophiaa_star: Ohh okay okay so what do you do beside streaming
sophiaa_star: like job or something?
sophiaa_star: It's 8:02 here
sophiaa_star: i'm in IT field
sophiaa_star: by the way how long have you been streaming? in general?
sophiaa_star: Okay by the way we've had a pretty good conversation can we connect with each other?
[Streamer] cheazycrust: <discord-invite>
sophiaa_star: Oh so this is your Discord?
sophiaa_star: okay i will join you
sophiaa_star: okay just sent you a friend req
```
**Tells:** generischer Beziehungsaufbau ohne Spielsubstanz, schleimiges Dauerlob („you deserve it", „good taste"), Mitleids-Haken („no money but I'll try"), Recon (PS5/PC, USA/Chicago, Job, Uhrzeit, Streaming-Dauer), Pivot („can we connect?", Friend-Request).

### Fall 2 — `minniepearl19` (POSITIV/scam, deutschsprachiger Kanal)
```
minniepearl19: Heya @<streamer>
minniepearl19: How's it going?
minniepearl19: If possible can we talk on chat now?
... (später)
minniepearl19: I'm back
minniepearl19: How's your day been?
```
**Tells:** englischer Erstschreiber in deutschem Kanal, sofortiger generischer Smalltalk + früher Pivot („can we talk on chat now?"), kein Spielbezug.

### Kontrast A — `charlie03q` (KEYWORD-Spam, NICHT unser Fall)
Plumpe Discord-Werbung, von `sery_bot` per Keyword sofort gebannt — **gehört `scam_pitch`, nicht uns**.
```
charlie03q: Yo bro ... I'd love to connect you with a top Twitch streamer who pulls thousands of live viewers and has over 3M followers ... add him on Discord lirikk_1 and tell him Charlie sent you ...
```

### Kontrast B — echter neuer Zuschauer (NEGATIV/clean, Referenz)
```
viewer_de: lohnt sich Haze grad? oder eher nerf gekriegt
viewer_de: gg, was baust du auf McGinnis?
```
**Warum clean:** konkrete Spiel-/Build-Fragen, Deutsch, kein Beziehungs-Skript, kein Off-Platform-Pivot.

---

## 7. Sicherheit / False-Positive

Mehrfach abgesichert: Trigger nur Erstschreiber; Mods/VIPs/Subs/Known-Bots aus; Ban nur bei `confidence ≥ 0.90`; „unsure → nie bannen"; Sprach-Prior schützt deutsche Viewer; Override per `!unban` + Dashboard; vollständiges Verdict-Log. Parse-Fehler/LLM-Ausfall → `unsure` (nie Ban). Kein Auto-Chat-Post (verrät die Masche nicht, verschmutzt Chat nicht); Begründung nur Dashboard/Audit + on-demand `!explain`.

---

## 8. Was Codex NICHT baut (CLAUDE.md-Regel)

User-sichtbarer deutscher Text (Judge-System-Prompt, `!explain`-Rahmen, Command-Antworten, Dashboard-Strings) → Codex setzt `"<<PLATZHALTER>>"` + meldet **Datei:Zeile**. **Claude** schreibt die finalen Texte nach dem Lauf. Grund: GPT bricht Umlaute + schwache Texte.

---

## 9. TDD-Plan

Test zuerst (Red→Green→Refactor), MiniMax/ChatApi gemockt (wiremock-Muster wie bestehende `scam_pitch`/`moderation`-Tests):
- Trigger: Erstschreiber vs. Mod/Sub/VIP/Known-Bot/Wiederkehrer.
- Verdict-Parsing: sauberes JSON, JSON-in-Fließtext, Müll → `unsure`.
- Decision×Mode-Matrix: scam/clean/unsure × auto_ban/timeout/alert_only.
- No-Mod-Fallback: `ban_user → Forbidden` ⇒ `alert_only` + `ban_failed_no_mod`.
- Schwelle: 0.89 kein Ban, 0.90 Ban.
- Commands: Auth (nur Mod/Broadcaster), `!explain`-Chunking ≤500 (kein Cap), `!unban` → unban + overturned.
- Migration: beide Tabellen + Defaults.

---

## 10. Implementierungs-DAG (Tickets)

- **T0 — Spike/Blocker:** EventSub-`channel.chat.message`-Subscription in Prod verifizieren. Fehlt → melden, Stopp. *(blockiert alles)*
- **T1 — Migration:** `twitch_scam_guard_settings` + `twitch_scam_guard_verdicts`. *(dep: —)*
- **T2 — Modul-Gerüst + Trigger:** `conversation_scam.rs`, Typen (`Verdict`, `DialogState`, `ScamJudge`-Trait), Trigger-Logik. *(dep: T1)*
- **T3 — MiniMax-Judge:** Prod-Impl über vorhandenes Plumbing, wachsendes `messages`-Array, robustes JSON-Parsing, **kein** Token-Cap, System-Prompt = Platzhalter. *(dep: T2)*
- **T4 — Decision-Engine + Persistenz:** Akkumulation, Schwelle, Mode-Mapping, No-Mod-Fallback, Verdict-Insert. *(dep: T2, T3)*
- **T5 — Pipeline-Wiring:** Einhängen in `pipeline.rs` nach :432, `settings.enabled` respektieren. *(dep: T4)*
- **T6 — Commands:** `!unban`, `!explain` (Chunking, kein Cap), Antworttexte = Platzhalter. *(dep: T4)*
- **T7 — Dashboard:** Verdict-Detail + Override-Button + Mode/Threshold/Enable-Toggle, Strings = Platzhalter. *(dep: T4)*

**DoD gesamt:** `cargo build` + `clippy` sauber, alle Tests grün, kein `.unwrap()`/`.expect()` in Prod-Pfaden, alle user-sichtbaren Texte als Platzhalter mit Datei:Zeile gemeldet, Migration läuft gegen frische DB.

---

## 11. Entscheidungs-Ledger (Quelle der Wahrheit)

1. Scope: **nur Partner-Kanäle** (EventSub), `enabled` default AN, opt-out.
2. Aktion: Auto-Ban + Begründung + Override; Modus pro Kanal im Dashboard (`auto_ban`\|`timeout`\|`alert_only`), default `auto_ban`.
3. Blast Radius: **kanal-lokal** (wie Viewbots), **nicht** global ban list.
4. Architektur: neues getrenntes Modul parallel zu `scam_pitch`; `SpamAiReviewer` unangetastet; nutzt `ModerationEngine` + MiniMax-Client + Pipeline-Andockpunkt.
5. Trigger: **Erstschreiber im Kanal**; Mods/VIPs/Subs/Known-Bots aus; Account-Alter egal; `is_first_global` nur als Judge-Hinweis.
6. Bewertung: sammeln → `{scam|clean|unsure, confidence, category, reasoning}` → unsure nachfassen, scam+≥0.90 → Ban, clean → erledigt.
7. Output: Dashboard-Detail + `twitch_ban_events`-Feed, **kein** Auto-Chat-Post; reasoning = Klartext für unerfahrene Streamer.
8. LLM-Input: nur Messages des Verdächtigen, als **fortlaufender MiniMax-Dialog**.
9. Judge = **MiniMax**, **kein** Token-Limit.
10. Commands (Mod/Broadcaster): `!unban [@user]`, `!explain [@user]` (beliebig viele ≤500-Zeichen-Häppchen, kein Cap).
11. Schwelle: **0.90**.
12. **Sprach-Prior:** Englisch = verdächtiger, Deutsch = entlastend (Indiz, kein Alleinkriterium).
13. User-sichtbare Texte → **Claude** schreibt, Codex setzt Platzhalter.
14. **Stufen-Modell:** ≥ 0.90 → Auto-Aktion je Modus; 0.70–<0.90 (Scam) → **Moderationsvorschlag** (Dashboard: Ban-Button + Begründung + Ignorieren, kein Auto-Eingriff); < 0.70 / unsure → weiter sammeln.
