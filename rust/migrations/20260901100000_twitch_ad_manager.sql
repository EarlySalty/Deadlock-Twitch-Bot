-- Intelligenter Twitch-Werbemanager: Konfiguration, aktueller Zustand und
-- belastbare Aktions-Queue mit Lease/Audit. Alle Tabellen sind strikt je
-- Twitch-User-ID getrennt; der Dashboard-Pfad nimmt die ID nur aus der Session.

ALTER TABLE twitch_raw_chat_ingest_health
    ADD COLUMN IF NOT EXISTS last_subscription_ok_at TIMESTAMPTZ;

CREATE TABLE IF NOT EXISTS twitch_ad_manager_settings (
    twitch_user_id TEXT PRIMARY KEY,
    twitch_login TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    strategy TEXT NOT NULL DEFAULT 'monitor'
        CHECK (strategy IN ('monitor', 'snooze', 'smart')),
    ad_duration_seconds INTEGER NOT NULL DEFAULT 90
        CHECK (ad_duration_seconds IN (30, 60, 90, 120, 150, 180)),
    min_interval_minutes INTEGER NOT NULL DEFAULT 30
        CHECK (min_interval_minutes BETWEEN 8 AND 180),
    startup_delay_minutes INTEGER NOT NULL DEFAULT 15
        CHECK (startup_delay_minutes BETWEEN 0 AND 180),
    quiet_window_minutes INTEGER NOT NULL DEFAULT 5
        CHECK (quiet_window_minutes BETWEEN 0 AND 60),
    action_lead_seconds INTEGER NOT NULL DEFAULT 60
        CHECK (action_lead_seconds BETWEEN 10 AND 300),
    worker_lease_until TIMESTAMPTZ,
    worker_lease_token TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK ((worker_lease_until IS NULL AND worker_lease_token IS NULL)
        OR (worker_lease_until IS NOT NULL AND worker_lease_token IS NOT NULL))
);

CREATE TABLE IF NOT EXISTS twitch_ad_manager_state (
    twitch_user_id TEXT PRIMARY KEY
        REFERENCES twitch_ad_manager_settings(twitch_user_id) ON DELETE CASCADE,
    twitch_login TEXT NOT NULL,
    is_live BOOLEAN NOT NULL DEFAULT FALSE,
    active_session_id BIGINT,
    stream_started_at TIMESTAMPTZ,
    next_ad_at TIMESTAMPTZ,
    last_ad_at TIMESTAMPTZ,
    duration_seconds INTEGER,
    preroll_free_seconds INTEGER,
    snooze_count INTEGER,
    snooze_refresh_at TIMESTAMPTZ,
    observed_at TIMESTAMPTZ,
    worker_heartbeat_at TIMESTAMPTZ,
    last_history_at TIMESTAMPTZ,
    last_decision TEXT,
    last_decision_reason TEXT,
    last_action_kind TEXT,
    last_action_outcome TEXT,
    last_action_detail TEXT,
    last_action_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS twitch_ad_manager_actions (
    id BIGSERIAL PRIMARY KEY,
    twitch_user_id TEXT NOT NULL
        REFERENCES twitch_ad_manager_settings(twitch_user_id) ON DELETE CASCADE,
    twitch_login TEXT NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('snooze', 'commercial')),
    duration_seconds INTEGER
        CHECK (duration_seconds IS NULL OR duration_seconds IN (30, 60, 90, 120, 150, 180)),
    source TEXT NOT NULL DEFAULT 'manual' CHECK (source IN ('manual', 'automatic')),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'leased', 'succeeded', 'failed', 'unknown', 'unresolved', 'cancelled')),
    due_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    idempotency_key TEXT NOT NULL UNIQUE,
    lease_until TIMESTAMPTZ,
    lease_token TEXT,
    completion_token TEXT,
    preflight_next_ad_at TIMESTAMPTZ,
    preflight_last_ad_at TIMESTAMPTZ,
    preflight_snooze_count INTEGER,
    marked_unknown_at TIMESTAMPTZ,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    requested_by_twitch_user_id TEXT,
    outcome_detail TEXT,
    retry_after_seconds INTEGER CHECK (retry_after_seconds IS NULL OR retry_after_seconds >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    CHECK ((action = 'snooze' AND duration_seconds IS NULL)
        OR (action = 'commercial' AND duration_seconds IS NOT NULL)),
    CHECK ((status = 'leased' AND lease_until IS NOT NULL AND lease_token IS NOT NULL)
        OR (status <> 'leased' AND lease_until IS NULL AND lease_token IS NULL)),
    CHECK ((status = 'unknown' AND completion_token IS NOT NULL)
        OR (status <> 'unknown' AND completion_token IS NULL)),
    CHECK ((source = 'manual' AND requested_by_twitch_user_id IS NOT NULL)
        OR (source = 'automatic' AND requested_by_twitch_user_id IS NULL))
);

-- Je Kanal/Aktionsart darf nur eine fällige/offene Aktion existieren. Das
-- verhindert Doppel-POSTs durch Doppelklicks und parallele Worker.
CREATE UNIQUE INDEX IF NOT EXISTS twitch_ad_manager_actions_one_open_due
    ON twitch_ad_manager_actions (twitch_user_id, action)
    WHERE status IN ('pending', 'leased', 'unknown');

CREATE INDEX IF NOT EXISTS twitch_ad_manager_actions_claim
    ON twitch_ad_manager_actions (due_at, id)
    WHERE status IN ('pending', 'leased');

CREATE INDEX IF NOT EXISTS twitch_ad_manager_actions_completed_retention
    ON twitch_ad_manager_actions (completed_at)
    WHERE completed_at IS NOT NULL;
