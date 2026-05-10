from __future__ import annotations

import contextlib
import json
import sqlite3
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from aiohttp import web

from bot.social_media import enrichment as enrichment_mod
from bot.social_media import settings as settings_mod
from bot.social_media.dashboard import SocialMediaDashboard
from bot.social_media.enrichment import (
    STATUS_DONE,
    STATUS_FAILED,
    STATUS_PENDING,
    STATUS_SKIPPED,
    ClipEnrichmentPipeline,
    ensure_enrichment_row,
    get_enrichment,
    iter_pending_enrichments,
    update_enrichment_status,
)
from bot.social_media.llm._parsing import parse_llm_payload
from bot.social_media.llm.base import (
    LLMProviderError,
    LLMRequest,
    LLMResponse,
    LLMUnavailable,
    PlatformEnrichment,
)
from bot.social_media.llm.dispatcher import LLMDispatcher
from bot.social_media.transcription.correction import correct_transcript
from bot.social_media.transcription.vocab import (
    VocabEntry,
    delete_vocab_entry,
    list_vocab,
    load_all_vocab,
    upsert_vocab_entry,
)
from bot.social_media.transcription.whisper import (
    TranscriberUnavailable,
    TranscriptionResult,
    TranscriptSegment,
)


class _SqliteCompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=(), *args, **kwargs):
        del args, kwargs
        normalized_sql = sql.replace("%s", "?")
        normalized_sql = normalized_sql.replace("CURRENT_TIMESTAMP", "datetime('now')")
        normalized_sql = normalized_sql.replace("'[]'::JSONB", "'[]'")
        normalized_sql = normalized_sql.replace("JSONB", "TEXT")
        normalized_sql = normalized_sql.replace("TIMESTAMPTZ", "TEXT")
        return self._conn.execute(normalized_sql, params)

    def __getattr__(self, item):
        return getattr(self._conn, item)


SCHEMA_SQL = """
CREATE TABLE twitch_streamers (
    twitch_login TEXT PRIMARY KEY,
    twitch_user_id TEXT
);

CREATE TABLE twitch_clips_social_media (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    clip_id TEXT NOT NULL UNIQUE,
    clip_url TEXT,
    clip_title TEXT,
    clip_thumbnail_url TEXT,
    streamer_login TEXT NOT NULL,
    twitch_user_id TEXT,
    created_at TEXT,
    duration_seconds REAL,
    view_count INTEGER DEFAULT 0,
    game_name TEXT,
    status TEXT DEFAULT 'pending',
    source_kind TEXT DEFAULT 'twitch',
    upload_local_path TEXT,
    local_file_path TEXT,
    retention_until TEXT,
    discarded_at TEXT,
    layout_override_json TEXT,
    uploaded_tiktok INTEGER DEFAULT 0,
    uploaded_youtube INTEGER DEFAULT 0,
    uploaded_instagram INTEGER DEFAULT 0
);

CREATE TABLE social_media_streamer_layout (
    streamer_login TEXT PRIMARY KEY,
    layout_json TEXT NOT NULL,
    cam_enabled INTEGER NOT NULL DEFAULT 1,
    mode TEXT NOT NULL DEFAULT 'pip',
    updated_at TEXT,
    updated_by TEXT
);

CREATE TABLE deadlock_vocab (
    term TEXT PRIMARY KEY,
    canonical TEXT NOT NULL,
    category TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'manual',
    aliases TEXT NOT NULL DEFAULT '[]',
    weight INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT
);

CREATE TABLE social_media_clip_enrichment (
    clip_db_id INTEGER PRIMARY KEY,
    transcript_raw TEXT,
    transcript_corrected TEXT,
    transcript_segments TEXT,
    transcript_lang TEXT,
    detected_terms TEXT NOT NULL DEFAULT '[]',
    title_youtube TEXT,
    title_tiktok TEXT,
    title_instagram TEXT,
    description_youtube TEXT,
    description_tiktok TEXT,
    description_instagram TEXT,
    hashtags_youtube TEXT NOT NULL DEFAULT '[]',
    hashtags_tiktok TEXT NOT NULL DEFAULT '[]',
    hashtags_instagram TEXT NOT NULL DEFAULT '[]',
    llm_provider TEXT,
    llm_model TEXT,
    cost_usd_estimate REAL,
    status TEXT NOT NULL DEFAULT 'pending',
    error_message TEXT,
    started_at TEXT,
    completed_at TEXT,
    edited_by TEXT,
    updated_at TEXT
);

CREATE TABLE social_media_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT,
    updated_by TEXT
);
"""


class _StorageStubBase(unittest.TestCase):
    def setUp(self) -> None:  # noqa: D401
        super().setUp()
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        self.conn.executescript(SCHEMA_SQL)
        self.conn.execute(
            "INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES (?, ?)",
            ("streamer_a", "user-1"),
        )
        self.patches = contextlib.ExitStack()
        for module in (
            "bot.social_media.transcription.vocab",
            "bot.social_media.enrichment",
            "bot.social_media.settings",
        ):
            self.patches.enter_context(
                patch(
                    f"{module}.transaction",
                    side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
                )
            )
            self.patches.enter_context(
                patch(
                    f"{module}.readonly_connection",
                    side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
                )
            )

    def tearDown(self) -> None:  # noqa: D401
        self.patches.close()
        self.conn.close()
        super().tearDown()


# ---------------------------------------------------------------------------
# Vocab CRUD
# ---------------------------------------------------------------------------

class VocabCrudTests(_StorageStubBase):
    def test_upsert_then_get_then_delete(self) -> None:
        entry = upsert_vocab_entry(
            term="Pocket",
            canonical="Pocket",
            category="hero",
            aliases=["the pocket", "PocketHero"],
            weight=5,
        )
        self.assertEqual(entry.term, "pocket")
        self.assertEqual(entry.canonical, "Pocket")
        self.assertIn("the pocket", entry.aliases)
        self.assertEqual(entry.weight, 5)

        loaded = load_all_vocab()
        self.assertEqual(len(loaded), 1)
        self.assertEqual(loaded[0].canonical, "Pocket")

        items, total = list_vocab(category="hero")
        self.assertEqual(total, 1)
        self.assertEqual(items[0].canonical, "Pocket")

        self.assertTrue(delete_vocab_entry("Pocket"))
        self.assertEqual(load_all_vocab(), [])

    def test_invalid_category_raises(self) -> None:
        with self.assertRaises(ValueError):
            upsert_vocab_entry(
                term="Foo",
                canonical="Foo",
                category="notacategory",
            )

    def test_search_filters_by_query(self) -> None:
        upsert_vocab_entry(term="vyper", canonical="Vyper", category="hero")
        upsert_vocab_entry(term="ricochet", canonical="Ricochet", category="item")
        items, total = list_vocab(query="vy")
        self.assertEqual(total, 1)
        self.assertEqual(items[0].canonical, "Vyper")


# ---------------------------------------------------------------------------
# Fuzzy-Korrektur
# ---------------------------------------------------------------------------

class CorrectionTests(unittest.TestCase):
    vocab = [
        VocabEntry(term="pocket", canonical="Pocket", category="hero", weight=5,
                   aliases=("the pocket",)),
        VocabEntry(term="vyper", canonical="Vyper", category="hero", weight=5),
        VocabEntry(term="ricochet", canonical="Ricochet", category="item", weight=4),
        VocabEntry(term="ult", canonical="Ultimate", category="slang", weight=2,
                   aliases=("ulti", "ultimate")),
        VocabEntry(term="soul orb", canonical="Soul Orb", category="slang", weight=2),
    ]

    def test_exact_replace_token_keeps_canonical_casing(self) -> None:
        result = correct_transcript("vyper used ricochet", vocab=self.vocab)
        self.assertIn("Vyper", result.corrected)
        self.assertIn("Ricochet", result.corrected)
        self.assertIn("Vyper", result.detected_terms)
        self.assertIn("Ricochet", result.detected_terms)

    def test_fuzzy_replace_token_within_threshold(self) -> None:
        # 'pocekt' has Levenshtein distance 2 from 'pocket'
        result = correct_transcript("the pocekt cleared the lane", vocab=self.vocab)
        self.assertIn("Pocket", result.corrected)
        self.assertIn("Pocket", result.detected_terms)

    def test_short_token_does_not_fuzzy_match(self) -> None:
        # 'al' is 2 chars; threshold(2)=0 -> no match
        result = correct_transcript("al played well", vocab=self.vocab)
        self.assertNotIn("Ultimate", result.corrected)

    def test_multi_word_phrase_replaced_first(self) -> None:
        result = correct_transcript("we got a soul orb on top", vocab=self.vocab)
        self.assertIn("Soul Orb", result.corrected)
        self.assertIn("Soul Orb", result.detected_terms)

    def test_returns_unique_detected_terms_in_order(self) -> None:
        text = "Vyper landed ult and used Ricochet, then another ult"
        result = correct_transcript(text, vocab=self.vocab)
        # Detected terms unique
        self.assertEqual(
            sorted(set(result.detected_terms)), sorted(result.detected_terms)
        )
        self.assertIn("Ultimate", result.detected_terms)


# ---------------------------------------------------------------------------
# LLM JSON-Parser
# ---------------------------------------------------------------------------

class LLMParserTests(unittest.TestCase):
    def test_parse_valid_payload(self) -> None:
        payload = {
            "youtube": {
                "title": "Pocket clutch ace",
                "description": "Insane Deadlock clutch.",
                "hashtags": ["#Deadlock", "#Pocket", "#Clutch"],
            },
            "tiktok": {
                "title": "Pocket goes off",
                "description": "Watch this.",
                "hashtags": ["Deadlock", "pocket", "fyp"],
            },
            "instagram": {
                "title": "Pocket clutch",
                "description": "1v3.",
                "hashtags": ["Deadlock", "POCKET"],
            },
        }
        response = parse_llm_payload(
            json.dumps(payload),
            provider="ollama",
            model="qwen2.5:7b-instruct",
            cost_usd_estimate=0.0,
        )
        self.assertEqual(response.provider, "ollama")
        self.assertIn("#Deadlock", response.tiktok.hashtags)
        self.assertIn("#pocket", response.tiktok.hashtags)
        self.assertEqual(response.youtube.title, "Pocket clutch ace")
        self.assertLessEqual(len(response.youtube.title), 100)

    def test_parse_truncates_overlong_titles(self) -> None:
        long_title = "A" * 200
        payload = {
            "youtube": {"title": long_title, "description": "x", "hashtags": ["#Deadlock"]},
            "tiktok": {"title": long_title, "description": "x", "hashtags": ["#Deadlock"]},
            "instagram": {"title": long_title, "description": "x", "hashtags": ["#Deadlock"]},
        }
        response = parse_llm_payload(
            json.dumps(payload),
            provider="ollama",
            model="qwen2.5",
        )
        self.assertLessEqual(len(response.youtube.title), 100)
        self.assertLessEqual(len(response.tiktok.title), 150)
        self.assertLessEqual(len(response.instagram.title), 125)

    def test_parse_handles_code_fence_wrapper(self) -> None:
        payload = {
            "youtube": {"title": "t", "description": "d", "hashtags": ["#a"]},
            "tiktok": {"title": "t", "description": "d", "hashtags": ["#a"]},
            "instagram": {"title": "t", "description": "d", "hashtags": ["#a"]},
        }
        wrapped = "```json\n" + json.dumps(payload) + "\n```"
        response = parse_llm_payload(wrapped, provider="x", model="y")
        self.assertEqual(response.youtube.description, "d")

    def test_parse_rejects_missing_platform(self) -> None:
        payload = {
            "youtube": {"title": "t", "description": "d", "hashtags": ["#x"]},
            "tiktok": {"title": "t", "description": "d", "hashtags": ["#x"]},
        }
        with self.assertRaises(LLMProviderError):
            parse_llm_payload(json.dumps(payload), provider="x", model="y")

    def test_parse_rejects_empty_title(self) -> None:
        payload = {
            "youtube": {"title": "", "description": "d", "hashtags": ["#x"]},
            "tiktok": {"title": "t", "description": "d", "hashtags": ["#x"]},
            "instagram": {"title": "t", "description": "d", "hashtags": ["#x"]},
        }
        with self.assertRaises(LLMProviderError):
            parse_llm_payload(json.dumps(payload), provider="x", model="y")


# ---------------------------------------------------------------------------
# LLM Dispatcher (Provider-Auswahl + Consent)
# ---------------------------------------------------------------------------

class _StubProvider:
    def __init__(self, name: str, response: LLMResponse | None = None,
                 raises: Exception | None = None) -> None:
        self.name = name
        self.model = "stub-model"
        self._response = response
        self._raises = raises
        self.calls = 0

    async def generate(self, request: LLMRequest) -> LLMResponse:
        self.calls += 1
        if self._raises:
            raise self._raises
        assert self._response is not None
        return self._response


def _stub_response(provider: str = "stub") -> LLMResponse:
    return LLMResponse(
        youtube=PlatformEnrichment(title="t", description="d", hashtags=("#Deadlock",)),
        tiktok=PlatformEnrichment(title="t", description="d", hashtags=("#Deadlock",)),
        instagram=PlatformEnrichment(title="t", description="d", hashtags=("#Deadlock",)),
        provider=provider,
        model="stub-model",
        cost_usd_estimate=0.0,
    )


class DispatcherTests(unittest.IsolatedAsyncioTestCase):
    async def test_default_uses_ollama_when_no_consent(self) -> None:
        ollama_stub = _StubProvider("ollama", _stub_response("ollama"))
        minimax_stub = _StubProvider("minimax", _stub_response("minimax"))

        dispatcher = LLMDispatcher(
            provider_override="minimax",  # External requested...
            consent_override=False,        # ...but no consent => fallback to ollama
        )
        with patch.object(
            LLMDispatcher,
            "_instantiate_provider",
            side_effect=lambda name: {"ollama": ollama_stub, "minimax": minimax_stub}[name],
        ):
            request = LLMRequest(transcript="hello world")
            response = await dispatcher.generate(request)
        self.assertEqual(response.provider, "ollama")
        self.assertEqual(ollama_stub.calls, 1)
        self.assertEqual(minimax_stub.calls, 0)

    async def test_external_used_when_consent_granted(self) -> None:
        ollama_stub = _StubProvider("ollama", _stub_response("ollama"))
        minimax_stub = _StubProvider("minimax", _stub_response("minimax"))
        dispatcher = LLMDispatcher(provider_override="minimax", consent_override=True)
        with patch.object(
            LLMDispatcher,
            "_instantiate_provider",
            side_effect=lambda name: {"ollama": ollama_stub, "minimax": minimax_stub}[name],
        ):
            response = await dispatcher.generate(LLMRequest(transcript="x"))
        self.assertEqual(response.provider, "minimax")
        self.assertEqual(minimax_stub.calls, 1)
        self.assertEqual(ollama_stub.calls, 0)

    async def test_falls_back_to_ollama_when_external_errors(self) -> None:
        ollama_stub = _StubProvider("ollama", _stub_response("ollama"))
        minimax_stub = _StubProvider("minimax", raises=LLMProviderError("boom"))
        dispatcher = LLMDispatcher(provider_override="minimax", consent_override=True)
        with patch.object(
            LLMDispatcher,
            "_instantiate_provider",
            side_effect=lambda name: {"ollama": ollama_stub, "minimax": minimax_stub}[name],
        ):
            response = await dispatcher.generate(LLMRequest(transcript="x"))
        self.assertEqual(response.provider, "ollama")
        self.assertEqual(minimax_stub.calls, 1)
        self.assertEqual(ollama_stub.calls, 1)

    async def test_raises_when_all_providers_fail(self) -> None:
        ollama_stub = _StubProvider("ollama", raises=LLMProviderError("local fail"))
        dispatcher = LLMDispatcher(provider_override="ollama", consent_override=False)
        with patch.object(
            LLMDispatcher,
            "_instantiate_provider",
            side_effect=lambda name: {"ollama": ollama_stub}[name],
        ):
            with self.assertRaises(LLMUnavailable):
                await dispatcher.generate(LLMRequest(transcript="x"))


# ---------------------------------------------------------------------------
# Pipeline (Whisper-Stub + LLM-Stub)
# ---------------------------------------------------------------------------

class _StubTranscriber:
    name = "stub"

    def __init__(self, *, raises: Exception | None = None) -> None:
        self._raises = raises

    def transcribe(self, audio_path) -> TranscriptionResult:
        if self._raises:
            raise self._raises
        return TranscriptionResult(
            text="vyper landed an ult",
            segments=(TranscriptSegment(start=0.0, end=2.5, text="vyper landed an ult"),),
            language="en",
            duration_seconds=2.5,
            engine=self.name,
            model="stub",
        )


class _StubDispatcher:
    def __init__(self, response: LLMResponse | None = None, raises: Exception | None = None) -> None:
        self._response = response
        self._raises = raises

    async def generate(self, request: LLMRequest) -> LLMResponse:
        if self._raises:
            raise self._raises
        assert self._response is not None
        return self._response


class PipelineTests(_StorageStubBase, unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:  # noqa: D401
        super().setUp()
        self.conn.execute(
            """
            INSERT INTO twitch_clips_social_media (
                clip_id, clip_url, clip_title, streamer_login, twitch_user_id,
                created_at, duration_seconds, status, source_kind, upload_local_path
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "clip-1",
                "https://example/clip-1",
                "Vyper clutch",
                "streamer_a",
                "user-1",
                "2026-04-26T12:00:00+00:00",
                12.0,
                "pending",
                "manual_upload",
                "data/clips/uploads/streamer_a/clip-1.mp4",
            ),
        )
        upsert_vocab_entry(term="vyper", canonical="Vyper", category="hero")
        # transcribe_clip-Wrapper überspringen (FFmpeg)
        self._patch_transcribe_clip = patch(
            "bot.social_media.enrichment.transcribe_clip",
            new=AsyncMock(side_effect=self._fake_transcribe),
        )
        self._patch_transcribe_clip.start()

    async def _fake_transcribe(self, video_path, *, engine=None):
        if engine is None:
            raise TranscriberUnavailable("no engine")
        return engine.transcribe(video_path)

    def tearDown(self) -> None:  # noqa: D401
        self._patch_transcribe_clip.stop()
        super().tearDown()

    def _clip_db_id(self) -> int:
        row = self.conn.execute(
            "SELECT id FROM twitch_clips_social_media WHERE clip_id = ?",
            ("clip-1",),
        ).fetchone()
        return int(row["id"])

    async def test_pipeline_full_path_done(self) -> None:
        pipeline = ClipEnrichmentPipeline(
            transcriber=_StubTranscriber(),
            dispatcher=_StubDispatcher(_stub_response("ollama")),
        )
        outcome = await pipeline.run(self._clip_db_id())
        self.assertEqual(outcome.status, STATUS_DONE)
        record = get_enrichment(self._clip_db_id())
        self.assertEqual(record.status, STATUS_DONE)
        self.assertEqual(record.llm_provider, "ollama")
        self.assertIn("Vyper", record.detected_terms)
        # Transcript persisted
        self.assertIn("Vyper", record.transcript_corrected or "")
        # Hashtags persisted
        self.assertEqual(record.hashtags_youtube, ["#Deadlock"])

    async def test_pipeline_skipped_when_no_transcriber_and_llm_unavailable(self) -> None:
        pipeline = ClipEnrichmentPipeline(
            transcriber=None,
            dispatcher=_StubDispatcher(raises=LLMUnavailable("no provider")),
        )
        outcome = await pipeline.run(self._clip_db_id())
        self.assertEqual(outcome.status, STATUS_SKIPPED)
        record = get_enrichment(self._clip_db_id())
        self.assertEqual(record.status, STATUS_SKIPPED)

    async def test_pipeline_failed_when_llm_errors_with_transcript(self) -> None:
        pipeline = ClipEnrichmentPipeline(
            transcriber=_StubTranscriber(),
            dispatcher=_StubDispatcher(raises=LLMUnavailable("boom")),
        )
        outcome = await pipeline.run(self._clip_db_id())
        self.assertEqual(outcome.status, STATUS_FAILED)

    async def test_iter_pending_returns_only_pending_or_failed(self) -> None:
        ensure_enrichment_row(self._clip_db_id())
        update_enrichment_status(self._clip_db_id(), status=STATUS_DONE)
        self.assertEqual(iter_pending_enrichments(), [])
        update_enrichment_status(self._clip_db_id(), status=STATUS_PENDING)
        self.assertEqual(iter_pending_enrichments(), [self._clip_db_id()])


# ---------------------------------------------------------------------------
# Admin API (Auth + Vocab + Enrichment-Routes)
# ---------------------------------------------------------------------------

class AdminApiTests(_StorageStubBase, unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:  # noqa: D401
        super().setUp()
        self.patches.enter_context(
            patch(
                "bot.social_media.dashboard.transaction",
                side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
            )
        )
        self.patches.enter_context(
            patch(
                "bot.social_media.dashboard.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(_SqliteCompatConn(self.conn)),
            )
        )
        self.conn.execute(
            """
            INSERT INTO twitch_clips_social_media (
                clip_id, clip_url, clip_title, streamer_login, twitch_user_id,
                created_at, duration_seconds, status, source_kind, upload_local_path
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "clip-x",
                "https://example/clip-x",
                "Pocket clutch",
                "streamer_a",
                "user-1",
                "2026-04-26T12:00:00+00:00",
                12.0,
                "pending",
                "manual_upload",
                "data/clips/uploads/streamer_a/clip-x.mp4",
            ),
        )
        self.conn.commit()
        self.partner = SocialMediaDashboard(
            clip_manager=SimpleNamespace(),
            auth_checker=lambda _r: True,
            auth_level_getter=lambda _r: "partner",
        )
        self.admin = SocialMediaDashboard(
            clip_manager=SimpleNamespace(),
            auth_checker=lambda _r: True,
            auth_level_getter=lambda _r: "admin",
            auth_session_getter=lambda _r: {"discord_user_id": "discord-1"},
        )

    def _clip_db_id(self) -> int:
        row = self.conn.execute(
            "SELECT id FROM twitch_clips_social_media WHERE clip_id = ?",
            ("clip-x",),
        ).fetchone()
        return int(row["id"])

    async def test_partner_blocked_on_all_phase2_admin_routes(self) -> None:
        cdb = self._clip_db_id()
        forbidden_calls = [
            self.partner.api_admin_clip_enrichment_get(
                SimpleNamespace(match_info={"clip_db_id": str(cdb)})
            ),
            self.partner.api_admin_clip_enrichment_put(
                SimpleNamespace(
                    match_info={"clip_db_id": str(cdb)},
                    json=AsyncMock(return_value={"title_youtube": "x"}),
                )
            ),
            self.partner.api_admin_clip_enrichment_run(
                SimpleNamespace(
                    match_info={"clip_db_id": str(cdb)},
                    body_exists=False,
                    json=AsyncMock(return_value={}),
                )
            ),
            self.partner.api_admin_vocab_list(SimpleNamespace(query={})),
            self.partner.api_admin_vocab_upsert(
                SimpleNamespace(json=AsyncMock(return_value={"term": "x", "canonical": "X", "category": "hero"}))
            ),
            self.partner.api_admin_vocab_delete(SimpleNamespace(match_info={"term": "x"})),
            self.partner.api_admin_vocab_seed(SimpleNamespace(body_exists=False)),
        ]
        for coro in forbidden_calls:
            with self.assertRaises(web.HTTPForbidden):
                await coro

    async def test_admin_vocab_crud_lifecycle(self) -> None:
        # Create
        upsert_response = await self.admin.api_admin_vocab_upsert(
            SimpleNamespace(
                json=AsyncMock(
                    return_value={
                        "term": "Pocket",
                        "canonical": "Pocket",
                        "category": "hero",
                        "aliases": ["the pocket"],
                        "weight": 5,
                    }
                )
            )
        )
        self.assertEqual(upsert_response.status, 200)
        body = json.loads(upsert_response.text)
        self.assertEqual(body["canonical"], "Pocket")

        # List
        list_response = await self.admin.api_admin_vocab_list(
            SimpleNamespace(query={"category": "hero"})
        )
        self.assertEqual(list_response.status, 200)
        listed = json.loads(list_response.text)
        self.assertEqual(listed["total"], 1)
        self.assertEqual(listed["items"][0]["canonical"], "Pocket")

        # Delete
        delete_response = await self.admin.api_admin_vocab_delete(
            SimpleNamespace(match_info={"term": "Pocket"})
        )
        self.assertEqual(delete_response.status, 204)

    async def test_admin_enrichment_run_uses_pipeline(self) -> None:
        cdb = self._clip_db_id()

        async def _run(self_pipeline, clip_db_id, *, force=False):  # noqa: ARG001
            ensure_enrichment_row(clip_db_id)
            update_enrichment_status(clip_db_id, status=STATUS_DONE)
            return enrichment_mod.EnrichmentOutcome(
                clip_db_id=clip_db_id,
                status=STATUS_DONE,
                provider="ollama",
                model="stub",
            )

        with patch.object(ClipEnrichmentPipeline, "run", new=_run):
            response = await self.admin.api_admin_clip_enrichment_run(
                SimpleNamespace(
                    match_info={"clip_db_id": str(cdb)},
                    body_exists=False,
                    json=AsyncMock(return_value={}),
                )
            )
        self.assertEqual(response.status, 200)
        body = json.loads(response.text)
        self.assertEqual(body["outcome"]["status"], STATUS_DONE)
        self.assertEqual(body["outcome"]["provider"], "ollama")
        self.assertEqual(body["enrichment"]["status"], STATUS_DONE)

    async def test_admin_enrichment_put_persists_manual_edits(self) -> None:
        cdb = self._clip_db_id()
        ensure_enrichment_row(cdb)

        response = await self.admin.api_admin_clip_enrichment_put(
            SimpleNamespace(
                match_info={"clip_db_id": str(cdb)},
                json=AsyncMock(
                    return_value={
                        "title_youtube": "Edited title",
                        "hashtags_youtube": ["#Deadlock", "POCKET", "#deadlock"],
                    }
                ),
            )
        )
        self.assertEqual(response.status, 200)
        record = get_enrichment(cdb)
        self.assertEqual(record.title_youtube, "Edited title")
        self.assertEqual(record.hashtags_youtube, ["#Deadlock", "#POCKET"])
        self.assertEqual(record.edited_by, "discord-1")


# ---------------------------------------------------------------------------
# Settings (consent toggle)
# ---------------------------------------------------------------------------

class SettingsTests(_StorageStubBase):
    def test_external_llm_consent_default_false(self) -> None:
        self.assertFalse(settings_mod.external_llm_consent())

    def test_external_llm_consent_after_set_true(self) -> None:
        settings_mod.set_setting(
            settings_mod.KEY_EXTERNAL_LLM_CONSENT, True, updated_by="admin"
        )
        self.assertTrue(settings_mod.external_llm_consent())


if __name__ == "__main__":
    unittest.main()
