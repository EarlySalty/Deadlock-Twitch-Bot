import pytest
from unittest.mock import AsyncMock, patch

from bot.title_generator.steam_lookup import (
    get_live_state_for_discord_user,
    get_rank_for_discord_user,
)


@pytest.mark.asyncio
async def test_get_rank_returns_dict_with_rank_fields():
    mock_row = {"deadlock_rank": 9, "deadlock_subrank": 2}
    with patch("bot.title_generator.steam_lookup._fetch_rank_row", AsyncMock(return_value=mock_row)):
        result = await get_rank_for_discord_user(123456789)
    assert result["rank_name"] == "Ascendant"
    assert result["subrank"] == 2


@pytest.mark.asyncio
async def test_get_rank_returns_none_when_not_linked():
    with patch("bot.title_generator.steam_lookup._fetch_rank_row", AsyncMock(return_value=None)):
        result = await get_rank_for_discord_user(999999)
    assert result is None


@pytest.mark.asyncio
async def test_get_live_state_returns_hero_when_in_match():
    mock_row = {
        "in_deadlock_now": True,
        "in_match_now_strict": True,
        "deadlock_hero": "Dynamo",
        "deadlock_party_hint": "duo",
        "deadlock_stage": "mid_game",
    }
    with patch("bot.title_generator.steam_lookup._fetch_live_row", AsyncMock(return_value=mock_row)):
        result = await get_live_state_for_discord_user(123456789)
    assert result["hero"] == "Dynamo"
    assert result["party_hint"] == "duo"


@pytest.mark.asyncio
async def test_get_live_state_returns_none_when_not_in_game():
    mock_row = {"in_deadlock_now": False, "in_match_now_strict": False}
    with patch("bot.title_generator.steam_lookup._fetch_live_row", AsyncMock(return_value=mock_row)):
        result = await get_live_state_for_discord_user(123456789)
    assert result is None
