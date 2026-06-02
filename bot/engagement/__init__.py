"""AI-Engagement-Layer: MiniMax-M3-Stammgast pro Twitch-Channel.

Spec: /home/naniadm/.claude/plans/ich-m-chte-das-wir-buzzing-pebble.md

Re-Exports + Singleton-Factory für die Pipeline. Konsumenten (bot/chat/bot.py
event_message-Hook) sollen `from bot.engagement import get_pipeline,
IncomingMessage` benutzen.
"""

from __future__ import annotations

import threading

from .auto_off import auto_disable_on_offline
from .background import ensure_started as ensure_background_started
from .conversation import ConversationBuffer
from .minimax_chat import EngagementMinimaxClient
from .lurker_signal import (
    LurkerHint,
    known_regulars_currently_lurking,
    lurker_hint_to_prompt_fragment,
)
from .match_context import MatchSnapshot, get_match_state, poll_match_state
from .persona import PersonaSnapshot, sample_tone
from .threads import (
    Thread,
    auto_close_stale,
    extract_threads,
    load_open_threads_for_user,
    mark_referenced,
    threads_to_prompt_fragment,
)
from .pipeline import (
    Decision,
    EngagementPipeline,
    EngagementSettings,
    HandleResult,
    IncomingMessage,
)
from .rhythm import RhythmGuard

__all__ = [
    "ConversationBuffer",
    "Decision",
    "EngagementMinimaxClient",
    "EngagementPipeline",
    "EngagementSettings",
    "HandleResult",
    "IncomingMessage",
    "LurkerHint",
    "MatchSnapshot",
    "PersonaSnapshot",
    "RhythmGuard",
    "Thread",
    "auto_close_stale",
    "auto_disable_on_offline",
    "ensure_background_started",
    "extract_threads",
    "get_match_state",
    "get_pipeline",
    "known_regulars_currently_lurking",
    "load_open_threads_for_user",
    "lurker_hint_to_prompt_fragment",
    "mark_referenced",
    "poll_match_state",
    "sample_tone",
    "threads_to_prompt_fragment",
]


_pipeline: EngagementPipeline | None = None
_pipeline_lock = threading.Lock()


def get_pipeline() -> EngagementPipeline:
    """Lazy-initialisierter, prozess-globaler Pipeline-Singleton."""
    global _pipeline
    if _pipeline is None:
        with _pipeline_lock:
            if _pipeline is None:
                _pipeline = EngagementPipeline(
                    conversation=ConversationBuffer(),
                    rhythm=RhythmGuard(),
                    minimax=EngagementMinimaxClient(),
                )
    return _pipeline
