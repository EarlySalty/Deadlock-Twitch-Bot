# title_generator/ — Architektur & Funktionsreferenz

> Pfad: `bot/title_generator/` · Stand: 2026-06-08 · 6 Dateien, ~955 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [analytics.md](analytics.md) (Session-/Stats-Quelle), [core.md](core.md) (MiniMax-Client). Plan/Spec: [docs/superpowers/specs/2026-04-19-stream-title-generator-design.md](../superpowers/specs/2026-04-19-stream-title-generator-design.md).

## 1. Zweck & Abgrenzung

`title_generator/` erzeugt per **MiniMax** Vorschläge für Stream-Titel — gestützt auf die eigene Titel-Historie, gelerntes Wissen darüber, welche Titel je Streamer-Größe funktionieren, den aktuellen Deadlock-Rang und den Live-Spielzustand. Zwei Hintergrund-Jobs pflegen die Wissensbasis (nächtlich) und erzeugen wöchentliche Insights.

Abgrenzung: Die Stats kommen aus [analytics.md](analytics.md)/`storage`; hier wird daraus ein Prompt gebaut und ein Titel generiert + rate-limitiert.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | Chat-Command + Dashboard (Titel-Generierung), Hintergrund-Jobs. |
| **Nutzt** | MiniMax (`core/llm_providers`), `storage/` (Titel-Historie/Wissen), Steam-Bot-DB (Rang/Live-State über `steam_lookup`). |
| **DB-Tabellen** | Titel-Historie + Wissens-Titel (`title_db`), Steam-Bot-DB (lesend). |
| **Externe Dienste** | MiniMax. |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `title_ai.py` | 391 | Titel-/Insight-Generierung via MiniMax + Rate-Limiter. |
| `title_db.py` | 191 | Persistenz: Titel-Historie + gelernte Wissens-Titel. |
| `knowledge_job.py` | 154 | Nächtlicher Job: lernt erfolgreiche Titel je Größenklasse. |
| `insight_job.py` | 128 | Wöchentlicher Insight-Job je Partner. |
| `steam_lookup.py` | 89 | Rang + Live-In-Game-State eines Discord-Users. |

## 4. Datenfluss / Lebenszyklus

**Generieren:** Ein Command/Dashboard-Aufruf gibt Keywords → `generate_title(streamer_id, keywords, title_history, knowledge_titles, rank_display, live_state, source)`. `TitleRateLimiter.check_and_record` begrenzt (5 Anfragen / 10 min, Dashboard ×2). `build_title_prompt` baut den Prompt aus Historie + Wissens-Titeln + Rang + Emoji-Ratio + Live-State; MiniMax antwortet, `parse_title_response` + `_sanitize_title_result` säubern (Code-Fences, JSON, Titel-Bereinigung).

**Wissensaufbau:** `run_knowledge_job` (nächtlich, `schedule_nightly_knowledge_job`) wertet jüngste Sessions aus, klassifiziert die Streamer-Größe (`_classify_size` über Avg-Viewer) und extrahiert Keywords (`_extract_keywords`) → speichert „was funktioniert“ in `title_db`.

**Insights:** `run_insight_job` (wöchentlich) baut je aktivem Partner eine Auswertung der Titel-Historie (`generate_insight`).

## 5. Funktionsreferenz pro Datei

### title_ai.py
- `generate_title(streamer_id, keywords, title_history, knowledge_titles, rank_display, live_state, source="chat") -> dict` — Haupt-Generierung; wirft `RateLimitExceeded(retry_after)` bei Überschreitung.
- `generate_insight(title_history, period_label) -> dict` — Wochen-Insight.
- `TitleRateLimiter(max_requests=5, window_seconds=600, dashboard_multiplier=2)` mit `check_and_record(streamer_id, source) -> bool`.
- `build_title_prompt(keywords, title_history, knowledge_titles, rank_display, emoji_ratio, live_state=None) -> str` / `parse_title_response(raw) -> dict`.
- Sanitizer: `_sanitize_generated_title(...)`, `_sanitize_title_result(...)`, `_strip_code_fence`, `_extract_json_payload`, `_emoji_ratio(titles)`, `_format_metric`. `_get_minimax_client()`.

### title_db.py
Persistenz der Titel-Historie und der gelernten Wissens-Titel (Lesen/Schreiben für Generierung + Jobs).

### knowledge_job.py
- `run_knowledge_job()` / `schedule_nightly_knowledge_job(start_delay_s=0)` (long-running Task).
- `_fetch_recent_sessions(days=7)`, `_classify_size(avg_viewers) -> str`, `_extract_keywords(title)`, `_resolve_streamer_id_for_login(login)`.

### insight_job.py
- `run_insight_job()` / `schedule_weekly_insight_job(start_delay_s=0)`.
- `_fetch_active_partner_ids()`, `_fetch_history_for_period(streamer_id, start, end)`, `_enrich_with_scores(sessions, own_avg)`.

### steam_lookup.py
- `get_rank_for_discord_user(discord_user_id) -> dict | None` — Rang (oder None, wenn nicht verknüpft).
- `get_live_state_for_discord_user(discord_user_id) -> dict | None` — Live-In-Game-State, falls gerade in Deadlock. (`_fetch_rank_row`/`_fetch_live_row`.)

## 6. Datenbank & externe Schnittstellen

- **DB:** Titel-Historie + Wissens-Titel (`title_db`), Steam-Bot-DB (Rang/Live, lesend).
- **Extern:** MiniMax.

## 7. Stolperfallen / Besonderheiten

- **Rate-Limit pro Streamer:** 5/10 min (Dashboard ×2). Tests/Bulk-Aufrufe laufen sonst in `RateLimitExceeded`.
- **Größenklassen-Wissen ist gelernt, nicht fest:** `knowledge_job` füllt die Wissens-Titel nächtlich — frisch aufgesetzt ist die Wissensbasis dünn, die Vorschläge entsprechend generisch.
- **Live-State ist optional:** `steam_lookup` liefert nur etwas, wenn der Discord-User verknüpft und gerade in Deadlock ist — sonst wird ohne Live-Kontext generiert.
- **MiniMax-Output muss gesäubert werden:** `_strip_code_fence`/`_extract_json_payload` fangen Code-Fences/Zusatztext ab; ungesäubert wäre die Antwort kein valides JSON.
