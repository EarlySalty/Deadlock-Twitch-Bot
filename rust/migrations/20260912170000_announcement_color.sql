-- Bisher wurden globale Announcements lila gesendet. Bestand unverändert lassen.
ALTER TABLE twitch_global_promo_modes
    ADD COLUMN announcement_color TEXT NOT NULL DEFAULT 'purple'
    CHECK (announcement_color IN ('primary', 'blue', 'green', 'orange', 'purple'));
