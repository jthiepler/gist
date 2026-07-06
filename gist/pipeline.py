"""Pipeline: transcribe audio then generate clinical note."""
from __future__ import annotations

import logging
import time
from typing import Optional

from .llm.factory import auto_detect_backend, create_backend
from .config import DEFAULT_OPENAI_ENDPOINT
from .models import resolve_model
from .transcription.base import ProgressCallback, TranscriptResult
from .transcription.factory import create_transcription_backend

log = logging.getLogger(__name__)


def _probe_audio_duration(audio_path: str) -> float:
    """Fast audio duration probe without full decode (miniaudio)."""
    try:
        import miniaudio
        info = miniaudio.get_file_info(audio_path)
        return float(info.duration)
    except Exception:
        return 0.0


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

    audio_duration = _probe_audio_duration(audio_path)

    # ETA tracking — rate-based with EMA smoothing.
    # The first chunk includes MLX kernel warmup (~10s) and is NOT representative
    # of steady-state speed. We skip it for rate computation, then use an
    # exponential moving average of the per-chunk rate for stable estimates.
    prev_time: Optional[float] = None
    prev_pct: Optional[int] = None
    smoothed_rate: Optional[float] = None  # % per second
    alpha = 0.4  # EMA weight on newest sample

    def _wrapped(pct: int, stage: str):
        if not progress_callback:
            return
        nonlocal prev_time, prev_pct, smoothed_rate
        now = time.monotonic()

        eta_seconds: Optional[float] = None
        if prev_time is not None and pct > prev_pct:
            dt = now - prev_time
            dpct = pct - prev_pct
            if dt > 0.1 and dpct > 0:
                instant_rate = dpct / dt
                if smoothed_rate is None:
                    smoothed_rate = instant_rate
                else:
                    smoothed_rate = alpha * instant_rate + (1 - alpha) * smoothed_rate
                if smoothed_rate > 0 and pct < 100:
                    eta_seconds = (100 - pct) / smoothed_rate
        prev_time = now
        prev_pct = pct

        progress_callback(
            pct,
            stage,
            eta_seconds=eta_seconds,
            audio_duration=audio_duration,
        )

    # Signal that transcription has started (small fill so bar isn't frozen
    # during the first chunk — ETA not yet available).
    if progress_callback:
        progress_callback(1, "transcribing", eta_seconds=None, audio_duration=audio_duration)

    result = backend.transcribe(
        audio_path, language=language, progress_callback=_wrapped
    )
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
