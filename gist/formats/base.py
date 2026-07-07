"""Clinical format base class."""
from __future__ import annotations

from abc import ABC, abstractmethod
from typing import List

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
    def build_messages(self, transcript: str) -> List[ChatMessage]:
        ...
