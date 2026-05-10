# tests/title_generator/test_title_db.py
from unittest.mock import MagicMock, patch

from bot.title_generator.title_db import (
    get_streamer_title_history,
    get_streamer_avg_viewers,
    get_streamer_session_count,
    get_top_knowledge_titles,
    upsert_knowledge_entry,
    insert_insight,
    get_latest_insights,
)


def _make_conn(fetchall_result=None, fetchone_result=None):
    """Build a mock conn with execute().fetchall() and execute().fetchone()."""
    cursor = MagicMock()
    cursor.fetchall.return_value = fetchall_result or []
    cursor.fetchone.return_value = fetchone_result
    conn = MagicMock()
    conn.execute.return_value = cursor
    conn.__enter__ = MagicMock(return_value=conn)
    conn.__exit__ = MagicMock(return_value=False)
    return conn


def _make_cursor(*, fetchall_result=None, fetchone_result=None):
    cursor = MagicMock()
    cursor.fetchall.return_value = fetchall_result or []
    cursor.fetchone.return_value = fetchone_result
    return cursor


def test_get_streamer_title_history_returns_list(sample_title_history):
    rows = [
        ("Ranked Grind", 180, 250, 1200, None),
        ("gaming today", 80, 120, 1180, None),
    ]
    conn = _make_conn()
    conn.execute.side_effect = [
        _make_cursor(fetchone_result=("streamer_login",)),
        _make_cursor(fetchall_result=rows),
    ]
    with patch("bot.title_generator.title_db.storage.readonly_connection", return_value=conn):
        result = get_streamer_title_history("streamer123", limit=30)
    assert isinstance(result, list)
    assert len(result) == 2
    assert result[0]["title"] == "Ranked Grind"
    assert result[0]["avg_viewers"] == 180


def test_get_streamer_avg_viewers_returns_float():
    conn = _make_conn()
    conn.execute.side_effect = [
        _make_cursor(fetchone_result=("streamer_login",)),
        _make_cursor(fetchone_result=(160.0,)),
    ]
    with patch("bot.title_generator.title_db.storage.readonly_connection", return_value=conn):
        avg = get_streamer_avg_viewers("streamer123")
    assert isinstance(avg, float)
    assert avg == 160.0


def test_get_streamer_avg_viewers_returns_zero_when_no_data():
    conn = _make_conn()
    conn.execute.side_effect = [
        _make_cursor(fetchone_result=("streamer_login",)),
        _make_cursor(fetchone_result=(None,)),
    ]
    with patch("bot.title_generator.title_db.storage.readonly_connection", return_value=conn):
        avg = get_streamer_avg_viewers("new_streamer")
    assert avg == 0.0


def test_get_streamer_session_count():
    conn = _make_conn()
    conn.execute.side_effect = [
        _make_cursor(fetchone_result=("streamer_login",)),
        _make_cursor(fetchone_result=(42,)),
    ]
    with patch("bot.title_generator.title_db.storage.readonly_connection", return_value=conn):
        count = get_streamer_session_count("streamer123")
    assert count == 42


def test_get_top_knowledge_titles_returns_list():
    rows = [
        ("Grind to Eternus", 2.1, ["ranked", "grind"], 3),
    ]
    conn = _make_conn(fetchall_result=rows)
    with patch("bot.title_generator.title_db.storage.readonly_connection", return_value=conn):
        result = get_top_knowledge_titles(limit=30)
    assert len(result) == 1
    assert result[0]["title"] == "Grind to Eternus"
    assert result[0]["normalized_score"] == 2.1


def test_upsert_knowledge_entry_calls_execute():
    conn = _make_conn()
    with patch("bot.title_generator.title_db.storage.transaction", return_value=conn):
        upsert_knowledge_entry(
            title="Epic Grind",
            keywords=["epic", "grind"],
            relative_perf=1.5,
            engagement_rate=0.05,
            history_weight=0.8,
            normalized_score=1.6,
            streamer_size="medium",
            source_streamer="test123...",
        )
    conn.execute.assert_called_once()


def test_insert_insight_calls_execute():
    from datetime import datetime, timezone
    now = datetime.now(timezone.utc)
    conn = _make_conn()
    with patch("bot.title_generator.title_db.storage.transaction", return_value=conn):
        insert_insight(
            streamer_id="s1",
            period_start=now,
            period_end=now,
            strengths="gut",
            weaknesses="schlecht",
            patterns="muster",
            recommendations="empfehlungen",
            raw_response={},
        )
    conn.execute.assert_called_once()


def test_get_latest_insights_returns_none_when_no_rows():
    conn = _make_conn(fetchone_result=None)
    with patch("bot.title_generator.title_db.storage.readonly_connection", return_value=conn):
        result = get_latest_insights("no_streamer")
    assert result is None


def test_get_latest_insights_returns_dict():
    from datetime import datetime, timezone
    now = datetime.now(timezone.utc)
    conn = _make_conn(fetchone_result=("gut", "schlecht", "muster", "empfehlung", now))
    with patch("bot.title_generator.title_db.storage.readonly_connection", return_value=conn):
        result = get_latest_insights("streamer123")
    assert result is not None
    assert result["strengths"] == "gut"
    assert result["weaknesses"] == "schlecht"
