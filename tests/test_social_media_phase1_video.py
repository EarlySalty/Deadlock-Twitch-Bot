from __future__ import annotations

import asyncio
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from bot.social_media.layout import DEFAULT_STREAMER_LAYOUT
from bot.social_media.uploaders.video_processor import _build_compose_filter
from bot.social_media.uploaders.video_processor import compose_vertical
from bot.social_media.uploaders.video_processor import VideoProcessor


class SocialMediaPhase1VideoTests(unittest.TestCase):
    def test_build_compose_filter_uses_layout_for_pip_and_stacked(self) -> None:
        pip_filter = _build_compose_filter(DEFAULT_STREAMER_LAYOUT, "pip", True)
        stacked_filter = _build_compose_filter(DEFAULT_STREAMER_LAYOUT, "stacked", True)
        no_cam_filter = _build_compose_filter(DEFAULT_STREAMER_LAYOUT, "pip", False)

        self.assertIn("crop=1080:1080:0:0", pip_filter)
        self.assertIn("overlay=W-w-48:48", pip_filter)
        self.assertIn("vstack=inputs=2", stacked_filter)
        self.assertIn("crop=1080:540", stacked_filter)
        self.assertNotIn("overlay=", no_cam_filter)
        self.assertTrue(no_cam_filter.endswith("[vout]"))

    def test_compose_vertical_invokes_ffmpeg_with_expected_args(self) -> None:
        with tempfile.TemporaryDirectory(prefix="social-media-video-") as temp_dir:
            input_path = Path(temp_dir) / "input.mp4"
            output_path = Path(temp_dir) / "output.mp4"
            input_path.write_bytes(b"fake")

            def _fake_run(cmd, capture_output, text, check):
                del capture_output, text, check
                output_path.write_bytes(b"rendered")

                class _Result:
                    returncode = 0
                    stderr = ""

                self.assertIn("-filter_complex", cmd)
                self.assertIn("-af", cmd)
                self.assertIn("loudnorm", cmd)
                self.assertIn(str(input_path), cmd)
                self.assertIn(str(output_path), cmd)
                filter_graph = cmd[cmd.index("-filter_complex") + 1]
                self.assertIn("overlay=W-w-48:48", filter_graph)
                return _Result()

            with patch("bot.social_media.uploaders.video_processor.subprocess.run", side_effect=_fake_run):
                compose_vertical(
                    str(input_path),
                    str(output_path),
                    DEFAULT_STREAMER_LAYOUT,
                    "pip",
                    True,
                )

            self.assertTrue(output_path.exists())

    @unittest.skipUnless(
        shutil.which("ffmpeg") and shutil.which("ffprobe"),
        "ffmpeg/ffprobe not available",
    )
    def test_compose_vertical_real_run_preserves_resolution_duration_and_audio(self) -> None:
        with tempfile.TemporaryDirectory(prefix="social-media-video-real-") as temp_dir:
            input_path = Path(temp_dir) / "input.mp4"
            output_path = Path(temp_dir) / "output.mp4"
            subprocess.run(
                [
                    "ffmpeg",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc=size=1920x1080:rate=30",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=1000",
                    "-shortest",
                    "-t",
                    "2",
                    "-c:v",
                    "libx264",
                    "-c:a",
                    "aac",
                    "-pix_fmt",
                    "yuv420p",
                    "-y",
                    str(input_path),
                ],
                capture_output=True,
                text=True,
                check=True,
            )

            compose_vertical(
                str(input_path),
                str(output_path),
                DEFAULT_STREAMER_LAYOUT,
                "pip",
                True,
            )

            info = asyncio.run(VideoProcessor().get_video_info(str(output_path)))
            audio_probe = subprocess.run(
                [
                    "ffprobe",
                    "-v",
                    "error",
                    "-select_streams",
                    "a",
                    "-show_entries",
                    "stream=index",
                    "-of",
                    "csv=p=0",
                    str(output_path),
                ],
                capture_output=True,
                text=True,
                check=True,
            )

            self.assertEqual(info["width"], 1080)
            self.assertEqual(info["height"], 1920)
            self.assertGreater(info["duration"], 1.5)
            self.assertLess(info["duration"], 2.5)
            self.assertTrue(audio_probe.stdout.strip())


if __name__ == "__main__":
    unittest.main()
