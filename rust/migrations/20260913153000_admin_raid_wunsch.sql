-- Admin-Wahl bleibt erhalten, wenn Token-/Bot-Ban-Selbstheilung technische Flags repariert.
ALTER TABLE twitch_partners
    ADD COLUMN raid_admin_enabled BOOLEAN NOT NULL DEFAULT TRUE;
