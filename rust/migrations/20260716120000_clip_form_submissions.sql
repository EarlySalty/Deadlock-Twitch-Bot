CREATE TABLE IF NOT EXISTS twitch_clip_form_submissions (
    id SERIAL PRIMARY KEY,
    clip_id INTEGER NOT NULL,
    form_key TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    http_status INTEGER,
    error TEXT,
    submitted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (clip_id, form_key)
);
