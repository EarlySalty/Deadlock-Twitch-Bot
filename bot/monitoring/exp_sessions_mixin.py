"""
_ExpSessionsMixin – Parallele Session-Logik für das Experimental Analytics System.

Schreibt in exp_sessions, exp_snapshots und exp_game_transitions.
KEIN Refactoring der bestehenden Session-Logik – nur additive Hooks.

Folgende Hooks werden von sessions_mixin.py aufgerufen:
  _exp_on_session_start(login, stream, started_at_iso) -> int | None
  _exp_on_session_sample(login, exp_session_id, stream)
  _exp_on_game_transition(login, exp_session_id, from_game, to_game, viewer_count)
  _exp_on_session_finalize(login, exp_session_id, follower_delta, now_dt)
"""

from __future__ import annotations

from datetime import UTC, datetime

from .. import storage
from ..core.constants import log


class _ExpSessionsMixin:
    """Additive Hooks für das Experimental Analytics System."""

    # ------------------------------------------------------------------ #
    #  In-Memory Cache: login -> exp_session_id                           #
    # ------------------------------------------------------------------ #

    def _get_exp_sessions_cache(self) -> dict[str, int]:
        cache = getattr(self, "_exp_active_sessions", None)
        if cache is None:
            cache = {}
            self._exp_active_sessions = cache
        return cache

    def _get_exp_session_id(self, login: str) -> int | None:
        return self._get_exp_sessions_cache().get(login.lower())

    def _set_exp_session_id(self, login: str, exp_id: int) -> None:
        self._get_exp_sessions_cache()[login.lower()] = exp_id

    def _clear_exp_session_id(self, login: str) -> None:
        self._get_exp_sessions_cache().pop(login.lower(), None)

    # ------------------------------------------------------------------ #
    #  Hook: Session startet                                               #
    # ------------------------------------------------------------------ #

    def _exp_on_session_start(
        self,
        *,
        login: str,
        stream: dict,
        started_at_iso: str | None,
    ) -> int | None:
        """Legt einen neuen exp_sessions-Eintrag an und gibt die neue ID zurück."""
        login_lower = login.lower()
        stream_id = str(stream.get("id") or "").strip() or None
        game_name = (stream.get("game_name") or "").strip() or None
        stream_title = (stream.get("title") or "").strip() or None
        viewer_count = int(stream.get("viewer_count") or 0)
        start_ts = started_at_iso or datetime.now(UTC).isoformat(timespec="seconds")

        # Idempotenz: Falls bereits eine offene exp_session für diesen stream_id
        # existiert, diese zurückgeben ohne eine neue anzulegen.
        if stream_id:
            try:
                with storage.readonly_connection() as c:
                    row = c.execute(
                        "SELECT id FROM exp_sessions WHERE stream_id = %s AND ended_at IS NULL LIMIT 1",
                        (stream_id,),
                    ).fetchone()
                if row:
                    exp_id = int(row[0])
                    self._set_exp_session_id(login_lower, exp_id)
                    return exp_id
            except Exception:
                log.debug("exp: Konnte offene Session nicht prüfen für %s", login, exc_info=True)

        try:
            with storage.transaction() as c:
                row = c.execute(
                    """
                    INSERT INTO exp_sessions (
                        streamer, stream_id, started_at, game_name, stream_title,
                        peak_viewers, avg_viewers, samples
                    ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s)
                    RETURNING id
                    """,
                    (
                        login_lower, stream_id, start_ts, game_name, stream_title,
                        viewer_count, float(viewer_count), 0,
                    ),
                ).fetchone()
            exp_id = int(row[0])
            self._set_exp_session_id(login_lower, exp_id)
            return exp_id
        except Exception:
            log.debug("exp: Konnte exp_session nicht anlegen für %s", login, exc_info=True)
            return None

    # ------------------------------------------------------------------ #
    #  Hook: Sample aufzeichnen                                            #
    # ------------------------------------------------------------------ #

    def _exp_on_session_sample(
        self,
        *,
        login: str,
        exp_session_id: int,
        stream: dict,
    ) -> None:
        """Schreibt einen Snapshot in exp_snapshots und aktualisiert Aggregat-Felder."""
        now_dt = datetime.now(UTC)
        viewer_count = int(stream.get("viewer_count") or 0)
        login_lower = login.lower()

        try:
            with storage.readonly_connection() as c:
                session_row = c.execute(
                    "SELECT started_at, samples, avg_viewers, peak_viewers "
                    "FROM exp_sessions WHERE id = %s",
                    (exp_session_id,),
                ).fetchone()
                if not session_row:
                    return

                started_at_raw = session_row[0]
                try:
                    start_dt = datetime.fromisoformat(str(started_at_raw))
                    if start_dt.tzinfo is None:
                        start_dt = start_dt.replace(tzinfo=UTC)
                except Exception:
                    start_dt = now_dt
                minutes_from_start = max(0.0, (now_dt - start_dt).total_seconds() / 60.0)

                c.execute(
                    """
                    INSERT INTO exp_snapshots (exp_session_id, ts_utc, viewer_count, minutes_from_start)
                    VALUES (%s, %s, %s, %s)
                    ON CONFLICT DO NOTHING
                    """,
                    (
                        exp_session_id,
                        now_dt.isoformat(timespec="seconds"),
                        viewer_count,
                        round(minutes_from_start, 2),
                    ),
                )

                old_samples = int(session_row[1] or 0)
                old_avg = float(session_row[2] or 0.0)
                old_peak = int(session_row[3] or 0)
                new_samples = old_samples + 1
                new_avg = ((old_avg * old_samples) + viewer_count) / max(1, new_samples)
                new_peak = max(old_peak, viewer_count)

                c.execute(
                    """
                    UPDATE exp_sessions
                       SET samples = %s, avg_viewers = %s, peak_viewers = %s
                     WHERE id = %s
                    """,
                    (new_samples, new_avg, new_peak, exp_session_id),
                )
        except Exception:
            log.debug("exp: Konnte Sample nicht schreiben für %s", login_lower, exc_info=True)

    # ------------------------------------------------------------------ #
    #  Hook: Spielwechsel                                                  #
    # ------------------------------------------------------------------ #

    def _exp_on_game_transition(
        self,
        *,
        login: str,
        exp_session_id: int,
        from_game: str,
        to_game: str,
        viewer_count: int,
    ) -> None:
        """Schreibt einen Spielwechsel in exp_game_transitions."""
        login_lower = login.lower()
        now_iso = datetime.now(UTC).isoformat(timespec="seconds")
        try:
            with storage.transaction() as c:
                c.execute(
                    """
                    INSERT INTO exp_game_transitions
                        (exp_session_id, streamer, ts_utc, from_game, to_game, viewer_count)
                    VALUES (%s, %s, %s, %s, %s, %s)
                    """,
                    (
                        exp_session_id,
                        login_lower,
                        now_iso,
                        from_game or None,
                        to_game or None,
                        viewer_count,
                    ),
                )
        except Exception:
            log.debug(
                "exp: Konnte game_transition nicht schreiben für %s (%s -> %s)",
                login_lower, from_game, to_game,
                exc_info=True,
            )

    # ------------------------------------------------------------------ #
    #  Hook: Session finalisieren                                          #
    # ------------------------------------------------------------------ #

    def _exp_on_session_finalize(
        self,
        *,
        login: str,
        exp_session_id: int,
        follower_delta: int | None,
        now_dt: datetime | None = None,
    ) -> None:
        """Schließt die exp_session ab (ended_at, follower_delta, duration_min)."""
        login_lower = login.lower()
        if now_dt is None:
            now_dt = datetime.now(UTC)
        now_iso = now_dt.isoformat(timespec="seconds")

        try:
            with storage.readonly_connection() as c:
                session_row = c.execute(
                    "SELECT started_at FROM exp_sessions WHERE id = %s",
                    (exp_session_id,),
                ).fetchone()
                if not session_row:
                    return

                started_at_raw = session_row[0]
                try:
                    start_dt = datetime.fromisoformat(str(started_at_raw))
                    if start_dt.tzinfo is None:
                        start_dt = start_dt.replace(tzinfo=UTC)
                    duration_min = max(0.0, (now_dt - start_dt).total_seconds() / 60.0)
                except Exception:
                    duration_min = None

                c.execute(
                    """
                    UPDATE exp_sessions
                       SET ended_at = %s, follower_delta = %s, duration_min = %s
                     WHERE id = %s
                    """,
                    (now_iso, follower_delta, duration_min, exp_session_id),
                )
        except Exception:
            log.debug("exp: Konnte Session nicht finalisieren für %s", login_lower, exc_info=True)
        finally:
            self._clear_exp_session_id(login_lower)
