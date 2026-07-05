"""Transcription backend factory."""
from __future__ import annotations

from .base import TranscriptionBackend


def create_transcription_backend(backend_type: str) -> TranscriptionBackend:
    if backend_type == "whisper":
        from .whisper_backend import WhisperBackend

        return WhisperBackend()
    else:
        raise ValueError(f"Unknown transcription backend: {backend_type}")
