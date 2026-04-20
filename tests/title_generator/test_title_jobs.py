from unittest.mock import MagicMock, patch

from bot.title_generator.insight_job import _fetch_active_partner_ids
from bot.title_generator.knowledge_job import _fetch_recent_sessions


def _make_conn(fetchall_result=None):
    cursor = MagicMock()
    cursor.fetchall.return_value = fetchall_result or []
    conn = MagicMock()
    conn.execute.return_value = cursor
    conn.__enter__ = MagicMock(return_value=conn)
    conn.__exit__ = MagicMock(return_value=False)
    return conn


def test_fetch_recent_sessions_uses_live_session_schema() -> None:
    rows = [
        ("earlysalty", "Am Juicen Asc 2", 120.0, 180.0, 2500, None),
    ]
    conn = _make_conn(fetchall_result=rows)

    with patch("bot.title_generator.knowledge_job.storage.readonly_connection", return_value=conn):
        result = _fetch_recent_sessions(7)

    assert result == [
        {
            "streamer_login": "earlysalty",
            "title": "Am Juicen Asc 2",
            "avg_viewers": 120.0,
            "peak_viewers": 180.0,
            "followers_start": 2500,
            "started_at": None,
        }
    ]
    executed_sql = conn.execute.call_args.args[0]
    assert "streamer_login" in executed_sql
    assert "stream_title" in executed_sql
    assert "twitch_user_id" not in executed_sql


def test_fetch_active_partner_ids_uses_partner_state_view() -> None:
    conn = _make_conn(fetchall_result=[("1186925760",), ("993954638",)])

    with patch("bot.title_generator.insight_job.storage.readonly_connection", return_value=conn):
        result = _fetch_active_partner_ids()

    assert result == ["1186925760", "993954638"]
    executed_sql = conn.execute.call_args.args[0]
    assert "twitch_streamers_partner_state" in executed_sql
    assert "is_partner_active = 1" in executed_sql
