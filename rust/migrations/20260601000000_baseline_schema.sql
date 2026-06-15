-- F1 baseline: vollständiges Ziel-Schema (storage pg.py ensure_schema +
-- billing + social_media phase0-4 + engagement + exp + viewer_presence).
-- Generiert aus dem materialisierten Oracle (pg_dump), idempotent gemacht:
-- CREATE ... IF NOT EXISTS, CREATE OR REPLACE, guarded constraints/trigger.
-- Re-runnbar; auf bestehendem Prod-Schema ein No-op.

CREATE OR REPLACE FUNCTION public.social_media_set_retention_until() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
        BEGIN
            IF NEW.created_at IS NULL OR BTRIM(NEW.created_at::text) = '' THEN
                RETURN NEW;
            END IF;
            NEW.retention_until := (NEW.created_at::timestamptz + INTERVAL '14 days');
            RETURN NEW;
        END;
        $$;

CREATE OR REPLACE FUNCTION public.sync_twitch_streamer_identity_from_partners() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
        BEGIN
            IF NEW.status = 'active' AND COALESCE(NEW.twitch_user_id, '') <> '' THEN
                INSERT INTO twitch_streamer_identities (
                    twitch_user_id,
                    twitch_login,
                    discord_user_id,
                    discord_display_name,
                    is_on_discord,
                    created_at,
                    updated_at
                ) VALUES (
                    NEW.twitch_user_id,
                    LOWER(NEW.twitch_login),
                    (SELECT discord_user_id FROM twitch_streamer_identities WHERE twitch_user_id = NEW.twitch_user_id),
                    (SELECT discord_display_name FROM twitch_streamer_identities WHERE twitch_user_id = NEW.twitch_user_id),
                    COALESCE((SELECT is_on_discord FROM twitch_streamer_identities WHERE twitch_user_id = NEW.twitch_user_id), 0),
                    COALESCE((SELECT created_at FROM twitch_streamer_identities WHERE twitch_user_id = NEW.twitch_user_id), CURRENT_TIMESTAMP::text),
                    CURRENT_TIMESTAMP::text
                )
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                    twitch_login = EXCLUDED.twitch_login,
                    updated_at = CURRENT_TIMESTAMP::text;
            END IF;
            RETURN NEW;
        END;
        $$;

CREATE OR REPLACE FUNCTION public.sync_twitch_streamer_identity_from_streamers() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
        BEGIN
            IF COALESCE(NEW.twitch_user_id, '') <> '' THEN
                INSERT INTO twitch_streamer_identities (
                    twitch_user_id,
                    twitch_login,
                    discord_user_id,
                    discord_display_name,
                    is_on_discord,
                    created_at,
                    updated_at
                ) VALUES (
                    NEW.twitch_user_id,
                    LOWER(NEW.twitch_login),
                    NEW.discord_user_id,
                    NEW.discord_display_name,
                    COALESCE(NEW.is_on_discord, 0),
                    CURRENT_TIMESTAMP::text,
                    CURRENT_TIMESTAMP::text
                )
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                    twitch_login = EXCLUDED.twitch_login,
                    discord_user_id = COALESCE(EXCLUDED.discord_user_id, twitch_streamer_identities.discord_user_id),
                    discord_display_name = COALESCE(EXCLUDED.discord_display_name, twitch_streamer_identities.discord_display_name),
                    is_on_discord = COALESCE(EXCLUDED.is_on_discord, twitch_streamer_identities.is_on_discord),
                    updated_at = CURRENT_TIMESTAMP::text;
            END IF;
            RETURN NEW;
        END;
        $$;

CREATE TABLE IF NOT EXISTS public.clip_fetch_history (
    id integer NOT NULL,
    streamer_login text NOT NULL,
    fetched_at text DEFAULT CURRENT_TIMESTAMP NOT NULL,
    clips_found integer DEFAULT 0,
    clips_new integer DEFAULT 0,
    fetch_duration_ms integer,
    error text
);

CREATE SEQUENCE IF NOT EXISTS public.clip_fetch_history_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.clip_fetch_history_id_seq OWNED BY public.clip_fetch_history.id;

CREATE TABLE IF NOT EXISTS public.clip_last_hashtags (
    streamer_login text NOT NULL,
    hashtags text NOT NULL,
    last_used_at text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.clip_templates_global (
    id integer NOT NULL,
    template_name text NOT NULL,
    description_template text NOT NULL,
    hashtags text NOT NULL,
    category text,
    usage_count integer DEFAULT 0,
    created_at text DEFAULT CURRENT_TIMESTAMP,
    created_by text
);

CREATE SEQUENCE IF NOT EXISTS public.clip_templates_global_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.clip_templates_global_id_seq OWNED BY public.clip_templates_global.id;

CREATE TABLE IF NOT EXISTS public.clip_templates_streamer (
    id integer NOT NULL,
    streamer_login text NOT NULL,
    template_name text NOT NULL,
    description_template text NOT NULL,
    hashtags text NOT NULL,
    is_default integer DEFAULT 0,
    created_at text DEFAULT CURRENT_TIMESTAMP,
    updated_at text DEFAULT CURRENT_TIMESTAMP
);

CREATE SEQUENCE IF NOT EXISTS public.clip_templates_streamer_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.clip_templates_streamer_id_seq OWNED BY public.clip_templates_streamer.id;

CREATE TABLE IF NOT EXISTS public.dashboard_sessions (
    session_id text NOT NULL,
    session_type text DEFAULT 'twitch'::text NOT NULL,
    payload_enc bytea NOT NULL,
    created_at double precision NOT NULL,
    expires_at double precision NOT NULL
);

CREATE TABLE IF NOT EXISTS public.deadlock_vocab (
    term text NOT NULL,
    canonical text NOT NULL,
    category text NOT NULL,
    source text DEFAULT 'manual'::text NOT NULL,
    aliases jsonb DEFAULT '[]'::jsonb NOT NULL,
    weight integer DEFAULT 1 NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT deadlock_vocab_category_chk CHECK ((category = ANY (ARRAY['hero'::text, 'item'::text, 'ability'::text, 'slang'::text]))),
    CONSTRAINT deadlock_vocab_source_chk CHECK ((source = ANY (ARRAY['deadlock_api'::text, 'manual'::text])))
);

CREATE TABLE IF NOT EXISTS public.discord_invite_codes (
    guild_id bigint NOT NULL,
    invite_code text NOT NULL,
    created_at text DEFAULT CURRENT_TIMESTAMP,
    last_seen_at text DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS public.eventsub_guard_state (
    kind text NOT NULL,
    guard_key text NOT NULL,
    expires_at double precision NOT NULL,
    updated_at double precision NOT NULL
);

CREATE TABLE IF NOT EXISTS public.exp_game_transitions (
    id bigint NOT NULL,
    exp_session_id bigint NOT NULL,
    streamer text NOT NULL,
    ts_utc text NOT NULL,
    from_game text,
    to_game text,
    viewer_count integer
);

CREATE SEQUENCE IF NOT EXISTS public.exp_game_transitions_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.exp_game_transitions_id_seq OWNED BY public.exp_game_transitions.id;

CREATE TABLE IF NOT EXISTS public.exp_sessions (
    id bigint NOT NULL,
    streamer text NOT NULL,
    stream_id text,
    started_at text NOT NULL,
    ended_at text,
    game_name text,
    stream_title text,
    peak_viewers integer DEFAULT 0,
    avg_viewers real DEFAULT 0,
    samples integer DEFAULT 0,
    follower_delta integer,
    duration_min real
);

CREATE SEQUENCE IF NOT EXISTS public.exp_sessions_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.exp_sessions_id_seq OWNED BY public.exp_sessions.id;

CREATE TABLE IF NOT EXISTS public.exp_snapshots (
    id bigint NOT NULL,
    exp_session_id bigint NOT NULL,
    ts_utc text NOT NULL,
    viewer_count integer,
    minutes_from_start real
);

CREATE SEQUENCE IF NOT EXISTS public.exp_snapshots_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.exp_snapshots_id_seq OWNED BY public.exp_snapshots.id;

CREATE TABLE IF NOT EXISTS public.oauth_state_tokens (
    state_token text NOT NULL,
    platform text NOT NULL,
    streamer_login text,
    redirect_uri text,
    pkce_verifier text,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP,
    expires_at timestamp with time zone NOT NULL,
    consumed_at timestamp with time zone
);

CREATE TABLE IF NOT EXISTS public.social_media_clip_approval (
    clip_db_id integer NOT NULL,
    state text DEFAULT 'awaiting_approval'::text NOT NULL,
    approved_platforms jsonb DEFAULT '[]'::jsonb NOT NULL,
    approver_user_id text,
    decided_at timestamp with time zone,
    dm_message_id text,
    dm_channel_id text,
    last_sent_at timestamp with time zone,
    CONSTRAINT social_media_clip_approval_state_chk CHECK ((state = ANY (ARRAY['awaiting_approval'::text, 'approved'::text, 'skipped'::text, 'editing'::text])))
);

CREATE TABLE IF NOT EXISTS public.social_media_clip_enrichment (
    clip_db_id integer NOT NULL,
    transcript_raw text,
    transcript_corrected text,
    transcript_segments jsonb,
    transcript_lang text,
    detected_terms jsonb DEFAULT '[]'::jsonb NOT NULL,
    title_youtube text,
    title_tiktok text,
    title_instagram text,
    description_youtube text,
    description_tiktok text,
    description_instagram text,
    hashtags_youtube jsonb DEFAULT '[]'::jsonb NOT NULL,
    hashtags_tiktok jsonb DEFAULT '[]'::jsonb NOT NULL,
    hashtags_instagram jsonb DEFAULT '[]'::jsonb NOT NULL,
    llm_provider text,
    llm_model text,
    cost_usd_estimate numeric(10,6),
    status text DEFAULT 'pending'::text NOT NULL,
    error_message text,
    started_at timestamp with time zone,
    completed_at timestamp with time zone,
    edited_by text,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT social_media_clip_enrichment_status_chk CHECK ((status = ANY (ARRAY['pending'::text, 'transcribing'::text, 'correcting'::text, 'llm'::text, 'done'::text, 'failed'::text, 'skipped_no_key'::text])))
);

CREATE TABLE IF NOT EXISTS public.social_media_platform_auth (
    id integer NOT NULL,
    platform text NOT NULL,
    streamer_login text,
    access_token_enc bytea NOT NULL,
    refresh_token_enc bytea,
    client_id text,
    client_secret_enc bytea,
    token_expires_at text,
    scopes text,
    platform_user_id text,
    platform_username text,
    enc_version integer DEFAULT 1,
    enc_kid text DEFAULT 'v1'::text,
    authorized_at text DEFAULT CURRENT_TIMESTAMP,
    last_refreshed_at text,
    enabled integer DEFAULT 1
);

DO $do$ BEGIN
    ALTER TABLE public.social_media_platform_auth ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.social_media_platform_auth_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);
EXCEPTION WHEN others THEN NULL;
END $do$;

CREATE TABLE IF NOT EXISTS public.social_media_reauth_notifications (
    streamer_login text NOT NULL,
    platform text NOT NULL,
    error_kind text NOT NULL,
    last_sent_at timestamp with time zone NOT NULL
);

CREATE TABLE IF NOT EXISTS public.social_media_reports (
    id integer NOT NULL,
    kind text NOT NULL,
    streamer_login text,
    period_start timestamp with time zone NOT NULL,
    period_end timestamp with time zone NOT NULL,
    content_md text NOT NULL,
    model text,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.social_media_reports_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.social_media_reports_id_seq OWNED BY public.social_media_reports.id;

CREATE TABLE IF NOT EXISTS public.social_media_settings (
    key text NOT NULL,
    value jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_by text
);

CREATE TABLE IF NOT EXISTS public.social_media_streamer_layout (
    streamer_login text NOT NULL,
    layout_json jsonb NOT NULL,
    cam_enabled boolean DEFAULT true NOT NULL,
    mode text DEFAULT 'pip'::text NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_by text,
    CONSTRAINT social_media_layout_mode_chk CHECK ((mode = ANY (ARRAY['pip'::text, 'stacked'::text])))
);

CREATE TABLE IF NOT EXISTS public.streamer_plans (
    twitch_user_id text NOT NULL,
    twitch_login text,
    plan_name text DEFAULT 'free'::text NOT NULL,
    promo_disabled integer DEFAULT 0 NOT NULL,
    activated_at text DEFAULT CURRENT_TIMESTAMP NOT NULL,
    expires_at text,
    notes text,
    raid_boost_enabled integer DEFAULT 0 NOT NULL,
    lurker_tax_enabled integer DEFAULT 0 NOT NULL,
    promo_message text,
    manual_plan_id text,
    manual_plan_expires_at text,
    manual_plan_notes text DEFAULT ''::text NOT NULL,
    manual_plan_updated_at text,
    trial_ever_granted integer DEFAULT 0 NOT NULL,
    first_login_at text
);

CREATE TABLE IF NOT EXISTS public.twitch_ad_break_events (
    id integer NOT NULL,
    session_id integer,
    twitch_user_id text NOT NULL,
    duration_seconds integer,
    is_automatic integer DEFAULT 0,
    started_at text DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_ad_break_events_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_ad_break_events_id_seq OWNED BY public.twitch_ad_break_events.id;

CREATE TABLE IF NOT EXISTS public.twitch_admin_roles (
    twitch_user_id text NOT NULL,
    role text NOT NULL,
    granted_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_ads_schedule_snapshot (
    id integer NOT NULL,
    twitch_user_id text NOT NULL,
    twitch_login text,
    next_ad_at text,
    last_ad_at text,
    duration integer,
    preroll_free_time integer,
    snooze_count integer,
    snooze_refresh_at text,
    snapshot_at text DEFAULT CURRENT_TIMESTAMP
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_ads_schedule_snapshot_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_ads_schedule_snapshot_id_seq OWNED BY public.twitch_ads_schedule_snapshot.id;

CREATE TABLE IF NOT EXISTS public.twitch_auto_raid_pause (
    twitch_user_id text NOT NULL,
    twitch_login text,
    paused_until timestamp with time zone NOT NULL,
    reason text,
    set_by text,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_ban_events (
    id integer NOT NULL,
    session_id integer,
    twitch_user_id text NOT NULL,
    event_type text NOT NULL,
    target_login text,
    target_id text,
    moderator_login text,
    reason text,
    ends_at text,
    received_at text DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_ban_events_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_ban_events_id_seq OWNED BY public.twitch_ban_events.id;

CREATE TABLE IF NOT EXISTS public.twitch_billing_subscriptions (
    stripe_subscription_id text NOT NULL,
    stripe_customer_id text,
    customer_reference text,
    status text DEFAULT 'unknown'::text NOT NULL,
    plan_id text,
    cycle_months integer DEFAULT 1 NOT NULL,
    quantity integer DEFAULT 1 NOT NULL,
    current_period_start text,
    current_period_end text,
    cancel_at_period_end integer DEFAULT 0 NOT NULL,
    canceled_at text,
    ended_at text,
    last_event_id text,
    updated_at text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_bits_events (
    id integer NOT NULL,
    session_id integer,
    twitch_user_id text NOT NULL,
    donor_login text,
    amount integer NOT NULL,
    message text,
    received_at text DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_bits_events_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_bits_events_id_seq OWNED BY public.twitch_bits_events.id;

CREATE TABLE IF NOT EXISTS public.twitch_channel_match_state (
    channel_login text NOT NULL,
    hero_id integer,
    hero_name text,
    match_id text,
    match_started_at timestamp with time zone,
    last_synced_at timestamp with time zone NOT NULL,
    is_live boolean DEFAULT false NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_channel_points_events (
    id integer NOT NULL,
    session_id integer,
    twitch_user_id text NOT NULL,
    user_login text,
    reward_id text,
    reward_title text,
    reward_cost integer,
    user_input text,
    redeemed_at text NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_channel_points_events_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_channel_points_events_id_seq OWNED BY public.twitch_channel_points_events.id;

CREATE TABLE IF NOT EXISTS public.twitch_channel_updates (
    id integer NOT NULL,
    twitch_user_id text NOT NULL,
    title text,
    game_name text,
    language text,
    recorded_at text DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_channel_updates_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_channel_updates_id_seq OWNED BY public.twitch_channel_updates.id;

CREATE TABLE IF NOT EXISTS public.twitch_chat_messages (
    id integer NOT NULL,
    session_id integer NOT NULL,
    streamer_login text NOT NULL,
    chatter_login text,
    chatter_id text,
    message_id text,
    message_ts text NOT NULL,
    is_command boolean DEFAULT false,
    content text
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_chat_messages_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_chat_messages_id_seq OWNED BY public.twitch_chat_messages.id;

CREATE TABLE IF NOT EXISTS public.twitch_chatter_global_ban (
    chatter_login text NOT NULL,
    chatter_id text,
    reason text,
    added_by text,
    added_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_chatter_global_ban_applied (
    chatter_login text NOT NULL,
    broadcaster_id text NOT NULL,
    applied_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_chatter_rollup (
    streamer_login text NOT NULL,
    chatter_login text NOT NULL,
    chatter_id text,
    first_seen_at text NOT NULL,
    last_seen_at text NOT NULL,
    total_messages integer DEFAULT 0,
    total_sessions integer DEFAULT 0
);

CREATE TABLE IF NOT EXISTS public.twitch_clips_social_analytics (
    id integer NOT NULL,
    clip_id integer NOT NULL,
    platform text NOT NULL,
    platform_video_id text,
    views integer DEFAULT 0,
    likes integer DEFAULT 0,
    comments integer DEFAULT 0,
    shares integer DEFAULT 0,
    saves integer DEFAULT 0,
    watch_time_avg real,
    completion_rate real,
    ctr real,
    engagement_rate real,
    external_clicks integer DEFAULT 0,
    new_followers integer DEFAULT 0,
    synced_at text NOT NULL,
    posted_at text,
    bucket text,
    watch_time_seconds integer,
    ctr_percent numeric(5,2),
    provider text,
    next_pull_at timestamp with time zone
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_clips_social_analytics_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_clips_social_analytics_id_seq OWNED BY public.twitch_clips_social_analytics.id;

CREATE TABLE IF NOT EXISTS public.twitch_clips_social_media (
    id integer NOT NULL,
    clip_id text NOT NULL,
    clip_url text NOT NULL,
    clip_title text,
    clip_thumbnail_url text,
    streamer_login text NOT NULL,
    twitch_user_id text,
    created_at text NOT NULL,
    duration_seconds real,
    view_count integer DEFAULT 0,
    game_name text,
    status text DEFAULT 'pending'::text,
    downloaded_at text,
    local_file_path text,
    converted_file_path text,
    uploaded_tiktok integer DEFAULT 0,
    uploaded_youtube integer DEFAULT 0,
    uploaded_instagram integer DEFAULT 0,
    tiktok_video_id text,
    youtube_video_id text,
    instagram_media_id text,
    tiktok_uploaded_at text,
    youtube_uploaded_at text,
    instagram_uploaded_at text,
    custom_title text,
    custom_description text,
    hashtags text,
    music_track text,
    last_analytics_sync text,
    layout_override_json jsonb,
    source_kind text DEFAULT 'twitch'::text NOT NULL,
    upload_local_path text,
    retention_until timestamp with time zone,
    discarded_at timestamp with time zone,
    CONSTRAINT twitch_clips_source_kind_chk CHECK ((source_kind = ANY (ARRAY['twitch'::text, 'manual_upload'::text])))
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_clips_social_media_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_clips_social_media_id_seq OWNED BY public.twitch_clips_social_media.id;

CREATE TABLE IF NOT EXISTS public.twitch_clips_upload_queue (
    id integer NOT NULL,
    clip_id integer NOT NULL,
    platform text NOT NULL,
    status text DEFAULT 'pending'::text,
    priority integer DEFAULT 0,
    title text,
    description text,
    hashtags text,
    scheduled_at text,
    attempts integer DEFAULT 0,
    last_error text,
    last_attempt_at text,
    created_at text DEFAULT CURRENT_TIMESTAMP NOT NULL,
    completed_at text
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_clips_upload_queue_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_clips_upload_queue_id_seq OWNED BY public.twitch_clips_upload_queue.id;

CREATE TABLE IF NOT EXISTS public.twitch_confirmed_external_recruitment_raids (
    id bigint NOT NULL,
    raid_flow_id text,
    from_broadcaster_id text,
    from_broadcaster_login text NOT NULL,
    to_broadcaster_id text NOT NULL,
    to_broadcaster_login text NOT NULL,
    viewer_count integer DEFAULT 0,
    confirmation_signal text,
    confirmed_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_confirmed_external_recruitment_raids_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_confirmed_external_recruitment_raids_id_seq OWNED BY public.twitch_confirmed_external_recruitment_raids.id;

CREATE TABLE IF NOT EXISTS public.twitch_engagement_conversation (
    id bigint NOT NULL,
    channel_login text NOT NULL,
    role text NOT NULL,
    twitch_user_id text,
    twitch_login text,
    content text NOT NULL,
    message_id text,
    ts timestamp with time zone DEFAULT now() NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_engagement_conversation_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_engagement_conversation_id_seq OWNED BY public.twitch_engagement_conversation.id;

CREATE TABLE IF NOT EXISTS public.twitch_engagement_log (
    id bigint NOT NULL,
    channel_login text NOT NULL,
    triggered_by_msg_id text,
    decision text NOT NULL,
    response_text text,
    referenced_thread_ids bigint[],
    model text NOT NULL,
    prompt_tokens integer,
    completion_tokens integer,
    cost_usd_estimate numeric(10,6),
    latency_ms integer,
    ts timestamp with time zone DEFAULT now() NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_engagement_log_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_engagement_log_id_seq OWNED BY public.twitch_engagement_log.id;

CREATE TABLE IF NOT EXISTS public.twitch_engagement_settings (
    channel_login text NOT NULL,
    enabled boolean DEFAULT false NOT NULL,
    steam_id text,
    persona_override text,
    tabu_topics text[],
    enabled_at timestamp with time zone,
    enabled_by text,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_engagement_stream_transcripts (
    id bigint NOT NULL,
    channel_login text NOT NULL,
    started_at timestamp with time zone NOT NULL,
    ended_at timestamp with time zone NOT NULL,
    text text NOT NULL,
    engine text NOT NULL,
    model text,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_engagement_stream_transcripts_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_engagement_stream_transcripts_id_seq OWNED BY public.twitch_engagement_stream_transcripts.id;

CREATE TABLE IF NOT EXISTS public.twitch_eventsub_bridge_dead_letter (
    message_id text NOT NULL,
    sub_type text NOT NULL,
    payload_json text NOT NULL,
    queued_at double precision NOT NULL,
    dead_lettered_at double precision NOT NULL,
    attempt_count integer NOT NULL,
    last_error text
);

CREATE TABLE IF NOT EXISTS public.twitch_eventsub_bridge_outbox (
    message_id text NOT NULL,
    sub_type text NOT NULL,
    payload_json text NOT NULL,
    queued_at double precision NOT NULL,
    next_attempt_at double precision NOT NULL,
    attempt_count integer DEFAULT 0 NOT NULL,
    last_error text
);

CREATE TABLE IF NOT EXISTS public.twitch_eventsub_capacity_snapshot (
    id integer NOT NULL,
    ts_utc text DEFAULT CURRENT_TIMESTAMP,
    trigger_reason text,
    listener_count integer DEFAULT 0,
    ready_listeners integer DEFAULT 0,
    failed_listeners integer DEFAULT 0,
    used_slots integer DEFAULT 0,
    total_slots integer DEFAULT 0,
    headroom_slots integer DEFAULT 0,
    listeners_at_limit integer DEFAULT 0,
    utilization_pct real DEFAULT 0,
    listeners_json text
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_eventsub_capacity_snapshot_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_eventsub_capacity_snapshot_id_seq OWNED BY public.twitch_eventsub_capacity_snapshot.id;

CREATE TABLE IF NOT EXISTS public.twitch_eventsub_processing_dead_letter (
    work_id text NOT NULL,
    work_type text NOT NULL,
    message_id text,
    payload_json text NOT NULL,
    queued_at double precision NOT NULL,
    dead_lettered_at double precision NOT NULL,
    attempt_count integer NOT NULL,
    last_error text
);

CREATE TABLE IF NOT EXISTS public.twitch_eventsub_processing_inbox (
    work_id text NOT NULL,
    work_type text NOT NULL,
    message_id text,
    payload_json text NOT NULL,
    queued_at double precision NOT NULL,
    next_attempt_at double precision NOT NULL,
    attempt_count integer DEFAULT 0 NOT NULL,
    last_error text
);

CREATE TABLE IF NOT EXISTS public.twitch_external_bot_ban_check_pending (
    target_id text NOT NULL,
    target_login text NOT NULL,
    source text NOT NULL,
    run_after timestamp with time zone NOT NULL,
    scheduled_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_external_recruitment_blacklist_pending (
    target_id text NOT NULL,
    target_login text NOT NULL,
    confirmed_raid_count integer NOT NULL,
    threshold_reached_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    blacklist_after timestamp with time zone NOT NULL,
    last_raid_flow_id text,
    updated_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_first_message_events (
    id bigint NOT NULL,
    streamer_login text NOT NULL,
    broadcaster_id text NOT NULL,
    chatter_login text NOT NULL,
    chatter_id text,
    message_id text,
    message_text text,
    event_ts timestamp with time zone DEFAULT now() NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_first_message_events_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_first_message_events_id_seq OWNED BY public.twitch_first_message_events.id;

CREATE TABLE IF NOT EXISTS public.twitch_follow_events (
    id integer NOT NULL,
    streamer_login text NOT NULL,
    twitch_user_id text NOT NULL,
    follower_login text NOT NULL,
    follower_id text,
    followed_at text DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_follow_events_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_follow_events_id_seq OWNED BY public.twitch_follow_events.id;

CREATE TABLE IF NOT EXISTS public.twitch_global_ban_sweep_due (
    broadcaster_login text NOT NULL,
    broadcaster_id text NOT NULL,
    run_after timestamp with time zone NOT NULL,
    scheduled_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_global_promo_modes (
    config_key text NOT NULL,
    mode text DEFAULT 'standard'::text NOT NULL,
    custom_message text,
    starts_at text,
    ends_at text,
    is_enabled integer DEFAULT 0 NOT NULL,
    updated_at text DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_by text
);

CREATE TABLE IF NOT EXISTS public.twitch_global_settings (
    setting_key text NOT NULL,
    setting_value text NOT NULL,
    updated_at text DEFAULT CURRENT_TIMESTAMP NOT NULL,
    updated_by text
);

CREATE TABLE IF NOT EXISTS public.twitch_hype_train_events (
    id integer NOT NULL,
    session_id integer,
    twitch_user_id text NOT NULL,
    started_at text,
    ended_at text,
    duration_seconds integer,
    level integer,
    total_progress integer,
    event_phase text DEFAULT 'end'::text
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_hype_train_events_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_hype_train_events_id_seq OWNED BY public.twitch_hype_train_events.id;

CREATE TABLE IF NOT EXISTS public.twitch_link_clicks (
    id integer NOT NULL,
    clicked_at text DEFAULT CURRENT_TIMESTAMP,
    streamer_login text NOT NULL,
    tracking_token text,
    discord_user_id text,
    discord_username text,
    guild_id text,
    channel_id text,
    message_id text,
    ref_code text,
    source_hint text
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_link_clicks_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_link_clicks_id_seq OWNED BY public.twitch_link_clicks.id;

CREATE TABLE IF NOT EXISTS public.twitch_live_announcement_configs (
    streamer_login text NOT NULL,
    config_json text NOT NULL,
    allowed_editor_role_ids text,
    updated_at text DEFAULT CURRENT_TIMESTAMP,
    updated_by text
);

CREATE TABLE IF NOT EXISTS public.twitch_live_state (
    twitch_user_id text NOT NULL,
    streamer_login text NOT NULL,
    last_stream_id text,
    last_started_at text,
    last_title text,
    last_game_id text,
    last_discord_message_id text,
    last_notified_at text,
    is_live integer DEFAULT 0,
    last_seen_at text,
    last_game text,
    last_viewer_count integer DEFAULT 0,
    last_tracking_token text,
    active_session_id integer,
    had_deadlock_in_session integer DEFAULT 0,
    last_deadlock_seen_at text
);

CREATE TABLE IF NOT EXISTS public.twitch_observability_events (
    id bigint NOT NULL,
    flow_type text NOT NULL,
    flow_id text NOT NULL,
    entity_login text,
    entity_id text,
    step text NOT NULL,
    decision text NOT NULL,
    details_json text DEFAULT '{}'::text NOT NULL,
    created_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_observability_events_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_observability_events_id_seq OWNED BY public.twitch_observability_events.id;

CREATE TABLE IF NOT EXISTS public.twitch_partner_outreach (
    streamer_login text NOT NULL,
    streamer_user_id text,
    detected_at text NOT NULL,
    contacted_at text,
    status text DEFAULT 'pending'::text,
    cooldown_until text,
    notes text,
    raid_used_at text,
    conversation_status text
);

CREATE TABLE IF NOT EXISTS public.twitch_partner_outreach_audit (
    id bigint NOT NULL,
    streamer_login text NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() NOT NULL,
    event_kind text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    correlation_id uuid
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_partner_outreach_audit_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_partner_outreach_audit_id_seq OWNED BY public.twitch_partner_outreach_audit.id;

CREATE TABLE IF NOT EXISTS public.twitch_partner_outreach_conversations (
    streamer_login text NOT NULL,
    streamer_user_id text,
    source text NOT NULL,
    state text DEFAULT 'open'::text NOT NULL,
    messages_json jsonb DEFAULT '[]'::jsonb NOT NULL,
    last_voice_capture_at timestamp with time zone,
    last_brain_call_at timestamp with time zone,
    last_bot_message_at timestamp with time zone,
    last_streamer_signal_at timestamp with time zone,
    last_stance text,
    last_confidence real,
    human_notify_sent_at timestamp with time zone,
    closed_at timestamp with time zone,
    error_kind text,
    error_detail text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    human_notify_pending_at timestamp with time zone
);

CREATE TABLE IF NOT EXISTS public.twitch_partner_raid_score_tracking (
    id integer NOT NULL,
    raid_history_id bigint,
    raid_history_executed_at timestamp with time zone,
    from_broadcaster_id text,
    from_broadcaster_login text NOT NULL,
    to_broadcaster_id text NOT NULL,
    to_broadcaster_login text NOT NULL,
    viewer_count integer DEFAULT 0 NOT NULL,
    confirmed_at text DEFAULT CURRENT_TIMESTAMP NOT NULL,
    target_session_id integer,
    target_stream_started_at text,
    score_last_computed_at text,
    final_score double precision,
    base_score double precision,
    duration_score double precision,
    time_pattern_score double precision,
    readiness_score double precision,
    fairness_score double precision,
    new_partner_multiplier double precision,
    raid_boost_multiplier double precision,
    today_received_raids integer DEFAULT 0 NOT NULL,
    was_deadlock_at_raid integer DEFAULT 0 NOT NULL,
    deadlock_continued_until text,
    deadlock_continued_sec integer,
    resolved_at text,
    resolution_reason text
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_partner_raid_score_tracking_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_partner_raid_score_tracking_id_seq OWNED BY public.twitch_partner_raid_score_tracking.id;

CREATE TABLE IF NOT EXISTS public.twitch_partner_raid_scores (
    twitch_user_id text NOT NULL,
    twitch_login text NOT NULL,
    avg_duration_sec integer DEFAULT 0 NOT NULL,
    time_pattern_score_base double precision DEFAULT 0.5 NOT NULL,
    received_successful_raids_total integer DEFAULT 0 NOT NULL,
    is_new_partner_preferred integer DEFAULT 1 NOT NULL,
    new_partner_multiplier double precision DEFAULT 1.0 NOT NULL,
    raid_boost_multiplier double precision DEFAULT 1.0 NOT NULL,
    is_live integer DEFAULT 0 NOT NULL,
    current_started_at text,
    current_uptime_sec integer DEFAULT 0 NOT NULL,
    duration_score double precision DEFAULT 0.5 NOT NULL,
    time_pattern_score double precision DEFAULT 0.5 NOT NULL,
    readiness_score double precision DEFAULT 0.5 NOT NULL,
    fairness_score double precision DEFAULT 0.5 NOT NULL,
    base_score double precision DEFAULT 0.5 NOT NULL,
    final_score double precision DEFAULT 0.5 NOT NULL,
    internal_sent_raids_30d integer DEFAULT 0 NOT NULL,
    internal_received_raids_30d integer DEFAULT 0 NOT NULL,
    internal_received_raids_7d integer DEFAULT 0 NOT NULL,
    today_received_raids integer DEFAULT 0 NOT NULL,
    last_computed_at text DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_partners (
    id bigint NOT NULL,
    twitch_user_id text NOT NULL,
    twitch_login text NOT NULL,
    require_discord_link integer DEFAULT 0,
    last_description text,
    last_link_ok integer,
    added_by text,
    last_link_checked_at text,
    next_link_check_at text,
    manual_verified_permanent integer DEFAULT 0,
    manual_verified_until text,
    manual_verified_at text,
    manual_partner_opt_out integer DEFAULT 0,
    raid_bot_enabled integer DEFAULT 0,
    silent_ban integer DEFAULT 0,
    silent_raid integer DEFAULT 0,
    live_ping_role_id bigint,
    live_ping_enabled integer DEFAULT 1,
    partnered_at text DEFAULT CURRENT_TIMESTAMP,
    admin_archived_at text,
    departnered_at text,
    technical_pause_reason text,
    status text DEFAULT 'active'::text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_streamer_identities (
    twitch_user_id text NOT NULL,
    twitch_login text NOT NULL,
    discord_user_id text,
    discord_display_name text,
    is_on_discord integer DEFAULT 0,
    created_at text DEFAULT CURRENT_TIMESTAMP,
    updated_at text DEFAULT CURRENT_TIMESTAMP
);

CREATE OR REPLACE VIEW public.twitch_partners_all_state AS
 SELECT p.id,
    p.twitch_login,
    p.twitch_user_id,
    p.require_discord_link,
    p.next_link_check_at,
    i.discord_user_id,
    i.discord_display_name,
    COALESCE(i.is_on_discord, 0) AS is_on_discord,
    p.manual_verified_permanent,
    p.manual_verified_until,
    p.manual_verified_at,
    p.manual_partner_opt_out,
    p.partnered_at AS created_at,
    COALESCE(p.admin_archived_at,
        CASE
            WHEN (p.status = 'archived'::text) THEN p.departnered_at
            ELSE NULL::text
        END) AS archived_at,
    p.raid_bot_enabled,
    p.silent_ban,
    p.silent_raid,
    0 AS is_monitored_only,
        CASE
            WHEN ((COALESCE(p.manual_verified_permanent, 0) = 1) OR ((p.manual_verified_until IS NOT NULL) AND ((p.manual_verified_until)::timestamp with time zone >= now())) OR (p.manual_verified_at IS NOT NULL)) THEN 1
            ELSE 0
        END AS is_verified,
    1 AS is_partner,
        CASE
            WHEN ((p.status = 'active'::text) AND (COALESCE(p.manual_partner_opt_out, 0) = 0) AND (COALESCE(p.technical_pause_reason, ''::text) = ''::text)) THEN 1
            ELSE 0
        END AS is_partner_active,
    p.live_ping_role_id,
    COALESCE(p.live_ping_enabled, 1) AS live_ping_enabled,
    p.status,
    p.departnered_at,
    p.technical_pause_reason,
        CASE
            WHEN (p.status <> 'active'::text) THEN 'inactive'::text
            WHEN (COALESCE(p.technical_pause_reason, ''::text) = 'blocked'::text) THEN 'blocked'::text
            WHEN (COALESCE(p.manual_partner_opt_out, 0) = 1) THEN 'admin_non_partner'::text
            WHEN (COALESCE(p.technical_pause_reason, ''::text) <> ''::text) THEN p.technical_pause_reason
            ELSE 'active'::text
        END AS operational_state
   FROM (public.twitch_partners p
     LEFT JOIN public.twitch_streamer_identities i ON ((i.twitch_user_id = p.twitch_user_id)));

CREATE SEQUENCE IF NOT EXISTS public.twitch_partners_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_partners_id_seq OWNED BY public.twitch_partners.id;

CREATE TABLE IF NOT EXISTS public.twitch_promo_cooldowns (
    login text NOT NULL,
    cooldown_type text NOT NULL,
    wall_ts double precision NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_raid_arrival_tracking (
    id integer NOT NULL,
    detected_at timestamp with time zone DEFAULT now() NOT NULL,
    last_signal_at timestamp with time zone DEFAULT now() NOT NULL,
    from_broadcaster_id text,
    from_broadcaster_login text NOT NULL,
    to_broadcaster_id text NOT NULL,
    to_broadcaster_login text NOT NULL,
    viewer_count integer DEFAULT 0 NOT NULL,
    classification text NOT NULL,
    confirmation_signals text DEFAULT ''::text NOT NULL,
    primary_signal text,
    correlation_status text,
    correlation_detail text,
    source_resolution text,
    raid_history_id bigint,
    raid_history_executed_at timestamp with time zone,
    unraid_seen boolean DEFAULT false NOT NULL,
    last_unraid_at timestamp with time zone
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_raid_arrival_tracking_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_raid_arrival_tracking_id_seq OWNED BY public.twitch_raid_arrival_tracking.id;

CREATE TABLE IF NOT EXISTS public.twitch_raid_auth (
    twitch_user_id text NOT NULL,
    twitch_login text NOT NULL,
    access_token text DEFAULT 'ENC'::text,
    refresh_token text DEFAULT 'ENC'::text,
    token_expires_at text NOT NULL,
    scopes text NOT NULL,
    authorized_at text DEFAULT CURRENT_TIMESTAMP,
    last_refreshed_at text,
    raid_enabled boolean DEFAULT true,
    created_at text DEFAULT CURRENT_TIMESTAMP,
    needs_reauth boolean DEFAULT false,
    reauth_notified_at text,
    access_token_enc bytea,
    refresh_token_enc bytea,
    enc_version integer DEFAULT 1,
    enc_kid text DEFAULT 'v1'::text,
    enc_migrated_at text
);

CREATE TABLE IF NOT EXISTS public.twitch_raid_blacklist (
    target_id text,
    target_login text NOT NULL,
    reason text,
    added_at text DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS public.twitch_raid_disabled_strikes (
    target_id text,
    target_login text NOT NULL,
    strike_count integer DEFAULT 1 NOT NULL,
    last_seen_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    last_reason text
);

CREATE TABLE IF NOT EXISTS public.twitch_raid_history (
    id bigint NOT NULL,
    from_broadcaster_id text NOT NULL,
    from_broadcaster_login text NOT NULL,
    to_broadcaster_id text NOT NULL,
    to_broadcaster_login text NOT NULL,
    viewer_count integer DEFAULT 0,
    stream_duration_sec integer,
    reason text,
    executed_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    success boolean DEFAULT true,
    error_message text,
    target_stream_started_at timestamp with time zone,
    candidates_count integer DEFAULT 0
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_raid_history_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_raid_history_id_seq OWNED BY public.twitch_raid_history.id;

CREATE TABLE IF NOT EXISTS public.twitch_raid_retention (
    raid_id bigint NOT NULL,
    from_broadcaster_login text NOT NULL,
    to_broadcaster_login text NOT NULL,
    viewer_count_sent integer NOT NULL,
    executed_at timestamp with time zone NOT NULL,
    target_session_id integer,
    chatters_at_plus5m integer,
    chatters_at_plus15m integer,
    chatters_at_plus30m integer,
    known_from_raider integer,
    new_to_target integer,
    new_chatters integer,
    computed_at timestamp with time zone DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS public.twitch_raw_chat_backfill_runs (
    id integer NOT NULL,
    streamer_login text NOT NULL,
    started_at text NOT NULL,
    finished_at text,
    status text DEFAULT 'not_started'::text NOT NULL,
    source_label text,
    imported_messages integer DEFAULT 0,
    deduped_messages integer DEFAULT 0,
    affected_sessions integer DEFAULT 0,
    note text
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_raw_chat_backfill_runs_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_raw_chat_backfill_runs_id_seq OWNED BY public.twitch_raw_chat_backfill_runs.id;

CREATE TABLE IF NOT EXISTS public.twitch_raw_chat_ingest_health (
    streamer_login text NOT NULL,
    last_raw_chat_message_at text,
    last_raw_chat_insert_ok_at text,
    last_raw_chat_insert_error_at text,
    last_raw_chat_error text,
    raw_chat_lag_seconds integer,
    updated_at text NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_session_chatters (
    session_id integer NOT NULL,
    streamer_login text NOT NULL,
    chatter_login text NOT NULL,
    chatter_id text,
    first_message_at text NOT NULL,
    messages integer DEFAULT 0,
    is_first_time_streamer boolean DEFAULT false,
    seen_via_chatters_api boolean DEFAULT false,
    last_seen_at text,
    confirmed_first_ever boolean DEFAULT false
);

CREATE TABLE IF NOT EXISTS public.twitch_session_viewers (
    session_id integer NOT NULL,
    ts_utc text NOT NULL,
    minutes_from_start integer,
    viewer_count integer NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_shoutout_events (
    id integer NOT NULL,
    twitch_user_id text NOT NULL,
    direction text NOT NULL,
    other_broadcaster_id text,
    other_broadcaster_login text,
    moderator_login text,
    viewer_count integer DEFAULT 0,
    received_at text DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_shoutout_events_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_shoutout_events_id_seq OWNED BY public.twitch_shoutout_events.id;

CREATE TABLE IF NOT EXISTS public.twitch_stats_category (
    ts_utc text,
    streamer text,
    viewer_count integer,
    is_partner integer DEFAULT 0,
    game_name text,
    stream_title text,
    tags text
);

CREATE TABLE IF NOT EXISTS public.twitch_stats_tracked (
    ts_utc text,
    streamer text,
    viewer_count integer,
    is_partner integer DEFAULT 0,
    game_name text,
    stream_title text,
    tags text
);

CREATE TABLE IF NOT EXISTS public.twitch_stream_sessions (
    id integer NOT NULL,
    streamer_login text NOT NULL,
    stream_id text,
    started_at text NOT NULL,
    ended_at text,
    duration_seconds integer DEFAULT 0,
    start_viewers integer DEFAULT 0,
    peak_viewers integer DEFAULT 0,
    end_viewers integer DEFAULT 0,
    avg_viewers real DEFAULT 0,
    samples integer DEFAULT 0,
    retention_5m real,
    retention_10m real,
    retention_20m real,
    dropoff_pct real,
    dropoff_label text,
    unique_chatters integer DEFAULT 0,
    first_time_chatters integer DEFAULT 0,
    returning_chatters integer DEFAULT 0,
    followers_start integer,
    followers_end integer,
    follower_delta integer,
    stream_title text,
    notification_text text,
    language text,
    is_mature integer DEFAULT 0,
    tags text,
    had_deadlock_in_session integer DEFAULT 0,
    game_name text,
    notes text
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_stream_sessions_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_stream_sessions_id_seq OWNED BY public.twitch_stream_sessions.id;

CREATE TABLE IF NOT EXISTS public.twitch_streamer_invites (
    streamer_login text NOT NULL,
    guild_id bigint NOT NULL,
    channel_id bigint NOT NULL,
    invite_code text NOT NULL,
    invite_url text NOT NULL,
    created_at text DEFAULT CURRENT_TIMESTAMP,
    last_sent_at text
);

CREATE TABLE IF NOT EXISTS public.twitch_streamers (
    twitch_login text NOT NULL,
    twitch_user_id text,
    discord_user_id text,
    discord_display_name text,
    is_on_discord integer DEFAULT 0,
    created_at text DEFAULT CURRENT_TIMESTAMP,
    archived_at text,
    is_monitored_only integer DEFAULT 0
);

CREATE OR REPLACE VIEW public.twitch_streamers_partner_state AS
 SELECT twitch_login,
    twitch_user_id,
    require_discord_link,
    next_link_check_at,
    discord_user_id,
    discord_display_name,
    is_on_discord,
    manual_verified_permanent,
    manual_verified_until,
    manual_verified_at,
    manual_partner_opt_out,
    created_at,
    archived_at,
    raid_bot_enabled,
    silent_ban,
    silent_raid,
    is_monitored_only,
    is_verified,
    is_partner,
    is_partner_active,
    live_ping_role_id,
    live_ping_enabled,
    technical_pause_reason,
    operational_state
   FROM public.twitch_partners_all_state
  WHERE (status = 'active'::text);

CREATE TABLE IF NOT EXISTS public.twitch_subscription_events (
    id integer NOT NULL,
    session_id integer,
    twitch_user_id text NOT NULL,
    event_type text NOT NULL,
    user_login text,
    tier text,
    is_gift integer DEFAULT 0,
    gifter_login text,
    cumulative_months integer,
    streak_months integer,
    message text,
    total_gifted integer,
    received_at text DEFAULT CURRENT_TIMESTAMP NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_subscription_events_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_subscription_events_id_seq OWNED BY public.twitch_subscription_events.id;

CREATE TABLE IF NOT EXISTS public.twitch_subscriptions_snapshot (
    id integer NOT NULL,
    twitch_user_id text NOT NULL,
    twitch_login text,
    total integer DEFAULT 0,
    tier1 integer DEFAULT 0,
    tier2 integer DEFAULT 0,
    tier3 integer DEFAULT 0,
    points integer DEFAULT 0,
    snapshot_at text DEFAULT CURRENT_TIMESTAMP
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_subscriptions_snapshot_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_subscriptions_snapshot_id_seq OWNED BY public.twitch_subscriptions_snapshot.id;

CREATE TABLE IF NOT EXISTS public.twitch_token_blacklist (
    twitch_user_id text NOT NULL,
    twitch_login text NOT NULL,
    error_message text,
    error_count integer DEFAULT 1,
    first_error_at text NOT NULL,
    last_error_at text NOT NULL,
    notified integer DEFAULT 0,
    grace_expires_at text,
    user_dm_sent integer DEFAULT 0,
    reminder_sent integer DEFAULT 0,
    role_removed integer DEFAULT 0
);

CREATE TABLE IF NOT EXISTS public.twitch_user_engagement_optout (
    twitch_user_id text NOT NULL,
    opted_out_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_user_profile (
    twitch_user_id text NOT NULL,
    twitch_login text NOT NULL,
    first_seen_at timestamp with time zone NOT NULL,
    last_seen_at timestamp with time zone NOT NULL,
    message_count integer DEFAULT 0 NOT NULL,
    channels jsonb DEFAULT '[]'::jsonb NOT NULL,
    tags text[] DEFAULT '{}'::text[],
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE IF NOT EXISTS public.twitch_user_threads (
    id bigint NOT NULL,
    twitch_user_id text NOT NULL,
    twitch_login text NOT NULL,
    channel_login text,
    thread_type text NOT NULL,
    summary text NOT NULL,
    due_at timestamp with time zone,
    status text DEFAULT 'open'::text NOT NULL,
    source_message_id text,
    last_referenced_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE SEQUENCE IF NOT EXISTS public.twitch_user_threads_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

ALTER SEQUENCE public.twitch_user_threads_id_seq OWNED BY public.twitch_user_threads.id;

CREATE TABLE IF NOT EXISTS public.twitch_viewer_presence_ticks (
    session_id bigint NOT NULL,
    streamer_login text NOT NULL,
    viewer_login text NOT NULL,
    tick_at timestamp with time zone NOT NULL
);

ALTER TABLE public.clip_fetch_history ALTER COLUMN id SET DEFAULT nextval('public.clip_fetch_history_id_seq'::regclass);

ALTER TABLE public.clip_templates_global ALTER COLUMN id SET DEFAULT nextval('public.clip_templates_global_id_seq'::regclass);

ALTER TABLE public.clip_templates_streamer ALTER COLUMN id SET DEFAULT nextval('public.clip_templates_streamer_id_seq'::regclass);

ALTER TABLE public.exp_game_transitions ALTER COLUMN id SET DEFAULT nextval('public.exp_game_transitions_id_seq'::regclass);

ALTER TABLE public.exp_sessions ALTER COLUMN id SET DEFAULT nextval('public.exp_sessions_id_seq'::regclass);

ALTER TABLE public.exp_snapshots ALTER COLUMN id SET DEFAULT nextval('public.exp_snapshots_id_seq'::regclass);

ALTER TABLE public.social_media_reports ALTER COLUMN id SET DEFAULT nextval('public.social_media_reports_id_seq'::regclass);

ALTER TABLE public.twitch_ad_break_events ALTER COLUMN id SET DEFAULT nextval('public.twitch_ad_break_events_id_seq'::regclass);

ALTER TABLE public.twitch_ads_schedule_snapshot ALTER COLUMN id SET DEFAULT nextval('public.twitch_ads_schedule_snapshot_id_seq'::regclass);

ALTER TABLE public.twitch_ban_events ALTER COLUMN id SET DEFAULT nextval('public.twitch_ban_events_id_seq'::regclass);

ALTER TABLE public.twitch_bits_events ALTER COLUMN id SET DEFAULT nextval('public.twitch_bits_events_id_seq'::regclass);

ALTER TABLE public.twitch_channel_points_events ALTER COLUMN id SET DEFAULT nextval('public.twitch_channel_points_events_id_seq'::regclass);

ALTER TABLE public.twitch_channel_updates ALTER COLUMN id SET DEFAULT nextval('public.twitch_channel_updates_id_seq'::regclass);

ALTER TABLE public.twitch_chat_messages ALTER COLUMN id SET DEFAULT nextval('public.twitch_chat_messages_id_seq'::regclass);

ALTER TABLE public.twitch_clips_social_analytics ALTER COLUMN id SET DEFAULT nextval('public.twitch_clips_social_analytics_id_seq'::regclass);

ALTER TABLE public.twitch_clips_social_media ALTER COLUMN id SET DEFAULT nextval('public.twitch_clips_social_media_id_seq'::regclass);

ALTER TABLE public.twitch_clips_upload_queue ALTER COLUMN id SET DEFAULT nextval('public.twitch_clips_upload_queue_id_seq'::regclass);

ALTER TABLE public.twitch_confirmed_external_recruitment_raids ALTER COLUMN id SET DEFAULT nextval('public.twitch_confirmed_external_recruitment_raids_id_seq'::regclass);

ALTER TABLE public.twitch_engagement_conversation ALTER COLUMN id SET DEFAULT nextval('public.twitch_engagement_conversation_id_seq'::regclass);

ALTER TABLE public.twitch_engagement_log ALTER COLUMN id SET DEFAULT nextval('public.twitch_engagement_log_id_seq'::regclass);

ALTER TABLE public.twitch_engagement_stream_transcripts ALTER COLUMN id SET DEFAULT nextval('public.twitch_engagement_stream_transcripts_id_seq'::regclass);

ALTER TABLE public.twitch_eventsub_capacity_snapshot ALTER COLUMN id SET DEFAULT nextval('public.twitch_eventsub_capacity_snapshot_id_seq'::regclass);

ALTER TABLE public.twitch_first_message_events ALTER COLUMN id SET DEFAULT nextval('public.twitch_first_message_events_id_seq'::regclass);

ALTER TABLE public.twitch_follow_events ALTER COLUMN id SET DEFAULT nextval('public.twitch_follow_events_id_seq'::regclass);

ALTER TABLE public.twitch_hype_train_events ALTER COLUMN id SET DEFAULT nextval('public.twitch_hype_train_events_id_seq'::regclass);

ALTER TABLE public.twitch_link_clicks ALTER COLUMN id SET DEFAULT nextval('public.twitch_link_clicks_id_seq'::regclass);

ALTER TABLE public.twitch_observability_events ALTER COLUMN id SET DEFAULT nextval('public.twitch_observability_events_id_seq'::regclass);

ALTER TABLE public.twitch_partner_outreach_audit ALTER COLUMN id SET DEFAULT nextval('public.twitch_partner_outreach_audit_id_seq'::regclass);

ALTER TABLE public.twitch_partner_raid_score_tracking ALTER COLUMN id SET DEFAULT nextval('public.twitch_partner_raid_score_tracking_id_seq'::regclass);

ALTER TABLE public.twitch_partners ALTER COLUMN id SET DEFAULT nextval('public.twitch_partners_id_seq'::regclass);

ALTER TABLE public.twitch_raid_arrival_tracking ALTER COLUMN id SET DEFAULT nextval('public.twitch_raid_arrival_tracking_id_seq'::regclass);

ALTER TABLE public.twitch_raid_history ALTER COLUMN id SET DEFAULT nextval('public.twitch_raid_history_id_seq'::regclass);

ALTER TABLE public.twitch_raw_chat_backfill_runs ALTER COLUMN id SET DEFAULT nextval('public.twitch_raw_chat_backfill_runs_id_seq'::regclass);

ALTER TABLE public.twitch_shoutout_events ALTER COLUMN id SET DEFAULT nextval('public.twitch_shoutout_events_id_seq'::regclass);

ALTER TABLE public.twitch_stream_sessions ALTER COLUMN id SET DEFAULT nextval('public.twitch_stream_sessions_id_seq'::regclass);

ALTER TABLE public.twitch_subscription_events ALTER COLUMN id SET DEFAULT nextval('public.twitch_subscription_events_id_seq'::regclass);

ALTER TABLE public.twitch_subscriptions_snapshot ALTER COLUMN id SET DEFAULT nextval('public.twitch_subscriptions_snapshot_id_seq'::regclass);

ALTER TABLE public.twitch_user_threads ALTER COLUMN id SET DEFAULT nextval('public.twitch_user_threads_id_seq'::regclass);

DO $do$ BEGIN
    ALTER TABLE public.clip_fetch_history
        ADD CONSTRAINT clip_fetch_history_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.clip_last_hashtags
        ADD CONSTRAINT clip_last_hashtags_pkey PRIMARY KEY (streamer_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.clip_templates_global
        ADD CONSTRAINT clip_templates_global_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.clip_templates_global
        ADD CONSTRAINT clip_templates_global_template_name_key UNIQUE (template_name);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.clip_templates_streamer
        ADD CONSTRAINT clip_templates_streamer_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.clip_templates_streamer
        ADD CONSTRAINT clip_templates_streamer_streamer_login_template_name_key UNIQUE (streamer_login, template_name);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.dashboard_sessions
        ADD CONSTRAINT dashboard_sessions_pkey PRIMARY KEY (session_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.deadlock_vocab
        ADD CONSTRAINT deadlock_vocab_pkey PRIMARY KEY (term);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.discord_invite_codes
        ADD CONSTRAINT discord_invite_codes_pkey PRIMARY KEY (guild_id, invite_code);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.eventsub_guard_state
        ADD CONSTRAINT eventsub_guard_state_pkey PRIMARY KEY (kind, guard_key);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.exp_game_transitions
        ADD CONSTRAINT exp_game_transitions_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.exp_sessions
        ADD CONSTRAINT exp_sessions_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.exp_snapshots
        ADD CONSTRAINT exp_snapshots_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.oauth_state_tokens
        ADD CONSTRAINT oauth_state_tokens_pkey PRIMARY KEY (state_token);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.social_media_clip_approval
        ADD CONSTRAINT social_media_clip_approval_pkey PRIMARY KEY (clip_db_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.social_media_clip_enrichment
        ADD CONSTRAINT social_media_clip_enrichment_pkey PRIMARY KEY (clip_db_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.social_media_platform_auth
        ADD CONSTRAINT social_media_platform_auth_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.social_media_reauth_notifications
        ADD CONSTRAINT social_media_reauth_notifications_pkey PRIMARY KEY (streamer_login, platform, error_kind);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.social_media_reports
        ADD CONSTRAINT social_media_reports_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.social_media_settings
        ADD CONSTRAINT social_media_settings_pkey PRIMARY KEY (key);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.social_media_streamer_layout
        ADD CONSTRAINT social_media_streamer_layout_pkey PRIMARY KEY (streamer_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.streamer_plans
        ADD CONSTRAINT streamer_plans_pkey PRIMARY KEY (twitch_user_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_ad_break_events
        ADD CONSTRAINT twitch_ad_break_events_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_admin_roles
        ADD CONSTRAINT twitch_admin_roles_pkey PRIMARY KEY (twitch_user_id, role);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_ads_schedule_snapshot
        ADD CONSTRAINT twitch_ads_schedule_snapshot_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_auto_raid_pause
        ADD CONSTRAINT twitch_auto_raid_pause_pkey PRIMARY KEY (twitch_user_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_ban_events
        ADD CONSTRAINT twitch_ban_events_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_billing_subscriptions
        ADD CONSTRAINT twitch_billing_subscriptions_pkey PRIMARY KEY (stripe_subscription_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_bits_events
        ADD CONSTRAINT twitch_bits_events_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_channel_match_state
        ADD CONSTRAINT twitch_channel_match_state_pkey PRIMARY KEY (channel_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_channel_points_events
        ADD CONSTRAINT twitch_channel_points_events_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_channel_updates
        ADD CONSTRAINT twitch_channel_updates_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_chat_messages
        ADD CONSTRAINT twitch_chat_messages_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_chatter_global_ban_applied
        ADD CONSTRAINT twitch_chatter_global_ban_applied_pkey PRIMARY KEY (chatter_login, broadcaster_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_chatter_global_ban
        ADD CONSTRAINT twitch_chatter_global_ban_pkey PRIMARY KEY (chatter_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_chatter_rollup
        ADD CONSTRAINT twitch_chatter_rollup_pkey PRIMARY KEY (streamer_login, chatter_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_clips_social_analytics
        ADD CONSTRAINT twitch_clips_social_analytics_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_clips_social_media
        ADD CONSTRAINT twitch_clips_social_media_clip_id_key UNIQUE (clip_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_clips_social_media
        ADD CONSTRAINT twitch_clips_social_media_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_clips_upload_queue
        ADD CONSTRAINT twitch_clips_upload_queue_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_confirmed_external_recruitment_raids
        ADD CONSTRAINT twitch_confirmed_external_recruitment_raids_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_confirmed_external_recruitment_raids
        ADD CONSTRAINT twitch_confirmed_external_recruitment_raids_raid_flow_id_key UNIQUE (raid_flow_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_engagement_conversation
        ADD CONSTRAINT twitch_engagement_conversation_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_engagement_log
        ADD CONSTRAINT twitch_engagement_log_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_engagement_settings
        ADD CONSTRAINT twitch_engagement_settings_pkey PRIMARY KEY (channel_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_engagement_stream_transcripts
        ADD CONSTRAINT twitch_engagement_stream_transcripts_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_eventsub_bridge_dead_letter
        ADD CONSTRAINT twitch_eventsub_bridge_dead_letter_pkey PRIMARY KEY (message_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_eventsub_bridge_outbox
        ADD CONSTRAINT twitch_eventsub_bridge_outbox_pkey PRIMARY KEY (message_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_eventsub_capacity_snapshot
        ADD CONSTRAINT twitch_eventsub_capacity_snapshot_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_eventsub_processing_dead_letter
        ADD CONSTRAINT twitch_eventsub_processing_dead_letter_pkey PRIMARY KEY (work_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_eventsub_processing_inbox
        ADD CONSTRAINT twitch_eventsub_processing_inbox_pkey PRIMARY KEY (work_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_external_bot_ban_check_pending
        ADD CONSTRAINT twitch_external_bot_ban_check_pending_pkey PRIMARY KEY (target_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_external_recruitment_blacklist_pending
        ADD CONSTRAINT twitch_external_recruitment_blacklist_pending_pkey PRIMARY KEY (target_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_first_message_events
        ADD CONSTRAINT twitch_first_message_events_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_follow_events
        ADD CONSTRAINT twitch_follow_events_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_global_ban_sweep_due
        ADD CONSTRAINT twitch_global_ban_sweep_due_pkey PRIMARY KEY (broadcaster_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_global_promo_modes
        ADD CONSTRAINT twitch_global_promo_modes_pkey PRIMARY KEY (config_key);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_global_settings
        ADD CONSTRAINT twitch_global_settings_pkey PRIMARY KEY (setting_key);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_hype_train_events
        ADD CONSTRAINT twitch_hype_train_events_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_link_clicks
        ADD CONSTRAINT twitch_link_clicks_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_live_announcement_configs
        ADD CONSTRAINT twitch_live_announcement_configs_pkey PRIMARY KEY (streamer_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_live_state
        ADD CONSTRAINT twitch_live_state_pkey PRIMARY KEY (twitch_user_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_observability_events
        ADD CONSTRAINT twitch_observability_events_pkey PRIMARY KEY (id, created_at);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_partner_outreach_audit
        ADD CONSTRAINT twitch_partner_outreach_audit_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_partner_outreach_conversations
        ADD CONSTRAINT twitch_partner_outreach_conversations_pkey PRIMARY KEY (streamer_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_partner_outreach
        ADD CONSTRAINT twitch_partner_outreach_pkey PRIMARY KEY (streamer_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_partner_raid_score_tracking
        ADD CONSTRAINT twitch_partner_raid_score_tracking_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_partner_raid_scores
        ADD CONSTRAINT twitch_partner_raid_scores_pkey PRIMARY KEY (twitch_user_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_partners
        ADD CONSTRAINT twitch_partners_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_promo_cooldowns
        ADD CONSTRAINT twitch_promo_cooldowns_pkey PRIMARY KEY (login, cooldown_type);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_raid_arrival_tracking
        ADD CONSTRAINT twitch_raid_arrival_tracking_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_raid_auth
        ADD CONSTRAINT twitch_raid_auth_pkey PRIMARY KEY (twitch_user_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_raid_blacklist
        ADD CONSTRAINT twitch_raid_blacklist_pkey PRIMARY KEY (target_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_raid_disabled_strikes
        ADD CONSTRAINT twitch_raid_disabled_strikes_pkey PRIMARY KEY (target_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_raid_history
        ADD CONSTRAINT twitch_raid_history_id_executed_at_key UNIQUE (id, executed_at);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_raid_history
        ADD CONSTRAINT twitch_raid_history_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_raid_retention
        ADD CONSTRAINT twitch_raid_retention_pkey PRIMARY KEY (raid_id, executed_at);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_raw_chat_backfill_runs
        ADD CONSTRAINT twitch_raw_chat_backfill_runs_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_raw_chat_ingest_health
        ADD CONSTRAINT twitch_raw_chat_ingest_health_pkey PRIMARY KEY (streamer_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_session_chatters
        ADD CONSTRAINT twitch_session_chatters_pkey PRIMARY KEY (session_id, chatter_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_session_viewers
        ADD CONSTRAINT twitch_session_viewers_pkey PRIMARY KEY (session_id, ts_utc);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_shoutout_events
        ADD CONSTRAINT twitch_shoutout_events_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_stream_sessions
        ADD CONSTRAINT twitch_stream_sessions_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_streamer_identities
        ADD CONSTRAINT twitch_streamer_identities_pkey PRIMARY KEY (twitch_user_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_streamer_invites
        ADD CONSTRAINT twitch_streamer_invites_pkey PRIMARY KEY (streamer_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_streamers
        ADD CONSTRAINT twitch_streamers_pkey PRIMARY KEY (twitch_login);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_subscription_events
        ADD CONSTRAINT twitch_subscription_events_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_subscriptions_snapshot
        ADD CONSTRAINT twitch_subscriptions_snapshot_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_token_blacklist
        ADD CONSTRAINT twitch_token_blacklist_pkey PRIMARY KEY (twitch_user_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_user_engagement_optout
        ADD CONSTRAINT twitch_user_engagement_optout_pkey PRIMARY KEY (twitch_user_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_user_profile
        ADD CONSTRAINT twitch_user_profile_pkey PRIMARY KEY (twitch_user_id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_user_threads
        ADD CONSTRAINT twitch_user_threads_pkey PRIMARY KEY (id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_viewer_presence_ticks
        ADD CONSTRAINT twitch_viewer_presence_ticks_pkey PRIMARY KEY (session_id, viewer_login, tick_at);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

CREATE INDEX IF NOT EXISTS idx_clip_fetch_history_streamer ON public.clip_fetch_history USING btree (streamer_login, fetched_at DESC);

CREATE INDEX IF NOT EXISTS idx_clip_templates_global_category ON public.clip_templates_global USING btree (category);

CREATE INDEX IF NOT EXISTS idx_clip_templates_streamer_login ON public.clip_templates_streamer USING btree (streamer_login);

CREATE INDEX IF NOT EXISTS idx_confirmed_external_recruitment_raids_target ON public.twitch_confirmed_external_recruitment_raids USING btree (to_broadcaster_id);

CREATE INDEX IF NOT EXISTS idx_dashboard_sessions_expires ON public.dashboard_sessions USING btree (expires_at);

CREATE INDEX IF NOT EXISTS idx_deadlock_vocab_canonical ON public.deadlock_vocab USING btree (canonical);

CREATE INDEX IF NOT EXISTS idx_deadlock_vocab_category ON public.deadlock_vocab USING btree (category);

CREATE INDEX IF NOT EXISTS idx_discord_invites_guild ON public.discord_invite_codes USING btree (guild_id);

CREATE INDEX IF NOT EXISTS idx_eng_conv_channel_ts ON public.twitch_engagement_conversation USING btree (channel_login, ts DESC);

CREATE INDEX IF NOT EXISTS idx_eng_log_channel_ts ON public.twitch_engagement_log USING btree (channel_login, ts DESC);

CREATE INDEX IF NOT EXISTS idx_eng_stream_transcripts_channel_ended ON public.twitch_engagement_stream_transcripts USING btree (channel_login, ended_at DESC);

CREATE INDEX IF NOT EXISTS idx_eventsub_guard_state_expiry ON public.eventsub_guard_state USING btree (expires_at);

CREATE UNIQUE INDEX IF NOT EXISTS idx_exp_sessions_stream_id ON public.exp_sessions USING btree (stream_id) WHERE (stream_id IS NOT NULL);

CREATE INDEX IF NOT EXISTS idx_exp_sessions_streamer ON public.exp_sessions USING btree (streamer, started_at);

CREATE INDEX IF NOT EXISTS idx_exp_snapshots_session ON public.exp_snapshots USING btree (exp_session_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_exp_snapshots_session_ts ON public.exp_snapshots USING btree (exp_session_id, ts_utc);

CREATE INDEX IF NOT EXISTS idx_exp_transitions_streamer ON public.exp_game_transitions USING btree (streamer, ts_utc);

CREATE INDEX IF NOT EXISTS idx_external_bot_ban_check_pending_due ON public.twitch_external_bot_ban_check_pending USING btree (run_after);

CREATE INDEX IF NOT EXISTS idx_external_recruitment_blacklist_pending_due ON public.twitch_external_recruitment_blacklist_pending USING btree (blacklist_after);

CREATE INDEX IF NOT EXISTS idx_live_announce_configs_updated_at ON public.twitch_live_announcement_configs USING btree (updated_at);

CREATE INDEX IF NOT EXISTS idx_oauth_state_consumed_at ON public.oauth_state_tokens USING btree (consumed_at);

CREATE INDEX IF NOT EXISTS idx_oauth_state_expires ON public.oauth_state_tokens USING btree (expires_at);

CREATE INDEX IF NOT EXISTS idx_oauth_state_platform_expires ON public.oauth_state_tokens USING btree (platform, expires_at);

CREATE INDEX IF NOT EXISTS idx_partner_raid_scores_computed ON public.twitch_partner_raid_scores USING btree (last_computed_at);

CREATE INDEX IF NOT EXISTS idx_partner_raid_scores_live_score ON public.twitch_partner_raid_scores USING btree (is_live, final_score DESC);

CREATE INDEX IF NOT EXISTS idx_partner_raid_scores_login ON public.twitch_partner_raid_scores USING btree (twitch_login);

CREATE INDEX IF NOT EXISTS idx_partner_raid_tracking_history ON public.twitch_partner_raid_score_tracking USING btree (raid_history_id);

CREATE INDEX IF NOT EXISTS idx_partner_raid_tracking_history_ref ON public.twitch_partner_raid_score_tracking USING btree (raid_history_id, raid_history_executed_at);

CREATE INDEX IF NOT EXISTS idx_partner_raid_tracking_session ON public.twitch_partner_raid_score_tracking USING btree (target_session_id, resolved_at);

CREATE INDEX IF NOT EXISTS idx_partner_raid_tracking_target ON public.twitch_partner_raid_score_tracking USING btree (to_broadcaster_id, confirmed_at);

CREATE INDEX IF NOT EXISTS idx_social_media_clip_approval_last_sent_at ON public.social_media_clip_approval USING btree (last_sent_at DESC);

CREATE INDEX IF NOT EXISTS idx_social_media_clip_approval_state ON public.social_media_clip_approval USING btree (state);

CREATE INDEX IF NOT EXISTS idx_social_media_clip_enrichment_status ON public.social_media_clip_enrichment USING btree (status);

CREATE INDEX IF NOT EXISTS idx_social_media_clip_enrichment_updated_at ON public.social_media_clip_enrichment USING btree (updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_social_media_reauth_notifications_last_sent ON public.social_media_reauth_notifications USING btree (last_sent_at DESC);

CREATE INDEX IF NOT EXISTS idx_social_media_reports_kind_period ON public.social_media_reports USING btree (kind, period_end DESC);

CREATE INDEX IF NOT EXISTS idx_social_media_reports_streamer_period ON public.social_media_reports USING btree (streamer_login, period_end DESC);

CREATE INDEX IF NOT EXISTS idx_social_platform_auth ON public.social_media_platform_auth USING btree (platform, streamer_login, enabled);

CREATE INDEX IF NOT EXISTS idx_social_platform_auth_expires ON public.social_media_platform_auth USING btree (token_expires_at) WHERE (enabled = 1);

CREATE UNIQUE INDEX IF NOT EXISTS idx_social_platform_auth_global_unique ON public.social_media_platform_auth USING btree (platform) WHERE (streamer_login IS NULL);

CREATE UNIQUE INDEX IF NOT EXISTS idx_social_platform_auth_streamer_unique ON public.social_media_platform_auth USING btree (platform, streamer_login) WHERE (streamer_login IS NOT NULL);

CREATE INDEX IF NOT EXISTS idx_streamer_plans_login ON public.streamer_plans USING btree (twitch_login);

CREATE INDEX IF NOT EXISTS idx_streamer_plans_login_lower ON public.streamer_plans USING btree (lower(COALESCE(twitch_login, ''::text)));

CREATE INDEX IF NOT EXISTS idx_threads_due ON public.twitch_user_threads USING btree (status, due_at) WHERE (status = ANY (ARRAY['open'::text, 'follow_up_due'::text]));

CREATE INDEX IF NOT EXISTS idx_threads_user_status ON public.twitch_user_threads USING btree (twitch_user_id, status);

CREATE INDEX IF NOT EXISTS idx_twitch_ad_break_events_session ON public.twitch_ad_break_events USING btree (session_id);

CREATE INDEX IF NOT EXISTS idx_twitch_ads_user_ts ON public.twitch_ads_schedule_snapshot USING btree (twitch_user_id, snapshot_at);

CREATE INDEX IF NOT EXISTS idx_twitch_auto_raid_pause_until ON public.twitch_auto_raid_pause USING btree (paused_until);

CREATE INDEX IF NOT EXISTS idx_twitch_ban_events_user ON public.twitch_ban_events USING btree (twitch_user_id, received_at);

CREATE INDEX IF NOT EXISTS idx_twitch_ban_events_user_type_received ON public.twitch_ban_events USING btree (twitch_user_id, event_type, received_at);

CREATE INDEX IF NOT EXISTS idx_twitch_billing_subscriptions_customer_reference ON public.twitch_billing_subscriptions USING btree (customer_reference);

CREATE INDEX IF NOT EXISTS idx_twitch_billing_subscriptions_customer_reference_lower ON public.twitch_billing_subscriptions USING btree (lower(COALESCE(customer_reference, ''::text)));

CREATE INDEX IF NOT EXISTS idx_twitch_bits_events_session ON public.twitch_bits_events USING btree (session_id);

CREATE INDEX IF NOT EXISTS idx_twitch_channel_points_events_user ON public.twitch_channel_points_events USING btree (twitch_user_id, redeemed_at);

CREATE INDEX IF NOT EXISTS idx_twitch_channel_updates_user ON public.twitch_channel_updates USING btree (twitch_user_id, recorded_at);

CREATE INDEX IF NOT EXISTS idx_twitch_chat_messages_chatter ON public.twitch_chat_messages USING btree (streamer_login, chatter_login, message_ts);

CREATE INDEX IF NOT EXISTS idx_twitch_chat_messages_message_id ON public.twitch_chat_messages USING btree (message_id);

CREATE INDEX IF NOT EXISTS idx_twitch_chat_messages_session ON public.twitch_chat_messages USING btree (session_id, message_ts);

CREATE INDEX IF NOT EXISTS idx_twitch_chat_messages_streamer_ts ON public.twitch_chat_messages USING btree (streamer_login, message_ts);

CREATE INDEX IF NOT EXISTS idx_twitch_chatter_global_ban_id ON public.twitch_chatter_global_ban USING btree (chatter_id) WHERE (chatter_id IS NOT NULL);

CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_analytics_bucket ON public.twitch_clips_social_analytics USING btree (clip_id, platform, bucket);

CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_analytics_clip ON public.twitch_clips_social_analytics USING btree (clip_id, synced_at);

CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_analytics_platform ON public.twitch_clips_social_analytics USING btree (platform, posted_at);

CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_media_discarded_at ON public.twitch_clips_social_media USING btree (discarded_at);

CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_media_retention ON public.twitch_clips_social_media USING btree (retention_until);

CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_media_status ON public.twitch_clips_social_media USING btree (status);

CREATE INDEX IF NOT EXISTS idx_twitch_clips_social_media_streamer ON public.twitch_clips_social_media USING btree (streamer_login, created_at);

CREATE INDEX IF NOT EXISTS idx_twitch_clips_upload_queue_status ON public.twitch_clips_upload_queue USING btree (status, priority DESC);

CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_bridge_dead_lettered_at ON public.twitch_eventsub_bridge_dead_letter USING btree (dead_lettered_at);

CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_bridge_outbox_due ON public.twitch_eventsub_bridge_outbox USING btree (next_attempt_at, queued_at);

CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_capacity_reason ON public.twitch_eventsub_capacity_snapshot USING btree (trigger_reason, ts_utc);

CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_capacity_ts ON public.twitch_eventsub_capacity_snapshot USING btree (ts_utc);

CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_processing_dead_lettered_at ON public.twitch_eventsub_processing_dead_letter USING btree (dead_lettered_at);

CREATE INDEX IF NOT EXISTS idx_twitch_eventsub_processing_inbox_due ON public.twitch_eventsub_processing_inbox USING btree (next_attempt_at, queued_at);

CREATE INDEX IF NOT EXISTS idx_twitch_first_message_events_chatter ON public.twitch_first_message_events USING btree (chatter_login);

CREATE INDEX IF NOT EXISTS idx_twitch_first_message_events_streamer ON public.twitch_first_message_events USING btree (streamer_login, event_ts DESC);

CREATE INDEX IF NOT EXISTS idx_twitch_follow_events_streamer ON public.twitch_follow_events USING btree (streamer_login, followed_at);

CREATE INDEX IF NOT EXISTS idx_twitch_global_ban_sweep_due_run ON public.twitch_global_ban_sweep_due USING btree (run_after);

CREATE INDEX IF NOT EXISTS idx_twitch_global_promo_modes_updated_at ON public.twitch_global_promo_modes USING btree (updated_at);

CREATE INDEX IF NOT EXISTS idx_twitch_global_settings_updated_at ON public.twitch_global_settings USING btree (updated_at);

CREATE INDEX IF NOT EXISTS idx_twitch_hype_train_events_session ON public.twitch_hype_train_events USING btree (session_id);

CREATE INDEX IF NOT EXISTS idx_twitch_link_clicks_streamer ON public.twitch_link_clicks USING btree (streamer_login);

CREATE INDEX IF NOT EXISTS idx_twitch_observability_events_entity ON public.twitch_observability_events USING btree (entity_login, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_twitch_observability_events_flow ON public.twitch_observability_events USING btree (flow_type, flow_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_twitch_partner_outreach_audit_correlation ON public.twitch_partner_outreach_audit USING btree (correlation_id) WHERE (correlation_id IS NOT NULL);

CREATE INDEX IF NOT EXISTS idx_twitch_partner_outreach_audit_event_kind ON public.twitch_partner_outreach_audit USING btree (event_kind, occurred_at DESC);

CREATE INDEX IF NOT EXISTS idx_twitch_partner_outreach_audit_streamer_time ON public.twitch_partner_outreach_audit USING btree (streamer_login, occurred_at DESC);

CREATE INDEX IF NOT EXISTS idx_twitch_partner_outreach_conv_active ON public.twitch_partner_outreach_conversations USING btree (last_streamer_signal_at DESC) WHERE (state = ANY (ARRAY['open'::text, 'listening'::text]));

CREATE INDEX IF NOT EXISTS idx_twitch_partner_outreach_conv_notify_pending ON public.twitch_partner_outreach_conversations USING btree (human_notify_pending_at) WHERE ((human_notify_pending_at IS NOT NULL) AND (human_notify_sent_at IS NULL));

CREATE INDEX IF NOT EXISTS idx_twitch_partner_outreach_conv_state ON public.twitch_partner_outreach_conversations USING btree (state) WHERE (state = ANY (ARRAY['open'::text, 'listening'::text, 'brain_pending'::text]));

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_partners_active_login_lower ON public.twitch_partners USING btree (lower(twitch_login)) WHERE (status = 'active'::text);

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_partners_active_user_id ON public.twitch_partners USING btree (twitch_user_id) WHERE (status = 'active'::text);

CREATE INDEX IF NOT EXISTS idx_twitch_promo_cooldowns_wall_ts ON public.twitch_promo_cooldowns USING btree (wall_ts);

CREATE INDEX IF NOT EXISTS idx_twitch_raid_arrival_tracking_history_ref ON public.twitch_raid_arrival_tracking USING btree (raid_history_id, raid_history_executed_at);

CREATE INDEX IF NOT EXISTS idx_twitch_raid_arrival_tracking_source ON public.twitch_raid_arrival_tracking USING btree (from_broadcaster_login, detected_at DESC);

CREATE INDEX IF NOT EXISTS idx_twitch_raid_arrival_tracking_target ON public.twitch_raid_arrival_tracking USING btree (to_broadcaster_id, detected_at DESC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_raid_auth_login ON public.twitch_raid_auth USING btree (lower(twitch_login));

CREATE INDEX IF NOT EXISTS idx_twitch_raid_history_executed ON public.twitch_raid_history USING btree (executed_at);

CREATE INDEX IF NOT EXISTS idx_twitch_raid_history_from ON public.twitch_raid_history USING btree (from_broadcaster_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_raid_history_id_executed_at ON public.twitch_raid_history USING btree (id, executed_at);

CREATE INDEX IF NOT EXISTS idx_twitch_raid_history_to ON public.twitch_raid_history USING btree (to_broadcaster_id);

CREATE INDEX IF NOT EXISTS idx_twitch_raid_retention_raid_id ON public.twitch_raid_retention USING btree (raid_id);

CREATE INDEX IF NOT EXISTS idx_twitch_raw_chat_backfill_runs_streamer ON public.twitch_raw_chat_backfill_runs USING btree (streamer_login, started_at DESC);

CREATE INDEX IF NOT EXISTS idx_twitch_raw_chat_ingest_health_updated ON public.twitch_raw_chat_ingest_health USING btree (updated_at);

CREATE INDEX IF NOT EXISTS idx_twitch_session_chatters_login ON public.twitch_session_chatters USING btree (streamer_login, session_id);

CREATE INDEX IF NOT EXISTS idx_twitch_session_viewers_session ON public.twitch_session_viewers USING btree (session_id);

CREATE INDEX IF NOT EXISTS idx_twitch_sessions_login ON public.twitch_stream_sessions USING btree (streamer_login, started_at);

CREATE INDEX IF NOT EXISTS idx_twitch_sessions_open ON public.twitch_stream_sessions USING btree (streamer_login) WHERE (ended_at IS NULL);

CREATE INDEX IF NOT EXISTS idx_twitch_shoutout_events_user ON public.twitch_shoutout_events USING btree (twitch_user_id, received_at);

CREATE INDEX IF NOT EXISTS idx_twitch_stats_category_streamer ON public.twitch_stats_category USING btree (streamer);

CREATE INDEX IF NOT EXISTS idx_twitch_stats_category_ts ON public.twitch_stats_category USING btree (ts_utc);

CREATE INDEX IF NOT EXISTS idx_twitch_stats_tracked_streamer ON public.twitch_stats_tracked USING btree (streamer);

CREATE INDEX IF NOT EXISTS idx_twitch_stats_tracked_ts ON public.twitch_stats_tracked USING btree (ts_utc);

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_streamer_identities_discord_user ON public.twitch_streamer_identities USING btree (discord_user_id) WHERE ((discord_user_id IS NOT NULL) AND (discord_user_id <> ''::text));

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_streamer_identities_login_lower ON public.twitch_streamer_identities USING btree (lower(twitch_login)) WHERE ((twitch_login IS NOT NULL) AND (twitch_login <> ''::text));

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_streamer_invites_code ON public.twitch_streamer_invites USING btree (invite_code);

CREATE INDEX IF NOT EXISTS idx_twitch_streamer_invites_guild ON public.twitch_streamer_invites USING btree (guild_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_streamers_user_id ON public.twitch_streamers USING btree (twitch_user_id);

CREATE INDEX IF NOT EXISTS idx_twitch_subs_user_ts ON public.twitch_subscriptions_snapshot USING btree (twitch_user_id, snapshot_at);

CREATE INDEX IF NOT EXISTS idx_twitch_subscription_events_session ON public.twitch_subscription_events USING btree (session_id);

CREATE INDEX IF NOT EXISTS idx_viewer_presence_ticks_session ON public.twitch_viewer_presence_ticks USING btree (session_id, viewer_login, tick_at);

CREATE INDEX IF NOT EXISTS twitch_observability_events_created_at_idx ON public.twitch_observability_events USING btree (created_at DESC);

DROP TRIGGER IF EXISTS social_media_retention_until_tg ON public.twitch_clips_social_media;
CREATE TRIGGER social_media_retention_until_tg BEFORE INSERT OR UPDATE OF created_at ON public.twitch_clips_social_media FOR EACH ROW EXECUTE FUNCTION public.social_media_set_retention_until();

DROP TRIGGER IF EXISTS trg_twitch_partners_sync_identity ON public.twitch_partners;
CREATE TRIGGER trg_twitch_partners_sync_identity AFTER INSERT OR UPDATE OF twitch_login, twitch_user_id, status ON public.twitch_partners FOR EACH ROW EXECUTE FUNCTION public.sync_twitch_streamer_identity_from_partners();

DROP TRIGGER IF EXISTS trg_twitch_streamers_sync_identity ON public.twitch_streamers;
CREATE TRIGGER trg_twitch_streamers_sync_identity AFTER INSERT OR UPDATE OF twitch_login, twitch_user_id, discord_user_id, discord_display_name, is_on_discord ON public.twitch_streamers FOR EACH ROW EXECUTE FUNCTION public.sync_twitch_streamer_identity_from_streamers();

DO $do$ BEGIN
    ALTER TABLE public.social_media_clip_approval
        ADD CONSTRAINT social_media_clip_approval_clip_db_id_fkey FOREIGN KEY (clip_db_id) REFERENCES public.twitch_clips_social_media(id) ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.social_media_clip_enrichment
        ADD CONSTRAINT social_media_clip_enrichment_clip_db_id_fkey FOREIGN KEY (clip_db_id) REFERENCES public.twitch_clips_social_media(id) ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.social_media_streamer_layout
        ADD CONSTRAINT social_media_streamer_layout_streamer_login_fkey FOREIGN KEY (streamer_login) REFERENCES public.twitch_streamers(twitch_login) ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_clips_social_analytics
        ADD CONSTRAINT twitch_clips_social_analytics_clip_id_fkey FOREIGN KEY (clip_id) REFERENCES public.twitch_clips_social_media(id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_clips_upload_queue
        ADD CONSTRAINT twitch_clips_upload_queue_clip_id_fkey FOREIGN KEY (clip_id) REFERENCES public.twitch_clips_social_media(id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_partner_outreach_audit
        ADD CONSTRAINT twitch_partner_outreach_audit_streamer_login_fkey FOREIGN KEY (streamer_login) REFERENCES public.twitch_partner_outreach(streamer_login) ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_partner_outreach_conversations
        ADD CONSTRAINT twitch_partner_outreach_conversations_streamer_login_fkey FOREIGN KEY (streamer_login) REFERENCES public.twitch_partner_outreach(streamer_login) ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_partner_raid_score_tracking
        ADD CONSTRAINT twitch_partner_raid_score_tracking_raid_history_ref_fkey FOREIGN KEY (raid_history_id, raid_history_executed_at) REFERENCES public.twitch_raid_history(id, executed_at) ON DELETE SET NULL;
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_raid_retention
        ADD CONSTRAINT twitch_raid_retention_raid_history_ref_fkey FOREIGN KEY (raid_id, executed_at) REFERENCES public.twitch_raid_history(id, executed_at) ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_raid_retention
        ADD CONSTRAINT twitch_raid_retention_target_session_id_fkey FOREIGN KEY (target_session_id) REFERENCES public.twitch_stream_sessions(id);
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

DO $do$ BEGIN
    ALTER TABLE public.twitch_viewer_presence_ticks
        ADD CONSTRAINT twitch_viewer_presence_ticks_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.twitch_stream_sessions(id) ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object OR duplicate_table
    OR invalid_table_definition OR feature_not_supported THEN NULL;
END $do$;

