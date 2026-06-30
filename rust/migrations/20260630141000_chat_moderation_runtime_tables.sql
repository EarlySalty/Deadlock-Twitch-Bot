-- Chat moderation runtime-created tables now owned by migrations:
-- tb_chat_autoban_log, twitch_outbound_chat_suppressions,
-- twitch_auto_learned_safe_patterns, twitch_auto_learned_spam_patterns.

CREATE TABLE IF NOT EXISTS public.tb_chat_autoban_log (
    id BIGSERIAL NOT NULL,
    channel_login TEXT NOT NULL,
    chatter_id TEXT NOT NULL,
    chatter_login TEXT NOT NULL,
    content TEXT,
    banned_at TIMESTAMPTZ DEFAULT now() NOT NULL,
    CONSTRAINT tb_chat_autoban_log_pkey PRIMARY KEY (id)
);

CREATE TABLE IF NOT EXISTS public.twitch_outbound_chat_suppressions (
    target_login TEXT NOT NULL,
    source TEXT NOT NULL,
    target_id TEXT,
    reason_code TEXT NOT NULL,
    reason_detail TEXT,
    suppressed_until TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ DEFAULT now() NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT now() NOT NULL,
    CONSTRAINT twitch_outbound_chat_suppressions_pkey PRIMARY KEY (target_login, source)
);

CREATE INDEX IF NOT EXISTS idx_twitch_outbound_chat_suppressions_until
    ON public.twitch_outbound_chat_suppressions USING btree (suppressed_until);

CREATE TABLE IF NOT EXISTS public.twitch_auto_learned_safe_patterns (
    pattern TEXT NOT NULL,
    source_message TEXT,
    source_channel TEXT,
    minimax_reasoning TEXT,
    hit_count INTEGER DEFAULT 0 NOT NULL,
    created_at TIMESTAMPTZ DEFAULT now() NOT NULL,
    CONSTRAINT twitch_auto_learned_safe_patterns_pkey PRIMARY KEY (pattern)
);

CREATE TABLE IF NOT EXISTS public.twitch_auto_learned_spam_patterns (
    pattern TEXT NOT NULL,
    pattern_type TEXT DEFAULT 'fragment'::text NOT NULL,
    source_message TEXT,
    source_channel TEXT,
    minimax_reasoning TEXT,
    hit_count INTEGER DEFAULT 0 NOT NULL,
    created_at TIMESTAMPTZ DEFAULT now() NOT NULL,
    CONSTRAINT twitch_auto_learned_spam_patterns_pkey PRIMARY KEY (pattern)
);
