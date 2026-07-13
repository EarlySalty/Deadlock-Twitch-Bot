CREATE TABLE IF NOT EXISTS dashboard_admin_audit_events (
    id          BIGSERIAL PRIMARY KEY,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    actor       TEXT NOT NULL,
    method      TEXT NOT NULL,
    path        TEXT NOT NULL,
    status_code INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dashboard_admin_audit_events_occurred_at
    ON dashboard_admin_audit_events (occurred_at DESC, id DESC);
