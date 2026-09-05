# Plan: llm_usage statt minimax

## Wächtertest (REQ-6), roter Ausgangslauf

- Testname: `no_minimax_identifiers::kein_minimax_bezeichner_mehr_im_code` (Crate `tb-llm`)
- Lauf: `cargo test -p tb-llm --test no_minimax_identifiers` ergibt FAILED (panic)
- Trefferzahl: 414
- Erste 20 Treffer:
  1. bot/dashboard_v2/src/api/ai.ts:81: export async function fetchChatMinimaxDeep(
  2. bot/dashboard_v2/src/api/ai.ts:90: return fetchApi('/chat-deep-minimax', ...)
  3. bot/dashboard_v2/src/components/cards/PostStreamReportCard.tsx:255: const modelLabel = data.model === 'opus' ? 'Claude Opus' : 'Minimax';
  4. bot/dashboard_v2/src/components/socialmedia/EnrichmentPanel.tsx:225: MINIMAX_API_KEY
  5. bot/dashboard_v2/src/components/verwaltung/AIEngagementSection.tsx:104: MiniMax-M3 liest deinen Chat mit ...
  6. bot/dashboard_v2/src/pages/StreamReports.tsx:1108: {data?.model || 'MiniMax'}
  7. bot/dashboard_v2/src/pages/chatAnalyticsContent.tsx:23: ChatMinimaxDeepSection,
  8. bot/dashboard_v2/src/pages/chatAnalyticsContent.tsx:374: <ChatMinimaxDeepSection
  9. bot/dashboard_v2/src/pages/chatAnalyticsDeepSections.tsx:28: import { fetchChatMinimaxDeep } from '@/api/ai';
  10. bot/dashboard_v2/src/pages/chatAnalyticsDeepSections.tsx:592: export function ChatMinimaxDeepSection({
  11. bot/dashboard_v2/src/pages/chatAnalyticsDeepSections.tsx:608: const res = await fetchChatMinimaxDeep(...)
  12. bot/dashboard_v2/src/pages/chatAnalyticsDeepSections.tsx:635: <h2 ...>MiniMax Chat-Analyse</h2>
  13. bot/dashboard_v2/src/pages/chatAnalyticsDeepSections.tsx:654: MiniMax analysiert die Nachrichten-Substanz...
  14. bot/dashboard_v2/src/types/analytics.ts:1353: model?: 'minimax' | 'opus';
  15. bot/dashboard_v2/src/types/analytics.ts:1372: model: 'minimax' | 'opus';
  16. rust/bin/tb-bot/src/chat_wiring.rs:30: ConversationScamGuard, MiniMaxScamJudge, ...
  17. rust/bin/tb-bot/src/chat_wiring.rs:48: LfgPitchResponder, MiniMaxInviteQuestionJudge, MiniMaxLfgJudge, ...
  18. rust/bin/tb-bot/src/chat_wiring.rs:54: use tb_engagement::minimax_chat::EngagementMinimaxClient;
  19. rust/bin/tb-bot/src/chat_wiring.rs:705: Arc::new(MiniMaxScamJudge::new(EngagementMinimaxClient::new(
  20. rust/bin/tb-bot/src/chat_wiring.rs:766: EngagementMinimaxClient::new(None, None, None, None),

## Wächter-Allowlist (Begründung)

REQ-6 nennt als Allowlist die tb-llm-Fixtures. Zusätzlich muss der Wächter zwei
weitere legitime Restvorkommen zulassen, die aus dem Scope fallen:

- `rust/crates/tb-llm/src/hub.rs`, `rust/crates/tb-llm/src/selection.rs`: die
  tb-llm-Fixtures (MiniMax-Endpunkt- und Legacy-ENV-Abweisung, REQ-3).
- Token `minimax_reasoning`: persistierte Spalte der Tabellen
  `twitch_auto_learned_safe_patterns`/`twitch_auto_learned_spam_patterns`, angelegt
  in der eingefrorenen Migration `20260630141000_chat_moderation_runtime_tables.sql`
  (INV-3). Umbenennen bräuchte eine Migration außerhalb von REQ-1 und einen
  Backfill, also außerhalb des Auftrags. Der Wächter erlaubt den Token pro Zeile,
  flaggt aber jedes andere `minimax` in denselben Dateien.
- Die Testdatei selbst.

## Naming-Map

- Modul `tb_engagement::minimax_chat` wird `llm_chat` (Datei umbenannt)
- Modul `tb_analytics::chat_deep_minimax` plus Handler wird `chat_deep_llm` (Dateien umbenannt)
- Route `/twitch/api/v2/chat-deep-minimax` wird `/twitch/api/v2/chat-deep-llm`
- Typ `EngagementMinimaxClient` wird `EngagementLlmClient`
- Typen `MiniMaxScamJudge` wird `LlmScamJudge`, `MiniMaxInviteQuestionJudge` wird `LlmInviteQuestionJudge`, `MiniMaxLfgJudge` wird `LlmLfgJudge`
- Konst `AI_MODEL_MINIMAX` wird `AI_MODEL_LLM`, `MINIMAX_HOURLY_FOLLOW_UP_LIMIT` wird `LLM_HOURLY_FOLLOW_UP_LIMIT`, `MINIMAX_MODEL` wird `LLM_MODEL`, `USE_CASE_MINIMAX` wird `USE_CASE_LLM`
- Enum `AiModel::Minimax` wird `AiModel::Llm`
- Variablen/Felder `minimax` wird `llm`, `minimax_uri` wird `llm_uri`
- Testmodellname `MiniMax-M3` wird `deepseek-v4-flash`
- Log- und Kommentar-Prosa: Anbietername entfernt (MiniMax wird KI oder gelöscht)
- Frontend `fetchChatMinimaxDeep` wird `fetchChatDeepLlm`, `ChatMinimaxDeepSection` wird `ChatDeepLlmSection`

## REQ-4 (persistierte Werte), Nachweis und Entscheidung

Read-only-Abfrage gegen `twitch_analytics` (SELECT, keine Änderung):
- `twitch_stream_ai_reports.model`: 4277 mal `minimax`, 489 mal `opus`.
- `ai_analyses.model`: nur `claude-opus-4-6` (24), kein `minimax`.

Codepfad-Analyse:
- `AI_MODEL_MINIMAX = "minimax"` (ai_state.rs, ai_analysis.rs): Der Wert liegt nur
  im In-Memory-State (`AI_STATE`, kein DB-Schreibpfad) und wird nie auf Gleichheit
  geprüft, verglichen wird nur `AI_MODEL_OPUS`, der else-Zweig deckt alles Übrige.
  In ai_analysis speist der Wert nur `model_name_for`s else-Zweig, der in prod nie
  genommen wird (Analytics vergibt ausschließlich Opus). Wert gefahrlos zu `"llm"`.
- `MINIMAX_MODEL = "MiniMax-M3"` (ai_analysis.rs): landet nur über den nie
  genommenen else-Zweig in `ai_analyses.model` (Beleg: keine `MiniMax-M3`-Zeile in
  der DB). Konstante `LLM_MODEL`, Wert `"deepseek-v4-flash"` (das real laufende
  Modell), da keine Zeile davon abhängt.
- `AiModel::Minimax => "minimax"` (post_stream.rs): geschrieben in
  `twitch_stream_ai_reports.model` (4277 Altzeilen). Kein Codepfad verzweigt auf
  den gespeicherten Wert (grep: keine `== "minimax"` außer In-Memory ai_chat und
  der Ledger-ILIKE); `plan_ai_model` liefert nur Opus/None, also entstehen keine
  neuen `minimax`-Zeilen mehr. Das Frontend bildet jeden Nicht-Opus-Wert auf "KI"
  ab. Variante `AiModel::Llm`, Wert `"llm"`; Altzeilen bleiben (kein Backfill
  nötig, da display-seitig auf "KI" gemappt).
- ai_history-Alias `"minimax"` (ai_history.rs): berechneter Alias in der
  API-Antwort (nicht persistiert). Wird `"llm"`, Frontend-Typ folgt.

## Ledger-Budget/Filter (REQ-2, INV-1)

- ENV `MINIMAX_5H_TOKEN_BUDGET` entfällt; Budget als Modulkonstante mit dem
  bisherigen Default 0 (Budget aus). `budget_from_env` und der zugehörige
  ENV-Test entfallen.
- `ModellFilter::NurMinimax` und ILIKE `'minimax%'`: Der Filter trennte MiniMax von
  anderen Ledgerzeilen fürs Budget. MiniMax läuft nicht mehr und das Budget ist per
  Konstante aus (`warn_if_over_budget` kehrt sofort zurück). Der Filter entfällt,
  `window_tokens`/`warn_if_over_budget` zählen alle Zeilen. Beobachtbare Wirkung:
  keine (Budget aus). Aufzeichnungssemantik (Spalten, best-effort) unverändert (INV-1).

## Pricing-ENV (pipeline.rs, INV-2)

- `MINIMAX_PRICE_INPUT/OUTPUT_PER_1K` entfallen; die Raten werden Modulkonstanten
  mit den bisherigen Defaults (0.0008 / 0.0024). Keine neue ENV (INV-2); prod setzt
  diese Legacy-ENV ohnehin nicht.

## Migrationen (REQ-1)

- `20260905130000_llm_usage_rename.sql` (jüngste Bestandsdatei: 20260905120000):
  benennt Tabelle, Indizes und Sequenz auf `llm_usage` um und legt den
  Kompat-View `public.minimax_usage` (SELECT * auf llm_usage, auto-updatable) an,
  damit das noch laufende alte Binary bis zum Restart weiterschreibt. Grants an
  twitchbot/twitchdash/twitchlegacy laufen rollen-gesichert (pg_roles-Check),
  damit frische Test-DBs ohne diese Rollen nicht scheitern.
- `20260905130001_llm_usage_drop_compat_view.sql`: droppt den Kompat-View, von
  Hand als postgres NACH dem Restart anzuwenden.
- Beide Dateien bewusst ohne SQL-Kommentare (Repo-Regel); Erklärung steht hier.
  Reihenfolge: erste Migration vor dem Restart, zweite danach.
- Beide Migrationen sind idempotent und guarded (to_regclass/relkind-Checks,
  CREATE OR REPLACE VIEW, DROP nur wenn View), wie migrate.rs es vorschreibt.
  Damit sind sie sowohl per Handanwendung als auch über den sqlx-Migrator
  wiederholbar, ohne beim nächsten Boot mit 42P07 abzubrechen. Hintergrund:
  tb-bot/tb-dashboard rufen `run_migrations` beim Start (Default `TB_DB_MIGRATE`
  true), in Prod steht `TB_DB_MIGRATE=0` (Infra-Notiz), also migriert der Bot
  dort nicht selbst; die Guards halten die Migration aber auch bei aktivem
  Migrator sauber. Der Kompat-View-Zweck (altes Binary schreibt bis zum
  Restart weiter) trägt nur im Prod-Handpfad, was der eingesetzte Modus ist.
