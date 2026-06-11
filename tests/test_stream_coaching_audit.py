from __future__ import annotations

import tempfile
import unittest
from collections import deque
from pathlib import Path

from scripts.audit_stream_tos import _new_findings
from bot.stream_coaching_audit.service import (
    AuditError,
    AuditFinding,
    AuditReport,
    AuditSegment,
    _classify_source,
    _extract_json_object,
    detect_rule_findings,
    notify_findings_discord_dm,
    redact_text,
    write_report,
)


class StreamCoachingAuditTests(unittest.TestCase):
    def test_detect_rule_findings_redacts_hate_speech_evidence(self) -> None:
        segment = AuditSegment(
            segment_id="s00001",
            start_seconds=62.0,
            end_seconds=65.0,
            text="Das war eine klar rassistische Aussage mit n1gger im Satz.",
        )

        findings = detect_rule_findings([segment])

        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0].category, "hate_speech_slur")
        self.assertEqual(findings[0].severity, "high")
        self.assertIn("[REDACTED]", findings[0].evidence_redacted)
        self.assertNotIn("n1gger", findings[0].evidence_redacted.lower())

    def test_redact_text_handles_obfuscated_word(self) -> None:
        self.assertEqual(redact_text("vorher n.i.g.g.e.r nachher"), "vorher [REDACTED] nachher")

    def test_classify_source_supports_file_vod_and_live(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            local_file = Path(tmp) / "capture.mp4"
            local_file.write_bytes(b"x")
            self.assertEqual(_classify_source(str(local_file), "auto"), "file")
        self.assertEqual(_classify_source("https://www.twitch.tv/videos/123456", "auto"), "vod")
        self.assertEqual(_classify_source("earlysalty", "auto"), "live")
        with self.assertRaises(AuditError):
            _classify_source("https://example.com/video/123", "auto")

    def test_extract_json_object_ignores_think_wrapper(self) -> None:
        payload = _extract_json_object('<think>intern</think>{"findings": []}')
        self.assertEqual(payload, {"findings": []})

    def test_write_report_persists_only_redacted_evidence(self) -> None:
        report = AuditReport(
            report_id="stream-audit-test",
            created_at="2026-06-02T10:00:00+00:00",
            source_type="vod",
            source_label="https://www.twitch.tv/videos/123",
            channel_login=None,
            transcriber_engine="faster_whisper",
            transcriber_model="small",
            llm_provider="none",
            raw_transcript_persisted=False,
            analyzed_segments=1,
            findings=(
                AuditFinding(
                    segment_id="s00001",
                    start_seconds=1.0,
                    end_seconds=2.0,
                    category="hate_speech_slur",
                    severity="high",
                    detector="local_rule",
                    confidence="high",
                    reason="Manuell pruefen.",
                    evidence_redacted="Text mit [REDACTED].",
                    evidence_sha256="abc",
                ),
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            json_path, markdown_path = write_report(report, output_dir=Path(tmp))

            self.assertIn("[REDACTED]", json_path.read_text(encoding="utf-8"))
            self.assertIn("[REDACTED]", markdown_path.read_text(encoding="utf-8"))
            self.assertEqual(json_path.stat().st_mode & 0o777, 0o600)
            self.assertEqual(markdown_path.stat().st_mode & 0o777, 0o600)

    def test_live_watch_deduplicates_recent_finding(self) -> None:
        finding = AuditFinding(
            segment_id="s00001",
            start_seconds=1.0,
            end_seconds=2.0,
            category="hate_speech_slur",
            severity="high",
            detector="local_rule",
            confidence="high",
            reason="Manuell pruefen.",
            evidence_redacted="Text mit [REDACTED].",
            evidence_sha256="abc",
        )
        seen: set[tuple[str, str]] = set()
        seen_order: deque[tuple[str, str]] = deque()

        self.assertEqual(_new_findings((finding,), seen=seen, seen_order=seen_order), [finding])
        self.assertEqual(_new_findings((finding,), seen=seen, seen_order=seen_order), [])

    def test_discord_dm_uses_redacted_finding_only(self) -> None:
        finding = AuditFinding(
            segment_id="s00001",
            start_seconds=1.0,
            end_seconds=2.0,
            category="hate_speech_slur",
            severity="high",
            detector="local_rule",
            confidence="high",
            reason="Manuell pruefen.",
            evidence_redacted="Text mit [REDACTED].",
            evidence_sha256="abc",
        )
        dm_call: list[tuple[str, str, dict]] = []

        async def sender(token: str, user_id: str, payload: dict) -> bool:
            dm_call.append((token, user_id, payload))
            return True

        import asyncio

        sent = asyncio.run(
            notify_findings_discord_dm(
                "https://twitch.tv/example",
                (finding,),
                discord_user_id="123456",
                bot_token="secret",
                sender=sender,
            )
        )

        self.assertTrue(sent)
        token, user_id, payload = dm_call[0]
        self.assertEqual(token, "secret")
        self.assertEqual(user_id, "123456")
        self.assertIn("[REDACTED]", payload["content"])


class YouTubeArchiveTests(unittest.TestCase):
    def test_parse_srt_maps_timestamps_and_skips_broken_blocks(self) -> None:
        from bot.stream_coaching_audit import youtube_archive

        srt = (
            "1\n00:00:01,500 --> 00:00:04,000\nhallo welt\n\n"
            "kaputter block ohne zeit\n\n"
            "2\n01:02:03,250 --> 01:02:05,000\nzweite\nzeile\n"
        )
        segments = youtube_archive.parse_srt(srt)
        self.assertEqual(len(segments), 2)
        self.assertEqual(segments[0].text, "hallo welt")
        self.assertAlmostEqual(segments[0].start_seconds, 1.5)
        self.assertAlmostEqual(segments[1].start_seconds, 3723.25)
        self.assertEqual(segments[1].text, "zweite zeile")
        self.assertEqual(segments[1].segment_id, "s00002")

    def test_censor_regex_matches_youtube_marker_variants(self) -> None:
        from bot.stream_coaching_audit.youtube_archive import CENSOR_RE

        self.assertTrue(CENSOR_RE.search("das ist [ __ ] krass"))
        self.assertTrue(CENSOR_RE.search("[__]"))
        self.assertTrue(CENSOR_RE.search("ein [ ____ ] wort"))
        self.assertFalse(CENSOR_RE.search("ganz normaler text [klammer]"))

    def test_next_offset_from_range(self) -> None:
        from bot.stream_coaching_audit.youtube_archive import _next_offset_from_range

        self.assertEqual(_next_offset_from_range("bytes=0-999"), 1000)
        self.assertEqual(_next_offset_from_range(None), 0)
        self.assertEqual(_next_offset_from_range("unfug"), 0)

    def test_vod_jump_url_supports_youtube_watch_links(self) -> None:
        from bot.stream_coaching_audit.service import _vod_jump_url

        self.assertEqual(
            _vod_jump_url("https://www.youtube.com/watch?v=abc", 75),
            "https://www.youtube.com/watch?v=abc&t=75s",
        )
        self.assertEqual(
            _vod_jump_url("https://www.twitch.tv/videos/123", 3725),
            "https://www.twitch.tv/videos/123?t=1h2m5s",
        )
        self.assertEqual(_vod_jump_url("https://twitch.tv/kanal", 10), "")

    def test_spot_check_replaces_censored_segments_only(self) -> None:
        from unittest import mock

        from bot.stream_coaching_audit import youtube_archive

        segments = [
            AuditSegment("s00001", 0.0, 2.0, "alles gut hier"),
            AuditSegment("s00002", 10.0, 12.0, "du bist so ein [ __ ] typ"),
        ]

        class _FakeTranscript:
            text = "du bist so ein vollidiot typ"

        class _FakeTranscriber:
            def transcribe(self, path: Path) -> _FakeTranscript:
                return _FakeTranscript()

        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            youtube_archive, "_cut_snippet", return_value=Path(tmp) / "snippet.wav"
        ), mock.patch(
            "bot.social_media.transcription.whisper.get_transcriber",
            return_value=_FakeTranscriber(),
        ):
            result, checked = youtube_archive.spot_check_censored_segments(
                segments, Path(tmp) / "media.mp4", Path(tmp) / "work"
            )

        self.assertEqual(checked, 1)
        self.assertEqual(result[0].text, "alles gut hier")
        self.assertEqual(result[1].text, "du bist so ein vollidiot typ")
        self.assertEqual(result[1].segment_id, "s00002")
        self.assertAlmostEqual(result[1].start_seconds, 10.0)

    def test_parser_accepts_watch_record_flags(self) -> None:
        from scripts.audit_stream_tos import _parser

        args = _parser().parse_args(
            ["--authorized", "--watch-record", "--discord-dm", "https://www.twitch.tv/foo"]
        )
        self.assertTrue(args.watch_record)
        self.assertTrue(args.discord_dm)
        self.assertEqual(args.source, ["https://www.twitch.tv/foo"])
        self.assertGreater(args.caption_timeout_hours, 0)


if __name__ == "__main__":
    unittest.main()
