ALTER TABLE streamer_plans ADD COLUMN IF NOT EXISTS sub_reminder_enabled integer NOT NULL DEFAULT 0;
ALTER TABLE streamer_plans ADD COLUMN IF NOT EXISTS sub_reminder_enabled_at timestamptz;
ALTER TABLE twitch_subscription_events ADD COLUMN IF NOT EXISTS viewer_user_id text;

CREATE TABLE twitch_sub_reminders (
    broadcaster_user_id text NOT NULL,
    viewer_user_id text NOT NULL,
    consent_at timestamptz NOT NULL DEFAULT now(),
    enabled boolean NOT NULL DEFAULT false,
    last_event_at timestamptz,
    ended_at timestamptz,
    end_message_id text,
    consumed_message_id text,
    retry_after timestamptz,
    PRIMARY KEY (broadcaster_user_id, viewer_user_id),
    CHECK (broadcaster_user_id <> '' AND viewer_user_id <> '')
);
