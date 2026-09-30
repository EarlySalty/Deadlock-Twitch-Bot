-- Explizite, reversible Antwortsperren. Keine Nachrichtenlöschung und keine
-- Ableitung eines Bot-Verdachts aus stillem Zuschauen oder bloßer Inaktivität.
-- Die stabile Twitch-ID ist Pflicht. Leerer channel_user_id = kanalübergreifend.
CREATE TABLE twitch_chat_response_blocks (
    chatter_user_id TEXT NOT NULL CHECK (chatter_user_id ~ '^[0-9]+$'),
    channel_user_id TEXT NOT NULL DEFAULT ''
        CHECK (channel_user_id = '' OR channel_user_id ~ '^[0-9]+$'),
    kind TEXT NOT NULL CHECK (kind IN ('viewer_bot', 'scam_bot', 'automation', 'suspected')),
    reason TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    source TEXT NOT NULL CHECK (length(trim(source)) > 0),
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (chatter_user_id, channel_user_id)
);

-- Aktuelle Urteile pro Identität lesen, damit aufgehobene Fehlalarme nicht
-- durch einen älteren Treffer wieder wirksam werden. Kein eigener Judge.
CREATE INDEX twitch_scam_guard_reply_identity_idx
    ON twitch_scam_guard_verdicts (lower(channel_login), chatter_id, created_at DESC, id DESC);
CREATE INDEX twitch_scam_guard_reply_login_idx
    ON twitch_scam_guard_verdicts (lower(channel_login), lower(chatter_login), created_at DESC, id DESC)
    WHERE chatter_id IS NULL OR trim(chatter_id) = '';
