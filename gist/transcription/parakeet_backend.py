"""Parakeet backend using mlx-audio (MLX, Apple Silicon)."""
from __future__ import annotations

import logging
from pathlib import Path
from typing import Optional

from .base import ProgressCallback, Segment, TranscriptionBackend, TranscriptResult

log = logging.getLogger(__name__)

_CHUNK_SECONDS = 120.0
_OVERLAP_SECONDS = 2.0


class ParakeetBackend(TranscriptionBackend):
    def __init__(self):
        self.model = None

    def load(self, model_path: str):
        log.info("Loading Parakeet model from %s", model_path)
        from mlx_audio.stt.utils import load as load_model

        self.model = load_model(model_path)
        log.info("Parakeet model loaded")

    def transcribe(
        self,
        audio_path: str,
        language: Optional[str] = None,
        progress_callback: Optional[ProgressCallback] = None,
    ) -> TranscriptResult:
        if self.model is None:
            raise RuntimeError("Model not loaded. Call load() first.")

        path = Path(audio_path)
        if not path.exists():
            raise FileNotFoundError(f"Audio file not found: {audio_path}")

        log.info("Transcribing %s (language=%s ignored)", audio_path, language or "auto")

        text_parts: list[str] = []
        segments: list[Segment] = []
        audio_duration = 0.0

        for chunk in self.model.generate(
            str(path),
            chunk_duration=_CHUNK_SECONDS,
            overlap_duration=_OVERLAP_SECONDS,
            stream=True,
        ):
            new_text = getattr(chunk, "text", "") or ""
            if new_text:
                text_parts.append(new_text)
                segments.append(
                    Segment(
                        start=float(getattr(chunk, "start_time", 0.0)),
                        end=float(getattr(chunk, "end_time", 0.0)),
                        text=new_text.strip(),
                    )
                )
            audio_duration = float(getattr(chunk, "audio_duration", 0.0) or 0.0)
            if progress_callback:
                is_final = bool(getattr(chunk, "is_final", False))
                pct = 100 if is_final else int(
                    (getattr(chunk, "progress", 0.0) or 0.0) * 100
                )
                progress_callback(pct, "transcribing")

        full_text = " ".join(text_parts).strip()
        duration = segments[-1].end if segments else audio_duration

        return TranscriptResult(
            text=full_text,
            segments=segments,
            duration=duration,
            language="",
        )

    def cleanup(self):
        self.model = None
