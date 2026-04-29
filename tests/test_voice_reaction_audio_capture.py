"""Tests für den Voice-Reaction-Audio-Capture."""

import asyncio
import time
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from bot.community.voice_reaction import audio_capture
from bot.community.voice_reaction.audio_capture import (
    AudioCaptureError,
    CAPTURE_TMP_PREFIX,
    capture,
    cleanup_stale_capture_dirs,
    cleanup_workdir,
)


class CaptureTests(unittest.TestCase):
    def test_capture_writes_file_and_returns_metadata(self) -> None:
        async def runner(*args: str) -> tuple[int, bytes]:
            # streamlink-args parsen: -o <path> ist Pflicht
            self.assertIn("-o", args)
            out_idx = args.index("-o")
            target = Path(args[out_idx + 1])
            target.write_bytes(b"\x00" * (64 * 1024))
            self.assertIn("--hls-duration", args)
            duration_idx = args.index("--hls-duration")
            self.assertEqual(args[duration_idx + 1], "00:01:15")
            return 0, b""

        with TemporaryDirectory() as tmp:
            result = asyncio.run(
                capture(
                    "SomeStreamer",
                    duration_seconds=75,
                    workdir_root=Path(tmp),
                    runner=runner,
                )
            )
            try:
                self.assertTrue(result.media_path.exists())
                self.assertEqual(result.requested_duration_seconds, 75)
                self.assertGreaterEqual(result.bytes, 64 * 1024)
                self.assertTrue(result.workdir.name.startswith(CAPTURE_TMP_PREFIX))
                self.assertEqual(result.media_path.parent, result.workdir)
            finally:
                result.cleanup()
                self.assertFalse(result.workdir.exists())

    def test_capture_rejects_blank_login(self) -> None:
        with self.assertRaises(AudioCaptureError):
            asyncio.run(capture("   ", workdir_root=Path("/tmp")))

    def test_capture_fails_when_no_file_written(self) -> None:
        async def runner(*args: str) -> tuple[int, bytes]:
            return 1, b"streamer offline"

        with TemporaryDirectory() as tmp:
            with self.assertRaises(AudioCaptureError) as ctx:
                asyncio.run(
                    capture(
                        "offline_user",
                        duration_seconds=30,
                        workdir_root=Path(tmp),
                        runner=runner,
                    )
                )
            self.assertIn("streamer offline", str(ctx.exception))
            # Workdir muss bei Fehler aufgeräumt sein
            leftovers = [
                p for p in Path(tmp).iterdir() if p.name.startswith(CAPTURE_TMP_PREFIX)
            ]
            self.assertEqual(leftovers, [])

    def test_capture_fails_when_file_too_small(self) -> None:
        async def runner(*args: str) -> tuple[int, bytes]:
            out_idx = args.index("-o")
            Path(args[out_idx + 1]).write_bytes(b"x" * 100)
            return 0, b""

        with TemporaryDirectory() as tmp:
            with self.assertRaises(AudioCaptureError) as ctx:
                asyncio.run(
                    capture(
                        "tiny",
                        duration_seconds=30,
                        workdir_root=Path(tmp),
                        runner=runner,
                    )
                )
            self.assertIn("Capture zu klein", str(ctx.exception))

    def test_capture_passes_quality_and_url(self) -> None:
        seen = {}

        async def runner(*args: str) -> tuple[int, bytes]:
            seen["args"] = list(args)
            out_idx = args.index("-o")
            Path(args[out_idx + 1]).write_bytes(b"\x00" * 70_000)
            return 0, b""

        with TemporaryDirectory() as tmp:
            result = asyncio.run(
                capture(
                    "FooBar",
                    duration_seconds=10,
                    quality="audio_only",
                    workdir_root=Path(tmp),
                    runner=runner,
                )
            )
            try:
                self.assertIn("https://twitch.tv/foobar", seen["args"])
                self.assertIn("audio_only", seen["args"])
                self.assertIn("--twitch-disable-ads", seen["args"])
            finally:
                result.cleanup()


class CleanupTests(unittest.TestCase):
    def test_cleanup_workdir_only_removes_voice_reaction_dirs(self) -> None:
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            target = root / f"{CAPTURE_TMP_PREFIX}safe"
            target.mkdir()
            (target / "audio.ts").write_bytes(b"x")
            unrelated = root / "other"
            unrelated.mkdir()

            cleanup_workdir(target)
            cleanup_workdir(unrelated)

            self.assertFalse(target.exists())
            self.assertTrue(unrelated.exists())

    def test_cleanup_stale_capture_dirs_removes_old_only(self) -> None:
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            old = root / f"{CAPTURE_TMP_PREFIX}old"
            old.mkdir()
            (old / "audio.ts").write_bytes(b"x")
            old_mtime = time.time() - 7200
            import os
            os.utime(old, (old_mtime, old_mtime))

            fresh = root / f"{CAPTURE_TMP_PREFIX}fresh"
            fresh.mkdir()
            (fresh / "audio.ts").write_bytes(b"y")

            unrelated = root / "system-d-tmp"
            unrelated.mkdir()
            os.utime(unrelated, (old_mtime, old_mtime))

            removed = cleanup_stale_capture_dirs(max_age_seconds=3600, workdir_root=root)
            self.assertEqual(removed, 1)
            self.assertFalse(old.exists())
            self.assertTrue(fresh.exists())
            self.assertTrue(unrelated.exists())


class StreamlinkBinTests(unittest.TestCase):
    def test_streamlink_bin_default(self) -> None:
        import os
        previous = os.environ.pop("VOICE_REACTION_STREAMLINK_BIN", None)
        try:
            self.assertEqual(audio_capture.streamlink_bin(), "streamlink")
        finally:
            if previous is not None:
                os.environ["VOICE_REACTION_STREAMLINK_BIN"] = previous

    def test_streamlink_bin_env_override(self) -> None:
        import os
        previous = os.environ.get("VOICE_REACTION_STREAMLINK_BIN")
        os.environ["VOICE_REACTION_STREAMLINK_BIN"] = "/custom/streamlink"
        try:
            self.assertEqual(audio_capture.streamlink_bin(), "/custom/streamlink")
        finally:
            if previous is None:
                os.environ.pop("VOICE_REACTION_STREAMLINK_BIN", None)
            else:
                os.environ["VOICE_REACTION_STREAMLINK_BIN"] = previous


if __name__ == "__main__":
    unittest.main()
