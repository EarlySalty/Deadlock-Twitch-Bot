-- Baseline-Nachzug: 14 Tabellen, die der F1-Clean-Baseline-Dump (pg_dump,
-- 20260601000000_baseline_schema.sql) gefehlt haben, obwohl der Rust-Code sie
-- aktiv nutzt. Auf einer frischen DB brachen sonst Affiliate (Geld-Pfad),
-- Title-Generator, Engagement (Soul/Sentiment/Channel-Profile) und die
-- Stream-AI-Reports.
--
-- Schema-Quelle (NUR-LESE-Orakel) sind die Python-Migrationen unter
-- bot/migrations/*.sql. Die DDL ist TREU 1:1 portiert (Spalten, Typen,
-- Constraints, Indizes, FK-REFERENCES, IDENTITY) — Python INTEGER bleibt
-- INTEGER, TEXT-Zeitstempel bleiben TEXT, BYTEA bleibt BYTEA. Keine
-- verhaltensaendernde Typ-"Modernisierung".
--
-- Alles idempotent (CREATE TABLE/INDEX IF NOT EXISTS), additiv, kein DROP.
-- FK-Abhaengigkeiten (twitch_stream_sessions) existieren bereits in der Baseline.

-- ============================================================================
-- Affiliate (Geld-Pfad) — Quelle: bot/migrations/affiliate_schema.sql
-- ============================================================================

-- Vertriebler-Konten
CREATE TABLE IF NOT EXISTS affiliate_accounts (
    twitch_login        TEXT PRIMARY KEY,
    twitch_user_id      TEXT NOT NULL,
    display_name        TEXT,
    email               TEXT NOT NULL,
    full_name           TEXT NOT NULL,
    address_line1       TEXT NOT NULL,
    address_city        TEXT NOT NULL,
    address_zip         TEXT NOT NULL,
    address_country     TEXT NOT NULL DEFAULT 'DE',
    stripe_account_id   TEXT,
    stripe_connected_at TEXT,
    stripe_connect_status TEXT DEFAULT 'pending',
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    is_active           INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS affiliate_pii (
    twitch_login        TEXT PRIMARY KEY REFERENCES affiliate_accounts(twitch_login),
    full_name_enc       BYTEA,
    email_enc           BYTEA,
    address_line1_enc   BYTEA,
    address_city_enc    BYTEA,
    address_zip_enc     BYTEA,
    tax_id_enc          BYTEA,
    address_country     TEXT NOT NULL DEFAULT 'DE',
    ust_status          TEXT NOT NULL DEFAULT 'unknown',
    updated_at          TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_aff_pii_ust_status
    ON affiliate_pii(ust_status);

CREATE TABLE IF NOT EXISTS affiliate_streamer_claims (
    id                      INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    affiliate_twitch_login  TEXT NOT NULL REFERENCES affiliate_accounts(twitch_login),
    claimed_streamer_login  TEXT NOT NULL,
    claimed_at              TEXT NOT NULL,
    UNIQUE (claimed_streamer_login)
);
CREATE INDEX IF NOT EXISTS idx_aff_claims_affiliate
    ON affiliate_streamer_claims(affiliate_twitch_login);

CREATE TABLE IF NOT EXISTS affiliate_commissions (
    id                      INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    affiliate_twitch_login  TEXT NOT NULL REFERENCES affiliate_accounts(twitch_login),
    streamer_login          TEXT NOT NULL,
    stripe_event_id         TEXT UNIQUE NOT NULL,
    stripe_invoice_id       TEXT,
    stripe_customer_id      TEXT,
    stripe_transfer_id      TEXT,
    brutto_cents            INTEGER NOT NULL,
    commission_cents        INTEGER NOT NULL,
    currency                TEXT NOT NULL DEFAULT 'eur',
    status                  TEXT NOT NULL DEFAULT 'pending',
    period_start            TEXT,
    period_end              TEXT,
    created_at              TEXT NOT NULL,
    transferred_at          TEXT,
    error_message           TEXT
);
CREATE INDEX IF NOT EXISTS idx_aff_comm_affiliate
    ON affiliate_commissions(affiliate_twitch_login, status);
CREATE INDEX IF NOT EXISTS idx_aff_comm_streamer
    ON affiliate_commissions(streamer_login);
CREATE INDEX IF NOT EXISTS idx_aff_comm_created_month
    ON affiliate_commissions(affiliate_twitch_login, created_at);

CREATE TABLE IF NOT EXISTS affiliate_gutschrift_counter (
    year_month          TEXT PRIMARY KEY,
    last_seq            INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS affiliate_gutschriften (
    id                      INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    gutschrift_number       TEXT UNIQUE NOT NULL,
    affiliate_twitch_login  TEXT NOT NULL REFERENCES affiliate_accounts(twitch_login),
    period_year             INTEGER NOT NULL,
    period_month            INTEGER NOT NULL,
    net_amount_cents        INTEGER NOT NULL,
    vat_rate_percent        NUMERIC(5,2) NOT NULL DEFAULT 0,
    vat_amount_cents        INTEGER NOT NULL DEFAULT 0,
    gross_amount_cents      INTEGER NOT NULL,
    affiliate_name          TEXT NOT NULL,
    affiliate_address       TEXT NOT NULL,
    affiliate_tax_id        TEXT,
    affiliate_ust_status    TEXT NOT NULL,
    issuer_name             TEXT NOT NULL,
    issuer_address          TEXT NOT NULL,
    issuer_tax_id           TEXT NOT NULL,
    pdf_blob                BYTEA,
    pdf_generated_at        TEXT,
    email_sent_at           TEXT,
    email_error             TEXT,
    commission_ids          TEXT,
    created_at              TEXT NOT NULL,
    UNIQUE (affiliate_twitch_login, period_year, period_month)
);
CREATE INDEX IF NOT EXISTS idx_aff_gutschriften_affiliate
    ON affiliate_gutschriften(affiliate_twitch_login, period_year DESC, period_month DESC);

-- ============================================================================
-- Title-Generator — Quelle: bot/migrations/title_generator_schema.sql
-- ============================================================================

CREATE TABLE IF NOT EXISTS title_generator_knowledge (
    id              SERIAL PRIMARY KEY,
    title           TEXT NOT NULL,
    keywords        TEXT[] DEFAULT '{}',
    game_context    TEXT NOT NULL DEFAULT 'deadlock',
    relative_perf   FLOAT NOT NULL,
    engagement_rate FLOAT NOT NULL,
    history_weight  FLOAT NOT NULL DEFAULT 1.0,
    normalized_score FLOAT NOT NULL,
    streamer_size   TEXT CHECK (streamer_size IN ('small','medium','large')),
    source_streamer TEXT,
    quality_tier    SMALLINT NOT NULL DEFAULT 1 CHECK (quality_tier IN (1,2,3)),
    added_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (title, game_context)
);
CREATE INDEX IF NOT EXISTS idx_tgk_score ON title_generator_knowledge (normalized_score DESC);
CREATE INDEX IF NOT EXISTS idx_tgk_keywords ON title_generator_knowledge USING GIN (keywords);

CREATE TABLE IF NOT EXISTS title_generator_insights (
    id              SERIAL PRIMARY KEY,
    streamer_id     TEXT NOT NULL,
    generated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    period_start    TIMESTAMPTZ NOT NULL,
    period_end      TIMESTAMPTZ NOT NULL,
    strengths       TEXT,
    weaknesses      TEXT,
    patterns        TEXT,
    recommendations TEXT,
    raw_response    JSONB
);
CREATE INDEX IF NOT EXISTS idx_tgi_streamer ON title_generator_insights (streamer_id, generated_at DESC);

-- ============================================================================
-- Twitch-Analytics — Quelle: bot/migrations/twitch_analytics_schema.sql
-- (twitch_chat_word_groups + twitch_stream_ai_reports referenzieren
--  twitch_stream_sessions(id), das bereits in der Baseline existiert.)
-- ============================================================================

-- Core Dimension
CREATE TABLE IF NOT EXISTS streamer_dim (
    twitch_login           TEXT PRIMARY KEY,
    twitch_user_id         TEXT,
    discord_user_id        TEXT,
    discord_display_name   TEXT,
    is_partner             BOOLEAN DEFAULT FALSE,
    is_monitored_only      BOOLEAN DEFAULT FALSE,
    archived_at            TIMESTAMPTZ,
    updated_at             TIMESTAMPTZ DEFAULT NOW()
);

-- Post-Stream: dynamisch erkannte Wortgruppen via Minimax
CREATE TABLE IF NOT EXISTS twitch_chat_word_groups (
    id              BIGSERIAL PRIMARY KEY,
    session_id      BIGINT NOT NULL REFERENCES twitch_stream_sessions(id),
    streamer_login  TEXT NOT NULL,
    group_name      TEXT NOT NULL,
    keywords        TEXT[] NOT NULL,
    message_count   INT DEFAULT 0,
    created_at      TIMESTAMPTZ DEFAULT NOW()
);

-- Post-Stream: vollstaendiger KI-Analysebericht
CREATE TABLE IF NOT EXISTS twitch_stream_ai_reports (
    id                  BIGSERIAL PRIMARY KEY,
    session_id          BIGINT NOT NULL REFERENCES twitch_stream_sessions(id),
    streamer_login      TEXT NOT NULL,
    model               TEXT NOT NULL,
    generated_at        TIMESTAMPTZ DEFAULT NOW(),
    status              TEXT DEFAULT 'pending',
    schema_version      TEXT DEFAULT 'post_stream_report_v1',
    report_variant      TEXT DEFAULT 'compact',
    input_snapshot_json JSONB,
    prompt_version      TEXT,
    started_at          TIMESTAMPTZ DEFAULT NOW(),
    finished_at         TIMESTAMPTZ,
    retry_count         INTEGER DEFAULT 0,
    report_json         JSONB,
    word_groups_json    JSONB,
    error               TEXT
);
CREATE INDEX IF NOT EXISTS idx_stream_ai_reports_streamer
    ON twitch_stream_ai_reports (streamer_login, generated_at DESC);
CREATE INDEX IF NOT EXISTS idx_stream_ai_reports_session
    ON twitch_stream_ai_reports (session_id);
CREATE INDEX IF NOT EXISTS idx_stream_ai_reports_session_variant
    ON twitch_stream_ai_reports (session_id, report_variant, generated_at DESC);

-- ============================================================================
-- Engagement-Soul — Quelle: bot/migrations/soul_schema.sql
-- ============================================================================

CREATE TABLE IF NOT EXISTS twitch_engagement_soul (
    id          BIGSERIAL PRIMARY KEY,
    kind        TEXT NOT NULL,
    content     TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_eng_soul_kind_created
    ON twitch_engagement_soul (kind, created_at DESC);

-- ============================================================================
-- Engagement-Global-Sentiment — Quelle: bot/migrations/global_sentiment_schema.sql
-- ============================================================================

CREATE TABLE IF NOT EXISTS twitch_engagement_global_sentiment (
    id              BIGSERIAL PRIMARY KEY,
    sentiment_text  TEXT NOT NULL,
    msg_count       INT NOT NULL DEFAULT 0,
    model           TEXT,
    built_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_eng_global_sentiment_built
    ON twitch_engagement_global_sentiment (built_at DESC);

-- ============================================================================
-- Engagement-Channel-Profile — Quelle: bot/migrations/channel_profile_schema.sql
-- ============================================================================

CREATE TABLE IF NOT EXISTS twitch_engagement_channel_profile (
    channel_login  TEXT PRIMARY KEY,
    profile_text   TEXT NOT NULL,
    msg_count      INT NOT NULL DEFAULT 0,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
