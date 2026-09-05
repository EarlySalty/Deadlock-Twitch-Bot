# Contract: MiniMax-Namensreste raus, Ledger heißt llm_usage

Datum: 2026-09-05. Klasse: medium (mechanische Umbenennung über viele Dateien, eine Tabellen-Umbenennung mit Deploy-Reihenfolge, keine Verhaltensänderung an KI-Aufrufen).

## Ziel

Alle KI-Aufrufe laufen bereits über den zentralen Connector tb-llm (Fireworks, Deepseek V4 Flash; MiniMax wird im Hub abgewiesen). Der Code trägt aber noch den alten Anbieternamen als Tabellen-, Modul-, Typ-, Konstanten- und Routennamen und zeigt Streamern im Dashboard Texte, die MiniMax als Modell nennen. Nach diesem Auftrag heißt der Ledger `llm_usage`, kein Bezeichner im Rust- und Frontend-Code enthält mehr "minimax", und kein Nutzertext behauptet ein Modell, das nicht läuft.

## Anforderungen

- REQ-1: Migration `rust/migrations/<zeitstempel>_llm_usage_rename.sql`: `ALTER TABLE public.minimax_usage RENAME TO llm_usage`, Indizes `idx_mmu_ts` -> `idx_llm_usage_ts`, `idx_mmu_source` -> `idx_llm_usage_source`, Sequenz `minimax_usage_id_seq` -> `llm_usage_id_seq`, danach `CREATE VIEW public.minimax_usage AS SELECT * FROM public.llm_usage` (automatisch aktualisierbar, damit das noch laufende alte Binary bis zum Restart weiterschreibt) mit GRANT SELECT/INSERT/UPDATE/DELETE an `twitchbot`, `twitchdash`, `twitchlegacy`. Zweite Migration `<zeitstempel+1>_llm_usage_drop_compat_view.sql`: `DROP VIEW public.minimax_usage`. Beide werden vom Orchestrator von Hand als postgres angewendet (erste vor dem Restart, zweite danach), der Bot migriert nicht selbst. Bestehende Migrationsdateien bleiben byteidentisch.
- REQ-2: tb-llm (`rust/crates/tb-llm`): Ledger schreibt und liest `llm_usage`; `sqlx/schema.sql`, Test-DDL in `ledger.rs`, Crate-Doku und `Cargo.toml`-Kommentare folgen. Das 5-Stunden-Budget kommt aus einer Konstante im Modul (Wert wie der bisherige Default von `MINIMAX_5H_TOKEN_BUDGET`), die ENV-Variable entfällt.
- REQ-3: Rust-Bezeichner ohne "minimax": Modul `tb_engagement::minimax_chat` -> `llm_chat`, Typ `EngagementMinimaxClient` -> `EngagementLlmClient`; Modul `tb_analytics::chat_deep_minimax` und Handler `chat_deep_minimax` -> `chat_deep_llm`; Route `/twitch/api/v2/chat-deep-minimax` -> `/twitch/api/v2/chat-deep-llm`; `AI_MODEL_MINIMAX`, `MINIMAX_HOURLY_FOLLOW_UP_LIMIT` und alle weiteren Konstanten, Funktionen, Felder, Variablen, Testnamen und Doku-Kommentare in `rust/` bekommen neutrale Namen (`llm`, `modell`, `ki`). Ausnahmen, die bleiben: Literale in `rust/migrations/*` (eingefroren), Testfixtures in tb-llm, die belegen, dass MiniMax-Endpunkte und die Legacy-ENV-Namen `MINIMAX_API_KEY`/`MINIMAX_TOKEN_PLAN_KEY` abgewiesen bzw. ignoriert werden, und der Ledger-Eintrag `model` alter Zeilen in der DB.
- REQ-4: Persistierte Werte bleiben: Strings, die als Wert in DB-Spalten oder gespeichertem Zustand landen (etwa `"minimax"` als Modellschlüssel in `ai_state.rs`), werden nur umbenannt, wenn nachweislich keine gespeicherte Zeile davon abhängt; der Nachweis (Abfrage oder Codepfad) steht in PLAN.md. Sonst behält die Konstante den alten Wert bei neutralem Namen.
- REQ-5: Frontend `bot/dashboard_v2/src`: `fetchChatMinimaxDeep` -> `fetchChatDeepLlm` auf die neue Route, `ChatMinimaxDeepSection` -> `ChatDeepLlmSection`, Überschrift "MiniMax Chat-Analyse" -> "KI-Chat-Analyse"; der Text "MiniMax-M3 liest deinen Chat mit ..." in `AIEngagementSection.tsx` wird zu einer neutralen, zutreffenden Aussage über die KI ohne Modellname; der Hinweis auf `MINIMAX_API_KEY` in `EnrichmentPanel.tsx` wird gegen den tatsächlich genutzten Schlüsselnamen ersetzt oder gestrichen, wenn der Panel-Pfad über tb-llm keinen Nutzerschlüssel mehr braucht; Modell-Labels in `PostStreamReportCard.tsx` und `StreamReports.tsx` zeigen den Modellnamen aus der API-Antwort, Fallback "KI", nie ein fest verdrahtetes "MiniMax". Deutsche Texte mit echten Umlauten, keine Em-Dashes.
- REQ-6: Wächtertest in `rust/crates/tb-llm/tests/` (oder bestehender Guard-Ort im Repo): liest rekursiv `rust/` (ohne `target`, `.sqlx`, `migrations`) und `bot/dashboard_v2/src` und schlägt fehl, wenn ein Dateiname oder Dateiinhalt "minimax" enthält (case-insensitiv), mit expliziter Allowlist genau der Dateien aus REQ-3 (tb-llm-Fixtures) und Ausgabe der Treffer. Vor dem Umbau rot mit Trefferliste (in PLAN.md festhalten), nach dem Umbau grün.
- REQ-7: Workspace-Tests mit `SQLX_OFFLINE=1` und Toolchain 1.97.1 gegen die Baseline grün (Baseline main: `ad_manager_store::queue_lease_idempotenz_und_state_sind_atomar` und `ledger_side_effects::engagement_client_verbucht_usage_ins_zentrale_ledger` vorbestehend rot wegen fehlender Tabellen im Test-Schema; `ledger_side_effects` liest künftig `public.llm_usage`). `cargo sqlx prepare --workspace` aktualisiert `rust/.sqlx`; Schema-Snapshot `rust/crates/tb-db/tests/fresh_schema_snapshot.txt` folgt. In `bot/dashboard_v2`: `npm run build`, `npm run lint`, `npm test` grün.

## Invarianten

- INV-1: Kein KI-Aufruf ändert Endpunkt, Modell, Parameter oder Ledger-Semantik; die Spaltenmenge von `llm_usage` ist identisch zu `minimax_usage`.
- INV-2: Keine neuen ENV-Variablen, keine neue Config-Datei.
- INV-3: Bestehende Migrationsdateien bleiben byteidentisch (Checksummen).
- INV-4: Keine Code-Kommentare neu schreiben; Kommentare in berührten Zeilen löschen statt umformulieren, außer sie tragen eine Fundstelle oder Regel, dann nur den Anbieternamen entfernen.
- INV-5: Kein Feature wird beworben, das der Code nicht hat: jeder geänderte Nutzertext ist gegen den Codepfad geprüft.

## Nicht-Ziele

- Der Python-Helfer `~/Documents/.claude/minimax-usage/minimax_usage.py` (SQLite, eigenes Werkzeug) und der TradingBot-Ledger (eigene Datenbank) werden nicht angefasst.
- Keine Änderung an `transcribe.rs` (lokaler STT-Server, kein Sprachmodell-Chat).
- Kein Backfill der Spalte `model` alter Ledger-Zeilen.

## Erlaubter Bereich

- rust/crates
- rust/bin
- rust/migrations
- rust/.sqlx
- bot/dashboard_v2/src
- .tasks/2026-09-05-llm-usage-statt-minimax
