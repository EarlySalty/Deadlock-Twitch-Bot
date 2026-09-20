"""Isolierte STT-Prüfung: keine Modell-Downloads und keine echten Streams."""
import dataclasses
import importlib.util
import io
from pathlib import Path
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
import wave

from fastapi.testclient import TestClient

SOURCE = Path(__file__).resolve().parents[1] / "stt_server.py"
spec = importlib.util.spec_from_file_location("stt_server_under_test", SOURCE)
server = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = server
spec.loader.exec_module(server)


def settings(**changes):
    result = server.Settings(
        host="127.0.0.1", port=19091, model="local/test-model", threads=3,
        language="", no_speech_max=0.4, avg_logprob_min=-0.8,
        max_upload_bytes=1048576, cache_directory="/tmp/tb-stt-test-model-cache",
        config_fingerprint="a" * 64,
    )
    return dataclasses.replace(result, **changes)


def wav_bytes():
    output = io.BytesIO()
    with wave.open(output, "wb") as audio:
        audio.setnchannels(1)
        audio.setsampwidth(2)
        audio.setframerate(16000)
        audio.writeframes(b"\0\0" * 1600)
    return output.getvalue()


class FakeModel:
    def __init__(self):
        self.calls = []

    def transcribe(self, pcm, **arguments):
        self.calls.append(arguments)
        return [
            SimpleNamespace(text="synthetische Probe", start=0.0, end=0.1,
                            no_speech_prob=0.1, avg_logprob=-0.4),
            SimpleNamespace(text="verworfen", start=0.1, end=0.2,
                            no_speech_prob=0.5, avg_logprob=-0.4),
        ], SimpleNamespace(language="en")


class ConfigurationTests(unittest.TestCase):
    def test_resolved_local_model_paths_keep_spaces_unicode_and_long_parents(self):
        with tempfile.TemporaryDirectory(prefix="stt geprüft ") as directory:
            model = Path(directory).joinpath(*(["modelle" * 12] * 7))
            model.mkdir(parents=True)
            self.assertGreater(len(str(model)), 512)
            settings(model=str(model)).validate()
            with self.assertRaises(ValueError):
                settings(model=str(model / "missing")).validate()

    def test_invalid_values_fail_before_model_factory(self):
        for changes in (
            {"host": "0.0.0.0"}, {"port": 0}, {"threads": 65},
            {"no_speech_max": float("nan")}, {"avg_logprob_min": float("inf")},
            {"model": "https://private.invalid/?token=REDACTION_SENTINEL"},
            {"cache_directory": "relative"}, {"config_fingerprint": "invalid"},
        ):
            with self.subTest(changes=tuple(changes)):
                called = []
                with self.assertRaises(ValueError) as caught:
                    server.create_app(settings(**changes), model_factory=lambda *a, **k: called.append(True))
                self.assertEqual(called, [])
                self.assertNotIn("REDACTION_SENTINEL", str(caught.exception))

    def test_parsed_values_are_the_explicit_arguments(self):
        parsed = server.parse_arguments([
            "--host", "127.0.0.1", "--port", "19092", "--model", "local/model",
            "--threads", "4", "--language", "en", "--no-speech-max", "0.3",
            "--avg-logprob-min", "-0.7", "--max-upload-bytes", "2048",
            "--cache-directory", "/tmp/models", "--config-fingerprint", "b" * 64,
        ])
        self.assertEqual(parsed.threads, 4)
        self.assertEqual(parsed.port, 19092)
        self.assertEqual(parsed.language, "en")
        self.assertEqual(parsed.max_upload_bytes, 2048)

    def test_no_periodic_inference_without_audio_and_settings_reach_model(self):
        model = FakeModel()
        creation = []

        def factory(*args, **kwargs):
            creation.append((args, kwargs))
            return model

        with TestClient(server.create_app(settings(), model_factory=factory)) as client:
            first = client.get("/health").json()
            time.sleep(0.1)
            second = client.get("/health").json()
            self.assertEqual(model.calls, [])
            self.assertEqual(len(creation), 1)
            self.assertEqual(creation[0][0], ("local/test-model",))
            self.assertEqual(creation[0][1]["cpu_threads"], 3)
            self.assertEqual(creation[0][1]["download_root"], "/tmp/tb-stt-test-model-cache")
            self.assertEqual(first["transcriptions_started"], 0)
            self.assertEqual(second["transcriptions_started"], 0)
            self.assertEqual(second["threads"], 3)
            self.assertEqual(second["config_fingerprint"], "a" * 64)

    def test_audio_uses_thresholds_and_real_model_not_requested_alias(self):
        model = FakeModel()
        with TestClient(server.create_app(settings(), model_factory=lambda *a, **k: model)) as client:
            response = client.post(
                "/v1/audio/transcriptions",
                files={"file": ("probe.wav", wav_bytes(), "audio/wav")},
                data={"model": "requested-alias", "language": "de", "response_format": "verbose_json"},
            )
            self.assertEqual(response.status_code, 200)
            body = response.json()
            self.assertEqual(body["text"], "synthetische Probe")
            self.assertEqual(body["model"], "local/test-model")
            self.assertEqual(len(body["segments"]), 1)
            self.assertIsNone(model.calls[0]["language"])
            self.assertEqual(client.get("/health").json()["transcriptions_completed"], 1)

    def test_forced_language_reaches_inference(self):
        model = FakeModel()
        with TestClient(server.create_app(settings(language="en"), model_factory=lambda *a, **k: model)) as client:
            response = client.post("/v1/audio/transcriptions", files={"file": ("probe.wav", wav_bytes(), "audio/wav")})
            self.assertEqual(response.status_code, 200)
            self.assertEqual(model.calls[0]["language"], "en")

    def test_invalid_empty_and_oversized_audio_does_not_start_inference(self):
        model = FakeModel()
        with TestClient(server.create_app(settings(max_upload_bytes=1024), model_factory=lambda *a, **k: model)) as client:
            for raw in (b"", b"invalid", b"a" * 1025):
                response = client.post("/v1/audio/transcriptions", files={"file": ("probe.wav", raw, "audio/wav")})
                self.assertEqual(response.status_code, 400)
            self.assertEqual(model.calls, [])
            self.assertEqual(client.get("/health").json()["transcriptions_started"], 0)

    def test_source_has_no_environment_configuration_reader(self):
        text = SOURCE.read_text()
        self.assertNotIn("os.environ", text)
        self.assertNotIn("getenv(", text)
        self.assertNotIn("tomllib", text)
        self.assertNotIn("load_dotenv", text)


if __name__ == "__main__":
    unittest.main()
