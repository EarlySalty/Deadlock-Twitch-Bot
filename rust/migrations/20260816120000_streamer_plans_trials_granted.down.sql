-- Rueckwaerts: nur die neue Spalte entfernen. trial_ever_granted bleibt.

ALTER TABLE streamer_plans
    DROP COLUMN IF EXISTS trials_granted;
