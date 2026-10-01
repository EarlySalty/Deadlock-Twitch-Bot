ALTER TABLE public.streamer_plans
    ALTER COLUMN greeting_reply_enabled SET DEFAULT 0;

UPDATE public.streamer_plans
   SET greeting_reply_enabled = 0
 WHERE greeting_reply_enabled <> 0;
