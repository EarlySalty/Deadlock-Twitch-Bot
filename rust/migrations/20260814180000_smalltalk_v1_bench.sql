-- Backtest des V1-Smalltalks gegen echte eigene Chatzeilen.
--
-- Zweck: messen statt raten, wie nah die KI an den Zeilen liegt, die der Owner
-- in derselben Lage wirklich geschrieben hat. Grundlage sind die Stimulus-
-- Response-Paare aus `twitch_engagement_reaction_samples`: dort steht der
-- Stream-Ton und der Chat der Sekunden davor plus die echte Antwort. Der
-- Backtest fuettert der KI nur den Stimulus und legt ihre Zeile daneben.
--
-- Warum eigene Tabellen und nicht `twitch_smalltalk_messages`: die ist der
-- Live-Schattenlauf im echten Stream (eine Sitzung, ein Kanal, eine Stunde).
-- Der Backtest laeuft offline ueber beliebig alte Samples, wiederholbar, und
-- soll ueber Laeufe hinweg vergleichbar bleiben. Beides in eine Tabelle zu
-- werfen haette die Auswertung beider Seiten unbrauchbar gemacht.
--
-- `IF NOT EXISTS`: die Tabellen wurden vor dem Merge von Hand angelegt, damit
-- der Backtest schon laufen konnte. Der regulaere Migrationslauf beim
-- naechsten Botstart soll darueber stolperfrei hinweggehen.
CREATE TABLE IF NOT EXISTS twitch_smalltalk_bench_runs (
    id           UUID PRIMARY KEY,
    started_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at  TIMESTAMPTZ,
    -- Welcher Prompt-Bau getestet wurde ('v1' = schlanker Smalltalk-Prompt,
    -- 'test_mode' = der grosse Prompt des Live-Schattenlaufs). Damit sind zwei
    -- Laeufe auf denselben Samples direkt vergleichbar.
    variant      TEXT NOT NULL,
    model        TEXT NOT NULL,
    judge_model  TEXT,
    sample_count INTEGER NOT NULL DEFAULT 0,
    note         TEXT
);

-- Eine Zeile je Sample: echte Antwort, KI-Antwort, Urteil.
--
-- `sample_id` ist bewusst ohne Fremdschluessel: Samples duerfen aufgeraeumt
-- werden, ein alter Benchmark soll davon nicht verschwinden. Der Text steht
-- deshalb hier nochmal vollstaendig statt nur als Verweis.
CREATE TABLE IF NOT EXISTS twitch_smalltalk_bench_lines (
    id             BIGSERIAL PRIMARY KEY,
    run_id         UUID NOT NULL
                   REFERENCES twitch_smalltalk_bench_runs(id) ON DELETE CASCADE,
    sample_id      BIGINT NOT NULL,
    channel_login  TEXT NOT NULL,
    message_ts     TIMESTAMPTZ NOT NULL,
    human_text     TEXT NOT NULL,
    stream_context TEXT NOT NULL DEFAULT '',
    chat_context   TEXT NOT NULL DEFAULT '',
    -- NULL = die KI hat geschwiegen oder der Ausgabefilter hat verworfen.
    ai_text        TEXT,
    reject_reason  TEXT,
    latency_ms     INTEGER,
    -- Blindprobe: welche der beiden Zeilen haelt der Richter fuer die KI.
    judge_pick     TEXT CHECK (judge_pick IN ('ai', 'human', 'unsure')),
    -- Hat der Richter richtig getippt. FALSE ist das gute Ergebnis.
    judge_correct  BOOLEAN,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT twitch_smalltalk_bench_lines_unique UNIQUE (run_id, sample_id)
);

CREATE INDEX IF NOT EXISTS idx_smalltalk_bench_lines_run
    ON twitch_smalltalk_bench_lines (run_id, id);
