"""Local speaker diarization using the bundled pyannote Community-1 model."""
from __future__ import annotations

import logging
import os
import sys
from functools import lru_cache
from pathlib import Path
from typing import Any, Callable, Dict, Iterable, List, Optional

from .transcription.base import Segment

# Keep development and direct CLI runs as private as the packaged sidecar. This
# must happen before the lazy pyannote import in _load_pipeline.
os.environ["PYANNOTE_METRICS_ENABLED"] = "false"

log = logging.getLogger(__name__)

MODEL_DIR_NAME = "speaker-diarization-community-1"
ProgressCallback = Callable[[int, str], None]


def resolve_model_path() -> Optional[Path]:
    """Find the local model in development and in a packaged Tauri app."""
    configured = os.environ.get("GIST_DIARIZATION_MODEL_PATH")
    candidates = []
    if configured:
        candidates.append(Path(configured).expanduser())

    project_root = Path(__file__).resolve().parent.parent
    candidates.extend(
        [
            project_root / MODEL_DIR_NAME,
            project_root / "src-tauri" / "resources" / "pyannote" / MODEL_DIR_NAME,
            Path.cwd() / MODEL_DIR_NAME,
            Path.cwd() / "src-tauri" / "resources" / "pyannote" / MODEL_DIR_NAME,
        ]
    )

    executable = Path(sys.executable).resolve()
    candidates.append(executable.parent.parent / "pyannote" / MODEL_DIR_NAME)

    for path in candidates:
        if (path / "config.yaml").is_file() and (path / "segmentation").is_dir():
            return path
    return None


def is_available() -> bool:
    available = resolve_model_path() is not None
    log.info("event=diarization_model_checked available=%s", available)
    return available


@lru_cache(maxsize=1)
def _load_pipeline(model_path: str) -> Any:
    from pyannote.audio import Pipeline

    log.info("event=diarization_pipeline_load_started")
    pipeline = Pipeline.from_pretrained(model_path)

    # Community-1 uses PyTorch. Prefer Metal on supported Macs, but keep the
    # CPU fallback for Intel Macs and environments where MPS is unavailable.
    import torch

    if torch.backends.mps.is_available():
        pipeline.to(torch.device("mps"))
        log.info("Using Metal acceleration for speaker diarization")
    else:
        log.info("Metal is unavailable; using CPU for speaker diarization")

    log.info("event=diarization_pipeline_loaded")
    return pipeline


def _iter_turns(annotation: Any) -> Iterable[Dict[str, Any]]:
    for turn, _, speaker in annotation.itertracks(yield_label=True):
        yield {
            "start": round(float(turn.start), 3),
            "end": round(float(turn.end), 3),
            "speaker": str(speaker),
        }


def diarize_audio(
    audio_path: str,
    progress_callback: Optional[ProgressCallback] = None,
    cancel_event: Any = None,
) -> List[Dict[str, Any]]:
    """Return speaker turns for an audio file using only local model files."""
    model_path = resolve_model_path()
    if model_path is None:
        log.error("event=diarization_model_missing")
        raise FileNotFoundError(
            "Local pyannote model not found. Set GIST_DIARIZATION_MODEL_PATH or "
            f"place it in {MODEL_DIR_NAME}."
        )

    if cancel_event and cancel_event.is_set():
        raise InterruptedError("Diarization cancelled")
    if progress_callback:
        progress_callback(0, "Preparing speaker identification...")

    pipeline = _load_pipeline(str(model_path))
    log.info("event=diarization_started")

    stage_ranges = {
        "segmentation": (5, 65, "Analyzing speech..."),
        "speaker_counting": (70, 70, "Estimating number of speakers..."),
        "embeddings": (72, 92, "Identifying speakers..."),
        "discrete_diarization": (96, 96, "Finalizing transcript..."),
    }

    def report_pipeline_progress(
        step_name: str,
        _artifact: Any,
        file: Any = None,
        total: Optional[int] = None,
        completed: Optional[int] = None,
    ) -> None:
        if cancel_event and cancel_event.is_set():
            raise InterruptedError("Diarization cancelled")
        if not progress_callback:
            return

        start, end, stage = stage_ranges.get(
            step_name, (96, 96, "Finalizing transcript...")
        )
        if total and completed is not None:
            fraction = min(1.0, max(0.0, completed / total))
            percent = start + round((end - start) * fraction)
        else:
            percent = end
        progress_callback(percent, stage)

    if progress_callback:
        progress_callback(5, "Analyzing speech...")
    output = pipeline(audio_path, hook=report_pipeline_progress)

    if cancel_event and cancel_event.is_set():
        raise InterruptedError("Diarization cancelled")

    annotation = getattr(output, "exclusive_speaker_diarization", None)
    if annotation is None:
        annotation = output.speaker_diarization
    turns = list(_iter_turns(annotation))
    if progress_callback:
        progress_callback(100, "Finalizing transcript...")
    log.info("event=diarization_finished turns=%d", len(turns))
    return turns


def attach_speakers(segments: List[Segment], turns: List[Dict[str, Any]]) -> None:
    """Assign best-overlap speakers with a linear chronological interval scan."""
    turn_idx = 0
    for segment in segments:
        while turn_idx < len(turns) and turns[turn_idx]["end"] <= segment.start:
            turn_idx += 1
        best_speaker: Optional[str] = None
        best_overlap = 0.0
        candidate_idx = turn_idx
        while candidate_idx < len(turns) and turns[candidate_idx]["start"] < segment.end:
            turn = turns[candidate_idx]
            overlap = min(segment.end, turn["end"]) - max(segment.start, turn["start"])
            if overlap > best_overlap:
                best_overlap = overlap
                best_speaker = turn["speaker"]
            candidate_idx += 1
        segment.speaker = best_speaker


def render_speaker_transcript(segments: List[Segment]) -> str:
    """Render timestamped transcription segments with speaker labels."""
    lines = []
    for segment in segments:
        text = segment.text.strip()
        if not text:
            continue
        label = segment.speaker or "Unknown speaker"
        lines.append(f"[{label}] {text}")
    return "\n\n".join(lines)
