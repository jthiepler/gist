"""Whisper backend using faster-whisper (CPU, int8, VAD)."""
from __future__ import annotations

import logging
from pathlib import Path
from typing import Optional

from faster_whisper import WhisperModel

from .base import ProgressCallback, Segment, TranscriptionBackend, TranscriptResult

log = logging.getLogger(__name__)


class WhisperBackend(TranscriptionBackend):
    def __init__(self):
        self.model = None

    def load(self, model_path: str):
        log.info("Loading Whisper model from %s", model_path)
        self.model = WhisperModel(
            model_path,
            device="cpu",
            compute_type="int8",
            num_workers=2,
        )
        log.info("Whisper model loaded")

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

        log.info("Transcribing %s (language=%s)", audio_path, language or "auto")

        segments, info = self.model.transcribe(
            str(path),
            language=language,
            vad_filter=False,
        )

        text_parts = []
        segment_list = []
        total_duration = info.duration

        for i, seg in enumerate(segments):
            text_parts.append(seg.text)
            segment_list.append(
                Segment(start=seg.start, end=seg.end, text=seg.text.strip())
            )
            if progress_callback:
                pct = int((seg.end / total_duration) * 100) if total_duration > 0 else 0
                progress_callback(pct, "transcribing")

        return TranscriptResult(
            text=" ".join(text_parts).strip(),
            segments=segment_list,
            duration=total_duration,
            language=info.language,
        )

    def cleanup(self):
        self.model = None
