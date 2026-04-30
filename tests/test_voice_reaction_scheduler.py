"""Integrationstests für den Voice-Reaction-Scheduler-Pfad.

Mockt sowohl Brain als auch State-Store/Audit-Log auf In-Memory-Stores,
sodass der gesamte Chat- und Voice-Trigger-Pfad ohne DB/Anthropic-Anfrage
testbar ist.
"""

from __future__ import annotations

import asyncio
import unittest
from dataclasses import dataclass, field
from typing import Any, Awaitable, Callable, Mapping
from unittest import mock

from bot.community.voice_reaction import scheduler as scheduler_module
from bot.community.voice_reaction.chat_listener import maybe_dispatch_chat_message
from bot.community.voice_reaction.conversation_brain import (
    BrainCallInput,
    BrainCallOutput,
    BrainDecision,
)
from bot.community.voice_reaction.scheduler import (
    VoiceReactionConfig,
    VoiceReactionScheduler,
)


# ---------- Mock state store / audit log ----------


class _MemoryConversation:
    def __init__(self, login: str) -> None:
        self.row: dict[str, Any] = {
            "streamer_login": login,
            "streamer_user_id": None,
            "source": "outreach",
            "state": "open",
            "messages_json": [],
            "last_voice_capture_at": None,
            "last_streamer_signal_at": None,
            "last_bot_message_at": None,
            "last_stance": None,
            "last_confidence": None,
            "human_notify_sent_at": None,
            "human_notify_pending_at": None,
            "closed_at": None,
        }


class _MemoryStore:
    def __init__(self) -> None:
        self.conversations: dict[str, _MemoryConversation] = {}

    def open_conversation(self, *, streamer_login, streamer_user_id, source, initial_messages=None, **_):
        login = streamer_login.lower()
        if login in self.conversations:
            return False
        conv = _MemoryConversation(login)
        conv.row["streamer_user_id"] = streamer_user_id
        conv.row["source"] = source
        conv.row["messages_json"] = list(initial_messages or [])
        self.conversations[login] = conv
        return True

    def append_message(self, *, streamer_login, role, text, meta=None, **_):
        conv = self.conversations.get(streamer_login.lower())
        if conv is None:
            return False
        conv.row["messages_json"].append(
            {"role": role, "ts": "now", "text": text, "meta": meta or {}}
        )
        if role == "voice":
            conv.row["last_voice_capture_at"] = "now"
        if role == "streamer_chat":
            conv.row["last_streamer_signal_at"] = "now"
        if role == "bot_chat":
            conv.row["last_bot_message_at"] = "now"
        return True

    def update_state(self, *, streamer_login, new_state, last_stance=None, last_confidence=None, **_):
        conv = self.conversations.get(streamer_login.lower())
        if conv is None:
            return False
        conv.row["state"] = new_state
        if last_stance is not None:
            conv.row["last_stance"] = last_stance
        if last_confidence is not None:
            conv.row["last_confidence"] = last_confidence
        return True

    def close_conversation(self, *, streamer_login, close_reason, extend_cooldown_days=None, **_):
        conv = self.conversations.get(streamer_login.lower())
        if conv is None:
            return False
        conv.row["state"] = f"closed_{close_reason}"
        conv.row["closed_at"] = "now"
        return True

    def get_conversation(self, *, streamer_login, **_):
        conv = self.conversations.get(streamer_login.lower())
        return None if conv is None else dict(conv.row)

    def load_active_conversations(self, **_):
        return [
            dict(conv.row)
            for conv in self.conversations.values()
            if conv.row["state"] in ("open", "listening", "brain_pending")
        ]

    def has_active_conversation(self, login, **_):
        conv = self.conversations.get(login.lower())
        return conv is not None and conv.row["state"] in ("open", "listening", "brain_pending")


class _MemoryAudit:
    def __init__(self) -> None:
        self.events: list[tuple[str, str, dict[str, Any], str | None]] = []

    def audit(self, login, kind, payload=None, *, correlation_id=None, **_):
        self.events.append((login.lower(), kind, dict(payload or {}), correlation_id))
        return len(self.events)

    def new_correlation_id(self) -> str:
        return f"corr-{len(self.events)}"

    def kinds(self) -> list[str]:
        return [k for _, k, _, _ in self.events]


# ---------- Mock brain ----------


@dataclass
class _BrainProgrammable:
    decisions: list[BrainDecision]
    calls: list[dict[str, Any]] = field(default_factory=list)

    async def respond(self, **kwargs: Any) -> tuple[BrainCallInput, BrainCallOutput]:
        self.calls.append(kwargs)
        decision = self.decisions.pop(0) if self.decisions else _silence_decision()
        call_input = BrainCallInput(
            system_prompt_version="test",
            model="claude-test",
            user_prompt="test-prompt",
            history_length=len(list(kwargs.get("history") or [])),
            latest_signal_kind=str(kwargs.get("latest_signal_kind") or ""),
            latest_signal_text=str(kwargs.get("latest_signal_text") or ""),
        )
        call_output = BrainCallOutput(
            decision=decision,
            raw_response={"mock": True},
            tokens_in=10,
            tokens_out=5,
            latency_ms=42,
            cost_usd_estimate=0.0001,
        )
        return call_input, call_output


def _decision(**overrides) -> BrainDecision:
    base = dict(
        stance="smalltalk",
        confidence=0.5,
        reasoning_summary="ok",
        should_respond=True,
        response_text="hey",
        should_notify_human=False,
        should_close=False,
        close_reason=None,
        suggest_voice_recheck_after_seconds=None,
        raw_tool_input={},
    )
    base.update(overrides)
    return BrainDecision(**base)


def _silence_decision() -> BrainDecision:
    return _decision(should_respond=False, response_text=None, stance="neutral")


# ---------- Mock chat bot ----------


class _StubChatBot:
    def __init__(self) -> None:
        self.sends: list[tuple[str, str]] = []

    def _get_outbound_chat_suppression(self, channel, source):  # noqa: ARG002
        return None

    async def _send_chat_message(self, channel, text, *, source):  # noqa: ARG002
        self.sends.append((channel.name, text))
        return True


# ---------- Helpers ----------


def _patch_state_and_audit(test_case: unittest.TestCase) -> tuple[_MemoryStore, _MemoryAudit]:
    store = _MemoryStore()
    audit = _MemoryAudit()
    patches = [
        mock.patch.object(scheduler_module.state_store, "open_conversation", side_effect=store.open_conversation),
        mock.patch.object(scheduler_module.state_store, "append_message", side_effect=store.append_message),
        mock.patch.object(scheduler_module.state_store, "update_state", side_effect=store.update_state),
        mock.patch.object(scheduler_module.state_store, "close_conversation", side_effect=store.close_conversation),
        mock.patch.object(scheduler_module.state_store, "get_conversation", side_effect=store.get_conversation),
        mock.patch.object(
            scheduler_module.state_store,
            "load_active_conversations",
            side_effect=store.load_active_conversations,
        ),
        mock.patch.object(
            scheduler_module.state_store,
            "has_active_conversation",
            side_effect=store.has_active_conversation,
        ),
        mock.patch.object(scheduler_module.audit_log, "audit", side_effect=audit.audit),
        mock.patch.object(
            scheduler_module.audit_log,
            "new_correlation_id",
            side_effect=audit.new_correlation_id,
        ),
    ]
    for patcher in patches:
        patcher.start()
        test_case.addCleanup(patcher.stop)
    return store, audit


def _build_scheduler(
    *,
    decisions: list[BrainDecision],
    chat_bot: Any | None = None,
    config: VoiceReactionConfig | None = None,
) -> tuple[VoiceReactionScheduler, _BrainProgrammable]:
    cfg = config or VoiceReactionConfig(
        enabled=True,
        dry_run=False,
        random_spread_seconds=(0, 0),
    )
    brain = _BrainProgrammable(decisions=decisions)
    sched = VoiceReactionScheduler(
        config=cfg,
        chat_bot=chat_bot,
        brain=brain,
        transcribe=None,
        live_check=None,
    )
    return sched, brain


# ---------- Tests ----------


class OpenConversationTests(unittest.TestCase):
    def test_open_conversation_creates_row_and_audit(self) -> None:
        store, audit = _patch_state_and_audit(self)
        sched, _ = _build_scheduler(decisions=[])
        correlation_id = asyncio.run(
            sched.open_conversation(
                login="FooStream",
                user_id="42",
                source="outreach",
                initial_text="hey foo",
            )
        )
        self.assertIsNotNone(correlation_id)
        self.assertIn("foostream", store.conversations)
        self.assertIn("conversation_opened", audit.kinds())
        self.assertTrue(sched.is_active_channel("foostream"))
        # Es muss genau ein voice-trigger in der Queue stecken
        self.assertEqual(sched._queue.qsize(), 1)

    def test_open_conversation_no_op_when_disabled(self) -> None:
        store, audit = _patch_state_and_audit(self)
        sched, _ = _build_scheduler(
            decisions=[],
            config=VoiceReactionConfig(enabled=False),
        )
        result = asyncio.run(
            sched.open_conversation(login="x", user_id=None, source="outreach")
        )
        self.assertIsNone(result)
        self.assertEqual(store.conversations, {})
        self.assertEqual(audit.events, [])


class ChatTriggerTests(unittest.TestCase):
    def test_chat_trigger_runs_brain_and_sends_response(self) -> None:
        store, audit = _patch_state_and_audit(self)
        chat_bot = _StubChatBot()
        sched, brain = _build_scheduler(
            decisions=[_decision(stance="smalltalk", response_text="lol gg")],
            chat_bot=chat_bot,
        )

        # Conversation manuell anlegen, ohne den voice-trigger
        store.open_conversation(
            streamer_login="streamer1",
            streamer_user_id="100",
            source="outreach",
            initial_messages=[
                {"role": "bot_chat", "ts": "t0", "text": "hey", "meta": {}},
            ],
        )
        sched._active_channels.add("streamer1")

        async def run() -> None:
            await sched.enqueue_chat(
                login="streamer1",
                text="moin",
                author="streamer1",
            )
            trigger = await sched._queue.get()
            await sched._handle_trigger(trigger)

        asyncio.run(run())

        self.assertEqual(chat_bot.sends, [("streamer1", "lol gg")])
        # State must be 'listening' nach erfolgreichem brain
        self.assertEqual(store.conversations["streamer1"].row["state"], "listening")
        self.assertIn("streamer_chat_received", audit.kinds())
        self.assertIn("brain_call_input", audit.kinds())
        self.assertIn("brain_call_output", audit.kinds())
        self.assertIn("bot_message_sent", audit.kinds())
        self.assertEqual(brain.calls[0]["latest_signal_kind"], "streamer_chat")

    def test_silence_decision_results_in_no_send(self) -> None:
        store, audit = _patch_state_and_audit(self)
        chat_bot = _StubChatBot()
        sched, _ = _build_scheduler(
            decisions=[_silence_decision()],
            chat_bot=chat_bot,
        )

        store.open_conversation(
            streamer_login="streamer2",
            streamer_user_id="42",
            source="outreach",
        )
        sched._active_channels.add("streamer2")

        async def run() -> None:
            await sched.enqueue_chat(login="streamer2", text="hi", author="streamer2")
            trigger = await sched._queue.get()
            await sched._handle_trigger(trigger)

        asyncio.run(run())
        self.assertEqual(chat_bot.sends, [])
        self.assertNotIn("bot_message_sent", audit.kinds())
        self.assertEqual(store.conversations["streamer2"].row["state"], "listening")

    def test_should_notify_human_sets_pending_marker(self) -> None:
        store, audit = _patch_state_and_audit(self)
        chat_bot = _StubChatBot()

        # Patch _mark_human_notify_pending → in-memory
        from bot.community.voice_reaction import scheduler as sched_mod

        def _stub_mark_pending(self, login):
            store.conversations[login.lower()].row["human_notify_pending_at"] = "now"

        patch = mock.patch.object(
            sched_mod.VoiceReactionScheduler,
            "_mark_human_notify_pending",
            _stub_mark_pending,
        )
        patch.start()
        self.addCleanup(patch.stop)

        sched, _ = _build_scheduler(
            decisions=[
                _decision(
                    stance="interested",
                    response_text="cool, schau gerne in mein Profil",
                    should_notify_human=True,
                ),
            ],
            chat_bot=chat_bot,
        )
        store.open_conversation(
            streamer_login="streamer_notify",
            streamer_user_id="555",
            source="outreach",
        )
        sched._active_channels.add("streamer_notify")

        async def run() -> None:
            await sched.enqueue_chat(
                login="streamer_notify",
                text="klingt nice, wo finde ich mehr?",
                author="streamer_notify",
            )
            trigger = await sched._queue.get()
            await sched._handle_trigger(trigger)

        asyncio.run(run())

        self.assertEqual(
            store.conversations["streamer_notify"].row["human_notify_pending_at"], "now"
        )
        self.assertIn("discord_notify_pending", audit.kinds())

    def test_decision_close_exhausted_sets_closed_state(self) -> None:
        store, _ = _patch_state_and_audit(self)
        chat_bot = _StubChatBot()
        sched, _ = _build_scheduler(
            decisions=[
                _decision(
                    stance="exhausted",
                    should_respond=False,
                    response_text=None,
                    should_close=True,
                    close_reason="exhausted",
                ),
            ],
            chat_bot=chat_bot,
        )
        store.open_conversation(
            streamer_login="streamer3",
            streamer_user_id="x",
            source="outreach",
        )
        sched._active_channels.add("streamer3")

        async def run() -> None:
            await sched.enqueue_chat(login="streamer3", text="nein danke", author="streamer3")
            trigger = await sched._queue.get()
            await sched._handle_trigger(trigger)

        asyncio.run(run())
        self.assertEqual(
            store.conversations["streamer3"].row["state"], "closed_exhausted"
        )
        self.assertFalse(sched.is_active_channel("streamer3"))

    def test_dry_run_skips_actual_send(self) -> None:
        store, audit = _patch_state_and_audit(self)
        chat_bot = _StubChatBot()
        sched, _ = _build_scheduler(
            decisions=[_decision(response_text="hey")],
            chat_bot=chat_bot,
            config=VoiceReactionConfig(
                enabled=True,
                dry_run=True,
                random_spread_seconds=(0, 0),
            ),
        )
        store.open_conversation(
            streamer_login="streamer4",
            streamer_user_id="y",
            source="outreach",
        )
        sched._active_channels.add("streamer4")

        async def run() -> None:
            await sched.enqueue_chat(login="streamer4", text="hi", author="streamer4")
            trigger = await sched._queue.get()
            await sched._handle_trigger(trigger)

        asyncio.run(run())
        self.assertEqual(chat_bot.sends, [])
        kinds = audit.kinds()
        # bot_message_send_failed wegen dry_run, ABER kein bot_message_sent
        self.assertNotIn("bot_message_sent", kinds)


class VoiceTriggerTests(unittest.TestCase):
    def test_voice_capture_to_brain_pipeline(self) -> None:
        store, audit = _patch_state_and_audit(self)
        chat_bot = _StubChatBot()

        @dataclass
        class _Result:
            text: str = "hi nathan, was geht"
            engine: str = "openai_api"
            model: str = "whisper-1"
            language: str = "de"
            duration_seconds: float = 60.0
            segments: tuple = ()

        async def transcribe(_path: Any) -> _Result:
            return _Result()

        async def runner(*args: str) -> tuple[int, bytes]:
            from pathlib import Path
            out_idx = args.index("-o")
            Path(args[out_idx + 1]).write_bytes(b"\x00" * 50_000)
            return 0, b""

        cfg = VoiceReactionConfig(
            enabled=True,
            dry_run=False,
            capture_seconds=10,
            random_spread_seconds=(0, 0),
        )
        sched = VoiceReactionScheduler(
            config=cfg,
            chat_bot=chat_bot,
            brain=_BrainProgrammable(
                decisions=[_decision(stance="questioning", response_text="moin")]
            ),
            transcribe=transcribe,
            live_check=None,
            capture_runner=runner,
        )

        store.open_conversation(
            streamer_login="streamer5",
            streamer_user_id="abc",
            source="outreach",
        )
        sched._active_channels.add("streamer5")
        sched._semaphore = asyncio.Semaphore(1)

        async def run() -> None:
            await sched.enqueue_voice(login="streamer5", user_id="abc", delay_seconds=0)
            trigger = await sched._queue.get()
            await sched._handle_trigger(trigger)

        asyncio.run(run())

        self.assertEqual(chat_bot.sends, [("streamer5", "moin")])
        kinds = audit.kinds()
        self.assertIn("voice_capture_started", kinds)
        self.assertIn("voice_capture_done", kinds)
        self.assertIn("whisper_call", kinds)
        self.assertIn("brain_call_input", kinds)
        self.assertIn("brain_call_output", kinds)

    def test_voice_capture_offline_skips_brain(self) -> None:
        store, audit = _patch_state_and_audit(self)
        chat_bot = _StubChatBot()

        async def live_check(_login: str) -> bool:
            return False

        sched = VoiceReactionScheduler(
            config=VoiceReactionConfig(
                enabled=True,
                dry_run=False,
                random_spread_seconds=(0, 0),
            ),
            chat_bot=chat_bot,
            brain=_BrainProgrammable(decisions=[]),
            transcribe=None,
            live_check=live_check,
        )

        store.open_conversation(
            streamer_login="streamer6",
            streamer_user_id="xyz",
            source="outreach",
        )
        sched._active_channels.add("streamer6")
        sched._semaphore = asyncio.Semaphore(1)

        async def run() -> None:
            await sched.enqueue_voice(login="streamer6", user_id="xyz", delay_seconds=0)
            trigger = await sched._queue.get()
            await sched._handle_trigger(trigger)

        asyncio.run(run())
        kinds = audit.kinds()
        self.assertIn("voice_capture_failed", kinds)
        self.assertNotIn("brain_call_output", kinds)
        self.assertEqual(chat_bot.sends, [])

    def test_cost_cap_skips_voice_capture(self) -> None:
        store, audit = _patch_state_and_audit(self)
        sched = VoiceReactionScheduler(
            config=VoiceReactionConfig(
                enabled=True,
                dry_run=False,
                max_daily_transcriptions=0,
                random_spread_seconds=(0, 0),
            ),
            chat_bot=None,
            brain=_BrainProgrammable(decisions=[]),
        )
        store.open_conversation(
            streamer_login="streamer7",
            streamer_user_id=None,
            source="outreach",
        )
        sched._active_channels.add("streamer7")
        sched._semaphore = asyncio.Semaphore(1)

        async def run() -> None:
            await sched.enqueue_voice(login="streamer7", delay_seconds=0)
            trigger = await sched._queue.get()
            await sched._handle_trigger(trigger)

        asyncio.run(run())
        kinds = audit.kinds()
        self.assertIn("cost_cap_skip", kinds)
        self.assertNotIn("voice_capture_started", kinds)


# ---------- Chat-Listener-Filter ----------


class ChatListenerFilterTests(unittest.TestCase):
    def test_streamer_message_dispatches(self) -> None:
        captured: list[dict[str, Any]] = []

        class _SchedulerStub:
            config = VoiceReactionConfig(enabled=True, bot_login="deadlock_partner")

            def is_active_channel(self, login: str) -> bool:
                return login == "streamerx"

            async def enqueue_chat(self, **kwargs):
                captured.append(kwargs)

        class _Author:
            name = "streamerx"

        result = asyncio.run(
            maybe_dispatch_chat_message(
                scheduler=_SchedulerStub(),
                channel_login="streamerx",
                author=_Author(),
                text="hi",
                bot_login="deadlock_partner",
            )
        )
        self.assertTrue(result)
        self.assertEqual(captured[0]["login"], "streamerx")

    def test_random_viewer_does_not_dispatch_without_mention(self) -> None:
        class _SchedulerStub:
            def is_active_channel(self, login: str) -> bool:
                return True

            async def enqueue_chat(self, **kwargs):  # noqa: ARG002
                raise AssertionError("should not be called")

        class _Author:
            name = "randomviewer"

        result = asyncio.run(
            maybe_dispatch_chat_message(
                scheduler=_SchedulerStub(),
                channel_login="streamerx",
                author=_Author(),
                text="lol",
                bot_login="deadlock_partner",
            )
        )
        self.assertFalse(result)

    def test_random_viewer_with_bot_mention_dispatches(self) -> None:
        captured: list[dict[str, Any]] = []

        class _SchedulerStub:
            config = VoiceReactionConfig(enabled=True, bot_login="deadlock_partner")

            def is_active_channel(self, login: str) -> bool:
                return True

            async def enqueue_chat(self, **kwargs):
                captured.append(kwargs)

        class _Author:
            name = "randomviewer"

        result = asyncio.run(
            maybe_dispatch_chat_message(
                scheduler=_SchedulerStub(),
                channel_login="streamerx",
                author=_Author(),
                text="@deadlock_partner welche features?",
                bot_login="deadlock_partner",
            )
        )
        self.assertTrue(result)
        self.assertEqual(captured[0]["author"], "randomviewer")

    def test_inactive_channel_short_circuits(self) -> None:
        class _SchedulerStub:
            def is_active_channel(self, login: str) -> bool:  # noqa: ARG002
                return False

            async def enqueue_chat(self, **kwargs):  # noqa: ARG002
                raise AssertionError("should not be called")

        result = asyncio.run(
            maybe_dispatch_chat_message(
                scheduler=_SchedulerStub(),
                channel_login="anyone",
                author=mock.Mock(name="x"),
                text="x",
            )
        )
        self.assertFalse(result)


if __name__ == "__main__":
    unittest.main()
