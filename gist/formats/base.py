"""Clinical format base class."""
from __future__ import annotations

from abc import ABC, abstractmethod
from typing import List, Optional

from ..llm.base import ChatMessage


class ClinicalFormat(ABC):
    @property
    @abstractmethod
    def name(self) -> str:
        ...

    @property
    @abstractmethod
    def description(self) -> str:
        ...

    @abstractmethod
    def build_messages(
        self, transcript: str, language: Optional[str] = None
    ) -> List[ChatMessage]:
        ...

    @staticmethod
    def _language_instruction(language: Optional[str] = None) -> str:
        if language:
            return f"Write the entire note in {language}. Use the clinical terminology appropriate for that language."
        return (
            "Detect the language of the transcript automatically and write the entire note "
            "in the same language as the session. Use clinical terminology appropriate for that language."
        )
