ALTER TABLE twitch_crew_review_events
    ADD COLUMN discord_channel_id BIGINT;

UPDATE twitch_crew_review_events
   SET discord_channel_id = 1374364800817303632
 WHERE discord_message_id IS NOT NULL;

ALTER TABLE twitch_crew_review_events
    ADD CONSTRAINT twitch_crew_review_events_discord_identity_pair_chk
    CHECK ((discord_channel_id IS NULL) = (discord_message_id IS NULL)),
    ADD CONSTRAINT twitch_crew_review_events_discord_channel_positive_chk
    CHECK (discord_channel_id IS NULL OR discord_channel_id > 0);
