"""
Social Media Phase 2 transcription pipeline.

Modules:
- vocab:       Deadlock vocabulary storage + CRUD
- correction:  Fuzzy correction of transcripts using deadlock_vocab
- whisper:     Speech-to-text adapter (faster-whisper + OpenAI API fallback)
- seed_vocab:  Initial seed of Deadlock vocabulary (heroes/items/abilities + slang)
"""

from .correction import CorrectionResult, correct_transcript
from .vocab import (
    VocabEntry,
    delete_vocab_entry,
    list_vocab,
    load_all_vocab,
    load_all_vocab_safe,
    upsert_vocab_entry,
)
from .whisper import (
    TranscriptionResult,
    TranscriberUnavailable,
    get_transcriber,
    transcribe_clip,
)

__all__ = [
    "CorrectionResult",
    "TranscriberUnavailable",
    "TranscriptionResult",
    "VocabEntry",
    "correct_transcript",
    "delete_vocab_entry",
    "get_transcriber",
    "list_vocab",
    "load_all_vocab",
    "load_all_vocab_safe",
    "transcribe_clip",
    "upsert_vocab_entry",
]
