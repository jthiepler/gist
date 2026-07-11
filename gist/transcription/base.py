"""Transcription abstraction: base class and types."""
from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Callable, List, Optional
import threading


@dataclass
class Segment:
    start: float
    end: float
    text: str
    speaker: Optional[str] = None


@dataclass
class TranscriptResult:
    text: str
    segments: List[Segment] = field(default_factory=list)
    duration: float = 0.0
    language: str = ""


ProgressCallback = Callable[..., None]


class TranscriptionBackend(ABC):
    @abstractmethod
    def load(self, model_path: str):
        ...

    @abstractmethod
    def transcribe(
        self,
        audio_path: str,
        language: Optional[str] = None,
        progress_callback: Optional[ProgressCallback] = None,
        cancel_event: Optional[threading.Event] = None,
    ) -> TranscriptResult:
        ...

    @abstractmethod
    def cleanup(self):
        ...
