-- Auditiert die Zustellung von Promo-Announcements und deren spaetere Loeschung.
-- Der Send-Endpoint fuer Announcements liefert keine Twitch-Message-ID; diese
-- wird nachgelagert aus channel.chat.notification (notice_type=announcement)
-- korreliert. channel.chat.message_delete liefert danach die konkrete ID.

CREATE TABLE IF NOT EXISTS public.twitch_promo_delivery_audit (
    id                    BIGSERIAL PRIMARY KEY,
    channel_login         TEXT NOT NULL,
    broadcaster_user_id   TEXT NOT NULL,
    source                TEXT NOT NULL,
    message_text          TEXT NOT NULL,
    send_accepted_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    twitch_message_id     TEXT,
    announcement_seen_at  TIMESTAMPTZ,
    deleted_at            TIMESTAMPTZ,
    deleted_target_user_id TEXT,
    bot_log_sent_at        TIMESTAMPTZ,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_twitch_promo_delivery_message_id
    ON public.twitch_promo_delivery_audit (twitch_message_id)
    WHERE twitch_message_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_twitch_promo_delivery_channel_time
    ON public.twitch_promo_delivery_audit (channel_login, send_accepted_at DESC);

CREATE INDEX IF NOT EXISTS idx_twitch_promo_delivery_unbound
    ON public.twitch_promo_delivery_audit (broadcaster_user_id, send_accepted_at DESC)
    WHERE twitch_message_id IS NULL;

-- Delete-Events werden separat gehalten, damit ein sehr schnelles Loeschen
-- nicht verloren geht, falls Twitch das message_delete vor dem Announcement-
-- Notification-Event zustellt. Beim spaeteren Binden wird deleted_at nachgezogen.
CREATE TABLE IF NOT EXISTS public.twitch_bot_message_delete_events (
    twitch_message_id      TEXT PRIMARY KEY,
    broadcaster_user_id    TEXT NOT NULL,
    broadcaster_user_login TEXT,
    target_user_id          TEXT,
    deleted_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_twitch_bot_message_delete_channel_time
    ON public.twitch_bot_message_delete_events (broadcaster_user_id, deleted_at DESC);

CREATE TABLE IF NOT EXISTS public.twitch_bot_announcement_events (
    twitch_message_id   TEXT PRIMARY KEY,
    broadcaster_user_id TEXT NOT NULL,
    channel_login       TEXT NOT NULL,
    chatter_user_id     TEXT NOT NULL,
    message_text        TEXT NOT NULL,
    seen_at             TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_twitch_bot_announcement_events_match
    ON public.twitch_bot_announcement_events (broadcaster_user_id, channel_login, seen_at DESC);
