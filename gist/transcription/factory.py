"""Transcription backend factory."""
from __future__ import annotations

from .base import TranscriptionBackend


def create_transcription_backend() -> TranscriptionBackend:
    """Create the bundled transcription backend.

    Keeping this factory preserves the backend boundary for future engines
    without exposing backend selection as part of the current application API.
    """
    from .parakeet_backend import ParakeetBackend

    return ParakeetBackend()
