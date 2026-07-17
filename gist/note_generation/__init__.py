"""Evidence-ledger clinical note generation."""

from .pipeline import generate_notes_with_backend
from .types import (
    EvidenceLedger,
    EvidenceRecord,
    GenerateNotesResult,
    NoteFormatRequest,
    NoteGenerationSource,
)

__all__ = [
    "EvidenceLedger",
    "EvidenceRecord",
    "GenerateNotesResult",
    "NoteFormatRequest",
    "NoteGenerationSource",
    "generate_notes_with_backend",
]
