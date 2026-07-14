"""Parakeet backend using mlx-audio (MLX, Apple Silicon)."""
from __future__ import annotations

import logging
import os
import sys
import threading
from pathlib import Path
from typing import Any, Optional

from .base import (
    ProgressCallback,
    Segment,
    TranscriptResult,
    TranscriptionBackend,
    Word,
)

log = logging.getLogger(__name__)

_CHUNK_SECONDS = 120.0
_OVERLAP_SECONDS = 2.0


def _token_end(token: Any) -> float:
    end = getattr(token, "end", None)
    if end is not None:
        return float(end)
    return float(token.start) + float(token.duration)


def _tokens_to_words(sentence: Any) -> list[Word]:
    """Group Parakeet's aligned subword tokens into word spans."""
    words: list[Word] = []
    current_tokens: list[Any] = []

    def append_word(tokens: list[Any]) -> None:
        if not tokens:
            return
        text = "".join(str(getattr(token, "text", "")) for token in tokens)
        if text.strip():
            words.append(
                Word(
                    start=float(tokens[0].start),
                    end=max(_token_end(token) for token in tokens),
                    text=text,
                )
            )

    for token in getattr(sentence, "tokens", []) or []:
        text = str(getattr(token, "text", ""))
        if not text.strip():
            continue
        if current_tokens and text.startswith(" "):
            append_word(current_tokens)
            current_tokens = []
        current_tokens.append(token)

    append_word(current_tokens)
    return words


MODEL_DIR_NAME = "parakeet-tdt-0.6b-v3-mlx-4bit"


def resolve_model_path() -> Optional[Path]:
    """Find the bundled Parakeet checkpoint in development or the app bundle."""
    configured = os.environ.get("GIST_PARAKEET_MODEL_PATH")
    candidates = []
    if configured:
        candidates.append(Path(configured).expanduser())

    project_root = Path(__file__).resolve().parent.parent.parent
    candidates.extend(
        [
            project_root / MODEL_DIR_NAME,
            project_root / "src-tauri" / "resources" / "parakeet" / MODEL_DIR_NAME,
            Path.cwd() / MODEL_DIR_NAME,
            Path.cwd() / "src-tauri" / "resources" / "parakeet" / MODEL_DIR_NAME,
        ]
    )

    executable = Path(sys.executable).resolve()
    candidates.append(executable.parent.parent / "parakeet" / MODEL_DIR_NAME)

    for path in candidates:
        if (path / "config.json").is_file() and (path / "model.safetensors").is_file():
            return path
    return None


class ParakeetBackend(TranscriptionBackend):
    def __init__(self):
        self.model = None

    def load(self, model_path: str):
        log.info("event=parakeet_model_load_started")
        from mlx_audio.stt.utils import load as load_model

        self.model = load_model(model_path)
        log.info("event=parakeet_model_loaded")

    def transcribe(
        self,
        audio_path: str,
        progress_callback: Optional[ProgressCallback] = None,
        cancel_event: Optional[threading.Event] = None,
    ) -> TranscriptResult:
        if self.model is None:
            raise RuntimeError("Model not loaded. Call load() first.")

        path = Path(audio_path)
        if not path.exists():
            raise FileNotFoundError(f"Audio file not found: {audio_path}")

        log.info("event=parakeet_transcription_started")

        segments: list[Segment] = []

        def report_chunk_progress(current: int, total: int) -> None:
            if cancel_event and cancel_event.is_set():
                raise InterruptedError("Transcription cancelled")
            if progress_callback:
                pct = int((current / total) * 100) if total > 0 else 0
                progress_callback(pct, "transcribing")

        # Non-streaming generation retains Parakeet's sentence/token
        # timestamps. The streaming API only exposes the accumulated text for
        # each large chunk, which is too coarse for speaker alignment.
        result = self.model.generate(
            str(path),
            chunk_duration=_CHUNK_SECONDS,
            overlap_duration=_OVERLAP_SECONDS,
            chunk_callback=report_chunk_progress,
            stream=False,
        )

        if cancel_event and cancel_event.is_set():
            raise InterruptedError("Transcription cancelled")

        for sentence in getattr(result, "sentences", []):
            text = getattr(sentence, "text", "").strip()
            if not text:
                continue
            segments.append(
                Segment(
                    start=float(sentence.start),
                    end=float(sentence.end),
                    text=text,
                    words=_tokens_to_words(sentence),
                )
            )

        if progress_callback:
            progress_callback(100, "transcribing")

        full_text = " ".join(segment.text for segment in segments).strip()
        duration = segments[-1].end if segments else 0.0
        log.info(
            "event=parakeet_transcription_completed segments=%d duration_seconds=%.1f",
            len(segments),
            duration,
        )

        return TranscriptResult(
            text=full_text,
            segments=segments,
            duration=duration,
        )

    def cleanup(self):
        self.model = None
        log.info("event=parakeet_model_released")
