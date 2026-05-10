# tests/title_generator/conftest.py
import pytest


@pytest.fixture
def sample_title_history():
    return [
        {"title": "Ranked Grind", "avg_viewers": 180, "peak_viewers": 250, "followers_start": 1200, "started_at": None},
        {"title": "gaming today", "avg_viewers": 80, "peak_viewers": 120, "followers_start": 1180, "started_at": None},
        {"title": "Eternus or bust", "avg_viewers": 220, "peak_viewers": 310, "followers_start": 1210, "started_at": None},
    ]
