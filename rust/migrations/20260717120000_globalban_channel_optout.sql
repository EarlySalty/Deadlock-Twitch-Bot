ALTER TABLE twitch_partners
    ADD COLUMN IF NOT EXISTS global_ban_enforcement_enabled BOOLEAN NOT NULL DEFAULT TRUE;
