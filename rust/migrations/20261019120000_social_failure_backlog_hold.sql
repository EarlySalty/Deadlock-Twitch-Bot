ALTER TABLE social_autopilot_schedule_state DROP CONSTRAINT social_autopilot_schedule_state_production_hold_check;
ALTER TABLE social_autopilot_schedule_state ADD CONSTRAINT social_autopilot_schedule_state_production_hold_check
    CHECK (production_hold IN ('disabled', 'paused', 'review_backlog', 'failure_backlog', 'empty_pool', 'no_schedule', 'outside_lead'));
