-- Entfernt in Produktivdatenbanken die tote id-Spalte samt Sequenz und setzt den
-- Primaerschluessel auf twitch_login. Frische Datenbanken sind bereits
-- login-keyed und laufen durch den Guard auf id-Spalten-Existenz als No-op.
-- Fuer den Constraint-Tausch wird der einzige eingehende FK
-- social_media_streamer_layout.streamer_login -> twitch_streamers.twitch_login
-- geloest und anschliessend wieder mit ON DELETE CASCADE gesetzt.

DO $$
BEGIN
  IF EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_schema='public' AND table_name='twitch_streamers' AND column_name='id'
  ) THEN
    ALTER TABLE public.social_media_streamer_layout
      DROP CONSTRAINT IF EXISTS social_media_streamer_layout_streamer_login_fkey;
    ALTER TABLE public.twitch_streamers DROP CONSTRAINT IF EXISTS twitch_streamers_pkey;
    ALTER TABLE public.twitch_streamers DROP CONSTRAINT IF EXISTS twitch_streamers_twitch_login_key;
    ALTER TABLE public.twitch_streamers ADD CONSTRAINT twitch_streamers_pkey PRIMARY KEY (twitch_login);
    ALTER TABLE public.twitch_streamers DROP COLUMN id;
    DROP SEQUENCE IF EXISTS public.twitch_streamers_id_seq;
    ALTER TABLE public.social_media_streamer_layout
      ADD CONSTRAINT social_media_streamer_layout_streamer_login_fkey
      FOREIGN KEY (streamer_login) REFERENCES public.twitch_streamers(twitch_login) ON DELETE CASCADE;
  END IF;
END $$;
