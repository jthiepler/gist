"""Local speaker diarization using the bundled pyannote Community-1 model."""
from __future__ import annotations

import logging
import os
import sys
from functools import lru_cache
from pathlib import Path
from typing import Any, Callable, Dict, Iterable, List, Optional

from .transcription.base import Segment, Word

# Keep development and direct CLI runs as private as the packaged sidecar. This
# must happen before the lazy pyannote import in _load_pipeline.
os.environ["PYANNOTE_METRICS_ENABLED"] = "false"

log = logging.getLogger(__name__)

MODEL_DIR_NAME = "speaker-diarization-community-1"
DEFAULT_NUM_SPEAKERS = 2
MIN_NUM_SPEAKERS = 2
MAX_NUM_SPEAKERS = 4
MIN_TURN_DURATION = 0.3
MAX_SAME_SPEAKER_GAP = 0.25
MAX_ALIGNMENT_GAP = 0.35
MAX_CONTEXTUAL_UNKNOWN_GAP = 2.5
MAX_CONTEXTUAL_UNKNOWN_WORDS = 2
MIN_ALIGNMENT_FRAGMENT_DURATION = 0.3
MAX_CONTIGUOUS_ALIGNMENT_GAP = 0.05
MIN_NEAREST_GAP_ADVANTAGE = 0.1
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


def _merge_same_speaker_turns(
    turns: List[Dict[str, Any]], max_gap: float
) -> List[Dict[str, Any]]:
    merged: List[Dict[str, Any]] = []
    for source in turns:
        start = float(source["start"])
        end = float(source["end"])
        if end <= start:
            continue
        turn = {"start": start, "end": end, "speaker": str(source["speaker"])}
        if (
            merged
            and merged[-1]["speaker"] == turn["speaker"]
            and turn["start"] - merged[-1]["end"] <= max_gap
        ):
            merged[-1]["end"] = max(merged[-1]["end"], turn["end"])
        else:
            merged.append(turn)
    return merged


def clean_speaker_turns(
    turns: List[Dict[str, Any]],
    min_duration: float = MIN_TURN_DURATION,
    max_same_speaker_gap: float = MAX_SAME_SPEAKER_GAP,
) -> List[Dict[str, Any]]:
    """Merge gaps and absorb tiny alternating fragments conservatively."""
    cleaned = _merge_same_speaker_turns(
        sorted(turns, key=lambda turn: float(turn["start"])),
        max_same_speaker_gap,
    )

    changed = True
    while changed:
        changed = False
        compacted: List[Dict[str, Any]] = []
        index = 0
        while index < len(cleaned):
            current = cleaned[index]
            previous = compacted[-1] if compacted else None
            next_turn = cleaned[index + 1] if index + 1 < len(cleaned) else None
            if (
                previous is not None
                and next_turn is not None
                and current["end"] - current["start"] < min_duration
                and previous["speaker"] == next_turn["speaker"]
                and previous["speaker"] != current["speaker"]
                and next_turn["start"] - previous["end"] <= max_same_speaker_gap
            ):
                previous["end"] = max(previous["end"], next_turn["end"])
                index += 2
                changed = True
                continue
            compacted.append(current)
            index += 1
        cleaned = _merge_same_speaker_turns(compacted, max_same_speaker_gap)

    return cleaned


def diarize_audio(
    audio_path: str,
    progress_callback: Optional[ProgressCallback] = None,
    cancel_event: Any = None,
    num_speakers: int = DEFAULT_NUM_SPEAKERS,
) -> List[Dict[str, Any]]:
    """Return speaker turns for an audio file using only local model files."""
    if (
        isinstance(num_speakers, bool)
        or not isinstance(num_speakers, int)
        or not MIN_NUM_SPEAKERS <= num_speakers <= MAX_NUM_SPEAKERS
    ):
        raise ValueError(
            f"Number of speakers must be between {MIN_NUM_SPEAKERS} and {MAX_NUM_SPEAKERS}."
        )

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
        "speaker_counting": (70, 70, f"Separating {num_speakers} speakers..."),
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
    output = pipeline(
        audio_path,
        hook=report_pipeline_progress,
        num_speakers=num_speakers,
    )

    if cancel_event and cancel_event.is_set():
        raise InterruptedError("Diarization cancelled")

    annotation = getattr(output, "exclusive_speaker_diarization", None)
    if annotation is None:
        annotation = output.speaker_diarization
    raw_turns = list(_iter_turns(annotation))
    turns = clean_speaker_turns(raw_turns)
    if progress_callback:
        progress_callback(100, "Finalizing transcript...")
    log.info(
        "event=diarization_finished raw_turns=%d cleaned_turns=%d",
        len(raw_turns),
        len(turns),
    )
    return turns


def _best_overlap_speaker(
    start: float,
    end: float,
    turns: List[Dict[str, Any]],
    turn_idx: int,
) -> tuple[Optional[str], int]:
    while turn_idx < len(turns) and turns[turn_idx]["end"] <= start:
        turn_idx += 1
    best_speaker: Optional[str] = None
    best_overlap = 0.0
    candidate_idx = turn_idx
    while candidate_idx < len(turns) and turns[candidate_idx]["start"] < end:
        turn = turns[candidate_idx]
        overlap = min(end, turn["end"]) - max(start, turn["start"])
        if overlap > best_overlap:
            best_overlap = overlap
            best_speaker = turn["speaker"]
        candidate_idx += 1
    return best_speaker, turn_idx


def _split_segment_by_words(
    segment: Segment,
    turns: List[Dict[str, Any]],
    turn_idx: int,
) -> tuple[List[Segment], int]:
    speakers: List[Optional[str]] = []
    for word in segment.words:
        speaker, turn_idx = _best_overlap_speaker(
            word.start, word.end, turns, turn_idx
        )
        speakers.append(speaker)

    _repair_unknown_word_speakers(segment.words, speakers)
    _smooth_alignment_fragments(segment.words, speakers)

    runs: List[tuple[Optional[str], List[Word]]] = []
    for word, speaker in zip(segment.words, speakers):
        if runs and runs[-1][0] == speaker:
            runs[-1][1].append(word)
        else:
            runs.append((speaker, [word]))

    if len(runs) <= 1:
        segment.speaker = runs[0][0] if runs else None
        return [segment], turn_idx

    split_segments: List[Segment] = []
    for speaker, words in runs:
        text = "".join(word.text for word in words).strip()
        if text:
            split_segments.append(
                Segment(
                    start=words[0].start,
                    end=words[-1].end,
                    text=text,
                    speaker=speaker,
                    words=words,
                )
            )
    return split_segments, turn_idx


def _repair_unknown_word_speakers(
    words: List[Word],
    speakers: List[Optional[str]],
    max_gap: float = MAX_CONTEXTUAL_UNKNOWN_GAP,
    edge_max_gap: float = MAX_ALIGNMENT_GAP,
) -> None:
    """Fill brief unknown runs when surrounding word context is unambiguous."""
    index = 0
    while index < len(speakers):
        if speakers[index] is not None:
            index += 1
            continue

        end = index
        while end < len(speakers) and speakers[end] is None:
            end += 1

        previous = index - 1 if index > 0 else None
        following = end if end < len(speakers) else None
        previous_speaker = speakers[previous] if previous is not None else None
        following_speaker = speakers[following] if following is not None else None

        replacement: Optional[str] = None
        if (
            end - index <= MAX_CONTEXTUAL_UNKNOWN_WORDS
            and previous_speaker is not None
            and previous_speaker == following_speaker
            and following is not None
            and words[index].start - words[previous].end <= max_gap
            and words[following].start - words[end - 1].end <= max_gap
        ):
            replacement = previous_speaker
        elif (
            end - index <= MAX_CONTEXTUAL_UNKNOWN_WORDS
            and previous_speaker is not None
            and following is None
        ):
            if words[index].start - words[previous].end <= edge_max_gap:
                replacement = previous_speaker
        elif (
            end - index <= MAX_CONTEXTUAL_UNKNOWN_WORDS
            and following_speaker is not None
            and previous is None
        ):
            if words[following].start - words[end - 1].end <= edge_max_gap:
                replacement = following_speaker

        if replacement is not None:
            for repair_index in range(index, end):
                speakers[repair_index] = replacement
        index = end


def _alignment_runs(
    speakers: List[Optional[str]],
) -> List[tuple[int, int, Optional[str]]]:
    runs: List[tuple[int, int, Optional[str]]] = []
    start = 0
    for index in range(1, len(speakers) + 1):
        if index == len(speakers) or speakers[index] != speakers[start]:
            runs.append((start, index, speakers[start]))
            start = index
    return runs


def _is_alignment_fragment(words: List[Word], start: int, end: int) -> bool:
    return words[end - 1].end - words[start].start < MIN_ALIGNMENT_FRAGMENT_DURATION


def _smooth_alignment_fragments(
    words: List[Word], speakers: List[Optional[str]]
) -> None:
    """Absorb tiny boundary artifacts without changing standalone replies."""
    changed = True
    while changed:
        changed = False
        runs = _alignment_runs(speakers)
        if len(runs) <= 1:
            return

        for run_index, (start, end, speaker) in enumerate(runs):
            if speaker is None or not _is_alignment_fragment(words, start, end):
                continue

            replacement: Optional[str] = None
            if 0 < run_index < len(runs) - 1:
                previous_speaker = runs[run_index - 1][2]
                following_speaker = runs[run_index + 1][2]
                if (
                    previous_speaker is not None
                    and previous_speaker == following_speaker
                ):
                    replacement = previous_speaker
            elif run_index == 0 and runs[1][2] is not None:
                next_start, next_end, next_speaker = runs[1]
                if next_end - next_start > 1 or not _is_alignment_fragment(
                    words, next_start, next_end
                ):
                    replacement = next_speaker
            elif run_index == len(runs) - 1 and runs[-2][2] is not None:
                previous_start, previous_end, previous_speaker = runs[-2]
                if previous_end - previous_start > 1 or not _is_alignment_fragment(
                    words, previous_start, previous_end
                ):
                    replacement = previous_speaker

            if replacement is not None and replacement != speaker:
                for repair_index in range(start, end):
                    speakers[repair_index] = replacement
                changed = True
                break


def _is_short_segment(segment: Segment) -> bool:
    return segment.end - segment.start < MIN_ALIGNMENT_FRAGMENT_DURATION


def _is_cross_segment_fragment(segment: Segment) -> bool:
    """Identify fragments that are safe to repair from adjacent context."""
    if _is_short_segment(segment):
        return True
    if segment.speaker is None:
        word_count = len(segment.words) if segment.words else len(segment.text.split())
        return word_count <= 2
    return False


def _unknown_fragment_word_count(segment: Segment) -> int:
    return len(segment.words) if segment.words else len(segment.text.split())


def _is_contextual_unknown_fragment(segment: Segment) -> bool:
    return (
        segment.speaker is None
        and _unknown_fragment_word_count(segment) <= MAX_CONTEXTUAL_UNKNOWN_WORDS
    )


def _segments_are_close(
    previous: Segment, following: Segment, max_gap: float = MAX_ALIGNMENT_GAP
) -> bool:
    return following.start - previous.end <= max_gap


def _repair_contextual_unknown_fragments(segments: List[Segment]) -> int:
    """Repair tiny unknown holes only when both neighboring speakers agree.

    Unlike known short replies, an unlabelled one- or two-word hole has no
    competing diarization evidence. The matching labels on each side are the
    useful signal here, even when a hesitation creates a longer timestamp gap.
    """
    repaired = 0
    index = 1
    while index < len(segments) - 1:
        if not _is_contextual_unknown_fragment(segments[index]):
            index += 1
            continue

        run_start = index
        word_count = 0
        while index < len(segments) - 1 and _is_contextual_unknown_fragment(
            segments[index]
        ):
            word_count += _unknown_fragment_word_count(segments[index])
            index += 1
        run_end = index
        previous = segments[run_start - 1]
        following = segments[run_end]

        if (
            word_count <= MAX_CONTEXTUAL_UNKNOWN_WORDS
            and previous.speaker is not None
            and previous.speaker == following.speaker
        ):
            for fragment in segments[run_start:run_end]:
                fragment.speaker = previous.speaker
                repaired += 1

    return repaired


def _repair_malformed_timestamp_fragments(segments: List[Segment]) -> int:
    """Attach implausibly short multi-word slivers to a clear nearest neighbor."""
    repaired = 0
    for index, segment in enumerate(segments):
        if (
            segment.speaker is not None
            or not _is_short_segment(segment)
            or _unknown_fragment_word_count(segment)
            <= MAX_CONTEXTUAL_UNKNOWN_WORDS
        ):
            continue

        previous = segments[index - 1] if index > 0 else None
        following = segments[index + 1] if index + 1 < len(segments) else None
        left_gap = (
            max(0.0, segment.start - previous.end) if previous else float("inf")
        )
        right_gap = (
            max(0.0, following.start - segment.end) if following else float("inf")
        )

        replacement: Optional[str] = None
        if (
            previous is not None
            and previous.speaker is not None
            and left_gap <= MAX_CONTIGUOUS_ALIGNMENT_GAP
            and right_gap - left_gap >= MIN_NEAREST_GAP_ADVANTAGE
        ):
            replacement = previous.speaker
        elif (
            following is not None
            and following.speaker is not None
            and right_gap <= MAX_CONTIGUOUS_ALIGNMENT_GAP
            and left_gap - right_gap >= MIN_NEAREST_GAP_ADVANTAGE
        ):
            replacement = following.speaker

        if replacement is not None:
            segment.speaker = replacement
            repaired += 1

    return repaired


def _repair_cross_segment_fragments(segments: List[Segment]) -> None:
    """Repair short output fragments using adjacent segment context."""
    _repair_contextual_unknown_fragments(segments)
    _repair_malformed_timestamp_fragments(segments)
    changed = True
    while changed:
        changed = False
        index = 1
        while index < len(segments) - 1:
            if not _is_cross_segment_fragment(segments[index]):
                index += 1
                continue

            run_start = index
            while index < len(segments) - 1 and _is_cross_segment_fragment(
                segments[index]
            ):
                index += 1
            run_end = index
            previous = segments[run_start - 1]
            following = segments[run_end]

            if (
                previous.speaker is not None
                and previous.speaker == following.speaker
                and _segments_are_close(previous, segments[run_start])
                and _segments_are_close(segments[run_end - 1], following)
                and any(
                    fragment.speaker != previous.speaker
                    for fragment in segments[run_start:run_end]
                )
            ):
                for fragment in segments[run_start:run_end]:
                    fragment.speaker = previous.speaker
                changed = True
                break


def _join_segment_text(previous: str, following: str) -> str:
    previous = previous.strip()
    following = following.strip()
    if not previous:
        return following
    if not following:
        return previous
    return f"{previous} {following}"


def _merge_segments(previous: Segment, following: Segment) -> Segment:
    return Segment(
        start=min(previous.start, following.start),
        end=max(previous.end, following.end),
        text=_join_segment_text(previous.text, following.text),
        speaker=previous.speaker,
        words=previous.words + following.words,
    )


def _coalesce_same_speaker_segments(segments: List[Segment]) -> List[Segment]:
    """Merge nearby output segments that have the same known speaker."""
    coalesced: List[Segment] = []
    for segment in segments:
        if (
            coalesced
            and segment.speaker is not None
            and coalesced[-1].speaker == segment.speaker
            and _segments_are_close(coalesced[-1], segment, MAX_SAME_SPEAKER_GAP)
        ):
            coalesced[-1] = _merge_segments(coalesced[-1], segment)
        else:
            coalesced.append(segment)
    return coalesced


def attach_speakers(segments: List[Segment], turns: List[Dict[str, Any]]) -> None:
    """Assign speakers, splitting ASR segments at word-level boundaries."""
    turn_idx = 0
    assigned_segments: List[Segment] = []
    for segment in segments:
        if segment.words:
            split_segments, turn_idx = _split_segment_by_words(
                segment, turns, turn_idx
            )
            assigned_segments.extend(split_segments)
        else:
            segment.speaker, turn_idx = _best_overlap_speaker(
                segment.start, segment.end, turns, turn_idx
            )
            assigned_segments.append(segment)

    unknown_after_word_alignment = sum(
        segment.speaker is None for segment in assigned_segments
    )
    _repair_cross_segment_fragments(assigned_segments)
    segments[:] = _coalesce_same_speaker_segments(assigned_segments)
    final_repairs = _repair_contextual_unknown_fragments(segments)
    final_repairs += _repair_malformed_timestamp_fragments(segments)
    if final_repairs:
        segments[:] = _coalesce_same_speaker_segments(segments)
    unknown_final = sum(segment.speaker is None for segment in segments)
    log.info(
        "event=speaker_alignment_completed assigned_segments=%d output_segments=%d "
        "unknown_after_word_alignment=%d unknown_final=%d",
        len(assigned_segments),
        len(segments),
        unknown_after_word_alignment,
        unknown_final,
    )


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
