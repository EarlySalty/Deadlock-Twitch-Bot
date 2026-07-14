-- ponytail: Analytics filtert bewusst noch nicht; falls Chatter-Zahlen stoeren:
-- WHERE moderation_action IS NULL.
ALTER TABLE twitch_chat_messages
    ADD COLUMN IF NOT EXISTS moderation_action TEXT;

ALTER TABLE twitch_chat_messages
    ADD COLUMN IF NOT EXISTS moderation_reason TEXT;

CREATE INDEX IF NOT EXISTS twitch_chat_messages_moderation_idx
    ON twitch_chat_messages (moderation_action)
    WHERE moderation_action IS NOT NULL;

ALTER TABLE tb_chat_autoban_log ADD COLUMN IF NOT EXISTS action TEXT;
ALTER TABLE tb_chat_autoban_log ADD COLUMN IF NOT EXISTS source_path TEXT;
ALTER TABLE tb_chat_autoban_log ADD COLUMN IF NOT EXISTS reason TEXT;
ALTER TABLE tb_chat_autoban_log ADD COLUMN IF NOT EXISTS score REAL;
ALTER TABLE tb_chat_autoban_log ADD COLUMN IF NOT EXISTS account_age_days BIGINT;

ALTER TABLE tb_chat_autoban_log ALTER COLUMN channel_login DROP NOT NULL;
ALTER TABLE tb_chat_autoban_log ALTER COLUMN chatter_login DROP NOT NULL;
