"""LLM abstraction: base class and chat message type."""
from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import List, Optional
import threading


@dataclass
class ChatMessage:
    role: str
    content: str
    # Character offset in ``content`` after the reusable prompt prefix. Backends
    # that support prompt caching may prefill everything through this point and
    # process only the remaining suffix for subsequent requests. Other backends
    # can ignore it because ``content`` always contains the complete message.
    cache_prefix_length: Optional[int] = None


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
        cancel_event: Optional[threading.Event] = None,
    ) -> str:
        ...

    @abstractmethod
    def generate_choice(
        self,
        messages: List[ChatMessage],
        choices: List[str],
        max_tokens: int = 16,
        cancel_event: Optional[threading.Event] = None,
    ) -> str:
        ...

    @abstractmethod
    def cleanup(self):
        ...
