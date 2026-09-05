CREATE TABLE IF NOT EXISTS public.twitch_chat_message_labels (
    message_id  BIGINT PRIMARY KEY,
    label       TEXT NOT NULL,
    quelle      TEXT NOT NULL CHECK (quelle IN ('regel', 'modell')),
    modell      TEXT,
    erstellt_am TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_twitch_chat_message_labels_label
    ON public.twitch_chat_message_labels (label);
