-- Deadlock-Pause: der Bot gibt seine Mod-Rechte in Kanaelen ab, die seit
-- Monaten kein Deadlock mehr streamen, und holt sie sich beim Comeback zurueck.
--
-- Bewusst eine eigene Spalte statt eines weiteren `technical_pause_reason`-Wertes:
-- die Partnerschaft laeuft weiter, nur die Mod-Rechte ruhen. Ein Pause-Reason
-- wuerde Raid-, Restore- und Grace-Sweeps mitziehen, die hier nichts zu suchen
-- haben. NULL heisst "Bot ist regulaer Moderator".
ALTER TABLE public.twitch_partners
    ADD COLUMN IF NOT EXISTS deadlock_pause_unmodded_at text;

COMMENT ON COLUMN public.twitch_partners.deadlock_pause_unmodded_at IS
    'ISO-Zeitpunkt, zu dem der Bot wegen Deadlock-Pause entmoddet wurde. NULL = Bot ist Moderator.';

-- Der Sweep sucht ueber diese Spalte nach Kanaelen in der Pause; die sind in der
-- Minderheit, deshalb ein partieller Index.
CREATE INDEX IF NOT EXISTS twitch_partners_deadlock_pause_idx
    ON public.twitch_partners (deadlock_pause_unmodded_at)
    WHERE deadlock_pause_unmodded_at IS NOT NULL;
