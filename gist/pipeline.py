"""Pipeline: transcribe audio then generate clinical note."""
from __future__ import annotations

import logging
from typing import Optional

from .llm.factory import auto_detect_backend, create_backend
from .config import DEFAULT_OPENAI_ENDPOINT
from .models import resolve_model
from .transcription.base import ProgressCallback, TranscriptResult
from .transcription.factory import create_transcription_backend

log = logging.getLogger(__name__)


def transcribe_audio(
    audio_path: str,
    model_name: str = "whisper-base",
    language: Optional[str] = None,
    progress_callback: Optional[ProgressCallback] = None,
) -> TranscriptResult:
    spec = resolve_model(model_name, "transcription")
    backend = create_transcription_backend(spec.backend)

    if progress_callback:
        progress_callback(0, f"loading {spec.name}")

    backend.load(spec.hf_repo)  # faster-whisper resolves by repo id

    if progress_callback:
        progress_callback(5, "transcribing")

    result = backend.transcribe(audio_path, language=language, progress_callback=progress_callback)
    backend.cleanup()

    return result


def generate_note(
    transcript: str,
    format_name: str = "soap",
    llm_model: str = "qwen-3.5-4b",
    backend_type: Optional[str] = None,
    endpoint: str = DEFAULT_OPENAI_ENDPOINT,
    max_tokens: int = 16384,
    thinking: bool = True,
    language: Optional[str] = None,
    progress_callback: Optional[ProgressCallback] = None,
) -> str:
    from .formats.registry import get_format

    fmt = get_format(format_name)

    if not backend_type:
        backend_type = auto_detect_backend()

    llm = create_backend(backend_type, endpoint=endpoint)

    spec = resolve_model(llm_model, "llm")

    if progress_callback:
        progress_callback(0, f"loading {spec.name}")

    llm.load(spec.hf_repo)

    if progress_callback:
        progress_callback(30, "generating")

    messages = fmt.build_messages(transcript, language=language)

    note = llm.generate(
        messages=messages,
        max_tokens=max_tokens,
        thinking=thinking,
    )

    llm.cleanup()

    if progress_callback:
        progress_callback(100, "done")

    return note
