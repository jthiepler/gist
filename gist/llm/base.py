"""LLM abstraction: base class and chat message type."""
from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import List, Optional
import threading


class GenerationLimitError(RuntimeError):
    """Raised when a generation consumes its token budget before completion."""


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
        allow_truncated: bool = False,
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
    def generate_batch(
        self,
        message_batches: List[List[ChatMessage]],
        max_tokens: int = 768,
        temperature: float = 0.0,
        thinking: bool = False,
        cancel_event: Optional[threading.Event] = None,
    ) -> List[str]:
        ...

    @abstractmethod
    def cleanup(self):
        ...
