-- Dashboard-Session-IDs sind Bearer-Tokens. Ihr Rohwert darf deshalb ebenso
-- wenig wie ein Passwort in PostgreSQL liegen: ein DB-Dump oder eine lesende
-- SQL-Injection dürfte sonst sofort nutzbare eingeloggte Sessions enthalten.
--
-- SHA-256 ist hier passend (anders als bei Nutzerpasswörtern): die IDs und
-- OAuth-State-Tokens werden aus 32 CSPRNG-Bytes erzeugt und sind nicht aus einem
-- kleinen Wörterbuch erratbar. Rust hasht jeden präsentierten Rohwert vor dem
-- Lookup. Das In-place-Backfill erhält dadurch bestehende aktive Sessions.
--
-- Rate-Limit-Hit-IDs sind keine Bearer-Tokens und werden per Präfix-LIKE
-- gezählt; sie bleiben deshalb unverändert.

UPDATE public.dashboard_sessions
SET session_id = encode(sha256(convert_to(session_id, 'UTF8')), 'hex')
WHERE session_type NOT LIKE 'rate_limit:%';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'dashboard_sessions_session_id_sha256'
          AND conrelid = 'public.dashboard_sessions'::regclass
    ) THEN
        ALTER TABLE public.dashboard_sessions
            ADD CONSTRAINT dashboard_sessions_session_id_sha256
            CHECK (
                session_type LIKE 'rate_limit:%'
                OR session_id ~ '^[0-9a-f]{64}$'
            ) NOT VALID;
    END IF;
END
$$;

ALTER TABLE public.dashboard_sessions
    VALIDATE CONSTRAINT dashboard_sessions_session_id_sha256;

-- Die plattformübergreifenden OAuth-State-Tokens sind ebenfalls kurzlebige
-- Bearer-Werte. Alle Rust-Schreib- und Lesepfade verwenden denselben
-- SHA-256-Lookup-Key; das Backfill erhält gerade laufende OAuth-Flows.
UPDATE public.oauth_state_tokens
SET state_token = encode(sha256(convert_to(state_token, 'UTF8')), 'hex')
WHERE state_token !~ '^[0-9a-f]{64}$';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'oauth_state_tokens_state_token_sha256'
          AND conrelid = 'public.oauth_state_tokens'::regclass
    ) THEN
        ALTER TABLE public.oauth_state_tokens
            ADD CONSTRAINT oauth_state_tokens_state_token_sha256
            CHECK (state_token ~ '^[0-9a-f]{64}$') NOT VALID;
    END IF;
END
$$;

ALTER TABLE public.oauth_state_tokens
    VALIDATE CONSTRAINT oauth_state_tokens_state_token_sha256;

-- PKCE-Verifier müssen für den Code-Tausch reversibel bleiben und werden
-- deshalb mit dem vorhandenen AES-256-GCM-Feldschlüssel verschlüsselt. Alte
-- Social-Media-Flows laufen höchstens zehn Minuten; beim Offline-Cutover werden
-- sie bewusst verworfen, statt einen Klartext-Fallback im neuen Code zu lassen.
DELETE FROM public.oauth_state_tokens
WHERE platform IN ('tiktok', 'youtube')
  AND pkce_verifier IS NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'oauth_state_tokens_pkce_encrypted'
          AND conrelid = 'public.oauth_state_tokens'::regclass
    ) THEN
        ALTER TABLE public.oauth_state_tokens
            ADD CONSTRAINT oauth_state_tokens_pkce_encrypted
            CHECK (
                platform NOT IN ('tiktok', 'youtube')
                OR pkce_verifier IS NULL
                OR pkce_verifier ~ '^enc:v1:[A-Za-z0-9_-]+$'
            ) NOT VALID;
    END IF;
END
$$;

ALTER TABLE public.oauth_state_tokens
    VALIDATE CONSTRAINT oauth_state_tokens_pkce_encrypted;
