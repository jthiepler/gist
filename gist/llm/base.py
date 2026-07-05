"""LLM abstraction: base class and chat message type."""
from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import List


@dataclass
class ChatMessage:
    role: str
    content: str


class LLMBackend(ABC):
    @abstractmethod
    def load(self, model_path: str):
        ...

    @abstractmethod
    def generate(
        self,
        messages: List[ChatMessage],
        max_tokens: int = 4096,
        temperature: float = 0.7,
        thinking: bool = False,
    ) -> str:
        ...

    @abstractmethod
    def cleanup(self):
        ...
