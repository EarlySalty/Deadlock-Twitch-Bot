import pytest

from bot.chat import spam_ai_review


@pytest.fixture(autouse=True)
def clear_review_cooldown():
    spam_ai_review._REVIEW_COOLDOWN.clear()


@pytest.mark.asyncio
async def test_score_positiv_mit_viewer_muster_ruft_minimax_und_speichert_spam(monkeypatch):
    content = "@cheazycrust Targeted viewers PeakPy. c0m SSSsss remove space"
    minimax_calls = []
    saved_spam = []

    async def fake_call_minimax(seen_content):
        minimax_calls.append(seen_content)
        return {
            "is_spam": True,
            "pattern": "PeakPy c0m",
            "pattern_type": "fragment",
            "reason": "obfuskierter viewer-service",
        }

    async def fake_save_pattern(**kwargs):
        saved_spam.append(kwargs)

    async def fail_safe_pattern(**kwargs):
        raise AssertionError(f"Safe-Liste darf bei Spam nicht beschrieben werden: {kwargs}")

    monkeypatch.setattr(spam_ai_review, "_call_minimax", fake_call_minimax)
    monkeypatch.setattr(spam_ai_review, "_save_pattern", fake_save_pattern)
    monkeypatch.setattr(spam_ai_review, "_save_safe_pattern", fail_safe_pattern)

    await spam_ai_review.run_spam_ai_review(
        content=content,
        channel="cheazycrust",
        chatter_login="yameskudas",
        spam_score=1,
        spam_reasons=["Muster: viewer + name"],
    )

    assert minimax_calls == [content]
    assert saved_spam == [
        {
            "pattern": "peakpy c0m",
            "pattern_type": "fragment",
            "source_message": content,
            "source_channel": "cheazycrust",
            "reasoning": "obfuskierter viewer-service",
        }
    ]


@pytest.mark.asyncio
async def test_score_positiv_ohne_reasons_ruft_minimax_und_speichert_safe(monkeypatch):
    content = "best viewers in chat today"
    minimax_calls = []
    saved_safe = []

    async def fake_call_minimax(seen_content):
        minimax_calls.append(seen_content)
        return {
            "is_spam": False,
            "pattern": "best viewers",
            "pattern_type": "fragment",
            "reason": "normales kompliment",
        }

    async def fail_spam_pattern(**kwargs):
        raise AssertionError(f"Spam-Liste darf bei False-Positive nicht beschrieben werden: {kwargs}")

    async def fake_save_safe_pattern(**kwargs):
        saved_safe.append(kwargs)

    monkeypatch.setattr(spam_ai_review, "_call_minimax", fake_call_minimax)
    monkeypatch.setattr(spam_ai_review, "_save_pattern", fail_spam_pattern)
    monkeypatch.setattr(spam_ai_review, "_save_safe_pattern", fake_save_safe_pattern)

    await spam_ai_review.run_spam_ai_review(
        content=content,
        channel="cheazycrust",
        chatter_login="normalviewer",
        spam_score=1,
        spam_reasons=[],
    )

    assert minimax_calls == [content]
    assert saved_safe == [
        {
            "pattern": "best viewers",
            "source_message": content,
            "source_channel": "cheazycrust",
            "reasoning": "normales kompliment",
        }
    ]


@pytest.mark.asyncio
async def test_score_null_ruft_minimax_nicht(monkeypatch):
    async def fail_call_minimax(content):
        raise AssertionError(f"MiniMax darf bei Score 0 nicht laufen: {content}")

    monkeypatch.setattr(spam_ai_review, "_call_minimax", fail_call_minimax)

    await spam_ai_review.run_spam_ai_review(
        content="harmlos",
        channel="cheazycrust",
        chatter_login="normalviewer",
        spam_score=0,
        spam_reasons=["Muster: viewer + name"],
    )
