-- Engagement-Layer – Postgres-Schema für AI-Stammgast im Twitch-Chat
-- Spec: /home/naniadm/.claude/plans/ich-m-chte-das-wir-buzzing-pebble.md
-- Idempotent: alle CREATE-Statements nutzen IF NOT EXISTS, Seed nutzt ON CONFLICT.

-- ========= pro Channel: Toggle, Persona-Override, Tabu-Themen, Steam-ID =========
CREATE TABLE IF NOT EXISTS twitch_engagement_settings (
    channel_login        TEXT PRIMARY KEY,
    enabled              BOOLEAN NOT NULL DEFAULT FALSE,
    steam_id             TEXT,
    persona_override     TEXT,
    tabu_topics          TEXT[],
    enabled_at           TIMESTAMPTZ,
    enabled_by           TEXT,
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ========= rolling Multi-Turn-Konversation pro Channel =========
CREATE TABLE IF NOT EXISTS twitch_engagement_conversation (
    id                   BIGSERIAL PRIMARY KEY,
    channel_login        TEXT NOT NULL,
    role                 TEXT NOT NULL,
    twitch_user_id       TEXT,
    twitch_login         TEXT,
    content              TEXT NOT NULL,
    message_id           TEXT,
    ts                   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_eng_conv_channel_ts
    ON twitch_engagement_conversation (channel_login, ts DESC);

-- ========= Aggregate pro Twitch-User =========
CREATE TABLE IF NOT EXISTS twitch_user_profile (
    twitch_user_id       TEXT PRIMARY KEY,
    twitch_login         TEXT NOT NULL,
    first_seen_at        TIMESTAMPTZ NOT NULL,
    last_seen_at         TIMESTAMPTZ NOT NULL,
    message_count        INT NOT NULL DEFAULT 0,
    channels             JSONB NOT NULL DEFAULT '[]'::jsonb,
    tags                 TEXT[] DEFAULT '{}',
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ========= Konversations-Fäden (Beziehungsführung, kein Trivia-Dump) =========
CREATE TABLE IF NOT EXISTS twitch_user_threads (
    id                   BIGSERIAL PRIMARY KEY,
    twitch_user_id       TEXT NOT NULL,
    twitch_login         TEXT NOT NULL,
    channel_login        TEXT,
    thread_type          TEXT NOT NULL,
    summary              TEXT NOT NULL,
    due_at               TIMESTAMPTZ,
    status               TEXT NOT NULL DEFAULT 'open',
    source_message_id    TEXT,
    last_referenced_at   TIMESTAMPTZ,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_threads_user_status
    ON twitch_user_threads (twitch_user_id, status);
CREATE INDEX IF NOT EXISTS idx_threads_due
    ON twitch_user_threads (status, due_at)
    WHERE status IN ('open', 'follow_up_due');

-- ========= Self-Opt-Out pro Chatter =========
CREATE TABLE IF NOT EXISTS twitch_user_engagement_optout (
    twitch_user_id       TEXT PRIMARY KEY,
    opted_out_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ========= Live-Match-State pro Channel =========
CREATE TABLE IF NOT EXISTS twitch_channel_match_state (
    channel_login        TEXT PRIMARY KEY,
    hero_id              INT,
    hero_name            TEXT,
    match_id             TEXT,
    match_started_at     TIMESTAMPTZ,
    last_synced_at       TIMESTAMPTZ NOT NULL,
    is_live              BOOLEAN NOT NULL DEFAULT FALSE
);

-- ========= kurze Voice-to-Text Segmente pro Channel =========
CREATE TABLE IF NOT EXISTS twitch_engagement_stream_transcripts (
    id                   BIGSERIAL PRIMARY KEY,
    channel_login        TEXT NOT NULL,
    started_at           TIMESTAMPTZ NOT NULL,
    ended_at             TIMESTAMPTZ NOT NULL,
    text                 TEXT NOT NULL,
    engine               TEXT NOT NULL,
    model                TEXT,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_eng_stream_transcripts_channel_ended
    ON twitch_engagement_stream_transcripts (channel_login, ended_at DESC);

-- ========= Engagement-Log (rein informativ, kein Budget-Gating) =========
CREATE TABLE IF NOT EXISTS twitch_engagement_log (
    id                    BIGSERIAL PRIMARY KEY,
    channel_login         TEXT NOT NULL,
    triggered_by_msg_id   TEXT,
    decision              TEXT NOT NULL,
    response_text         TEXT,
    referenced_thread_ids BIGINT[],
    model                 TEXT NOT NULL,
    prompt_tokens         INT,
    completion_tokens     INT,
    cost_usd_estimate     NUMERIC(10, 6),
    latency_ms            INT,
    ts                    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_eng_log_channel_ts
    ON twitch_engagement_log (channel_login, ts DESC);

-- ========= Admin-Rollen (Super-Mod kann in jedem Channel toggeln) =========
CREATE TABLE IF NOT EXISTS twitch_admin_roles (
    twitch_user_id       TEXT NOT NULL,
    role                 TEXT NOT NULL,
    granted_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (twitch_user_id, role)
);

-- ========= Seed: EarlySalty als super_mod =========
-- Lookup via twitch_streamers (Login ist case-insensitive). No-op falls Streamer
-- noch nicht registriert ist; in dem Fall manuell INSERTen sobald twitch_user_id
-- bekannt ist.
INSERT INTO twitch_admin_roles (twitch_user_id, role)
SELECT s.twitch_user_id, 'super_mod'
FROM twitch_streamers s
WHERE LOWER(s.twitch_login) = 'earlysalty'
  AND s.twitch_user_id IS NOT NULL
ON CONFLICT (twitch_user_id, role) DO NOTHING;
