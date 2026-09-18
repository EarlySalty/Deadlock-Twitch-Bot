ALTER TABLE public.twitch_partner_raid_scores
    ADD COLUMN IF NOT EXISTS viewer_fairness_score DOUBLE PRECISION NOT NULL DEFAULT 0.5;

ALTER TABLE public.twitch_partner_raid_scores
    ADD COLUMN IF NOT EXISTS sent_viewers_30d BIGINT NOT NULL DEFAULT 0;

ALTER TABLE public.twitch_partner_raid_scores
    ADD COLUMN IF NOT EXISTS received_viewers_30d BIGINT NOT NULL DEFAULT 0;
