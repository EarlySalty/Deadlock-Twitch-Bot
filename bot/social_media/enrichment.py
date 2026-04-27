"""Phase-2 Clip-Enrichment-Pipeline.

Status-Maschine pro Clip:

    pending -> transcribing -> correcting -> llm -> done
                       |             |        |
                       v             v        v
                    failed        failed   failed

Spezial-Status:
- `skipped_no_key`: kein lokales LLM erreichbar UND keine Engine fuer Transkription;
  Pipeline wurde in einer Stage uebersprungen, der Clip kann manuell editiert werden.
"""

from __future__ import annotations

import asyncio
import json
import logging
from dataclasses import dataclass, field
from datetime import UTC, datetime
from typing import Any, Iterable

from ..storage import readonly_connection, transaction
from .approval import mark_clip_awaiting_approval
from .llm import (
    LLMDispatcher,
    LLMRequest,
    LLMUnavailable,
    PlatformEnrichment,
    StreamerProfile,
)
from .transcription import (
    TranscriberUnavailable,
    correct_transcript,
    load_all_vocab_safe,
    transcribe_clip,
)

log = logging.getLogger("TwitchStreams.SocialMedia.Enrichment")


STATUS_PENDING = "pending"
STATUS_TRANSCRIBING = "transcribing"
STATUS_CORRECTING = "correcting"
STATUS_LLM = "llm"
STATUS_DONE = "done"
STATUS_FAILED = "failed"
STATUS_SKIPPED = "skipped_no_key"


@dataclass(frozen=True)
class EnrichmentClipContext:
    clip_db_id: int
    clip_id: str
    streamer_login: str
    title: str | None
    duration_seconds: float | None
    game_name: str | None
    upload_local_path: str | None
    local_file_path: str | None


@dataclass
class EnrichmentRecord:
    clip_db_id: int
    transcript_raw: str | None = None
    transcript_corrected: str | None = None
    transcript_segments: list[dict[str, Any]] = field(default_factory=list)
    transcript_lang: str | None = None
    detected_terms: list[str] = field(default_factory=list)
    title_youtube: str | None = None
    title_tiktok: str | None = None
    title_instagram: str | None = None
    description_youtube: str | None = None
    description_tiktok: str | None = None
    description_instagram: str | None = None
    hashtags_youtube: list[str] = field(default_factory=list)
    hashtags_tiktok: list[str] = field(default_factory=list)
    hashtags_instagram: list[str] = field(default_factory=list)
    llm_provider: str | None = None
    llm_model: str | None = None
    cost_usd_estimate: float | None = None
    status: str = STATUS_PENDING
    error_message: str | None = None
    started_at: str | None = None
    completed_at: str | None = None
    edited_by: str | None = None
    updated_at: str | None = None


# ---------------------------------------------------------------------------
# Storage helpers
# ---------------------------------------------------------------------------

def _decode_json(raw: Any, default: Any) -> Any:
    if raw is None:
        return default
    if isinstance(raw, (list, dict)):
        return raw
    if isinstance(raw, (bytes, bytearray)):
        try:
            return json.loads(raw.decode("utf-8"))
        except Exception:
            return default
    if isinstance(raw, str):
        if not raw:
            return default
        try:
            return json.loads(raw)
        except Exception:
            return default
    return default


def _row_to_record(row: Any) -> EnrichmentRecord:
    def _val(name: str) -> Any:
        if hasattr(row, "keys"):
            return row[name]
        index = _COLUMN_ORDER.index(name)
        return row[index]

    return EnrichmentRecord(
        clip_db_id=int(_val("clip_db_id")),
        transcript_raw=_val("transcript_raw"),
        transcript_corrected=_val("transcript_corrected"),
        transcript_segments=_decode_json(_val("transcript_segments"), []),
        transcript_lang=_val("transcript_lang"),
        detected_terms=_decode_json(_val("detected_terms"), []) or [],
        title_youtube=_val("title_youtube"),
        title_tiktok=_val("title_tiktok"),
        title_instagram=_val("title_instagram"),
        description_youtube=_val("description_youtube"),
        description_tiktok=_val("description_tiktok"),
        description_instagram=_val("description_instagram"),
        hashtags_youtube=_decode_json(_val("hashtags_youtube"), []) or [],
        hashtags_tiktok=_decode_json(_val("hashtags_tiktok"), []) or [],
        hashtags_instagram=_decode_json(_val("hashtags_instagram"), []) or [],
        llm_provider=_val("llm_provider"),
        llm_model=_val("llm_model"),
        cost_usd_estimate=(
            float(_val("cost_usd_estimate"))
            if _val("cost_usd_estimate") is not None
            else None
        ),
        status=str(_val("status") or STATUS_PENDING),
        error_message=_val("error_message"),
        started_at=str(_val("started_at")) if _val("started_at") is not None else None,
        completed_at=str(_val("completed_at")) if _val("completed_at") is not None else None,
        edited_by=_val("edited_by"),
        updated_at=str(_val("updated_at")) if _val("updated_at") is not None else None,
    )


_COLUMN_ORDER: tuple[str, ...] = (
    "clip_db_id",
    "transcript_raw",
    "transcript_corrected",
    "transcript_segments",
    "transcript_lang",
    "detected_terms",
    "title_youtube",
    "title_tiktok",
    "title_instagram",
    "description_youtube",
    "description_tiktok",
    "description_instagram",
    "hashtags_youtube",
    "hashtags_tiktok",
    "hashtags_instagram",
    "llm_provider",
    "llm_model",
    "cost_usd_estimate",
    "status",
    "error_message",
    "started_at",
    "completed_at",
    "edited_by",
    "updated_at",
)

_SELECT_SQL = (
    "SELECT clip_db_id, transcript_raw, transcript_corrected, transcript_segments, "
    "transcript_lang, detected_terms, title_youtube, title_tiktok, title_instagram, "
    "description_youtube, description_tiktok, description_instagram, "
    "hashtags_youtube, hashtags_tiktok, hashtags_instagram, llm_provider, llm_model, "
    "cost_usd_estimate, status, error_message, started_at, completed_at, edited_by, updated_at "
    "FROM social_media_clip_enrichment WHERE clip_db_id = %s"
)


def get_enrichment(clip_db_id: int) -> EnrichmentRecord | None:
    with readonly_connection() as conn:
        row = conn.execute(_SELECT_SQL, (clip_db_id,)).fetchone()
    return _row_to_record(row) if row else None


def ensure_enrichment_row(clip_db_id: int) -> EnrichmentRecord:
    existing = get_enrichment(clip_db_id)
    if existing is not None:
        return existing
    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO social_media_clip_enrichment (clip_db_id, status, updated_at)
            VALUES (%s, %s, CURRENT_TIMESTAMP)
            ON CONFLICT (clip_db_id) DO NOTHING
            """,
            (clip_db_id, STATUS_PENDING),
        )
    return get_enrichment(clip_db_id) or EnrichmentRecord(clip_db_id=clip_db_id)


def update_enrichment_status(
    clip_db_id: int,
    *,
    status: str,
    error_message: str | None = ...,  # type: ignore[assignment]
    started_at: str | None = ...,  # type: ignore[assignment]
    completed_at: str | None = ...,  # type: ignore[assignment]
) -> None:
    """Update the status (and optional timestamps/error) for a clip enrichment row.

    Sentinel `...` means "leave column unchanged"; pass `None` to clear it.
    """
    ensure_enrichment_row(clip_db_id)
    sets = ["status = %s", "updated_at = CURRENT_TIMESTAMP"]
    params: list[Any] = [status]
    if error_message is not ...:
        sets.append("error_message = %s")
        params.append(error_message)
    if started_at is not ...:
        sets.append("started_at = %s")
        params.append(started_at)
    if completed_at is not ...:
        sets.append("completed_at = %s")
        params.append(completed_at)
    set_sql = ", ".join(sets)
    params.append(clip_db_id)
    with transaction() as conn:
        conn.execute(
            f"UPDATE social_media_clip_enrichment SET {set_sql} WHERE clip_db_id = %s",
            tuple(params),
        )


def save_transcript(
    clip_db_id: int,
    *,
    transcript_raw: str | None,
    transcript_segments: Iterable[dict[str, Any]] | None,
    transcript_lang: str | None,
) -> None:
    payload = json.dumps(list(transcript_segments)) if transcript_segments else None
    with transaction() as conn:
        conn.execute(
            """
            UPDATE social_media_clip_enrichment
               SET transcript_raw = %s,
                   transcript_segments = %s,
                   transcript_lang = %s,
                   updated_at = CURRENT_TIMESTAMP
             WHERE clip_db_id = %s
            """,
            (transcript_raw, payload, transcript_lang, clip_db_id),
        )


def save_corrected(
    clip_db_id: int,
    *,
    transcript_corrected: str | None,
    detected_terms: Iterable[str] | None,
) -> None:
    payload = json.dumps(list(detected_terms or []))
    with transaction() as conn:
        conn.execute(
            """
            UPDATE social_media_clip_enrichment
               SET transcript_corrected = %s,
                   detected_terms = %s,
                   updated_at = CURRENT_TIMESTAMP
             WHERE clip_db_id = %s
            """,
            (transcript_corrected, payload, clip_db_id),
        )


def save_llm_output(
    clip_db_id: int,
    *,
    youtube: PlatformEnrichment,
    tiktok: PlatformEnrichment,
    instagram: PlatformEnrichment,
    provider: str,
    model: str | None,
    cost_usd_estimate: float | None,
) -> None:
    with transaction() as conn:
        conn.execute(
            """
            UPDATE social_media_clip_enrichment
               SET title_youtube = %s,
                   title_tiktok = %s,
                   title_instagram = %s,
                   description_youtube = %s,
                   description_tiktok = %s,
                   description_instagram = %s,
                   hashtags_youtube = %s,
                   hashtags_tiktok = %s,
                   hashtags_instagram = %s,
                   llm_provider = %s,
                   llm_model = %s,
                   cost_usd_estimate = %s,
                   updated_at = CURRENT_TIMESTAMP
             WHERE clip_db_id = %s
            """,
            (
                youtube.title,
                tiktok.title,
                instagram.title,
                youtube.description,
                tiktok.description,
                instagram.description,
                json.dumps(list(youtube.hashtags)),
                json.dumps(list(tiktok.hashtags)),
                json.dumps(list(instagram.hashtags)),
                provider,
                model,
                cost_usd_estimate,
                clip_db_id,
            ),
        )


def update_manual_edit(
    clip_db_id: int,
    *,
    edited_by: str | None,
    title_youtube: str | None = ...,  # type: ignore[assignment]
    title_tiktok: str | None = ...,  # type: ignore[assignment]
    title_instagram: str | None = ...,  # type: ignore[assignment]
    description_youtube: str | None = ...,  # type: ignore[assignment]
    description_tiktok: str | None = ...,  # type: ignore[assignment]
    description_instagram: str | None = ...,  # type: ignore[assignment]
    hashtags_youtube: list[str] | None = None,
    hashtags_tiktok: list[str] | None = None,
    hashtags_instagram: list[str] | None = None,
) -> None:
    """Persist manual edits from the admin UI."""
    sets: list[str] = ["updated_at = CURRENT_TIMESTAMP", "edited_by = %s"]
    params: list[Any] = [edited_by]

    def _maybe(field_name: str, value: Any) -> None:
        if value is ...:
            return
        sets.append(f"{field_name} = %s")
        params.append(value)

    _maybe("title_youtube", title_youtube)
    _maybe("title_tiktok", title_tiktok)
    _maybe("title_instagram", title_instagram)
    _maybe("description_youtube", description_youtube)
    _maybe("description_tiktok", description_tiktok)
    _maybe("description_instagram", description_instagram)

    if hashtags_youtube is not None:
        sets.append("hashtags_youtube = %s")
        params.append(json.dumps(list(hashtags_youtube)))
    if hashtags_tiktok is not None:
        sets.append("hashtags_tiktok = %s")
        params.append(json.dumps(list(hashtags_tiktok)))
    if hashtags_instagram is not None:
        sets.append("hashtags_instagram = %s")
        params.append(json.dumps(list(hashtags_instagram)))

    set_sql = ", ".join(sets)
    params.append(clip_db_id)
    with transaction() as conn:
        conn.execute(
            f"""
            INSERT INTO social_media_clip_enrichment (clip_db_id, status, updated_at, edited_by)
            VALUES (%s, %s, CURRENT_TIMESTAMP, %s)
            ON CONFLICT (clip_db_id) DO NOTHING
            """,
            (clip_db_id, STATUS_PENDING, edited_by),
        )
        conn.execute(
            f"UPDATE social_media_clip_enrichment SET {set_sql} WHERE clip_db_id = %s",
            tuple(params),
        )


# ---------------------------------------------------------------------------
# Pipeline
# ---------------------------------------------------------------------------

def _utcnow_iso() -> str:
    return datetime.now(UTC).isoformat()


def _load_clip_context(clip_db_id: int) -> EnrichmentClipContext | None:
    with readonly_connection() as conn:
        row = conn.execute(
            """
            SELECT id, clip_id, streamer_login, clip_title, duration_seconds, game_name,
                   upload_local_path, local_file_path
              FROM twitch_clips_social_media
             WHERE id = %s
             LIMIT 1
            """,
            (clip_db_id,),
        ).fetchone()
    if not row:
        return None
    if hasattr(row, "keys"):
        return EnrichmentClipContext(
            clip_db_id=int(row["id"]),
            clip_id=str(row["clip_id"] or ""),
            streamer_login=str(row["streamer_login"] or ""),
            title=row["clip_title"],
            duration_seconds=(
                float(row["duration_seconds"])
                if row["duration_seconds"] is not None
                else None
            ),
            game_name=row["game_name"],
            upload_local_path=row["upload_local_path"],
            local_file_path=row["local_file_path"],
        )
    return EnrichmentClipContext(
        clip_db_id=int(row[0]),
        clip_id=str(row[1] or ""),
        streamer_login=str(row[2] or ""),
        title=row[3],
        duration_seconds=float(row[4]) if row[4] is not None else None,
        game_name=row[5],
        upload_local_path=row[6],
        local_file_path=row[7],
    )


@dataclass(frozen=True)
class EnrichmentOutcome:
    clip_db_id: int
    status: str
    error_message: str | None = None
    provider: str | None = None
    model: str | None = None


class ClipEnrichmentPipeline:
    """Orchestrates Whisper -> Vocab-Korrektur -> LLM."""

    def __init__(
        self,
        *,
        transcriber: Any | None = None,
        dispatcher: LLMDispatcher | None = None,
        vocab_loader: Any | None = None,
    ) -> None:
        self._transcriber = transcriber
        self._dispatcher = dispatcher
        self._vocab_loader = vocab_loader or load_all_vocab_safe

    async def run(self, clip_db_id: int, *, force: bool = False) -> EnrichmentOutcome:
        ctx = _load_clip_context(clip_db_id)
        if not ctx:
            raise ValueError(f"clip_db_id {clip_db_id} not found")

        existing = ensure_enrichment_row(clip_db_id)
        if existing.status == STATUS_DONE and not force:
            return EnrichmentOutcome(
                clip_db_id=clip_db_id,
                status=STATUS_DONE,
                provider=existing.llm_provider,
                model=existing.llm_model,
            )

        # ---- Transcribe ----
        update_enrichment_status(
            clip_db_id,
            status=STATUS_TRANSCRIBING,
            error_message=None,
            started_at=_utcnow_iso(),
            completed_at=None,
        )

        transcript_text = ""
        transcript_segments: list[dict[str, Any]] = []
        transcript_lang: str | None = None
        skipped_transcription = False

        video_path = ctx.upload_local_path or ctx.local_file_path
        if not video_path:
            log.info("Clip %s has no local video path; transcription skipped", clip_db_id)
            skipped_transcription = True
        else:
            try:
                result = await transcribe_clip(video_path, engine=self._transcriber)
                transcript_text = result.text or ""
                transcript_segments = result.segments_as_dicts()
                transcript_lang = result.language
            except TranscriberUnavailable as exc:
                log.warning("Transcriber unavailable for clip %s: %s", clip_db_id, exc)
                skipped_transcription = True
            except FileNotFoundError as exc:
                log.warning("Clip file missing for %s: %s", clip_db_id, exc)
                skipped_transcription = True
            except Exception as exc:
                log.exception("Transcription failed for clip %s", clip_db_id)
                update_enrichment_status(
                    clip_db_id,
                    status=STATUS_FAILED,
                    error_message=f"transcription: {exc}",
                    completed_at=_utcnow_iso(),
                )
                return EnrichmentOutcome(
                    clip_db_id=clip_db_id,
                    status=STATUS_FAILED,
                    error_message=f"transcription: {exc}",
                )

        save_transcript(
            clip_db_id,
            transcript_raw=transcript_text or None,
            transcript_segments=transcript_segments,
            transcript_lang=transcript_lang,
        )

        # ---- Correct ----
        update_enrichment_status(clip_db_id, status=STATUS_CORRECTING)
        vocab_entries = []
        try:
            vocab_entries = self._vocab_loader()
        except Exception:
            log.exception("Vocab load failed; continuing with empty vocab")

        if transcript_text:
            try:
                correction = correct_transcript(transcript_text, vocab=vocab_entries)
            except Exception as exc:
                log.exception("Correction failed for clip %s", clip_db_id)
                update_enrichment_status(
                    clip_db_id,
                    status=STATUS_FAILED,
                    error_message=f"correction: {exc}",
                    completed_at=_utcnow_iso(),
                )
                return EnrichmentOutcome(
                    clip_db_id=clip_db_id,
                    status=STATUS_FAILED,
                    error_message=f"correction: {exc}",
                )
        else:
            from .transcription import CorrectionResult
            correction = CorrectionResult(corrected="")

        save_corrected(
            clip_db_id,
            transcript_corrected=correction.corrected or None,
            detected_terms=correction.detected_terms,
        )

        # ---- LLM ----
        update_enrichment_status(clip_db_id, status=STATUS_LLM)
        request = LLMRequest(
            transcript=correction.corrected or "",
            detected_terms=tuple(correction.detected_terms),
            streamer=StreamerProfile(
                streamer_login=ctx.streamer_login,
                language=transcript_lang,
            ),
            clip_title=ctx.title,
            game_name=ctx.game_name or "Deadlock",
            duration_seconds=ctx.duration_seconds,
        )

        dispatcher = self._dispatcher or LLMDispatcher()
        try:
            response = await dispatcher.generate(request)
        except LLMUnavailable as exc:
            log.warning("LLM dispatcher failed for clip %s: %s", clip_db_id, exc)
            status = STATUS_SKIPPED if skipped_transcription else STATUS_FAILED
            update_enrichment_status(
                clip_db_id,
                status=status,
                error_message=f"llm: {exc}",
                completed_at=_utcnow_iso(),
            )
            return EnrichmentOutcome(
                clip_db_id=clip_db_id,
                status=status,
                error_message=f"llm: {exc}",
            )
        except Exception as exc:
            log.exception("LLM dispatcher errored unexpectedly for clip %s", clip_db_id)
            update_enrichment_status(
                clip_db_id,
                status=STATUS_FAILED,
                error_message=f"llm: {exc}",
                completed_at=_utcnow_iso(),
            )
            return EnrichmentOutcome(
                clip_db_id=clip_db_id,
                status=STATUS_FAILED,
                error_message=f"llm: {exc}",
            )

        save_llm_output(
            clip_db_id,
            youtube=response.youtube,
            tiktok=response.tiktok,
            instagram=response.instagram,
            provider=response.provider,
            model=response.model,
            cost_usd_estimate=response.cost_usd_estimate,
        )
        update_enrichment_status(
            clip_db_id,
            status=STATUS_DONE,
            error_message=None,
            completed_at=_utcnow_iso(),
        )
        try:
            mark_clip_awaiting_approval(clip_db_id)
        except Exception:
            log.warning(
                "Could not transition clip %s into approval state; continuing with enrichment result",
                clip_db_id,
                exc_info=True,
            )
        return EnrichmentOutcome(
            clip_db_id=clip_db_id,
            status=STATUS_DONE,
            provider=response.provider,
            model=response.model,
        )


async def run_enrichment(clip_db_id: int, *, force: bool = False) -> EnrichmentOutcome:
    """Convenience-Wrapper fuer die Default-Pipeline."""
    pipeline = ClipEnrichmentPipeline()
    return await pipeline.run(clip_db_id, force=force)


# ---------------------------------------------------------------------------
# Worker queries
# ---------------------------------------------------------------------------

def iter_pending_enrichments(limit: int = 5) -> list[int]:
    """Liefert Clip-IDs, die enrichment brauchen.

    Selektiert Clips,
    - die nicht discarded sind,
    - bei denen entweder kein enrichment-Eintrag existiert
    - oder der enrichment-Status `pending` oder `failed` ist und nicht aktuell laeuft.
    """
    with readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT c.id
              FROM twitch_clips_social_media c
              LEFT JOIN social_media_clip_enrichment e ON e.clip_db_id = c.id
             WHERE c.discarded_at IS NULL
               AND COALESCE(c.upload_local_path, c.local_file_path) IS NOT NULL
               AND (e.status IS NULL OR e.status IN ('pending', 'failed'))
             ORDER BY c.created_at DESC
             LIMIT %s
            """,
            (max(1, int(limit)),),
        ).fetchall()
    return [int(r["id"] if hasattr(r, "keys") else r[0]) for r in rows]
