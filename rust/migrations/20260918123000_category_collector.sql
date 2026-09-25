-- Kategoriesammler Deadlock (tb-category-collector): Tabellen für die
-- dauerhafte, weltweit alle Sprachen umfassende Beobachtung der
-- Twitch-Kategorie Deadlock. TIMESTAMPTZ durchgehend, UTC; stabile
-- Twitch-IDs (user_id/stream_id) statt Login-Identität.

-- Konfiguration: eine Zeile (id = 1). Der Sammler liest sie zyklisch; es
-- gibt bewusst keine ENV-Optionen für das Sammelverhalten. retention_days
-- ist verpflichtend mindestens 90 (Rohchat-Default), discovery alle Minute.
CREATE TABLE category_collector_config (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    deadlock_game_id TEXT NOT NULL,
    discovery_interval_seconds INTEGER NOT NULL DEFAULT 60
        CHECK (discovery_interval_seconds BETWEEN 30 AND 3600),
    chat_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    roster_decay_seconds INTEGER NOT NULL DEFAULT 900
        CHECK (roster_decay_seconds BETWEEN 60 AND 86400),
    retention_days INTEGER NOT NULL DEFAULT 90
        CHECK (retention_days >= 90),
    rollup_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    vod_metadata_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    -- NULL = Minuten-Snapshots dauerhaft behalten (Default-Entscheidung);
    -- gesetzt kürzt der Retention-Job auch Snapshots ab diesem Alter.
    snapshot_retention_days INTEGER
        CHECK (snapshot_retention_days IS NULL OR snapshot_retention_days >= 30),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO category_collector_config (id, deadlock_game_id)
VALUES (1, '142775730');

-- Eine Discovery/Beobachtung = ein Poll. Status und Fehler bleiben sichtbar;
-- unvollständige Polls erzeugen KEINE Offline-/Null-Zeilen, werden aber als
-- Ereignis geloggt (Abdeckungs-/Lücken-Nachweis für die Auswertung).
CREATE TABLE category_polls (
    poll_id BIGSERIAL PRIMARY KEY,
    started_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'running'
        CHECK (status IN ('running', 'complete', 'incomplete', 'failed')),
    stream_count INTEGER NOT NULL DEFAULT 0,
    viewer_total BIGINT NOT NULL DEFAULT 0,
    page_count INTEGER NOT NULL DEFAULT 0,
    error_detail TEXT
);
CREATE INDEX category_polls_started_at_idx ON category_polls (started_at DESC);

-- Stammdaten je Kanal (user_id ist die stabile Identität).
CREATE TABLE category_channels (
    user_id TEXT PRIMARY KEY,
    login TEXT NOT NULL,
    display_name TEXT,
    description TEXT,
    broadcaster_type TEXT,
    broadcaster_language TEXT,
    profile_created_at TIMESTAMPTZ,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_live_at TIMESTAMPTZ
);

-- Zeitreihe je beobachtetem Live-Stream: ein Zeile je Stream und Poll.
-- Snapshots bleiben dauerhaft (kleine Zeilen, Aggregationsbasis).
-- tags sind die aktuellen String-Tags (nicht veraltete tag_ids).
CREATE TABLE category_stream_snapshots (
    snapshot_id BIGSERIAL PRIMARY KEY,
    poll_id BIGINT REFERENCES category_polls (poll_id) ON DELETE SET NULL,
    snapshot_at TIMESTAMPTZ NOT NULL,
    stream_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    user_login TEXT NOT NULL,
    viewer_count INTEGER NOT NULL DEFAULT 0,
    title TEXT,
    language TEXT,
    started_at TIMESTAMPTZ,
    tags JSONB,
    is_mature BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE INDEX category_snapshots_time_idx ON category_stream_snapshots (snapshot_at);
CREATE INDEX category_snapshots_stream_time_idx
    ON category_stream_snapshots (stream_id, snapshot_at);
CREATE INDEX category_snapshots_user_time_idx
    ON category_stream_snapshots (user_id, snapshot_at);

-- Roh-Chat (Storage-Treiber): anonym justinfan gelesen, 90 Tage Retention.
-- message_id mit deterministischem Fallback, damit (room_user_id, message_id)
-- idempotent bleibt; source_message_id markiert Shared-Chat als Weiterleitung
-- fremder Aktivität (zählt nicht als originale Aktivität). Rohtext verlässt
-- die DB nie über die normale Admin-API.
CREATE TABLE category_chat_messages (
    room_user_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    source_message_id TEXT,
    sent_at TIMESTAMPTZ NOT NULL,
    chatter_user_id TEXT NOT NULL,
    chatter_login TEXT NOT NULL,
    message_text TEXT NOT NULL,
    text_len INTEGER NOT NULL DEFAULT 0,
    detected_lang TEXT,
    lang_confidence REAL,
    lang_method TEXT NOT NULL DEFAULT 'unknown',
    emote_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (room_user_id, message_id)
);
CREATE INDEX category_chat_messages_sent_at_idx
    ON category_chat_messages (sent_at);
CREATE INDEX category_chat_messages_room_time_idx
    ON category_chat_messages (room_user_id, sent_at);

-- Stunden-Rollup je Kanal und Sprachdimension. distinct_chatters gilt
-- ausschließlich innerhalb genau dieser Stunde + Kanal + Sprache und darf
-- in Auswertungen niemals über Stunden/Tage summiert werden.
CREATE TABLE category_chat_rollup (
    hour_bucket TIMESTAMPTZ NOT NULL,
    room_user_id TEXT NOT NULL,
    lang TEXT,
    messages BIGINT NOT NULL DEFAULT 0,
    distinct_chatters BIGINT NOT NULL DEFAULT 0,
    total_len BIGINT NOT NULL DEFAULT 0,
    avg_len REAL,
    computed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (hour_bucket, room_user_id, lang)
);
CREATE INDEX category_rollup_hour_idx ON category_chat_rollup (hour_bucket);

-- Sparsame VOD-Metadaten je Kanal (nur Helix-Listenfelder; niemals
-- Video/Audio-Download, kein STT).
CREATE TABLE category_channel_vods (
    vod_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    created_at TIMESTAMPTZ,
    duration TEXT,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX category_vods_user_idx ON category_channel_vods (user_id, created_at DESC);

-- Laufzeit-/Gesundheitsstatus des Sammlers (eine Zeile, id = 1). Die
-- Drop-Zähler stammen aus dem anonymen Chat-Transport und belegen mögliche
-- Chat-Lücken sichtbar, statt sie zu verschweigen.
CREATE TABLE category_collector_status (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_poll_started_at TIMESTAMPTZ,
    last_complete_poll_at TIMESTAMPTZ,
    last_error TEXT,
    last_error_at TIMESTAMPTZ,
    roster_size INTEGER NOT NULL DEFAULT 0,
    chat_privmsgs_dispatched BIGINT NOT NULL DEFAULT 0,
    chat_privmsgs_dropped BIGINT NOT NULL DEFAULT 0,
    chat_queue_dropped BIGINT NOT NULL DEFAULT 0,
    chat_commands_dropped BIGINT NOT NULL DEFAULT 0,
    chat_invalid_logins_rejected BIGINT NOT NULL DEFAULT 0,
    chat_reconnects BIGINT NOT NULL DEFAULT 0,
    chat_duplicates_skipped BIGINT NOT NULL DEFAULT 0,
    chat_redactions BIGINT NOT NULL DEFAULT 0,
    retention_deleted_total BIGINT NOT NULL DEFAULT 0,
    last_retention_run_at TIMESTAMPTZ,
    last_rollup_hour TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO category_collector_status (id) VALUES (1);
