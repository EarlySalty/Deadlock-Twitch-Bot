CREATE TABLE IF NOT EXISTS public.twitch_moderation_settings (
    channel_user_id      TEXT PRIMARY KEY,
    global_ban_enabled   BOOLEAN NOT NULL DEFAULT TRUE,
    scam_pitch_enabled   BOOLEAN NOT NULL DEFAULT TRUE,
    spam_autoban_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    sus_invite_enabled   BOOLEAN NOT NULL DEFAULT TRUE
);
