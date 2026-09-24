-- Schema-Gate-Fixture: DDL des brain-Schemas aus der zentralen PostgreSQL.
--
-- Der Online-SQLx-Check (Workflow rust-sqlx-check.yml, Job schema-gate) prüft
-- auch die Makros der gepinnten dbrain-Crates aus Deadlock-Brain; deren
-- Tabellen liegen im Schema `brain` und werden von rust/migrations nicht
-- angelegt. Diese Datei enthält daher den Schema-Abbild (ohne Daten) der
-- Produktions-DB zum gepinnten Stand in rust/Cargo.lock.
--
-- Erzeugen mit:
--   pg_dump --schema-only --no-owner --no-privileges --schema=brain deadlock \
--     > rust/schema-gate/brain-schema.sql
-- Views sind danach zu entfernen (brain.patch_changes hängt am Schema
-- `patchnotes`, das im Gate nicht existiert); hier ist sie herausgeschnitten.
-- Nach jedem Bump des dbrain-Pins neu erzeugen, sonst schlägt der
-- Online-Check mit `relation "brain.*" does not exist` fehl.

--
-- PostgreSQL database dump
--

\restrict uPhOXcpyTf08SmcOCxE9CTWZBgYRs9IXEeBKgt8tMEnutRPLT67EDSYE0owgVxW

-- Dumped from database version 16.14 (Ubuntu 16.14-1.pgdg24.04+1)
-- Dumped by pg_dump version 16.15 (Ubuntu 16.15-0ubuntu0.24.04.1)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: brain; Type: SCHEMA; Schema: -; Owner: -
--

CREATE SCHEMA brain;


SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: analysis_notes; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.analysis_notes (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    query text NOT NULL,
    entity_type text,
    entity_name text,
    context_kind text NOT NULL,
    context_hash text NOT NULL,
    prompt_version text NOT NULL,
    prompt_text text NOT NULL,
    result_text text,
    model text,
    confidence double precision,
    status text NOT NULL,
    source_references jsonb DEFAULT '[]'::jsonb NOT NULL,
    context jsonb NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: analysis_notes_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.analysis_notes_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: analysis_notes_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.analysis_notes_id_seq OWNED BY brain.analysis_notes.id;


--
-- Name: build_learning_notes; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.build_learning_notes (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    learned_build_id bigint,
    legacy_learned_build_id bigint,
    hero_name text,
    source text NOT NULL,
    context_hash text NOT NULL,
    prompt_version text NOT NULL,
    prompt_text text NOT NULL,
    result_text text,
    insights jsonb DEFAULT '{}'::jsonb NOT NULL,
    model text,
    status text NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: build_learning_notes_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.build_learning_notes_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: build_learning_notes_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.build_learning_notes_id_seq OWNED BY brain.build_learning_notes.id;


--
-- Name: current_entity_state; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.current_entity_state (
    id bigint NOT NULL,
    entity_type text NOT NULL,
    entity_name text NOT NULL,
    state_kind text NOT NULL,
    winning_event_id bigint,
    winning_snapshot_id bigint,
    source text NOT NULL,
    source_priority integer NOT NULL,
    valid_from timestamp with time zone,
    observed_at timestamp with time zone DEFAULT now() NOT NULL,
    content_hash text NOT NULL,
    payload jsonb NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: current_entity_state_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.current_entity_state_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: current_entity_state_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.current_entity_state_id_seq OWNED BY brain.current_entity_state.id;


--
-- Name: entities; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.entities (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    entity_type text NOT NULL,
    canonical_name text NOT NULL,
    primary_external_id text,
    source text NOT NULL,
    first_snapshot_id bigint,
    legacy_first_snapshot_id bigint,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: entities_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.entities_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: entities_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.entities_id_seq OWNED BY brain.entities.id;


--
-- Name: entity_aliases; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.entity_aliases (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    entity_id bigint,
    legacy_entity_id bigint,
    alias text NOT NULL,
    alias_norm text NOT NULL,
    alias_kind text NOT NULL,
    source text NOT NULL,
    external_id text,
    snapshot_id bigint,
    legacy_snapshot_id bigint,
    created_at timestamp with time zone NOT NULL
);


--
-- Name: entity_aliases_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.entity_aliases_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: entity_aliases_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.entity_aliases_id_seq OWNED BY brain.entity_aliases.id;


--
-- Name: entity_lineage; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.entity_lineage (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    patch_event_id bigint,
    legacy_patch_event_id bigint NOT NULL,
    relation_type text NOT NULL,
    source_entity_type text,
    source_name text NOT NULL,
    source_name_norm text NOT NULL,
    target_entity_type text,
    target_name text,
    target_name_norm text,
    owner_entity_type text,
    owner_name text,
    owner_name_norm text,
    confidence double precision NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: entity_lineage_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.entity_lineage_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: entity_lineage_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.entity_lineage_id_seq OWNED BY brain.entity_lineage.id;


--
-- Name: entity_snapshots; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.entity_snapshots (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    source text NOT NULL,
    entity_type text NOT NULL,
    external_id text NOT NULL,
    canonical_name text,
    payload_hash text NOT NULL,
    payload jsonb NOT NULL,
    fetched_at timestamp with time zone NOT NULL,
    source_document_id bigint,
    legacy_source_document_id bigint,
    imported_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: entity_snapshots_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.entity_snapshots_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: entity_snapshots_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.entity_snapshots_id_seq OWNED BY brain.entity_snapshots.id;


--
-- Name: feeder_runs; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.feeder_runs (
    id bigint NOT NULL,
    run_at timestamp with time zone DEFAULT now() NOT NULL,
    period_start timestamp with time zone NOT NULL,
    period_end timestamp with time zone NOT NULL,
    digest_path text,
    gesehen integer,
    relevant integer,
    kategorien text[],
    status text NOT NULL,
    error text,
    committed boolean DEFAULT false NOT NULL,
    pushed boolean DEFAULT false NOT NULL,
    CONSTRAINT feeder_runs_status_check CHECK ((status = ANY (ARRAY['ok'::text, 'partial'::text, 'error'::text])))
);


--
-- Name: feeder_runs_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

ALTER TABLE brain.feeder_runs ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME brain.feeder_runs_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: forum_claims; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.forum_claims (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    post_snapshot_id bigint,
    legacy_post_snapshot_id bigint NOT NULL,
    thread_id text NOT NULL,
    post_id text NOT NULL,
    thread_title text,
    source_url text NOT NULL,
    posted_at timestamp with time zone,
    author text,
    author_role text,
    claim_hash text NOT NULL,
    claim_index bigint NOT NULL,
    claim_type text NOT NULL,
    entity_type text,
    entity_name text,
    claim_text text NOT NULL,
    evidence_quote text NOT NULL,
    source_trust text NOT NULL,
    validity_status text NOT NULL,
    currentness text NOT NULL,
    confidence double precision NOT NULL,
    safety_labels jsonb DEFAULT '[]'::jsonb NOT NULL,
    source_references jsonb DEFAULT '[]'::jsonb NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL,
    imported_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: forum_claims_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.forum_claims_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: forum_claims_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.forum_claims_id_seq OWNED BY brain.forum_claims.id;


--
-- Name: hero_ability_orders; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.hero_ability_orders (
    hero_id bigint NOT NULL,
    bracket text NOT NULL,
    abilities jsonb NOT NULL,
    wins bigint NOT NULL,
    losses bigint NOT NULL,
    matches bigint NOT NULL,
    players bigint NOT NULL,
    patch_tag text NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: hero_catalog; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.hero_catalog (
    hero_id bigint NOT NULL,
    name text NOT NULL,
    base_health bigint NOT NULL,
    archetype text NOT NULL,
    stats jsonb NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: hero_item_stats; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.hero_item_stats (
    hero_id bigint NOT NULL,
    item_id bigint NOT NULL,
    bracket text NOT NULL,
    prevalence_builds bigint NOT NULL,
    wins bigint NOT NULL,
    losses bigint NOT NULL,
    matches bigint NOT NULL,
    players bigint NOT NULL,
    avg_buy_time_relative double precision,
    lift_pp double precision,
    patch_tag text NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: hero_item_synergies; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.hero_item_synergies (
    hero_id bigint NOT NULL,
    item_id bigint NOT NULL,
    with_item_id bigint NOT NULL,
    wins bigint NOT NULL,
    losses bigint NOT NULL,
    matches bigint NOT NULL,
    patch_tag text NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: hero_stat_profiles; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.hero_stat_profiles (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    snapshot_id bigint NOT NULL,
    legacy_snapshot_id bigint NOT NULL,
    entity_id bigint,
    legacy_entity_id bigint,
    hero_name text NOT NULL,
    source text NOT NULL,
    external_id text NOT NULL,
    payload_hash text NOT NULL,
    row_number bigint,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: hero_stat_profiles_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.hero_stat_profiles_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: hero_stat_profiles_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.hero_stat_profiles_id_seq OWNED BY brain.hero_stat_profiles.id;


--
-- Name: hero_stat_values; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.hero_stat_values (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    profile_id bigint NOT NULL,
    legacy_profile_id bigint NOT NULL,
    entity_id bigint,
    legacy_entity_id bigint,
    hero_name text NOT NULL,
    stat_key text NOT NULL,
    stat_label text NOT NULL,
    numeric_value double precision,
    raw_value text NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: hero_stat_values_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.hero_stat_values_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: hero_stat_values_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.hero_stat_values_id_seq OWNED BY brain.hero_stat_values.id;


--
-- Name: insight_records; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.insight_records (
    id bigint NOT NULL,
    insight_hash text NOT NULL,
    insight_type text NOT NULL,
    entity_type text,
    entity_name text,
    subject text,
    summary text NOT NULL,
    reason text,
    validity_status text NOT NULL,
    currentness text NOT NULL,
    trust_tier text NOT NULL,
    confidence double precision DEFAULT 0.0 NOT NULL,
    occurred_at timestamp with time zone,
    observed_at timestamp with time zone DEFAULT now() NOT NULL,
    source_patch_event_ids bigint[] DEFAULT '{}'::bigint[] NOT NULL,
    source_urls text[] DEFAULT '{}'::text[] NOT NULL,
    source_references jsonb DEFAULT '[]'::jsonb NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: insight_records_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.insight_records_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: insight_records_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.insight_records_id_seq OWNED BY brain.insight_records.id;


--
-- Name: item_catalog; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.item_catalog (
    item_id bigint NOT NULL,
    name text NOT NULL,
    slot_type text NOT NULL,
    tier bigint NOT NULL,
    defense_kind jsonb NOT NULL,
    damage_axis text NOT NULL,
    properties jsonb NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: knowledge_events; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.knowledge_events (
    id bigint NOT NULL,
    event_hash text NOT NULL,
    event_source text NOT NULL,
    source_table text NOT NULL,
    source_legacy_id bigint,
    source_document_id bigint,
    snapshot_id bigint,
    patch_event_id bigint,
    forum_claim_id bigint,
    entity_type text,
    entity_name text,
    subject text,
    event_type text NOT NULL,
    validity_status text NOT NULL,
    currentness text NOT NULL,
    trust_tier text NOT NULL,
    source_url text,
    occurred_at timestamp with time zone,
    observed_at timestamp with time zone DEFAULT now() NOT NULL,
    effective_from timestamp with time zone,
    effective_to timestamp with time zone,
    raw_text text,
    normalized_text text,
    evidence_quote text,
    old_value text,
    new_value text,
    confidence double precision DEFAULT 0.0 NOT NULL,
    safety_labels jsonb DEFAULT '[]'::jsonb NOT NULL,
    source_references jsonb DEFAULT '[]'::jsonb NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: knowledge_events_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.knowledge_events_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: knowledge_events_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.knowledge_events_id_seq OWNED BY brain.knowledge_events.id;


--
-- Name: learned_builds; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.learned_builds (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    source text NOT NULL,
    source_build_id text NOT NULL,
    hero_id bigint,
    hero_name text,
    language bigint,
    source_rank bigint,
    quality_tier text NOT NULL,
    quality_score double precision NOT NULL,
    name text,
    author_account_id text,
    description text,
    tags jsonb DEFAULT '[]'::jsonb NOT NULL,
    details jsonb DEFAULT '{}'::jsonb NOT NULL,
    item_names jsonb DEFAULT '[]'::jsonb NOT NULL,
    ability_order jsonb DEFAULT '[]'::jsonb NOT NULL,
    source_metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    imported_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: learned_builds_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.learned_builds_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: learned_builds_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.learned_builds_id_seq OWNED BY brain.learned_builds.id;


--
-- Name: legacy_entities; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.legacy_entities (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    legacy_type text NOT NULL,
    canonical_name text NOT NULL,
    name_norm text NOT NULL,
    observed_entity_type text NOT NULL,
    first_patch_event_id bigint,
    last_patch_event_id bigint,
    legacy_first_patch_event_id bigint,
    legacy_last_patch_event_id bigint,
    first_seen_at timestamp with time zone,
    last_seen_at timestamp with time zone,
    event_count bigint NOT NULL,
    confidence double precision NOT NULL,
    status text NOT NULL,
    samples jsonb DEFAULT '[]'::jsonb NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: legacy_entities_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.legacy_entities_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: legacy_entities_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.legacy_entities_id_seq OWNED BY brain.legacy_entities.id;


--
-- Name: mechanic_notes; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.mechanic_notes (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    title text NOT NULL,
    content text NOT NULL,
    source text NOT NULL,
    category text NOT NULL,
    sqlite_rowid bigint NOT NULL,
    created_at timestamp with time zone NOT NULL
);


--
-- Name: mechanic_notes_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.mechanic_notes_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: mechanic_notes_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.mechanic_notes_id_seq OWNED BY brain.mechanic_notes.id;


--
-- Name: meta_trend_notes; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.meta_trend_notes (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    entity_name text NOT NULL,
    trend_direction text NOT NULL,
    winrate_delta double precision NOT NULL,
    context jsonb DEFAULT '{}'::jsonb NOT NULL,
    result_text text,
    status text NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: meta_trend_notes_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.meta_trend_notes_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: meta_trend_notes_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.meta_trend_notes_id_seq OWNED BY brain.meta_trend_notes.id;


--
-- Name: patch_event_enrichments; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.patch_event_enrichments (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    patch_event_id bigint,
    legacy_patch_event_id bigint NOT NULL,
    stat_name text,
    old_value text,
    new_value text,
    unit text,
    ability_name text,
    secondary_entity_name text,
    confidence double precision NOT NULL,
    flags jsonb DEFAULT '[]'::jsonb NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: patch_events; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.patch_events (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    patch_snapshot_id bigint,
    legacy_patch_snapshot_id bigint NOT NULL,
    patch_external_id text NOT NULL,
    patch_title text,
    patch_url text,
    source_kind text NOT NULL,
    posted_at timestamp with time zone,
    line_index bigint NOT NULL,
    section text,
    entity_type text NOT NULL,
    entity_name text,
    subject text,
    change_type text NOT NULL,
    raw_line text NOT NULL,
    normalized_line text NOT NULL,
    old_value text,
    new_value text,
    confidence double precision NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    event_hash text NOT NULL,
    created_at timestamp with time zone NOT NULL,
    imported_at timestamp with time zone DEFAULT now() NOT NULL
);



--
-- Name: patch_event_enrichments_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.patch_event_enrichments_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: patch_event_enrichments_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.patch_event_enrichments_id_seq OWNED BY brain.patch_event_enrichments.id;


--
-- Name: patch_events_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.patch_events_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: patch_events_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.patch_events_id_seq OWNED BY brain.patch_events.id;


--
-- Name: patch_impact_notes; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.patch_impact_notes (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    entity_type text NOT NULL,
    entity_name text NOT NULL,
    entity_id bigint,
    legacy_entity_id bigint,
    context_hash text NOT NULL,
    prompt_version text DEFAULT 'patch_impact_de_v1'::text NOT NULL,
    prompt_text text NOT NULL,
    result_text text,
    insights jsonb DEFAULT '{}'::jsonb NOT NULL,
    model text,
    status text NOT NULL,
    patch_range_start text,
    patch_range_end text,
    event_count bigint,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: patch_impact_notes_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.patch_impact_notes_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: patch_impact_notes_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.patch_impact_notes_id_seq OWNED BY brain.patch_impact_notes.id;


--
-- Name: plan_items; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.plan_items (
    id bigint NOT NULL,
    run_id bigint NOT NULL,
    prioritaet smallint NOT NULL,
    bereich text NOT NULL,
    titel text NOT NULL,
    begruendung text NOT NULL,
    aktion text NOT NULL,
    beleg text NOT NULL,
    beleg_art text NOT NULL,
    belegt_gemessen boolean NOT NULL,
    fingerprint text NOT NULL,
    status text DEFAULT 'offen'::text NOT NULL,
    kommentar text,
    entschieden_am timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT plan_items_beleg_art_check CHECK ((beleg_art = ANY (ARRAY['digest'::text, 'wiki'::text, 'pg'::text, 'betrieb'::text]))),
    CONSTRAINT plan_items_bereich_check CHECK ((bereich = ANY (ARRAY['discord'::text, 'twitch'::text, 'steam'::text, 'turniere'::text, 'website'::text, 'brain'::text]))),
    CONSTRAINT plan_items_prioritaet_check CHECK (((prioritaet >= 1) AND (prioritaet <= 3))),
    CONSTRAINT plan_items_status_check CHECK ((status = ANY (ARRAY['offen'::text, 'angenommen'::text, 'abgelehnt'::text, 'erledigt'::text])))
);


--
-- Name: plan_items_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

ALTER TABLE brain.plan_items ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME brain.plan_items_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: plan_items_verworfen; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.plan_items_verworfen (
    id bigint NOT NULL,
    run_id bigint NOT NULL,
    prioritaet smallint NOT NULL,
    bereich text NOT NULL,
    titel text NOT NULL,
    begruendung text NOT NULL,
    aktion text NOT NULL,
    beleg text NOT NULL,
    beleg_art text NOT NULL,
    verworfen_grund text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: plan_items_verworfen_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

ALTER TABLE brain.plan_items_verworfen ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME brain.plan_items_verworfen_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: plan_runs; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.plan_runs (
    id bigint NOT NULL,
    run_at timestamp with time zone DEFAULT now() NOT NULL,
    period_start timestamp with time zone NOT NULL,
    period_end timestamp with time zone NOT NULL,
    plan_path text,
    modell text,
    lage text,
    vorgeschlagen integer DEFAULT 0 NOT NULL,
    uebernommen integer DEFAULT 0 NOT NULL,
    verworfen integer DEFAULT 0 NOT NULL,
    verworfen_gruende jsonb,
    quellen_fehlend text[],
    status text NOT NULL,
    error text,
    committed boolean DEFAULT false NOT NULL,
    pushed boolean DEFAULT false NOT NULL,
    CONSTRAINT plan_runs_status_check CHECK ((status = ANY (ARRAY['ok'::text, 'partial'::text, 'error'::text])))
);


--
-- Name: plan_runs_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

ALTER TABLE brain.plan_runs ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME brain.plan_runs_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: player_match_decision_notes; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.player_match_decision_notes (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    account_id text NOT NULL,
    match_id text NOT NULL,
    hero_id text,
    hero_name text,
    context_hash text NOT NULL,
    prompt_version text NOT NULL,
    prompt_text text NOT NULL,
    result_text text,
    insights jsonb DEFAULT '{}'::jsonb NOT NULL,
    model text,
    status text NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: player_match_decision_notes_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.player_match_decision_notes_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: player_match_decision_notes_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.player_match_decision_notes_id_seq OWNED BY brain.player_match_decision_notes.id;


--
-- Name: population_ability_order; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.population_ability_order (
    hero_id bigint NOT NULL,
    bucket text NOT NULL,
    "position" smallint NOT NULL,
    ability_id bigint NOT NULL,
    followers integer NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: population_hero_buckets; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.population_hero_buckets (
    hero_id bigint NOT NULL,
    bucket text NOT NULL,
    players_raw integer NOT NULL,
    players_weighted double precision NOT NULL,
    ability_order_followers integer DEFAULT 0 NOT NULL,
    ability_order_players integer DEFAULT 0 NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: population_imbue_stats; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.population_imbue_stats (
    hero_id bigint NOT NULL,
    bucket text NOT NULL,
    item_id bigint NOT NULL,
    target_ability_id bigint NOT NULL,
    target_count integer NOT NULL,
    total_imbues integer NOT NULL,
    is_split boolean NOT NULL,
    is_thin boolean NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: population_item_stats; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.population_item_stats (
    hero_id bigint NOT NULL,
    bucket text NOT NULL,
    item_id bigint NOT NULL,
    buyers integer NOT NULL,
    prevalence_raw double precision NOT NULL,
    prevalence_weighted double precision NOT NULL,
    median_position double precision NOT NULL,
    median_buy_time_s double precision NOT NULL,
    sell_rate double precision NOT NULL,
    is_staple boolean NOT NULL,
    next_item_id bigint,
    next_item_share double precision DEFAULT 0 NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: population_player_matches; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.population_player_matches (
    match_id bigint NOT NULL,
    account_id bigint NOT NULL,
    hero_id bigint NOT NULL,
    team smallint NOT NULL,
    won boolean NOT NULL,
    average_badge integer,
    duration_s integer,
    start_time timestamp with time zone,
    items bigint[] NOT NULL,
    buy_times_s integer[] NOT NULL,
    sold_times_s integer[] NOT NULL,
    net_worth_rank smallint[] NOT NULL,
    imbue_targets bigint[] NOT NULL,
    ability_points bigint[] NOT NULL,
    ability_times_s integer[] NOT NULL,
    fetched_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: population_sync_runs; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.population_sync_runs (
    run_id bigint NOT NULL,
    started_at timestamp with time zone DEFAULT now() NOT NULL,
    finished_at timestamp with time zone,
    requested_matches integer NOT NULL,
    hero_filter bigint,
    since_unix bigint,
    window_low_match_id bigint,
    window_high_match_id bigint,
    matches_seen integer DEFAULT 0 NOT NULL,
    player_matches_inserted integer DEFAULT 0 NOT NULL,
    player_matches_skipped integer DEFAULT 0 NOT NULL,
    duration_ms bigint DEFAULT 0 NOT NULL
);


--
-- Name: population_sync_runs_run_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

ALTER TABLE brain.population_sync_runs ALTER COLUMN run_id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME brain.population_sync_runs_run_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: reasoner_backtests; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.reasoner_backtests (
    run_id uuid DEFAULT gen_random_uuid() NOT NULL,
    hero_id bigint NOT NULL,
    patch_tag text NOT NULL,
    author text NOT NULL,
    core_coverage double precision NOT NULL,
    order_proximity double precision,
    switch_detected boolean,
    detail jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: reasoner_builds; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.reasoner_builds (
    hero_id bigint NOT NULL,
    patch_tag text NOT NULL,
    hero_name text NOT NULL,
    build jsonb NOT NULL,
    confidence text NOT NULL,
    used_ai boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: reasoner_item_scores; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.reasoner_item_scores (
    hero_id bigint NOT NULL,
    patch_tag text NOT NULL,
    item_id bigint NOT NULL,
    combat_value double precision NOT NULL,
    per_slot_value double precision NOT NULL,
    per_soul_value double precision NOT NULL,
    purchase_bonus double precision NOT NULL,
    condition_factor double precision NOT NULL,
    active_value double precision NOT NULL,
    passive_value double precision NOT NULL,
    meta_support double precision NOT NULL,
    total double precision NOT NULL,
    confidence text NOT NULL,
    buy_phase text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: reasoner_patch_deltas; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.reasoner_patch_deltas (
    hero_id bigint NOT NULL,
    patch_tag text NOT NULL,
    target_kind text NOT NULL,
    target_id bigint NOT NULL,
    mechanic text NOT NULL,
    sign smallint NOT NULL,
    magnitude double precision NOT NULL,
    note text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: sheet_boons_ap; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.sheet_boons_ap (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    snapshot_id bigint NOT NULL,
    legacy_snapshot_id bigint NOT NULL,
    souls bigint NOT NULL,
    boons bigint,
    ap bigint,
    note text,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: sheet_boons_ap_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.sheet_boons_ap_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: sheet_boons_ap_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.sheet_boons_ap_id_seq OWNED BY brain.sheet_boons_ap.id;


--
-- Name: sheet_hero_rankings; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.sheet_hero_rankings (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    snapshot_id bigint NOT NULL,
    legacy_snapshot_id bigint NOT NULL,
    entity_id bigint,
    legacy_entity_id bigint,
    hero_name text NOT NULL,
    carry double precision,
    crowd_control double precision,
    disengage double precision,
    early double precision,
    engage double precision,
    frontline double precision,
    late double precision,
    mid double precision,
    mid_contest double precision,
    mobility double precision,
    nuke_phys double precision,
    nuke_spirit double precision,
    pick double precision,
    poke double precision,
    support double precision,
    wave_clear double precision,
    sustain_dps_phys double precision,
    sustain_dps_spirit double precision,
    average_rank double precision,
    payload_hash text NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: sheet_hero_rankings_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.sheet_hero_rankings_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: sheet_hero_rankings_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.sheet_hero_rankings_id_seq OWNED BY brain.sheet_hero_rankings.id;


--
-- Name: sheet_heroes_stats; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.sheet_heroes_stats (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    snapshot_id bigint NOT NULL,
    legacy_snapshot_id bigint NOT NULL,
    entity_id bigint,
    legacy_entity_id bigint,
    hero_name text NOT NULL,
    alt_fire_type text,
    hero_labs text,
    base_hp double precision,
    base_move_speed double precision,
    base_sprint double precision,
    base_stamina double precision,
    base_regen double precision,
    base_ammo double precision,
    pellets double precision,
    alt_fire_pellets double precision,
    base_bullet_dmg double precision,
    base_fire_rate double precision,
    base_dps double precision,
    max_gun_dps double precision,
    max_gun_damage double precision,
    dpm double precision,
    max_dpm double precision,
    falloff_range_min double precision,
    falloff_range_max double precision,
    hp_gain double precision,
    dmg_gain double precision,
    spirit_gain double precision,
    spirit_bonus double precision,
    spirit_bonus_2 double precision,
    spirit_ratio double precision,
    spirit_ratio_2 double precision,
    spirit_scaling double precision,
    spirit_scaling_2 double precision,
    aggregate_growth_pct double precision,
    dps_growth_pct double precision,
    hp_growth_pct double precision,
    melee_ratio double precision,
    total_bullet_ratio double precision,
    total_spirit_ratio double precision,
    max_level_hp double precision,
    payload_hash text NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: sheet_heroes_stats_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.sheet_heroes_stats_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: sheet_heroes_stats_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.sheet_heroes_stats_id_seq OWNED BY brain.sheet_heroes_stats.id;


--
-- Name: sheet_items; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.sheet_items (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    snapshot_id bigint NOT NULL,
    legacy_snapshot_id bigint NOT NULL,
    item_id bigint,
    code_name text NOT NULL,
    game_name text NOT NULL,
    canonical_name text NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: sheet_items_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.sheet_items_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: sheet_items_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.sheet_items_id_seq OWNED BY brain.sheet_items.id;


--
-- Name: sheet_raw_heroes; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.sheet_raw_heroes (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    snapshot_id bigint NOT NULL,
    legacy_snapshot_id bigint NOT NULL,
    entity_id bigint,
    legacy_entity_id bigint,
    hero_name text NOT NULL,
    hero_id bigint,
    disabled bigint DEFAULT 0 NOT NULL,
    move_speed double precision,
    sprint_speed double precision,
    crouch_speed double precision,
    move_accel double precision,
    light_melee_dmg double precision,
    heavy_melee_dmg double precision,
    max_hp double precision,
    base_stamina double precision,
    stam_regen double precision,
    hp_regen double precision,
    base_health double precision,
    gun_growth double precision,
    alt_gun_growth double precision,
    hp_per_boon double precision,
    melee_gain double precision,
    spirit_per_boon double precision,
    payload_hash text NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: sheet_raw_heroes_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.sheet_raw_heroes_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: sheet_raw_heroes_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.sheet_raw_heroes_id_seq OWNED BY brain.sheet_raw_heroes.id;


--
-- Name: sheet_shop_bonuses; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.sheet_shop_bonuses (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    snapshot_id bigint NOT NULL,
    legacy_snapshot_id bigint NOT NULL,
    souls_cost bigint NOT NULL,
    weapon bigint,
    spirit bigint,
    vitality bigint,
    inc_from_prev_pct double precision,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: sheet_shop_bonuses_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.sheet_shop_bonuses_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: sheet_shop_bonuses_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.sheet_shop_bonuses_id_seq OWNED BY brain.sheet_shop_bonuses.id;


--
-- Name: sheet_tab_rows; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.sheet_tab_rows (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    snapshot_id bigint NOT NULL,
    legacy_snapshot_id bigint NOT NULL,
    tab_name text NOT NULL,
    gid text NOT NULL,
    row_number bigint,
    canonical_name text,
    row_data jsonb NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: sheet_tab_rows_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.sheet_tab_rows_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: sheet_tab_rows_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.sheet_tab_rows_id_seq OWNED BY brain.sheet_tab_rows.id;


--
-- Name: source_documents; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.source_documents (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    source text NOT NULL,
    external_id text NOT NULL,
    title text,
    url text,
    content_type text NOT NULL,
    raw_path text NOT NULL,
    content_hash text NOT NULL,
    fetched_at timestamp with time zone NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    imported_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: source_documents_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.source_documents_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: source_documents_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.source_documents_id_seq OWNED BY brain.source_documents.id;


--
-- Name: source_runs; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.source_runs (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    source text NOT NULL,
    status text NOT NULL,
    started_at timestamp with time zone NOT NULL,
    finished_at timestamp with time zone,
    summary jsonb DEFAULT '{}'::jsonb NOT NULL,
    imported_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: source_runs_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.source_runs_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: source_runs_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.source_runs_id_seq OWNED BY brain.source_runs.id;


--
-- Name: youtube_feed_sources; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.youtube_feed_sources (
    feed_key text NOT NULL,
    source_type text NOT NULL,
    url text NOT NULL,
    handle text,
    playlist_id text,
    channel_id text,
    title text,
    enabled bigint DEFAULT 1 NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: youtube_learning_claims; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.youtube_learning_claims (
    id bigint NOT NULL,
    legacy_sqlite_id bigint,
    video_id text NOT NULL,
    claim_hash text NOT NULL,
    claim_index bigint NOT NULL,
    entity_type text,
    entity_name text,
    claim_type text NOT NULL,
    claim_text text NOT NULL,
    evidence_quote text NOT NULL,
    timestamp_seconds double precision,
    model_confidence double precision NOT NULL,
    verifier_confidence double precision NOT NULL,
    status text NOT NULL,
    model text,
    prompt_version text NOT NULL,
    prompt_text text NOT NULL,
    model_response_text text NOT NULL,
    provider_metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    verifier jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: youtube_learning_claims_id_seq; Type: SEQUENCE; Schema: brain; Owner: -
--

CREATE SEQUENCE brain.youtube_learning_claims_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: youtube_learning_claims_id_seq; Type: SEQUENCE OWNED BY; Schema: brain; Owner: -
--

ALTER SEQUENCE brain.youtube_learning_claims_id_seq OWNED BY brain.youtube_learning_claims.id;


--
-- Name: youtube_transcript_claim_attempts; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.youtube_transcript_claim_attempts (
    video_id text NOT NULL,
    prompt_version text NOT NULL,
    mode text DEFAULT 'normal'::text NOT NULL,
    status text NOT NULL,
    claim_count bigint DEFAULT 0 NOT NULL,
    char_len bigint DEFAULT 0 NOT NULL,
    note text,
    attempted_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: youtube_transcripts; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.youtube_transcripts (
    video_id text NOT NULL,
    language text,
    source_kind text NOT NULL,
    transcript_text text NOT NULL,
    content_hash text NOT NULL,
    source_document_id bigint,
    legacy_source_document_id bigint,
    imported_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: youtube_videos; Type: TABLE; Schema: brain; Owner: -
--

CREATE TABLE brain.youtube_videos (
    video_id text NOT NULL,
    feed_key text NOT NULL,
    channel_id text,
    channel_title text,
    title text NOT NULL,
    url text NOT NULL,
    published_at timestamp with time zone,
    description text,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    transcript_status text DEFAULT 'missing'::text NOT NULL,
    learning_status text DEFAULT 'queued'::text NOT NULL,
    discovered_at timestamp with time zone NOT NULL,
    updated_at timestamp with time zone NOT NULL
);


--
-- Name: analysis_notes id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.analysis_notes ALTER COLUMN id SET DEFAULT nextval('brain.analysis_notes_id_seq'::regclass);


--
-- Name: build_learning_notes id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.build_learning_notes ALTER COLUMN id SET DEFAULT nextval('brain.build_learning_notes_id_seq'::regclass);


--
-- Name: current_entity_state id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.current_entity_state ALTER COLUMN id SET DEFAULT nextval('brain.current_entity_state_id_seq'::regclass);


--
-- Name: entities id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entities ALTER COLUMN id SET DEFAULT nextval('brain.entities_id_seq'::regclass);


--
-- Name: entity_aliases id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_aliases ALTER COLUMN id SET DEFAULT nextval('brain.entity_aliases_id_seq'::regclass);


--
-- Name: entity_lineage id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_lineage ALTER COLUMN id SET DEFAULT nextval('brain.entity_lineage_id_seq'::regclass);


--
-- Name: entity_snapshots id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_snapshots ALTER COLUMN id SET DEFAULT nextval('brain.entity_snapshots_id_seq'::regclass);


--
-- Name: forum_claims id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.forum_claims ALTER COLUMN id SET DEFAULT nextval('brain.forum_claims_id_seq'::regclass);


--
-- Name: hero_stat_profiles id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_stat_profiles ALTER COLUMN id SET DEFAULT nextval('brain.hero_stat_profiles_id_seq'::regclass);


--
-- Name: hero_stat_values id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_stat_values ALTER COLUMN id SET DEFAULT nextval('brain.hero_stat_values_id_seq'::regclass);


--
-- Name: insight_records id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.insight_records ALTER COLUMN id SET DEFAULT nextval('brain.insight_records_id_seq'::regclass);


--
-- Name: knowledge_events id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.knowledge_events ALTER COLUMN id SET DEFAULT nextval('brain.knowledge_events_id_seq'::regclass);


--
-- Name: learned_builds id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.learned_builds ALTER COLUMN id SET DEFAULT nextval('brain.learned_builds_id_seq'::regclass);


--
-- Name: legacy_entities id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.legacy_entities ALTER COLUMN id SET DEFAULT nextval('brain.legacy_entities_id_seq'::regclass);


--
-- Name: mechanic_notes id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.mechanic_notes ALTER COLUMN id SET DEFAULT nextval('brain.mechanic_notes_id_seq'::regclass);


--
-- Name: meta_trend_notes id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.meta_trend_notes ALTER COLUMN id SET DEFAULT nextval('brain.meta_trend_notes_id_seq'::regclass);


--
-- Name: patch_event_enrichments id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.patch_event_enrichments ALTER COLUMN id SET DEFAULT nextval('brain.patch_event_enrichments_id_seq'::regclass);


--
-- Name: patch_events id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.patch_events ALTER COLUMN id SET DEFAULT nextval('brain.patch_events_id_seq'::regclass);


--
-- Name: patch_impact_notes id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.patch_impact_notes ALTER COLUMN id SET DEFAULT nextval('brain.patch_impact_notes_id_seq'::regclass);


--
-- Name: player_match_decision_notes id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.player_match_decision_notes ALTER COLUMN id SET DEFAULT nextval('brain.player_match_decision_notes_id_seq'::regclass);


--
-- Name: sheet_boons_ap id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_boons_ap ALTER COLUMN id SET DEFAULT nextval('brain.sheet_boons_ap_id_seq'::regclass);


--
-- Name: sheet_hero_rankings id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_hero_rankings ALTER COLUMN id SET DEFAULT nextval('brain.sheet_hero_rankings_id_seq'::regclass);


--
-- Name: sheet_heroes_stats id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_heroes_stats ALTER COLUMN id SET DEFAULT nextval('brain.sheet_heroes_stats_id_seq'::regclass);


--
-- Name: sheet_items id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_items ALTER COLUMN id SET DEFAULT nextval('brain.sheet_items_id_seq'::regclass);


--
-- Name: sheet_raw_heroes id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_raw_heroes ALTER COLUMN id SET DEFAULT nextval('brain.sheet_raw_heroes_id_seq'::regclass);


--
-- Name: sheet_shop_bonuses id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_shop_bonuses ALTER COLUMN id SET DEFAULT nextval('brain.sheet_shop_bonuses_id_seq'::regclass);


--
-- Name: sheet_tab_rows id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_tab_rows ALTER COLUMN id SET DEFAULT nextval('brain.sheet_tab_rows_id_seq'::regclass);


--
-- Name: source_documents id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.source_documents ALTER COLUMN id SET DEFAULT nextval('brain.source_documents_id_seq'::regclass);


--
-- Name: source_runs id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.source_runs ALTER COLUMN id SET DEFAULT nextval('brain.source_runs_id_seq'::regclass);


--
-- Name: youtube_learning_claims id; Type: DEFAULT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.youtube_learning_claims ALTER COLUMN id SET DEFAULT nextval('brain.youtube_learning_claims_id_seq'::regclass);


--
-- Name: analysis_notes analysis_notes_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.analysis_notes
    ADD CONSTRAINT analysis_notes_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: analysis_notes analysis_notes_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.analysis_notes
    ADD CONSTRAINT analysis_notes_pkey PRIMARY KEY (id);


--
-- Name: build_learning_notes build_learning_notes_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.build_learning_notes
    ADD CONSTRAINT build_learning_notes_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: build_learning_notes build_learning_notes_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.build_learning_notes
    ADD CONSTRAINT build_learning_notes_pkey PRIMARY KEY (id);


--
-- Name: current_entity_state current_entity_state_entity_type_entity_name_state_kind_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.current_entity_state
    ADD CONSTRAINT current_entity_state_entity_type_entity_name_state_kind_key UNIQUE (entity_type, entity_name, state_kind);


--
-- Name: current_entity_state current_entity_state_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.current_entity_state
    ADD CONSTRAINT current_entity_state_pkey PRIMARY KEY (id);


--
-- Name: entities entities_entity_type_canonical_name_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entities
    ADD CONSTRAINT entities_entity_type_canonical_name_key UNIQUE (entity_type, canonical_name);


--
-- Name: entities entities_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entities
    ADD CONSTRAINT entities_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: entities entities_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entities
    ADD CONSTRAINT entities_pkey PRIMARY KEY (id);


--
-- Name: entity_aliases entity_aliases_entity_id_alias_norm_alias_kind_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_aliases
    ADD CONSTRAINT entity_aliases_entity_id_alias_norm_alias_kind_key UNIQUE (entity_id, alias_norm, alias_kind);


--
-- Name: entity_aliases entity_aliases_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_aliases
    ADD CONSTRAINT entity_aliases_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: entity_aliases entity_aliases_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_aliases
    ADD CONSTRAINT entity_aliases_pkey PRIMARY KEY (id);


--
-- Name: entity_lineage entity_lineage_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_lineage
    ADD CONSTRAINT entity_lineage_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: entity_lineage entity_lineage_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_lineage
    ADD CONSTRAINT entity_lineage_pkey PRIMARY KEY (id);


--
-- Name: entity_snapshots entity_snapshots_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_snapshots
    ADD CONSTRAINT entity_snapshots_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: entity_snapshots entity_snapshots_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_snapshots
    ADD CONSTRAINT entity_snapshots_pkey PRIMARY KEY (id);


--
-- Name: entity_snapshots entity_snapshots_source_entity_type_external_id_payload_has_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_snapshots
    ADD CONSTRAINT entity_snapshots_source_entity_type_external_id_payload_has_key UNIQUE (source, entity_type, external_id, payload_hash);


--
-- Name: feeder_runs feeder_runs_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.feeder_runs
    ADD CONSTRAINT feeder_runs_pkey PRIMARY KEY (id);


--
-- Name: forum_claims forum_claims_claim_hash_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.forum_claims
    ADD CONSTRAINT forum_claims_claim_hash_key UNIQUE (claim_hash);


--
-- Name: forum_claims forum_claims_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.forum_claims
    ADD CONSTRAINT forum_claims_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: forum_claims forum_claims_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.forum_claims
    ADD CONSTRAINT forum_claims_pkey PRIMARY KEY (id);


--
-- Name: hero_ability_orders hero_ability_orders_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_ability_orders
    ADD CONSTRAINT hero_ability_orders_pkey PRIMARY KEY (hero_id, bracket, patch_tag);


--
-- Name: hero_catalog hero_catalog_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_catalog
    ADD CONSTRAINT hero_catalog_pkey PRIMARY KEY (hero_id);


--
-- Name: hero_item_stats hero_item_stats_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_item_stats
    ADD CONSTRAINT hero_item_stats_pkey PRIMARY KEY (hero_id, item_id, bracket, patch_tag);


--
-- Name: hero_item_synergies hero_item_synergies_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_item_synergies
    ADD CONSTRAINT hero_item_synergies_pkey PRIMARY KEY (hero_id, item_id, with_item_id, patch_tag);


--
-- Name: hero_stat_profiles hero_stat_profiles_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_stat_profiles
    ADD CONSTRAINT hero_stat_profiles_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: hero_stat_profiles hero_stat_profiles_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_stat_profiles
    ADD CONSTRAINT hero_stat_profiles_pkey PRIMARY KEY (id);


--
-- Name: hero_stat_profiles hero_stat_profiles_snapshot_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_stat_profiles
    ADD CONSTRAINT hero_stat_profiles_snapshot_id_key UNIQUE (snapshot_id);


--
-- Name: hero_stat_values hero_stat_values_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_stat_values
    ADD CONSTRAINT hero_stat_values_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: hero_stat_values hero_stat_values_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_stat_values
    ADD CONSTRAINT hero_stat_values_pkey PRIMARY KEY (id);


--
-- Name: hero_stat_values hero_stat_values_profile_id_stat_key_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_stat_values
    ADD CONSTRAINT hero_stat_values_profile_id_stat_key_key UNIQUE (profile_id, stat_key);


--
-- Name: insight_records insight_records_insight_hash_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.insight_records
    ADD CONSTRAINT insight_records_insight_hash_key UNIQUE (insight_hash);


--
-- Name: insight_records insight_records_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.insight_records
    ADD CONSTRAINT insight_records_pkey PRIMARY KEY (id);


--
-- Name: item_catalog item_catalog_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.item_catalog
    ADD CONSTRAINT item_catalog_pkey PRIMARY KEY (item_id);


--
-- Name: knowledge_events knowledge_events_event_hash_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.knowledge_events
    ADD CONSTRAINT knowledge_events_event_hash_key UNIQUE (event_hash);


--
-- Name: knowledge_events knowledge_events_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.knowledge_events
    ADD CONSTRAINT knowledge_events_pkey PRIMARY KEY (id);


--
-- Name: learned_builds learned_builds_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.learned_builds
    ADD CONSTRAINT learned_builds_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: learned_builds learned_builds_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.learned_builds
    ADD CONSTRAINT learned_builds_pkey PRIMARY KEY (id);


--
-- Name: learned_builds learned_builds_source_source_build_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.learned_builds
    ADD CONSTRAINT learned_builds_source_source_build_id_key UNIQUE (source, source_build_id);


--
-- Name: legacy_entities legacy_entities_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.legacy_entities
    ADD CONSTRAINT legacy_entities_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: legacy_entities legacy_entities_legacy_type_name_norm_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.legacy_entities
    ADD CONSTRAINT legacy_entities_legacy_type_name_norm_key UNIQUE (legacy_type, name_norm);


--
-- Name: legacy_entities legacy_entities_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.legacy_entities
    ADD CONSTRAINT legacy_entities_pkey PRIMARY KEY (id);


--
-- Name: mechanic_notes mechanic_notes_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.mechanic_notes
    ADD CONSTRAINT mechanic_notes_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: mechanic_notes mechanic_notes_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.mechanic_notes
    ADD CONSTRAINT mechanic_notes_pkey PRIMARY KEY (id);


--
-- Name: mechanic_notes mechanic_notes_title_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.mechanic_notes
    ADD CONSTRAINT mechanic_notes_title_key UNIQUE (title);


--
-- Name: meta_trend_notes meta_trend_notes_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.meta_trend_notes
    ADD CONSTRAINT meta_trend_notes_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: meta_trend_notes meta_trend_notes_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.meta_trend_notes
    ADD CONSTRAINT meta_trend_notes_pkey PRIMARY KEY (id);


--
-- Name: patch_event_enrichments patch_event_enrichments_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.patch_event_enrichments
    ADD CONSTRAINT patch_event_enrichments_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: patch_event_enrichments patch_event_enrichments_patch_event_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.patch_event_enrichments
    ADD CONSTRAINT patch_event_enrichments_patch_event_id_key UNIQUE (patch_event_id);


--
-- Name: patch_event_enrichments patch_event_enrichments_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.patch_event_enrichments
    ADD CONSTRAINT patch_event_enrichments_pkey PRIMARY KEY (id);


--
-- Name: patch_events patch_events_event_hash_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.patch_events
    ADD CONSTRAINT patch_events_event_hash_key UNIQUE (event_hash);


--
-- Name: patch_events patch_events_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.patch_events
    ADD CONSTRAINT patch_events_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: patch_events patch_events_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.patch_events
    ADD CONSTRAINT patch_events_pkey PRIMARY KEY (id);


--
-- Name: patch_impact_notes patch_impact_notes_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.patch_impact_notes
    ADD CONSTRAINT patch_impact_notes_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: patch_impact_notes patch_impact_notes_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.patch_impact_notes
    ADD CONSTRAINT patch_impact_notes_pkey PRIMARY KEY (id);


--
-- Name: plan_items plan_items_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.plan_items
    ADD CONSTRAINT plan_items_pkey PRIMARY KEY (id);


--
-- Name: plan_items_verworfen plan_items_verworfen_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.plan_items_verworfen
    ADD CONSTRAINT plan_items_verworfen_pkey PRIMARY KEY (id);


--
-- Name: plan_runs plan_runs_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.plan_runs
    ADD CONSTRAINT plan_runs_pkey PRIMARY KEY (id);


--
-- Name: player_match_decision_notes player_match_decision_notes_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.player_match_decision_notes
    ADD CONSTRAINT player_match_decision_notes_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: player_match_decision_notes player_match_decision_notes_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.player_match_decision_notes
    ADD CONSTRAINT player_match_decision_notes_pkey PRIMARY KEY (id);


--
-- Name: population_ability_order population_ability_order_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.population_ability_order
    ADD CONSTRAINT population_ability_order_pkey PRIMARY KEY (hero_id, bucket, "position");


--
-- Name: population_hero_buckets population_hero_buckets_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.population_hero_buckets
    ADD CONSTRAINT population_hero_buckets_pkey PRIMARY KEY (hero_id, bucket);


--
-- Name: population_imbue_stats population_imbue_stats_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.population_imbue_stats
    ADD CONSTRAINT population_imbue_stats_pkey PRIMARY KEY (hero_id, bucket, item_id);


--
-- Name: population_item_stats population_item_stats_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.population_item_stats
    ADD CONSTRAINT population_item_stats_pkey PRIMARY KEY (hero_id, bucket, item_id);


--
-- Name: population_player_matches population_player_matches_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.population_player_matches
    ADD CONSTRAINT population_player_matches_pkey PRIMARY KEY (match_id, account_id);


--
-- Name: population_sync_runs population_sync_runs_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.population_sync_runs
    ADD CONSTRAINT population_sync_runs_pkey PRIMARY KEY (run_id);


--
-- Name: reasoner_backtests reasoner_backtests_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.reasoner_backtests
    ADD CONSTRAINT reasoner_backtests_pkey PRIMARY KEY (run_id, hero_id, author);


--
-- Name: reasoner_builds reasoner_builds_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.reasoner_builds
    ADD CONSTRAINT reasoner_builds_pkey PRIMARY KEY (hero_id, patch_tag);


--
-- Name: reasoner_item_scores reasoner_item_scores_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.reasoner_item_scores
    ADD CONSTRAINT reasoner_item_scores_pkey PRIMARY KEY (hero_id, patch_tag, item_id);


--
-- Name: reasoner_patch_deltas reasoner_patch_deltas_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.reasoner_patch_deltas
    ADD CONSTRAINT reasoner_patch_deltas_pkey PRIMARY KEY (hero_id, patch_tag, target_kind, target_id, mechanic);


--
-- Name: sheet_boons_ap sheet_boons_ap_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_boons_ap
    ADD CONSTRAINT sheet_boons_ap_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: sheet_boons_ap sheet_boons_ap_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_boons_ap
    ADD CONSTRAINT sheet_boons_ap_pkey PRIMARY KEY (id);


--
-- Name: sheet_boons_ap sheet_boons_ap_snapshot_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_boons_ap
    ADD CONSTRAINT sheet_boons_ap_snapshot_id_key UNIQUE (snapshot_id);


--
-- Name: sheet_hero_rankings sheet_hero_rankings_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_hero_rankings
    ADD CONSTRAINT sheet_hero_rankings_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: sheet_hero_rankings sheet_hero_rankings_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_hero_rankings
    ADD CONSTRAINT sheet_hero_rankings_pkey PRIMARY KEY (id);


--
-- Name: sheet_hero_rankings sheet_hero_rankings_snapshot_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_hero_rankings
    ADD CONSTRAINT sheet_hero_rankings_snapshot_id_key UNIQUE (snapshot_id);


--
-- Name: sheet_heroes_stats sheet_heroes_stats_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_heroes_stats
    ADD CONSTRAINT sheet_heroes_stats_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: sheet_heroes_stats sheet_heroes_stats_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_heroes_stats
    ADD CONSTRAINT sheet_heroes_stats_pkey PRIMARY KEY (id);


--
-- Name: sheet_heroes_stats sheet_heroes_stats_snapshot_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_heroes_stats
    ADD CONSTRAINT sheet_heroes_stats_snapshot_id_key UNIQUE (snapshot_id);


--
-- Name: sheet_items sheet_items_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_items
    ADD CONSTRAINT sheet_items_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: sheet_items sheet_items_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_items
    ADD CONSTRAINT sheet_items_pkey PRIMARY KEY (id);


--
-- Name: sheet_items sheet_items_snapshot_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_items
    ADD CONSTRAINT sheet_items_snapshot_id_key UNIQUE (snapshot_id);


--
-- Name: sheet_raw_heroes sheet_raw_heroes_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_raw_heroes
    ADD CONSTRAINT sheet_raw_heroes_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: sheet_raw_heroes sheet_raw_heroes_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_raw_heroes
    ADD CONSTRAINT sheet_raw_heroes_pkey PRIMARY KEY (id);


--
-- Name: sheet_raw_heroes sheet_raw_heroes_snapshot_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_raw_heroes
    ADD CONSTRAINT sheet_raw_heroes_snapshot_id_key UNIQUE (snapshot_id);


--
-- Name: sheet_shop_bonuses sheet_shop_bonuses_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_shop_bonuses
    ADD CONSTRAINT sheet_shop_bonuses_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: sheet_shop_bonuses sheet_shop_bonuses_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_shop_bonuses
    ADD CONSTRAINT sheet_shop_bonuses_pkey PRIMARY KEY (id);


--
-- Name: sheet_shop_bonuses sheet_shop_bonuses_snapshot_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_shop_bonuses
    ADD CONSTRAINT sheet_shop_bonuses_snapshot_id_key UNIQUE (snapshot_id);


--
-- Name: sheet_tab_rows sheet_tab_rows_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_tab_rows
    ADD CONSTRAINT sheet_tab_rows_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: sheet_tab_rows sheet_tab_rows_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_tab_rows
    ADD CONSTRAINT sheet_tab_rows_pkey PRIMARY KEY (id);


--
-- Name: sheet_tab_rows sheet_tab_rows_snapshot_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_tab_rows
    ADD CONSTRAINT sheet_tab_rows_snapshot_id_key UNIQUE (snapshot_id);


--
-- Name: source_documents source_documents_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.source_documents
    ADD CONSTRAINT source_documents_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: source_documents source_documents_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.source_documents
    ADD CONSTRAINT source_documents_pkey PRIMARY KEY (id);


--
-- Name: source_documents source_documents_source_external_id_content_hash_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.source_documents
    ADD CONSTRAINT source_documents_source_external_id_content_hash_key UNIQUE (source, external_id, content_hash);


--
-- Name: source_runs source_runs_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.source_runs
    ADD CONSTRAINT source_runs_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: source_runs source_runs_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.source_runs
    ADD CONSTRAINT source_runs_pkey PRIMARY KEY (id);


--
-- Name: youtube_feed_sources youtube_feed_sources_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.youtube_feed_sources
    ADD CONSTRAINT youtube_feed_sources_pkey PRIMARY KEY (feed_key);


--
-- Name: youtube_learning_claims youtube_learning_claims_claim_hash_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.youtube_learning_claims
    ADD CONSTRAINT youtube_learning_claims_claim_hash_key UNIQUE (claim_hash);


--
-- Name: youtube_learning_claims youtube_learning_claims_legacy_sqlite_id_key; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.youtube_learning_claims
    ADD CONSTRAINT youtube_learning_claims_legacy_sqlite_id_key UNIQUE (legacy_sqlite_id);


--
-- Name: youtube_learning_claims youtube_learning_claims_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.youtube_learning_claims
    ADD CONSTRAINT youtube_learning_claims_pkey PRIMARY KEY (id);


--
-- Name: youtube_transcript_claim_attempts youtube_transcript_claim_attempts_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.youtube_transcript_claim_attempts
    ADD CONSTRAINT youtube_transcript_claim_attempts_pkey PRIMARY KEY (video_id, prompt_version);


--
-- Name: youtube_transcripts youtube_transcripts_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.youtube_transcripts
    ADD CONSTRAINT youtube_transcripts_pkey PRIMARY KEY (video_id);


--
-- Name: youtube_videos youtube_videos_pkey; Type: CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.youtube_videos
    ADD CONSTRAINT youtube_videos_pkey PRIMARY KEY (video_id);


--
-- Name: brain_analysis_notes_entity_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_analysis_notes_entity_idx ON brain.analysis_notes USING btree (entity_type, entity_name);


--
-- Name: brain_analysis_notes_query_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_analysis_notes_query_idx ON brain.analysis_notes USING btree (query, created_at);


--
-- Name: brain_analysis_notes_unique_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE UNIQUE INDEX brain_analysis_notes_unique_idx ON brain.analysis_notes USING btree (query, context_hash, prompt_version, COALESCE(model, ''::text), status);


--
-- Name: brain_build_learning_notes_build_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_build_learning_notes_build_idx ON brain.build_learning_notes USING btree (learned_build_id);


--
-- Name: brain_build_learning_notes_hero_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_build_learning_notes_hero_idx ON brain.build_learning_notes USING btree (hero_name, updated_at DESC);


--
-- Name: brain_build_learning_notes_unique_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE UNIQUE INDEX brain_build_learning_notes_unique_idx ON brain.build_learning_notes USING btree (learned_build_id, context_hash, prompt_version, COALESCE(model, ''::text), status);


--
-- Name: brain_current_entity_state_observed_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_current_entity_state_observed_idx ON brain.current_entity_state USING btree (observed_at DESC);


--
-- Name: brain_current_entity_state_source_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_current_entity_state_source_idx ON brain.current_entity_state USING btree (source, source_priority DESC);


--
-- Name: brain_entities_type_name_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_entities_type_name_idx ON brain.entities USING btree (entity_type, canonical_name);


--
-- Name: brain_entity_aliases_norm_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_entity_aliases_norm_idx ON brain.entity_aliases USING btree (alias_norm);


--
-- Name: brain_entity_lineage_source_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_entity_lineage_source_idx ON brain.entity_lineage USING btree (source_name_norm);


--
-- Name: brain_entity_lineage_target_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_entity_lineage_target_idx ON brain.entity_lineage USING btree (target_name_norm);


--
-- Name: brain_entity_lineage_unique_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE UNIQUE INDEX brain_entity_lineage_unique_idx ON brain.entity_lineage USING btree (patch_event_id, relation_type, source_name_norm, COALESCE(target_name_norm, ''::text), COALESCE(owner_name_norm, ''::text));


--
-- Name: brain_entity_snapshots_entity_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_entity_snapshots_entity_idx ON brain.entity_snapshots USING btree (entity_type, canonical_name);


--
-- Name: brain_entity_snapshots_source_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_entity_snapshots_source_idx ON brain.entity_snapshots USING btree (source, entity_type, external_id);


--
-- Name: brain_forum_claims_currentness_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_forum_claims_currentness_idx ON brain.forum_claims USING btree (currentness, validity_status);


--
-- Name: brain_forum_claims_entity_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_forum_claims_entity_idx ON brain.forum_claims USING btree (entity_type, entity_name);


--
-- Name: brain_forum_claims_posted_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_forum_claims_posted_idx ON brain.forum_claims USING btree (posted_at DESC);


--
-- Name: brain_hero_ability_orders_patch_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_hero_ability_orders_patch_idx ON brain.hero_ability_orders USING btree (patch_tag);


--
-- Name: brain_hero_catalog_name_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_hero_catalog_name_idx ON brain.hero_catalog USING btree (name);


--
-- Name: brain_hero_item_stats_hero_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_hero_item_stats_hero_idx ON brain.hero_item_stats USING btree (hero_id, bracket, patch_tag, prevalence_builds, matches);


--
-- Name: brain_hero_item_stats_item_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_hero_item_stats_item_idx ON brain.hero_item_stats USING btree (item_id, patch_tag);


--
-- Name: brain_hero_item_synergies_item_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_hero_item_synergies_item_idx ON brain.hero_item_synergies USING btree (hero_id, item_id, patch_tag, matches);


--
-- Name: brain_hero_item_synergies_with_item_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_hero_item_synergies_with_item_idx ON brain.hero_item_synergies USING btree (with_item_id, patch_tag);


--
-- Name: brain_hero_stat_profiles_entity_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_hero_stat_profiles_entity_idx ON brain.hero_stat_profiles USING btree (entity_id);


--
-- Name: brain_hero_stat_profiles_hero_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_hero_stat_profiles_hero_idx ON brain.hero_stat_profiles USING btree (hero_name);


--
-- Name: brain_hero_stat_values_entity_key_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_hero_stat_values_entity_key_idx ON brain.hero_stat_values USING btree (entity_id, stat_key);


--
-- Name: brain_hero_stat_values_key_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_hero_stat_values_key_idx ON brain.hero_stat_values USING btree (stat_key);


--
-- Name: brain_insight_records_currentness_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_insight_records_currentness_idx ON brain.insight_records USING btree (currentness, validity_status, trust_tier);


--
-- Name: brain_insight_records_entity_time_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_insight_records_entity_time_idx ON brain.insight_records USING btree (entity_type, entity_name, COALESCE(occurred_at, observed_at) DESC);


--
-- Name: brain_insight_records_type_time_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_insight_records_type_time_idx ON brain.insight_records USING btree (insight_type, COALESCE(occurred_at, observed_at) DESC);


--
-- Name: brain_item_catalog_name_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_item_catalog_name_idx ON brain.item_catalog USING btree (name);


--
-- Name: brain_item_catalog_slot_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_item_catalog_slot_idx ON brain.item_catalog USING btree (slot_type);


--
-- Name: brain_knowledge_events_currentness_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_knowledge_events_currentness_idx ON brain.knowledge_events USING btree (currentness, validity_status, trust_tier);


--
-- Name: brain_knowledge_events_entity_time_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_knowledge_events_entity_time_idx ON brain.knowledge_events USING btree (entity_type, entity_name, COALESCE(occurred_at, observed_at) DESC);


--
-- Name: brain_knowledge_events_source_legacy_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE UNIQUE INDEX brain_knowledge_events_source_legacy_idx ON brain.knowledge_events USING btree (event_source, source_table, source_legacy_id) WHERE (source_legacy_id IS NOT NULL);


--
-- Name: brain_knowledge_events_type_time_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_knowledge_events_type_time_idx ON brain.knowledge_events USING btree (event_type, COALESCE(occurred_at, observed_at) DESC);


--
-- Name: brain_learned_builds_hero_id_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_learned_builds_hero_id_idx ON brain.learned_builds USING btree (hero_id);


--
-- Name: brain_learned_builds_hero_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_learned_builds_hero_idx ON brain.learned_builds USING btree (hero_name, quality_score);


--
-- Name: brain_learned_builds_quality_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_learned_builds_quality_idx ON brain.learned_builds USING btree (quality_tier, quality_score);


--
-- Name: brain_legacy_entities_name_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_legacy_entities_name_idx ON brain.legacy_entities USING btree (name_norm);


--
-- Name: brain_legacy_entities_type_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_legacy_entities_type_idx ON brain.legacy_entities USING btree (legacy_type, status);


--
-- Name: brain_mechanic_notes_category_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_mechanic_notes_category_idx ON brain.mechanic_notes USING btree (category);


--
-- Name: brain_meta_trend_notes_entity_name_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_meta_trend_notes_entity_name_idx ON brain.meta_trend_notes USING btree (entity_name);


--
-- Name: brain_patch_event_enrichments_stat_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_patch_event_enrichments_stat_idx ON brain.patch_event_enrichments USING btree (stat_name);


--
-- Name: brain_patch_events_entity_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_patch_events_entity_idx ON brain.patch_events USING btree (entity_type, entity_name, posted_at DESC);


--
-- Name: brain_patch_events_patch_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_patch_events_patch_idx ON brain.patch_events USING btree (patch_external_id, line_index);


--
-- Name: brain_patch_events_source_kind_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_patch_events_source_kind_idx ON brain.patch_events USING btree (source_kind);


--
-- Name: brain_patch_impact_notes_entity_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_patch_impact_notes_entity_idx ON brain.patch_impact_notes USING btree (entity_id);


--
-- Name: brain_patch_impact_notes_entity_name_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_patch_impact_notes_entity_name_idx ON brain.patch_impact_notes USING btree (entity_name);


--
-- Name: brain_patch_impact_notes_status_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_patch_impact_notes_status_idx ON brain.patch_impact_notes USING btree (status);


--
-- Name: brain_patch_impact_notes_unique_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE UNIQUE INDEX brain_patch_impact_notes_unique_idx ON brain.patch_impact_notes USING btree (entity_name, context_hash, prompt_version, COALESCE(model, ''::text));


--
-- Name: brain_player_match_decision_notes_lookup_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_player_match_decision_notes_lookup_idx ON brain.player_match_decision_notes USING btree (account_id, match_id, updated_at DESC);


--
-- Name: brain_player_match_decision_notes_unique_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE UNIQUE INDEX brain_player_match_decision_notes_unique_idx ON brain.player_match_decision_notes USING btree (account_id, match_id, context_hash, prompt_version, COALESCE(model, ''::text), status);


--
-- Name: brain_sheet_boons_ap_souls_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_sheet_boons_ap_souls_idx ON brain.sheet_boons_ap USING btree (souls);


--
-- Name: brain_sheet_hero_rankings_entity_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_sheet_hero_rankings_entity_idx ON brain.sheet_hero_rankings USING btree (entity_id);


--
-- Name: brain_sheet_hero_rankings_name_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_sheet_hero_rankings_name_idx ON brain.sheet_hero_rankings USING btree (hero_name);


--
-- Name: brain_sheet_heroes_stats_entity_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_sheet_heroes_stats_entity_idx ON brain.sheet_heroes_stats USING btree (entity_id);


--
-- Name: brain_sheet_heroes_stats_name_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_sheet_heroes_stats_name_idx ON brain.sheet_heroes_stats USING btree (hero_name);


--
-- Name: brain_sheet_items_canonical_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_sheet_items_canonical_idx ON brain.sheet_items USING btree (canonical_name);


--
-- Name: brain_sheet_items_code_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_sheet_items_code_idx ON brain.sheet_items USING btree (code_name);


--
-- Name: brain_sheet_items_game_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_sheet_items_game_idx ON brain.sheet_items USING btree (game_name);


--
-- Name: brain_sheet_items_item_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_sheet_items_item_idx ON brain.sheet_items USING btree (item_id);


--
-- Name: brain_sheet_raw_heroes_entity_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_sheet_raw_heroes_entity_idx ON brain.sheet_raw_heroes USING btree (entity_id);


--
-- Name: brain_sheet_raw_heroes_name_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_sheet_raw_heroes_name_idx ON brain.sheet_raw_heroes USING btree (hero_name);


--
-- Name: brain_sheet_shop_bonuses_souls_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_sheet_shop_bonuses_souls_idx ON brain.sheet_shop_bonuses USING btree (souls_cost);


--
-- Name: brain_sheet_tab_rows_canonical_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_sheet_tab_rows_canonical_idx ON brain.sheet_tab_rows USING btree (canonical_name);


--
-- Name: brain_sheet_tab_rows_tab_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_sheet_tab_rows_tab_idx ON brain.sheet_tab_rows USING btree (tab_name);


--
-- Name: brain_source_documents_fetched_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_source_documents_fetched_idx ON brain.source_documents USING btree (fetched_at DESC);


--
-- Name: brain_source_documents_source_external_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_source_documents_source_external_idx ON brain.source_documents USING btree (source, external_id);


--
-- Name: brain_source_runs_source_started_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_source_runs_source_started_idx ON brain.source_runs USING btree (source, started_at DESC);


--
-- Name: brain_youtube_learning_claims_entity_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_youtube_learning_claims_entity_idx ON brain.youtube_learning_claims USING btree (entity_type, entity_name);


--
-- Name: brain_youtube_learning_claims_video_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_youtube_learning_claims_video_idx ON brain.youtube_learning_claims USING btree (video_id, status);


--
-- Name: brain_youtube_transcript_claim_attempts_prompt_status_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_youtube_transcript_claim_attempts_prompt_status_idx ON brain.youtube_transcript_claim_attempts USING btree (prompt_version, status);


--
-- Name: brain_youtube_transcripts_source_document_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_youtube_transcripts_source_document_idx ON brain.youtube_transcripts USING btree (source_document_id);


--
-- Name: brain_youtube_videos_feed_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_youtube_videos_feed_idx ON brain.youtube_videos USING btree (feed_key);


--
-- Name: brain_youtube_videos_learning_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX brain_youtube_videos_learning_idx ON brain.youtube_videos USING btree (learning_status, transcript_status, published_at);


--
-- Name: feeder_runs_run_at_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX feeder_runs_run_at_idx ON brain.feeder_runs USING btree (run_at DESC);


--
-- Name: plan_items_fingerprint_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX plan_items_fingerprint_idx ON brain.plan_items USING btree (fingerprint);


--
-- Name: plan_items_offen_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX plan_items_offen_idx ON brain.plan_items USING btree (status) WHERE (status = 'offen'::text);


--
-- Name: plan_items_run_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX plan_items_run_idx ON brain.plan_items USING btree (run_id);


--
-- Name: plan_items_verworfen_run_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX plan_items_verworfen_run_idx ON brain.plan_items_verworfen USING btree (run_id);


--
-- Name: population_player_matches_hero_idx; Type: INDEX; Schema: brain; Owner: -
--

CREATE INDEX population_player_matches_hero_idx ON brain.population_player_matches USING btree (hero_id);


--
-- Name: reasoner_backtests_hero_patch_author; Type: INDEX; Schema: brain; Owner: -
--

CREATE UNIQUE INDEX reasoner_backtests_hero_patch_author ON brain.reasoner_backtests USING btree (hero_id, patch_tag, author);


--
-- Name: build_learning_notes build_learning_notes_learned_build_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.build_learning_notes
    ADD CONSTRAINT build_learning_notes_learned_build_id_fkey FOREIGN KEY (learned_build_id) REFERENCES brain.learned_builds(id) ON DELETE CASCADE;


--
-- Name: current_entity_state current_entity_state_winning_event_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.current_entity_state
    ADD CONSTRAINT current_entity_state_winning_event_id_fkey FOREIGN KEY (winning_event_id) REFERENCES brain.knowledge_events(id) ON DELETE SET NULL;


--
-- Name: current_entity_state current_entity_state_winning_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.current_entity_state
    ADD CONSTRAINT current_entity_state_winning_snapshot_id_fkey FOREIGN KEY (winning_snapshot_id) REFERENCES brain.entity_snapshots(id) ON DELETE SET NULL;


--
-- Name: entities entities_first_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entities
    ADD CONSTRAINT entities_first_snapshot_id_fkey FOREIGN KEY (first_snapshot_id) REFERENCES brain.entity_snapshots(id) ON DELETE SET NULL;


--
-- Name: entity_aliases entity_aliases_entity_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_aliases
    ADD CONSTRAINT entity_aliases_entity_id_fkey FOREIGN KEY (entity_id) REFERENCES brain.entities(id) ON DELETE CASCADE;


--
-- Name: entity_aliases entity_aliases_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_aliases
    ADD CONSTRAINT entity_aliases_snapshot_id_fkey FOREIGN KEY (snapshot_id) REFERENCES brain.entity_snapshots(id) ON DELETE SET NULL;


--
-- Name: entity_lineage entity_lineage_patch_event_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_lineage
    ADD CONSTRAINT entity_lineage_patch_event_id_fkey FOREIGN KEY (patch_event_id) REFERENCES brain.patch_events(id) ON DELETE CASCADE;


--
-- Name: entity_snapshots entity_snapshots_source_document_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.entity_snapshots
    ADD CONSTRAINT entity_snapshots_source_document_id_fkey FOREIGN KEY (source_document_id) REFERENCES brain.source_documents(id) ON DELETE SET NULL;


--
-- Name: forum_claims forum_claims_post_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.forum_claims
    ADD CONSTRAINT forum_claims_post_snapshot_id_fkey FOREIGN KEY (post_snapshot_id) REFERENCES brain.entity_snapshots(id) ON DELETE SET NULL;


--
-- Name: hero_ability_orders hero_ability_orders_hero_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_ability_orders
    ADD CONSTRAINT hero_ability_orders_hero_id_fkey FOREIGN KEY (hero_id) REFERENCES brain.hero_catalog(hero_id);


--
-- Name: hero_item_stats hero_item_stats_hero_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_item_stats
    ADD CONSTRAINT hero_item_stats_hero_id_fkey FOREIGN KEY (hero_id) REFERENCES brain.hero_catalog(hero_id);


--
-- Name: hero_item_stats hero_item_stats_item_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_item_stats
    ADD CONSTRAINT hero_item_stats_item_id_fkey FOREIGN KEY (item_id) REFERENCES brain.item_catalog(item_id);


--
-- Name: hero_item_synergies hero_item_synergies_hero_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_item_synergies
    ADD CONSTRAINT hero_item_synergies_hero_id_fkey FOREIGN KEY (hero_id) REFERENCES brain.hero_catalog(hero_id);


--
-- Name: hero_item_synergies hero_item_synergies_item_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_item_synergies
    ADD CONSTRAINT hero_item_synergies_item_id_fkey FOREIGN KEY (item_id) REFERENCES brain.item_catalog(item_id);


--
-- Name: hero_item_synergies hero_item_synergies_with_item_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_item_synergies
    ADD CONSTRAINT hero_item_synergies_with_item_id_fkey FOREIGN KEY (with_item_id) REFERENCES brain.item_catalog(item_id);


--
-- Name: hero_stat_profiles hero_stat_profiles_entity_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_stat_profiles
    ADD CONSTRAINT hero_stat_profiles_entity_id_fkey FOREIGN KEY (entity_id) REFERENCES brain.entities(id) ON DELETE SET NULL;


--
-- Name: hero_stat_profiles hero_stat_profiles_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_stat_profiles
    ADD CONSTRAINT hero_stat_profiles_snapshot_id_fkey FOREIGN KEY (snapshot_id) REFERENCES brain.entity_snapshots(id) ON DELETE CASCADE;


--
-- Name: hero_stat_values hero_stat_values_entity_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_stat_values
    ADD CONSTRAINT hero_stat_values_entity_id_fkey FOREIGN KEY (entity_id) REFERENCES brain.entities(id) ON DELETE SET NULL;


--
-- Name: hero_stat_values hero_stat_values_profile_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.hero_stat_values
    ADD CONSTRAINT hero_stat_values_profile_id_fkey FOREIGN KEY (profile_id) REFERENCES brain.hero_stat_profiles(id) ON DELETE CASCADE;


--
-- Name: knowledge_events knowledge_events_forum_claim_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.knowledge_events
    ADD CONSTRAINT knowledge_events_forum_claim_id_fkey FOREIGN KEY (forum_claim_id) REFERENCES brain.forum_claims(id) ON DELETE SET NULL;


--
-- Name: knowledge_events knowledge_events_patch_event_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.knowledge_events
    ADD CONSTRAINT knowledge_events_patch_event_id_fkey FOREIGN KEY (patch_event_id) REFERENCES brain.patch_events(id) ON DELETE SET NULL;


--
-- Name: knowledge_events knowledge_events_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.knowledge_events
    ADD CONSTRAINT knowledge_events_snapshot_id_fkey FOREIGN KEY (snapshot_id) REFERENCES brain.entity_snapshots(id) ON DELETE SET NULL;


--
-- Name: knowledge_events knowledge_events_source_document_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.knowledge_events
    ADD CONSTRAINT knowledge_events_source_document_id_fkey FOREIGN KEY (source_document_id) REFERENCES brain.source_documents(id) ON DELETE SET NULL;


--
-- Name: legacy_entities legacy_entities_first_patch_event_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.legacy_entities
    ADD CONSTRAINT legacy_entities_first_patch_event_id_fkey FOREIGN KEY (first_patch_event_id) REFERENCES brain.patch_events(id) ON DELETE SET NULL;


--
-- Name: legacy_entities legacy_entities_last_patch_event_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.legacy_entities
    ADD CONSTRAINT legacy_entities_last_patch_event_id_fkey FOREIGN KEY (last_patch_event_id) REFERENCES brain.patch_events(id) ON DELETE SET NULL;


--
-- Name: patch_event_enrichments patch_event_enrichments_patch_event_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.patch_event_enrichments
    ADD CONSTRAINT patch_event_enrichments_patch_event_id_fkey FOREIGN KEY (patch_event_id) REFERENCES brain.patch_events(id) ON DELETE CASCADE;


--
-- Name: patch_events patch_events_patch_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.patch_events
    ADD CONSTRAINT patch_events_patch_snapshot_id_fkey FOREIGN KEY (patch_snapshot_id) REFERENCES brain.entity_snapshots(id) ON DELETE SET NULL;


--
-- Name: patch_impact_notes patch_impact_notes_entity_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.patch_impact_notes
    ADD CONSTRAINT patch_impact_notes_entity_id_fkey FOREIGN KEY (entity_id) REFERENCES brain.entities(id) ON DELETE SET NULL;


--
-- Name: plan_items plan_items_run_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.plan_items
    ADD CONSTRAINT plan_items_run_id_fkey FOREIGN KEY (run_id) REFERENCES brain.plan_runs(id) ON DELETE CASCADE;


--
-- Name: plan_items_verworfen plan_items_verworfen_run_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.plan_items_verworfen
    ADD CONSTRAINT plan_items_verworfen_run_id_fkey FOREIGN KEY (run_id) REFERENCES brain.plan_runs(id) ON DELETE CASCADE;


--
-- Name: sheet_boons_ap sheet_boons_ap_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_boons_ap
    ADD CONSTRAINT sheet_boons_ap_snapshot_id_fkey FOREIGN KEY (snapshot_id) REFERENCES brain.entity_snapshots(id) ON DELETE CASCADE;


--
-- Name: sheet_hero_rankings sheet_hero_rankings_entity_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_hero_rankings
    ADD CONSTRAINT sheet_hero_rankings_entity_id_fkey FOREIGN KEY (entity_id) REFERENCES brain.entities(id) ON DELETE SET NULL;


--
-- Name: sheet_hero_rankings sheet_hero_rankings_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_hero_rankings
    ADD CONSTRAINT sheet_hero_rankings_snapshot_id_fkey FOREIGN KEY (snapshot_id) REFERENCES brain.entity_snapshots(id) ON DELETE CASCADE;


--
-- Name: sheet_heroes_stats sheet_heroes_stats_entity_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_heroes_stats
    ADD CONSTRAINT sheet_heroes_stats_entity_id_fkey FOREIGN KEY (entity_id) REFERENCES brain.entities(id) ON DELETE SET NULL;


--
-- Name: sheet_heroes_stats sheet_heroes_stats_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_heroes_stats
    ADD CONSTRAINT sheet_heroes_stats_snapshot_id_fkey FOREIGN KEY (snapshot_id) REFERENCES brain.entity_snapshots(id) ON DELETE CASCADE;


--
-- Name: sheet_items sheet_items_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_items
    ADD CONSTRAINT sheet_items_snapshot_id_fkey FOREIGN KEY (snapshot_id) REFERENCES brain.entity_snapshots(id) ON DELETE CASCADE;


--
-- Name: sheet_raw_heroes sheet_raw_heroes_entity_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_raw_heroes
    ADD CONSTRAINT sheet_raw_heroes_entity_id_fkey FOREIGN KEY (entity_id) REFERENCES brain.entities(id) ON DELETE SET NULL;


--
-- Name: sheet_raw_heroes sheet_raw_heroes_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_raw_heroes
    ADD CONSTRAINT sheet_raw_heroes_snapshot_id_fkey FOREIGN KEY (snapshot_id) REFERENCES brain.entity_snapshots(id) ON DELETE CASCADE;


--
-- Name: sheet_shop_bonuses sheet_shop_bonuses_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_shop_bonuses
    ADD CONSTRAINT sheet_shop_bonuses_snapshot_id_fkey FOREIGN KEY (snapshot_id) REFERENCES brain.entity_snapshots(id) ON DELETE CASCADE;


--
-- Name: sheet_tab_rows sheet_tab_rows_snapshot_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.sheet_tab_rows
    ADD CONSTRAINT sheet_tab_rows_snapshot_id_fkey FOREIGN KEY (snapshot_id) REFERENCES brain.entity_snapshots(id) ON DELETE CASCADE;


--
-- Name: youtube_learning_claims youtube_learning_claims_video_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.youtube_learning_claims
    ADD CONSTRAINT youtube_learning_claims_video_id_fkey FOREIGN KEY (video_id) REFERENCES brain.youtube_videos(video_id);


--
-- Name: youtube_transcript_claim_attempts youtube_transcript_claim_attempts_video_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.youtube_transcript_claim_attempts
    ADD CONSTRAINT youtube_transcript_claim_attempts_video_id_fkey FOREIGN KEY (video_id) REFERENCES brain.youtube_videos(video_id);


--
-- Name: youtube_transcripts youtube_transcripts_source_document_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.youtube_transcripts
    ADD CONSTRAINT youtube_transcripts_source_document_id_fkey FOREIGN KEY (source_document_id) REFERENCES brain.source_documents(id) ON DELETE SET NULL;


--
-- Name: youtube_transcripts youtube_transcripts_video_id_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.youtube_transcripts
    ADD CONSTRAINT youtube_transcripts_video_id_fkey FOREIGN KEY (video_id) REFERENCES brain.youtube_videos(video_id);


--
-- Name: youtube_videos youtube_videos_feed_key_fkey; Type: FK CONSTRAINT; Schema: brain; Owner: -
--

ALTER TABLE ONLY brain.youtube_videos
    ADD CONSTRAINT youtube_videos_feed_key_fkey FOREIGN KEY (feed_key) REFERENCES brain.youtube_feed_sources(feed_key);


--
-- PostgreSQL database dump complete
--

\unrestrict uPhOXcpyTf08SmcOCxE9CTWZBgYRs9IXEeBKgt8tMEnutRPLT67EDSYE0owgVxW

