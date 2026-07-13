"""Pipeline: transcribe audio then generate clinical note."""
from __future__ import annotations

import logging
import threading
import time
from typing import Optional

from .models import resolve_model
from .transcription.base import ProgressCallback, TranscriptResult
from .transcription.factory import create_transcription_backend
from .transcription.parakeet_backend import resolve_model_path

log = logging.getLogger(__name__)

_cached_llm = None
_cached_llm_repo: Optional[str] = None


def _get_cached_llm(model_repo: str, revision: str):
    """Return the loaded MLX model, retaining one model between note requests."""
    global _cached_llm, _cached_llm_repo
    cache_key = f"{model_repo}@{revision}"
    if _cached_llm is not None and _cached_llm_repo == cache_key:
        log.info("event=llm_model_cache_hit model_repo=%s revision=%s", model_repo, revision)
        return _cached_llm

    log.info("event=llm_model_cache_miss model_repo=%s revision=%s", model_repo, revision)
    release_cached_llm()
    from .llm.mlx_backend import MLXBackend

    llm = MLXBackend()
    llm.load(model_repo, revision=revision)
    _cached_llm = llm
    _cached_llm_repo = cache_key
    log.info("event=llm_model_cached model_repo=%s revision=%s", model_repo, revision)
    return llm


def release_cached_llm(model_name: Optional[str] = None) -> None:
    """Evict the cached model, optionally only when it matches a catalog name."""
    global _cached_llm, _cached_llm_repo
    if model_name is not None:
        try:
            spec = resolve_model(model_name, "llm")
            if _cached_llm_repo != f"{spec.hf_repo}@{spec.revision}":
                return
        except (KeyError, ValueError):
            return
    if _cached_llm is not None:
        log.info("event=llm_model_cache_released")
        _cached_llm.cleanup()
        _cached_llm = None
        _cached_llm_repo = None


def _probe_audio_duration(audio_path: str) -> float:
    """Fast audio duration probe without full decode (miniaudio)."""
    try:
        import miniaudio
        info = miniaudio.get_file_info(audio_path)
        return float(info.duration)
    except Exception as e:
        log.warning("event=audio_duration_probe_failed error_type=%s", type(e).__name__)
        return 0.0


def transcribe_audio(
    audio_path: str,
    diarize: bool = False,
    progress_callback: Optional[ProgressCallback] = None,
    cancel_event: Optional[threading.Event] = None,
) -> TranscriptResult:
    log.info("event=transcription_pipeline_started diarize=%s", diarize)
    model_path = resolve_model_path()
    if model_path is None:
        log.error("event=transcription_model_missing")
        raise FileNotFoundError(
            "Bundled transcription model not found. Rebuild the app with "
            "the required model resources."
        )
    backend = create_transcription_backend()
    log.info("event=transcription_backend_created backend=%s", type(backend).__name__)

    if progress_callback:
        progress_callback(0, "Preparing transcription...")

    try:
        backend.load(str(model_path))
        log.info("event=transcription_model_ready backend=%s", type(backend).__name__)
        audio_duration = _probe_audio_duration(audio_path)
        log.info("event=audio_duration_ready duration_seconds=%.1f", audio_duration)

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
            audio_path, progress_callback=_wrapped, cancel_event=cancel_event
        )
        log.info(
            "event=transcription_backend_completed segments=%d duration_seconds=%.1f",
            len(result.segments),
            result.duration,
        )
        if audio_duration > 0:
            result.duration = audio_duration

        # Session recordings are diarized locally after transcription. Clinician
        # dictation can opt out because it is a single-speaker source.
        if diarize and result.segments:
            log.info("event=diarization_started segments=%d", len(result.segments))
            from .diarization import attach_speakers, diarize_audio, is_available, render_speaker_transcript

            if not is_available():
                log.error("event=diarization_model_missing")
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
            log.info("event=diarization_completed turns=%d", len(turns))

        return result
    finally:
        backend.cleanup()
        log.info("event=transcription_backend_cleaned backend=%s", type(backend).__name__)


def generate_note(
    transcript: str,
    format_name: str = "soap",
    llm_model: str = "qwen-3.5-4b",
    max_tokens: int = 4096,
    thinking: bool = False,
    progress_callback: Optional[ProgressCallback] = None,
    prompt: Optional[str] = None,
    cancel_event: Optional[threading.Event] = None,
) -> str:
    from .formats.registry import get_format
    from .formats.defaults import build_messages

    spec = resolve_model(llm_model, "llm")
    log.info(
        "event=note_generation_pipeline_started model=%s format=%s source_chars=%d thinking=%s prompt_provided=%s",
        llm_model,
        format_name,
        len(transcript),
        thinking,
        prompt is not None,
    )

    if progress_callback:
        progress_callback(0, "Preparing note generation...")

    llm = _get_cached_llm(spec.hf_repo, spec.revision)

    if progress_callback:
        progress_callback(30, "Generating note...")

    if cancel_event and cancel_event.is_set():
        raise InterruptedError("Note generation cancelled")

    if prompt:
        messages = build_messages({"prompt": prompt}, transcript)
    else:
        messages = get_format(format_name).build_messages(transcript)
    log.info("event=note_generation_messages_ready message_count=%d", len(messages))

    note = llm.generate(
        messages=messages,
        max_tokens=max_tokens,
        thinking=thinking,
        cancel_event=cancel_event,
    )

    if progress_callback:
        progress_callback(100, "Finalizing note...")

    log.info("event=note_generation_pipeline_completed note_chars=%d", len(note))
    return note
