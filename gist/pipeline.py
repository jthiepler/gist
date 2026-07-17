"""Pipeline: transcribe audio then generate clinical note."""
from __future__ import annotations

import logging
import threading
import time
from typing import Optional

from .audio import cleanup_normalized_audio, normalize_audio_for_pipeline
from .diarization import DEFAULT_NUM_SPEAKERS
from .models import DEFAULT_LLM, resolve_model
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


def _postprocess_diarization(
    result: TranscriptResult,
    audio_path: str,
    diarize: bool,
    progress_callback: Optional[ProgressCallback],
    cancel_event: Optional[threading.Event],
    num_speakers: int,
    llm_model: str,
    audio_duration: float,
    transcription_end: int,
) -> TranscriptResult:
    """Diarize and optionally infer roles using the shared source WAV."""
    if not diarize or not result.segments:
        return result

    log.info("event=diarization_started segments=%d", len(result.segments))
    from .diarization import attach_speakers, diarize_audio, is_available, render_speaker_transcript
    from .speaker_roles import canonicalize_speaker_labels, infer_practitioner_speaker, relabel_speaker_roles

    if not is_available():
        log.error("event=diarization_model_missing")
        raise FileNotFoundError(
            "Speaker diarization model is not bundled. Rebuild the app with "
            "speaker-diarization-community-1 in src-tauri/resources/pyannote/."
        )

    diarization_end = 25

    def _diarization_progress(pct: int, stage: str) -> None:
        if progress_callback:
            progress_callback(
                transcription_end
                + round((pct / 100) * (diarization_end - transcription_end)),
                stage,
                eta_seconds=None,
                audio_duration=audio_duration,
            )

    turns = diarize_audio(
        audio_path,
        progress_callback=_diarization_progress if progress_callback else None,
        cancel_event=cancel_event,
        num_speakers=num_speakers,
    )
    attach_speakers(result.segments, turns)
    # Keep the diarization output useful even if the optional role model
    # cannot be loaded or does not produce a valid answer.
    canonicalize_speaker_labels(result.segments)
    log.info("event=diarization_completed turns=%d", len(turns))

    if progress_callback:
        progress_callback(
            26,
            "Preparing speaker role identification...",
            eta_seconds=None,
            audio_duration=audio_duration,
        )
    role_identified = False
    try:
        spec = resolve_model(llm_model, "llm")
        llm = _get_cached_llm(spec.hf_repo, spec.revision)
        if progress_callback:
            progress_callback(
                28,
                "Identifying practitioner...",
                eta_seconds=None,
                audio_duration=audio_duration,
            )
        practitioner_speaker = infer_practitioner_speaker(
            result.segments,
            llm,
            num_speakers,
            cancel_event=cancel_event,
        )
        relabel_speaker_roles(result.segments, practitioner_speaker)
        role_identified = True
    except InterruptedError:
        raise
    except Exception as error:
        # Role identification is best-effort. Diarization has already
        # produced stable generic labels, so return those rather than
        # failing an otherwise usable transcript.
        log.warning(
            "event=speaker_role_identification_skipped error_type=%s",
            type(error).__name__,
        )
    result.text = render_speaker_transcript(result.segments)
    if progress_callback:
        progress_callback(
            30,
            "Finalizing transcript...",
            eta_seconds=None,
            audio_duration=audio_duration,
        )
    log.info(
        "event=speaker_role_identification_completed identified=%s",
        role_identified,
    )
    return result


def transcribe_audio(
    audio_path: str,
    diarize: bool = False,
    progress_callback: Optional[ProgressCallback] = None,
    cancel_event: Optional[threading.Event] = None,
    num_speakers: int = DEFAULT_NUM_SPEAKERS,
    llm_model: str = DEFAULT_LLM,
) -> TranscriptResult:
    log.info(
        "event=transcription_pipeline_started diarize=%s num_speakers=%s llm_model=%s",
        diarize,
        num_speakers,
        llm_model if diarize else "unused",
    )
    model_path = resolve_model_path()
    if model_path is None:
        log.error("event=transcription_model_missing")
        raise FileNotFoundError(
            "Bundled transcription model not found. Rebuild the app with "
            "the required model resources."
        )
    # A cached note model from an earlier session would otherwise overlap the
    # transcription model in memory. It is loaded again only after ASR cleanup.
    release_cached_llm()
    if progress_callback:
        progress_callback(0, "Preparing transcription...")

    pipeline_audio_path = audio_path
    normalized_audio_path = None
    backend = None
    audio_duration = 0.0
    try:
        pipeline_audio_path, normalized_audio_path = normalize_audio_for_pipeline(
            audio_path,
            cancel_event=cancel_event,
        )
        backend = create_transcription_backend()
        log.info("event=transcription_backend_created backend=%s", type(backend).__name__)
        backend.load(str(model_path))
        log.info("event=transcription_model_ready backend=%s", type(backend).__name__)
        audio_duration = _probe_audio_duration(pipeline_audio_path)
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
        transcription_end = 16 if diarize else 30

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
            pipeline_audio_path,
            progress_callback=_wrapped,
            cancel_event=cancel_event,
        )
        log.info(
            "event=transcription_backend_completed segments=%d duration_seconds=%.1f",
            len(result.segments),
            result.duration,
        )
        if audio_duration > 0:
            result.duration = audio_duration
    finally:
        if backend is not None:
            backend.cleanup()
            log.info("event=transcription_backend_cleaned backend=%s", type(backend).__name__)

    try:
        return _postprocess_diarization(
            result,
            pipeline_audio_path,
            diarize,
            progress_callback,
            cancel_event,
            num_speakers,
            llm_model,
            audio_duration,
            transcription_end,
        )
    finally:
        cleanup_normalized_audio(normalized_audio_path)


def generate_notes(
    sources,
    formats,
    llm_model: str = "qwen-3.5-4b",
    max_tokens: int = 4096,
    thinking: bool = False,
    verification_mode: str = "off",
    progress_callback: Optional[ProgressCallback] = None,
    cancel_event: Optional[threading.Event] = None,
    diagnostic_capture=None,
):
    from .note_generation.pipeline import generate_notes_with_backend
    from .note_generation.pipeline import build_evidence_cache_key

    sources = tuple(sources)
    formats = tuple(formats)
    spec = resolve_model(llm_model, "llm")
    if diagnostic_capture:
        diagnostic_capture.set_stage(
            "model",
            {
                "requested_model": llm_model,
                "repository": spec.hf_repo,
                "revision": spec.revision,
                "max_tokens": max_tokens,
                "thinking": thinking,
                "verification_mode": verification_mode,
            },
        )
    log.info(
        "event=note_generation_pipeline_started model=%s source_count=%d format_count=%d source_chars=%d thinking=%s verification_mode=%s",
        llm_model,
        len(sources),
        len(formats),
        sum(len(source.text) for source in sources),
        thinking,
        verification_mode,
    )
    llm = _get_cached_llm(spec.hf_repo, spec.revision)
    evidence_cache_key = build_evidence_cache_key(
        sources,
        f"{spec.hf_repo}@{spec.revision}",
    )
    result = generate_notes_with_backend(
        llm,
        sources,
        formats,
        max_tokens=max_tokens,
        thinking=thinking,
        verification_mode=verification_mode,
        evidence_cache_key=evidence_cache_key,
        progress_callback=progress_callback,
        cancel_event=cancel_event,
        diagnostic_capture=diagnostic_capture,
    )
    log.info(
        "event=note_generation_pipeline_completed note_count=%d failure_count=%d evidence_records=%d",
        len(result.notes),
        len(result.failures),
        result.ledger_stats.get("evidence_records", 0),
    )
    return result


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
    from .note_generation.types import NoteFormatRequest, NoteGenerationSource

    result = generate_notes(
        sources=(
            NoteGenerationSource(
                id="source-1",
                kind="session_transcript",
                origin="legacy",
                title="Session transcript",
                text=transcript,
            ),
        ),
        formats=(NoteFormatRequest(name=format_name, prompt=prompt),),
        llm_model=llm_model,
        max_tokens=max_tokens,
        thinking=thinking,
        verification_mode="off",
        progress_callback=progress_callback,
        cancel_event=cancel_event,
    )
    if result.notes:
        return result.notes[0].note
    message = result.failures[0].message if result.failures else "The note could not be generated."
    raise RuntimeError(message)
