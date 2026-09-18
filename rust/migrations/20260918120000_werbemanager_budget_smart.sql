-- Werbemanager: Stundenbudget, Match-Uebergaenge, Entscheidungsverlauf und die
-- Bereinigung der Geisterzeile ohne twitch_user_id. Die Strategie 'monitor'
-- entfaellt; ausschliesslich 'snooze' und 'smart' bleiben zulaessig.

ALTER TABLE twitch_ad_manager_settings
    ADD COLUMN IF NOT EXISTS budget_minutes_per_hour INTEGER NOT NULL DEFAULT 3
        CHECK (budget_minutes_per_hour BETWEEN 1 AND 8);

UPDATE twitch_ad_manager_settings
    SET enabled = FALSE, strategy = 'smart'
    WHERE strategy = 'monitor';

ALTER TABLE twitch_ad_manager_settings
    ALTER COLUMN strategy SET DEFAULT 'smart';

ALTER TABLE twitch_ad_manager_settings
    DROP CONSTRAINT IF EXISTS twitch_ad_manager_settings_strategy_check;
ALTER TABLE twitch_ad_manager_settings
    ADD CONSTRAINT twitch_ad_manager_settings_strategy_check
        CHECK (strategy IN ('snooze', 'smart'));

DELETE FROM twitch_ad_manager_settings WHERE BTRIM(twitch_user_id) = '';
ALTER TABLE twitch_ad_manager_settings
    DROP CONSTRAINT IF EXISTS twitch_ad_manager_settings_user_id_not_blank;
ALTER TABLE twitch_ad_manager_settings
    ADD CONSTRAINT twitch_ad_manager_settings_user_id_not_blank
        CHECK (BTRIM(twitch_user_id) <> '');

ALTER TABLE twitch_ad_manager_state
    ADD COLUMN IF NOT EXISTS match_active BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS match_started_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS match_ended_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS avg_match_seconds INTEGER,
    ADD COLUMN IF NOT EXISTS avg_queue_seconds INTEGER,
    ADD COLUMN IF NOT EXISTS plan_next_block_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS plan_block_seconds INTEGER,
    ADD COLUMN IF NOT EXISTS plan_blocks_per_hour INTEGER,
    ADD COLUMN IF NOT EXISTS plan_fit TEXT,
    ADD COLUMN IF NOT EXISTS budget_used_seconds INTEGER;

CREATE TABLE IF NOT EXISTS twitch_ad_manager_decisions (
    id BIGSERIAL PRIMARY KEY,
    twitch_user_id TEXT NOT NULL
        REFERENCES twitch_ad_manager_settings(twitch_user_id) ON DELETE CASCADE,
    session_id BIGINT,
    decided_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    decision TEXT NOT NULL
        CHECK (decision IN ('commercial', 'snooze', 'postpone', 'none')),
    reason TEXT NOT NULL,
    block_seconds INTEGER,
    detail TEXT
);

CREATE INDEX IF NOT EXISTS twitch_ad_manager_decisions_session
    ON twitch_ad_manager_decisions (twitch_user_id, session_id, decided_at DESC);

CREATE INDEX IF NOT EXISTS twitch_ad_manager_decisions_retention
    ON twitch_ad_manager_decisions (decided_at);
