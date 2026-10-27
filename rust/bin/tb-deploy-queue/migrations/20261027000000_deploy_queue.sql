CREATE TABLE runs (
    id BIGSERIAL PRIMARY KEY,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    parent_sha TEXT,
    target_sha TEXT,
    outcome TEXT CHECK (outcome IN ('deployed', 'no_deploy_needed', 'failed')),
    failed_step TEXT,
    log_path TEXT NOT NULL,
    built TEXT NOT NULL DEFAULT '',
    carried TEXT NOT NULL DEFAULT '',
    build_seconds INTEGER,
    restarted TEXT NOT NULL DEFAULT '',
    pruefen_exit INTEGER,
    exe_report TEXT NOT NULL DEFAULT '',
    live_at TIMESTAMPTZ
);

CREATE TABLE tickets (
    id BIGSERIAL PRIMARY KEY,
    sha TEXT NOT NULL CHECK (sha ~ '^[0-9a-f]{40}$'),
    thread_id TEXT NOT NULL CHECK (thread_id <> ''),
    task_dir TEXT NOT NULL CHECK (task_dir LIKE '/%'),
    deploy_now BOOLEAN NOT NULL DEFAULT false,
    submitted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'running', 'done')),
    compaction TEXT NOT NULL DEFAULT 'pending'
        CHECK (compaction IN ('pending', 'requested', 'confirmed', 'failed', 'timeout', 'skipped')),
    compaction_message_id TEXT,
    compaction_requested_at TIMESTAMPTZ,
    compaction_detail TEXT,
    run_id BIGINT REFERENCES runs (id),
    outcome TEXT CHECK (outcome IN ('deployed', 'no_deploy_needed', 'failed')),
    final_message TEXT,
    notify_attempts INTEGER NOT NULL DEFAULT 0,
    notify_error TEXT,
    notify_next_at TIMESTAMPTZ,
    notified_at TIMESTAMPTZ
);

CREATE INDEX tickets_open_idx ON tickets (state) WHERE notified_at IS NULL;
