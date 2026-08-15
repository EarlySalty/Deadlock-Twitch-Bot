-- Trial-Zaehler: Boolean trial_ever_granted bleibt, Backfill auf trials_granted.
-- Nicht auf Prod anwenden ohne Freigabe.

ALTER TABLE streamer_plans
    ADD COLUMN IF NOT EXISTS trials_granted INTEGER NOT NULL DEFAULT 0;

UPDATE streamer_plans
   SET trials_granted = 1
 WHERE COALESCE(trial_ever_granted, 0) = 1
   AND COALESCE(trials_granted, 0) = 0;
