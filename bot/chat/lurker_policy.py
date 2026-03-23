from __future__ import annotations

PASSIVE_LURKER_STATE = "passive_lurker"
PASSIVE_LURKER_DETAIL = (
    "monitored-only channel without broadcaster authorization runs in passive lurker mode"
)


def is_passive_lurker_channel(
    *,
    is_monitored_only: bool,
    is_partner_active: bool,
    has_raid_auth: bool,
) -> bool:
    """Return True when passive observation is the expected terminal state."""
    return bool(is_monitored_only) and not bool(is_partner_active) and not bool(has_raid_auth)


def should_attempt_runtime_heal(*, is_monitored_only: bool, is_ready: bool) -> bool:
    """Monitored-only lurker channels are not chat-runtime heal targets."""
    if bool(is_monitored_only):
        return False
    return not bool(is_ready)
