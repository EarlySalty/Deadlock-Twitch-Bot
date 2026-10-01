UPDATE public.social_media_category
   SET twitch_game_id = '2132205352'
 WHERE category_key = 'deadlock'
   AND (twitch_game_id IS NULL OR twitch_game_id = '');
