-- Ein Twitch-Login ist kein stabiler Schlüssel: benennt sich ein Kanal um,
-- schreibt der Rename `twitch_streamers.twitch_login` fort. Der Fremdschlüssel
-- aus social_media_streamer_layout kannte bisher nur ON DELETE CASCADE, also
-- scheiterte jede Umbenennung mit 23503, sobald dort eine Zeile lag.
ALTER TABLE social_media_streamer_layout
    DROP CONSTRAINT IF EXISTS social_media_streamer_layout_streamer_login_fkey;

ALTER TABLE social_media_streamer_layout
    ADD CONSTRAINT social_media_streamer_layout_streamer_login_fkey
    FOREIGN KEY (streamer_login) REFERENCES twitch_streamers (twitch_login)
    ON UPDATE CASCADE ON DELETE CASCADE;
