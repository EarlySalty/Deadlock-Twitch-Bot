-- Auto-Raid-Pause: Broadcaster oder Mod kann die automatischen Raids fuer den
-- eigenen Kanal befristet aussetzen (Standard 12h). Ausloeser sind der Chat-
-- Command !raidpause und der Dashboard-Toggle. Nach Ablauf des Zeitstempels
-- laufen die Auto-Raids ohne weiteres Zutun wieder an.
--
-- Bewusst eine eigene Spalte auf twitch_partners statt einer neuen Tabelle:
-- die Eligibility-Pruefung laedt ohnehin schon die Partner-Zeile, ein weiterer
-- selektierter Wert reicht. Der Ablauf ist ein reiner now()-Vergleich, es
-- braucht keinen Sweep und keinen Cleanup-Job. NULL oder Vergangenheit heisst
-- "Auto-Raids laufen normal".
ALTER TABLE public.twitch_partners
    ADD COLUMN IF NOT EXISTS auto_raid_paused_until timestamptz;

COMMENT ON COLUMN public.twitch_partners.auto_raid_paused_until IS
    'Zeitpunkt, bis zu dem Auto-Raids fuer diesen Kanal ausgesetzt sind. NULL oder Vergangenheit = Auto-Raids laufen normal.';

-- Nur die Minderheit der Kanaele ist gleichzeitig pausiert, deshalb ein
-- partieller Index fuer die Eligibility-Abfrage.
CREATE INDEX IF NOT EXISTS twitch_partners_auto_raid_pause_idx
    ON public.twitch_partners (auto_raid_paused_until)
    WHERE auto_raid_paused_until IS NOT NULL;
