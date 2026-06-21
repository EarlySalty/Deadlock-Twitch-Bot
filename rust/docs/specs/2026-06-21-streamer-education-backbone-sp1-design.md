# SP1 — Streamer-Bildungs-Rückgrat (Design-Spec)

**Datum:** 2026-06-21
**Status:** Brainstorm/Grillme abgeschlossen, Architektur freigegeben (mit Refinements), **wartet auf User-Review dieser Spec** → danach `writing-plans`.
**Repo:** Deadlock-Twitch-Bot (Rust LIVE, Code unter `rust/crates/`).
**Methodik:** Grillme/Brainstorming-Session; gestützt auf zwei verifizierte Recherche-Läufe (Web-Patterns + Deadlock-Daten/Statlocked/GameTracking).

---

## 0. Das wahre Ziel — das Flywheel (nicht nur SP1)

Der Bot ist heute ein „Raid-Tool, das 90 % der Streamer nicht verstehen". Das übergeordnete Ziel ist **kein** einzelnes Feature, sondern ein **selbstverstärkendes Flywheel**, bei dem jede Stufe die nächste antreibt — alles muss ineinander verlaufen:

1. Streamer kommt dazu → **Onboarding** macht klar, was der Bot kann.
2. Geht live mit Deadlock → **1 Tipp/Stream** + **AI-Support-Chat/Hilfeseite** → versteht & bleibt.
3. **Steam-/Datenwert** macht den Stream messbar besser → echter Mehrwert.
4. Bot stupst zu **Co-Streams** + Mitspieler in den **Discord** holen.
5. Discord wächst als zentraler Umschlagplatz → mehr Streamer entdecken den Bot → zurück zu 1.

**Monetarisierung ist explizit KEIN Designtreiber** — Nebenprodukt des Werts. Wer darauf optimiert, ruiniert das Produkt (User-Direktive).

**Sub-Projekte (nacheinander grillen & bauen):**
- **SP1 (dieses Dok):** Bildungs-Rückgrat — Wissensbasis + AI-Support-Chat + Hilfeseite + Go-Live-Tipp + Onboarding-Wizard + `!ask`/`!help`. Identitäts-Spine (Steam-Link) startet hier.
- **SP2:** Steam-/Daten-Wert — eigener Steam-Bot für Streamer, Live-/Match-Daten, Overlays, „zuverlässiger als alle anderen". (Hebel: User kennt den Betreiber von `deadlock-api.com`.)
- **SP3:** Netzwerk & Discord-Funnel — Co-Stream-Vorschläge + Streamer ziehen Mitspieler Stück für Stück in den Discord (mitspielen statt solo).

---

## 1. Gemeinsames Rückgrat (warum „alles ineinander" technisch funktioniert)

Drei geteilte Bausteine, die jede Flywheel-Stufe wiederverwendet:
1. **Eine Streamer-Identität** (Twitch ⊕ Steam ⊕ Discord verknüpft) — Datenbasis für SP1-Wizard, SP2-Steam-Wert, SP3-Matching.
2. **Eine SSOT-Wissensbasis** — speist Pull (Chat/Hilfeseite) UND Push (Tipp). Eine Quelle, kein Drift.
3. **Ein Auswahl-/Nachrichten-Hirn** (Tipp-Ranker) — dieselbe Scoring-Mechanik rankt später auch SP2/SP3-Nudges im selben Go-Live-Slot.

---

## 2. SP1-Architektur

### §1 SSOT — Wissensspeicher (Fundament)
- Versioniertes **Markdown-Repo** im Twitch-Bot-Repo, **eine Datei pro Wissens-Einheit**, **zwei Namespaces**.
- **Frontmatter:** `title · namespace (bot|deadlock) · category · audience (streamer|viewer) · last_updated · source (manual|deadlock-api|gametracking) · tip_eligible (bool) · tip_flags ([feature|costream|discord|patch|...]) · time_to_value (1-5)`.
- **Kein RAG/Vektor-DB.** Kuratiertes Markdown, **per Frontmatter selektiert** in den Prompt (nie ganzer Korpus). Deterministisch, präzise für Setup-Schritte, günstig bei niedrigen Hunderten Dokus. Hybrid/RAG nur vorbereiten, später nur bei objektivem Trigger (Korpus passt nicht mehr in Kontext / Relevanz-Ratio zu niedrig).
- **Zwei Befüllungs-Pipelines:**
  - **Bot-Wissen** = handverlesen, DE-Wording **von Claude** (nicht GPT — Umlaute), eine Datei pro Feature/Befehl/Setup.
  - **Deadlock-Brain** = Infra jetzt bauen, **Inhalt erst später** füllen (User: „noch nicht soweit, aber Infra schon bauen"). Auto-ingestiert.

### §2 Ingest & Frische (kein Doc-Rot)
- **Bot-Wissen:** docs-as-code — Feature gilt erst fertig, wenn seine Wissensdatei steht (Definition-of-Done). Setup/How-to = „volatile"-Tier → bei jeder UI-Änderung neu verifizieren. Review-Cadence: High-Risk 30–60 T, Feature 90 T, Evergreen 180 T, Volatile ereignisgetrieben. Owner + `last_updated` im Frontmatter.
- **Deadlock-Brain (Infra in SP1):** geplanter Ingest-Job + Patch-Watcher.
  - Quelle für Spieldaten: `deadlock-api.com` Assets-API (`assets.deadlock-api.com/v2/heroes`, `/v2/items` etc.).
  - **WICHTIG (User-Entscheidung): Helden- und Item-Namen/Begriffe bleiben ENGLISCH** — auf Deutsch klingt es mies, die Community denkt EN. Also EN-Quelle für Terminologie; deutsche Asset-API (`?language=german`) höchstens für Fließtext/Lore, nicht für Namen/Items.
  - **Patch-Watcher:** `steam.inf`-Build-Nummer + `citadel_patch_notes`-Diffs im `github.com/SteamTracking/GameTracking-Deadlock` → Patch-Wissen + löst „Patch live"-Tipp-Kandidat aus.

### §3 AI-Support-Chat (Website, NUR Bot-Wissen) + Guardrails
- **Scope-Korrektur (User):** Dieser Chat ist **ausschließlich für den Twitch-Bot** (was kann er / wie richte ich X ein). **Keine Deadlock-Spielfragen** hier — die laufen über `!ask`/Discord (Deadlock-Brain). Vorteil: Der Website-Chat kann keine Spiel-Fakten halluzinieren, weil er das Brain gar nicht sieht.
- **Einbettung:** iframe-Widget (sandboxed, lazy/facade-load) → eigenes Rust-Backend, baut auf bestehendem `tb-llm`/`tb-dashboard-api/handlers/ai_chat.rs` auf.
- **Modell (User):** **Nur MiniMax M3.** Kein Opus (vorerst).
- **Anti-Halluzinations-Guardrails:** nur-aus-Kontext antworten; **Pflicht-Zitate** auf Quelldatei; **Refusal bei schwacher Evidenz** („Feature gibt's nicht / ist nicht dokumentiert"); deterministische Lookups für Befehls-/Setup-Fakten gegen die strukturierte KB.
- **Wissenslücken-Loop (NEU, User):** Wenn der Bot etwas **nicht** weiß → (1) nach außen ehrlich „weiß ich noch nicht / nicht dokumentiert" (keine Halluzination), (2) intern: unsere Daten durchsuchen → **Doku-Entwurf** schreiben → **in eine Review-Queue, die der User abnimmt** → bei Freigabe ab in die SSOT. Selbstlernend mit Human-in-the-Loop.

### §4 Tipp-Ranker + Go-Live-Push (Push-Aktuator)
- **Trigger:** bestehender `stream.online`-Hook (`tb-monitoring/src/dispatch.rs`) + Deadlock-Kategorie-Check. Es existiert bereits `channel.chat.user_first_message`-Registrierung; die **erste Bot-Chat-Nachricht beim Go-Live fehlt noch** und wird hier gebaut.
- **Gates (User-korrigiert):** **≥12h seit letztem Tipp (hart)** + **Opt-out (Dashboard-Flag)**. **KEIN Aktivitäts-Gate** — der Tipp feuert beim Go-Live so oder so. Viewer-Mitlesen ist okay/gewollt (Mini-Funnel).
- **Ranker (pro Streamer):** Score je `tip_eligible`-Einheit = f(nie genutzt ↑, **zuletzt genutzt vor langem ↑ = Reminder-Boost**, `time_to_value`, nicht kürzlich gezeigt). Höchster Score gewinnt den nächsten Slot. **Gewichtet mit Abklingen, NICHT binär** (User: schon genutzt ≠ raus — wir dürfen Nutzen tiefer erklären; vergessene Perlen kommen zurück). Braucht **Feature-Nutzungs-Tracking pro Streamer**.
- **Inhalts-Mix (User):** Hauptgewicht **C** (Bot-Feature, das eine Flywheel-Verhaltensweise anstößt — Co-Stream/Discord via `tip_flags`) **+ A** (Feature-Unlock, inkl. Stat-Befehle als früheste/wertvollste Tipps); **B** (reiner Streaming-/Spiel-Mehrwert) sparsam als Vertrauensanker.
- **Format:** erste Chat-Nachricht = **1 Nutzen-Satz + 1 Befehl/Aktion + optional Deep-Link** zur Hilfeseite. Kein Wall-of-Text.
- **Messung:** gezeigt → angeklickt/Feature-danach-genutzt → Opt-out-Rate; schwache/abgenutzte Tipps aus der Rotation nehmen; Reihenfolge nach Time-to-Value.

### §5 Onboarding-Wizard (geführt, abschließbar)
- First-Run im Dashboard: **3–5 lineare Schritte**, sichtbarer Fortschritt, **resumierbar**, Abschluss-Button (StreamElements-Muster, nicht leeres Dashboard).
- Schritte: ① **Steam/Deadlock-Profil verknüpfen** (Steam32-ID) → ② **Stat-Befehle aktivieren** → ③ **Go-Live-Tipp + Raid/Recruiting opt-in** → ④ Abschluss.
- **Sofortiges In-Chat-Erfolgs-/Fehler-Feedback** pro Schritt (Sery-Bot-Muster: „Verbunden! Probier !rank").
- **Scope-Transparenz** bei Twitch-Auth (jeden Scope begründen, vor Über-Berechtigung warnen).
- Die **Steam32-Verknüpfung ist der heimliche MVP**: zugleich Onboarding-Schritt UND Identitäts-Spine für SP2/SP3.

### §6 Konsum-Flächen (geschärfte Karte)
- **Bot-Wissen** → Website-**Support-Chat** (§3) · selbstgebaute **Hilfeseite** (wie normale Support-/Help-Seiten, User: „großes Produkt") · **Go-Live-Tipp** (§4) · `!help <feature>` im Chat · **`!commands`** → öffentliche, gruppierte Befehlsseite (Stats/Match/Mod/Fun).
- **Deadlock-Brain** → **`!ask <frage>`** (Twitch-Chat, Deadlock-Spielfragen für Viewer+Streamer) · **Discord-Bot** (Infra-ready, Anbindung später). Inkl. geplantem **Patch-Q&A-Feature**: Rückfragen zu Patches *mit Erklärungen* (Deadlock-Brain + Patch-Diffs).

---

## 3. Stat-Befehle & Streamer-Steam-Instanz (entschiedener Fork)

- **`!rank` kommt in SP1** als Tracer-Bullet, damit das Onboarding ab Sekunde 1 echten Wert zeigt (USP: generische Bots Nightbot/Fossabot/Moobot bieten out-of-the-box keine Game-Stats).
- **GELOCKT — kein Neubau, sondern zweite Instanz (User-Entscheidung):** Wir nutzen **denselben Code** (`Deadlock-Steam-Bot` Rust-Workspace) und betreiben eine **separate Streamer-Instanz neben** der bestehenden. Die bestehende Instanz bleibt unangetastet für User/Playtest-Invites. Kein Code-Fork, kein Copy-Paste — Multi-Tenant per Deployment.
- **Datenpfad existiert bereits:** `steam-core` fragt den Rang über den Deadlock **Game Coordinator** ab — `CMsgClientToGcGetProfileCard` → `CMsgCitadelProfileCard.ranked_badge_level` (Parsing in `steam-flows/src/rank.rs`). Für einen *einzelnen* Account ist GC autoritativ (besser als `deadlock-api` in niedrigen/mittleren Rängen — das der Bot ohnehin nicht nutzt). **`!rank` braucht also keinen neuen GC-Code** — nur Instanz-Isolierung + Wiring zum Twitch-Bot.
- **SP1-Scope der Streamer-Instanz = nur `steam-core`** (GC-Client + HTTP-API auf eigenem Port). Der `steam-bot`-Binary (Discord-Flows/Ranking-Scheduler) ist für `!rank` **nicht** nötig.
- **Instanz-Isolierung (Deployment/Config):** separater Steam-Account + eigene Infisical-Secret-Keys · eigener `STEAM_DATA_DIR` (Token/Session-Cache) · eigene Ports (`STEAM_CORE_API_ADDR`) · neue systemd-User-Service-Datei (`steam-core-streamer`).
- **Geteilte DB (User-Entscheidung) → Tenant-Namespacing kommt nach SP1 rein** (kleiner Code-Anteil, nicht nur Deployment): distinkte `bot`-Werte (`steam` vs. `steam-streamer`) im **schon vorhandenen** `standalone_bot_state.bot`-Key; **Tenant-Discriminator + gefilterte Claim-Query auf der `steam_tasks`-Queue**, damit Instanzen sich keine Tasks klauen/doppelt ausführen; Rang-Snapshots (`steam_links`) sind per Ziel-Steam-ID ohnehin disjunkt. WAL + `busy_timeout` sind bereits gesetzt → kein Korruptionsrisiko, nur logische Trennung.
- **DB-Fallback (dokumentiert, NICHT jetzt bauen):** Falls zwei Instanzen die SQLite unter Write-Contention nicht sauber bedienen → Migration auf eine stärkere DB (Postgres). Bewusst YAGNI für SP1 (WAL trägt zwei schreibarme Instanzen problemlos).
- **`!rank`-Flow:** Twitch-Chat `!rank` → `tb-bot` → HTTP an Streamer-`steam-core` (Rang für verknüpfte Steam-ID; vorhandenen Lookup-Endpoint wiederverwenden oder dünn ergänzen — Verifikation in `writing-plans`) → GC-ProfileCard → Badge→Rang-String → Chat-Antwort. Steam-Link kommt aus dem Onboarding-Wizard (§5, Identitäts-Spine).
- **NICHT in SP1:** volle Befehls-Suite (`!ranks`/`!avg`/`!wr`/`!meta`/`!lg`/`!smurf`…), Live-Lobby (Demo-SSE), Overlays → **SP2**-Ausbau **derselben** Instanz. (Tenant-Namespacing der DB ist wegen der geteilten DB **schon in SP1** dabei, siehe oben.)

---

## 4. Außerhalb SP1 (YAGNI)
Volle Deadlock-Brain-Befüllung · Discord-Bot-Anbindung (nur Namespace bereit) · Live-Overlays/Demo-SSE · volle Stat-Befehls-Suite · Co-Stream-Matching (SP3) · Monetarisierung.

---

## 5. Anhang — Deadlock-Daten-Realität (verifiziert, prägt SP1-Versprechen + SP2)
- **Keine offizielle Valve-GSI** für Deadlock (anders als Dota 2/CS2). Markt ist NICHT leer: Statlocked, deadlocktracker, Deadlock Labs — alle auf `deadlock-api.com`. Statlocked kann **nur EN/RU** → **DE ist unser Differenzierer** (für Bot-Texte/UI; Spiel-Terminologie bleibt aber EN, siehe §2).
- **LIVE In-Game-State:** `deadlock-api` Demo-SSE (`/v1/matches/{id}/live/demo/events`) liefert autoritativ Souls/KDA/Held/Score/Position, ~30 s Anlauf + Demo-Lag, **keine Ränge im Demo**. ODER Overwolf GEP (Echtzeit, nur lokaler PC, Install nötig, **kein Rang/MMR**).
- **POST-MATCH:** `deadlock-api` (Match-History/KDA/Builds), ingest-abhängig.
- **PROFIL/Rang:** `deadlock-api` — oben verlässlich, **niedrige/mittlere Ränge lückenhaft**. **ABER für SP1 irrelevant:** unsere Streamer-`steam-core`-Instanz holt den Rang eines *einzelnen* verknüpften Accounts direkt per **GC `CMsgCitadelProfileCard.ranked_badge_level`** — autoritativ über alle Ränge, kein deadlock-api-Loch. **Live-Lobby-Ränge** (andere Spieler im Match) bleiben **nur ab Eternus+** zuverlässig (Valve-Limit, betrifft alle Tools) → das ist eine **SP2-Live-Frage**, nicht `!rank`.
- **Konsequenz:** Elegantes *graceful degradation* (niedriger Rang → Profil/Post-Match statt Live), niemals Dota-Stil-Live-Garantien für alle versprechen.
- **GameTracking-Deadlock** (`github.com/SteamTracking/GameTracking-Deadlock`): Protobufs, Source2-Schemas, EN-Lokalisierungs-Strings, `steam.inf`-Build → gut für **autoritative Patch-Diffs** + EN-Namen/IDs-Kanonik + Proto-/Schema-Wahrheit (falls wir selbst parsen). **Keine** DE-Lokalisierung im Repo. (SP2-Hebel: User kennt den `deadlock-api`-Betreiber → ggf. zuverlässiger als alle anderen.)

---

## 6. Locked Decisions (Schnellreferenz)
1. Flywheel als Ziel; SP1→SP2→SP3 nacheinander. Monetarisierung kein Treiber.
2. 1 Tipp pro Stream, erste Chat-Nachricht, **best-effort**; ≥12h-Cap; **Opt-out im Dashboard**; **kein** Aktivitäts-Gate.
3. Tipp-Progression **adaptiv & gewichtet-abklingend** (nie genutzt ↑, lange-her = Reminder ↑), nicht binär. Mix C+A, B sparsam.
4. SSOT = Markdown+Frontmatter, **kein RAG** in SP1; zwei Namespaces (bot|deadlock); zwei Befüllungs-Pipelines.
5. AI-Support-Chat: **nur Bot-Wissen**, **nur MiniMax**, Grounding+Zitate+Refusal, **Wissenslücken-Loop mit User-Abnahme**.
6. Deadlock-Brain: Infra jetzt, Inhalt später; **Helden-/Item-Namen Englisch**.
7. `!rank` in SP1 via **zweiter `steam-core`-Instanz desselben Codes** (kein Neubau, kein Fork), neben der bestehenden laufend; bestehende Instanz bleibt für User/Playtest unangetastet. Rang via vorhandenem GC-ProfileCard-Pfad; Isolierung über Ports/Account/`STEAM_DATA_DIR`/Service + **geteilte DB mit `bot`-Tenant-Namespacing** (`steam_tasks`-Queue + `standalone_bot_state` in SP1; SQLite→Postgres als dokumentierter Fallback). Voller Suite/Live/Overlays = SP2.
8. User-sichtbare Texte schreibt **Claude**, nicht GPT (Umlaute). Implementierung delegierbar an GPT (gpt-5.5/xhigh), Claude reviewt.

---

## 7. Nächste Schritte
1. **User-Review dieser Spec** (Sub-Fork §3 gelockt: zweite `steam-core`-Instanz).
2. `writing-plans` → Implementierungsplan (DAG, vertikale Tracer-Bullets, TDD).
3. Bau via Worktrees + GPT-Delegation; CHANGELOG/Discord/In-App nach Push-Reihenfolge (siehe CLAUDE.md).
