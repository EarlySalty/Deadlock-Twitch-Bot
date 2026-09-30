-- Bestehende angewandte Migrationen bleiben unverändert.
CREATE TABLE IF NOT EXISTS public.twitch_bot_tokens (
    service_name TEXT PRIMARY KEY CHECK (service_name='twitch-chat'),
    oauth_client_id TEXT NOT NULL,
    twitch_user_id TEXT,
    access_token_enc BYTEA,
    refresh_token_enc BYTEA NOT NULL,
    enc_version INTEGER NOT NULL DEFAULT 1 CHECK(enc_version=1),
    revision BIGINT NOT NULL DEFAULT 0,
    revoked_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Die bestehenden kurzlebigen OAuth-Werte sind bereits gehasht bzw.
-- verschlüsselt. Nur abgelaufene Zustände entfernen, niemals aktive Logins.
DELETE FROM public.oauth_state_tokens WHERE expires_at < now() - interval '7 days';

-- Vor VALIDATE den expliziten Rust-Migrator migrate_resume_sessions ausführen.
-- NOT VALID lässt alte Zeilen bis zum koordinierten Cutover bestehen, verhindert
-- aber neue ungeschützte Sitzungsadressen. Niemals stillschweigend verwerfen.
ALTER TABLE public.twitch_vod_archive_parts
    ADD CONSTRAINT vod_resume_session_encrypted
    CHECK (upload_session_uri IS NULL OR upload_session_uri LIKE 'enc:v1:%') NOT VALID;
