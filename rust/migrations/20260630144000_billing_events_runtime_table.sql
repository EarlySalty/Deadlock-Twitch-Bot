-- Billing event runtime-created table now owned by migration:
-- twitch_billing_events.

CREATE TABLE IF NOT EXISTS public.twitch_billing_events (
    stripe_event_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    object_id TEXT,
    received_at TEXT NOT NULL,
    livemode INTEGER DEFAULT 0 NOT NULL,
    payload TEXT NOT NULL,
    CONSTRAINT twitch_billing_events_pkey PRIMARY KEY (stripe_event_id)
);
