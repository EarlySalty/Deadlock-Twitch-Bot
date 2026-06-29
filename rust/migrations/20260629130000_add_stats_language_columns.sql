ALTER TABLE public.twitch_stats_category ADD COLUMN IF NOT EXISTS language text;
ALTER TABLE public.twitch_stats_tracked  ADD COLUMN IF NOT EXISTS language text;
