-- Requirements-DMs dürfen pro Twitch-Nutzer und Zweck nur einmal beansprucht
-- werden. Der Runtime-Bot besitzt bewusst keine Schema-Rechte; deshalb ist die
-- Dedupe-Tabelle Teil des migrationsverwalteten Vertrags.

CREATE TABLE IF NOT EXISTS public.twitch_raid_requirements_dm_dedupe (
    twitch_user_id TEXT NOT NULL,
    purpose TEXT NOT NULL,
    twitch_login TEXT NOT NULL DEFAULT '',
    discord_user_id TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    message_id TEXT,
    error_message TEXT,
    claimed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    sent_at TEXT,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (twitch_user_id, purpose)
);
