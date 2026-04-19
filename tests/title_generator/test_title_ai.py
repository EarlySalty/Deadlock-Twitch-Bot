import pytest
from unittest.mock import AsyncMock, MagicMock, patch

from bot.title_generator.title_ai import (
    RateLimitExceeded,
    TitleRateLimiter,
    build_title_prompt,
    generate_title,
    parse_title_response,
)


def test_rate_limiter_allows_under_limit():
    limiter = TitleRateLimiter(max_requests=5, window_seconds=600)
    for _ in range(5):
        assert limiter.check_and_record("streamer1", source="chat") is True


def test_rate_limiter_blocks_over_limit():
    limiter = TitleRateLimiter(max_requests=5, window_seconds=600)
    for _ in range(5):
        limiter.check_and_record("streamer1", source="chat")
    with pytest.raises(RateLimitExceeded):
        limiter.check_and_record("streamer1", source="chat")


def test_rate_limiter_dashboard_has_higher_limit():
    limiter = TitleRateLimiter(max_requests=5, window_seconds=600, dashboard_multiplier=2)
    for _ in range(10):
        assert limiter.check_and_record("streamer1", source="dashboard") is True
    with pytest.raises(RateLimitExceeded):
        limiter.check_and_record("streamer1", source="dashboard")


def test_rate_limiter_different_streamers_independent():
    limiter = TitleRateLimiter(max_requests=2, window_seconds=600)
    limiter.check_and_record("a", source="chat")
    limiter.check_and_record("a", source="chat")
    assert limiter.check_and_record("b", source="chat") is True


def test_build_title_prompt_contains_keywords():
    prompt = build_title_prompt(
        keywords="ranked solo grind",
        title_history=[{"title": "gaming today", "relative_perf": 0.5, "engagement_rate": 0.05}],
        knowledge_titles=[{"title": "Grind to Eternus", "normalized_score": 2.1}],
        rank_display="Eternus 2",
        emoji_ratio=0.1,
    )
    assert "ranked solo grind" in prompt
    assert "Eternus 2" in prompt
    assert "gaming today" in prompt


def test_build_title_prompt_no_emoji_when_ratio_low():
    prompt = build_title_prompt(
        keywords="ranked",
        title_history=[],
        knowledge_titles=[],
        rank_display=None,
        emoji_ratio=0.1,
    )
    assert "KEINE Emojis" in prompt


def test_parse_title_response_extracts_primary():
    raw = '{"primary_title": "Eternal Grind", "alternatives": ["A", "B"], "title_analysis": []}'
    result = parse_title_response(raw)
    assert result["primary"] == "Eternal Grind"
    assert len(result["alternatives"]) == 2


def test_parse_title_response_handles_malformed():
    result = parse_title_response("not json at all")
    assert result["primary"] == ""
    assert result["alternatives"] == []


@pytest.mark.asyncio
async def test_generate_title_calls_minimax_and_returns_result():
    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = (
        '{"primary_title": "Grind Session", "alternatives": ["Alt A", "Alt B"], "title_analysis": []}'
    )
    mock_client = AsyncMock()
    mock_client.chat.completions.create = AsyncMock(return_value=mock_response)

    with patch("bot.title_generator.title_ai._get_minimax_client", return_value=mock_client):
        result = await generate_title(
            streamer_id="s1",
            keywords="grind",
            title_history=[],
            knowledge_titles=[],
            rank_display="Eternus 1",
            live_state=None,
            source="chat",
        )
    assert result["primary"] == "Grind Session"
