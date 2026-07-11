"""Pipeline: transcribe audio then generate clinical note."""
from __future__ import annotations

import logging
import threading
import time
from typing import Optional

from .llm.factory import auto_detect_backend, create_backend
from .config import DEFAULT_OPENAI_ENDPOINT
from .models import resolve_model
from .transcription.base import ProgressCallback, TranscriptResult
from .transcription.factory import create_transcription_backend
from .transcription.parakeet_backend import resolve_model_path

log = logging.getLogger(__name__)


def _probe_audio_duration(audio_path: str) -> float:
    """Fast audio duration probe without full decode (miniaudio)."""
    try:
        import miniaudio
        info = miniaudio.get_file_info(audio_path)
        return float(info.duration)
    except Exception as e:
        log.warning("Audio duration probe failed: %s", e)
        return 0.0


def transcribe_audio(
    audio_path: str,
    language: Optional[str] = None,
    diarize: bool = False,
    progress_callback: Optional[ProgressCallback] = None,
    cancel_event: Optional[threading.Event] = None,
) -> TranscriptResult:
    model_path = resolve_model_path()
    if model_path is None:
        raise FileNotFoundError(
            "Bundled transcription model not found. Rebuild the app with "
            "the required model resources."
        )
    backend = create_transcription_backend()

    if progress_callback:
        progress_callback(0, "Preparing transcription...")

    try:
        backend.load(str(model_path))
        audio_duration = _probe_audio_duration(audio_path)

        # ETA tracking — rate-based with EMA smoothing.
        # The first chunk includes MLX kernel warmup (~10s) and is NOT representative
        # of steady-state speed. We skip it for rate computation, then use an
        # exponential moving average of the per-chunk rate for stable estimates.
        prev_time: Optional[float] = None
        prev_pct: Optional[int] = None
        smoothed_rate: Optional[float] = None  # % per second
        alpha = 0.4  # EMA weight on newest sample

        # The frontend reserves 30% of the overall workflow for transcription,
        # including the optional diarization pass, and the final 70% for note
        # generation. Keeping this mapping here prevents the bar from jumping
        # backwards when it changes phase.
        transcription_end = 18 if diarize else 30

        def _wrapped(pct: int, stage: str):
            if not progress_callback:
                return
            if cancel_event and cancel_event.is_set():
                raise InterruptedError("Transcription cancelled")
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
                max(1, round((pct / 100) * transcription_end)),
                "Transcribing...",
                eta_seconds=eta_seconds,
                audio_duration=audio_duration,
            )

        # Signal that transcription has started (small fill so bar isn't frozen
        # during the first chunk — ETA not yet available).
        if progress_callback:
            progress_callback(1, "Transcribing...", eta_seconds=None, audio_duration=audio_duration)

        result = backend.transcribe(
            audio_path, language=language, progress_callback=_wrapped, cancel_event=cancel_event
        )

        # Session recordings are diarized locally after transcription. Clinician
        # dictation can opt out because it is a single-speaker source.
        if diarize and result.segments:
            from .diarization import attach_speakers, diarize_audio, is_available, render_speaker_transcript

            if not is_available():
                raise FileNotFoundError(
                    "Speaker diarization model is not bundled. Rebuild the app with "
                    "speaker-diarization-community-1 in src-tauri/resources/pyannote/."
                )

            def _diarization_progress(pct: int, stage: str) -> None:
                if progress_callback:
                    progress_callback(
                        transcription_end + round((pct / 100) * (30 - transcription_end)),
                        stage,
                        eta_seconds=None,
                        audio_duration=audio_duration,
                    )

            turns = diarize_audio(
                audio_path,
                progress_callback=_diarization_progress if progress_callback else None,
                cancel_event=cancel_event,
            )
            attach_speakers(result.segments, turns)
            result.text = render_speaker_transcript(result.segments)

        return result
    finally:
        backend.cleanup()


def generate_note(
    transcript: str,
    format_name: str = "soap",
    llm_model: str = "qwen-3.5-4b",
    backend_type: Optional[str] = None,
    endpoint: str = DEFAULT_OPENAI_ENDPOINT,
    max_tokens: int = 16384,
    thinking: bool = True,
    progress_callback: Optional[ProgressCallback] = None,
    prompt: Optional[str] = None,
    cancel_event: Optional[threading.Event] = None,
) -> str:
    from .formats.registry import get_format
    from .llm.base import ChatMessage

    if not backend_type:
        backend_type = auto_detect_backend()

    llm = create_backend(backend_type, endpoint=endpoint)

    spec = resolve_model(llm_model, "llm")

    if progress_callback:
        progress_callback(0, "Preparing note generation...")

    try:
        llm.load(spec.hf_repo)

        if progress_callback:
            progress_callback(30, "Generating note...")

        if cancel_event and cancel_event.is_set():
            raise InterruptedError("Note generation cancelled")

        if prompt:
            messages = [
                ChatMessage(role="system", content=prompt),
                ChatMessage(
                    role="user",
                    content=f"Generate a {format_name} note from this therapy session transcript:\n\n{transcript}",
                ),
            ]
        else:
            messages = get_format(format_name).build_messages(transcript)

        note = llm.generate(
            messages=messages,
            max_tokens=max_tokens,
            thinking=thinking,
            cancel_event=cancel_event,
        )

        if progress_callback:
            progress_callback(100, "Finalizing note...")

        return note
    finally:
        llm.cleanup()
