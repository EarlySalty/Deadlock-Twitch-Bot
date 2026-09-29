CREATE TABLE IF NOT EXISTS public.tb_chat_brain_answers (
    id BIGSERIAL PRIMARY KEY,
    broadcaster_user_id TEXT NOT NULL,
    broadcaster_login TEXT NOT NULL,
    chatter_user_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    question TEXT NOT NULL,
    answer TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL CHECK (status IN ('Pending', 'Answered', 'NoEvidence', 'Fehler')),
    duration_ms BIGINT CHECK (duration_ms >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    UNIQUE (broadcaster_user_id, message_id),
    CHECK (char_length(question) <= 500),
    CHECK (char_length(answer) <= 450)
);

CREATE INDEX IF NOT EXISTS tb_chat_brain_answers_channel_time_idx
    ON public.tb_chat_brain_answers (broadcaster_user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS tb_chat_brain_answers_user_time_idx
    ON public.tb_chat_brain_answers (chatter_user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS tb_chat_brain_answers_created_at_idx
    ON public.tb_chat_brain_answers (created_at DESC);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchbot') THEN
        GRANT SELECT, INSERT, UPDATE ON public.tb_chat_brain_answers TO twitchbot;
        GRANT USAGE, SELECT ON SEQUENCE public.tb_chat_brain_answers_id_seq TO twitchbot;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'twitchdash') THEN
        GRANT SELECT ON public.tb_chat_brain_answers TO twitchdash;
    END IF;
END $$;
