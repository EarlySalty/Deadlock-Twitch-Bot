-- Helix liefert die Zuschauer-ID bereits beim Anwesenheitsschreiben.
ALTER TABLE twitch_viewer_presence_ticks ADD COLUMN viewer_twitch_user_id TEXT;
