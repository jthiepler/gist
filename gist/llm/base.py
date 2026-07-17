"""LLM abstraction: base class and chat message type."""
from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import List, Optional
import threading


class GenerationIncompleteError(RuntimeError):
    """The model produced output but did not complete it with a normal stop."""

    def __init__(self, message: str, partial_output: str | None = None):
        super().__init__(message)
        self.partial_output = partial_output


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

    def generate_batch(
        self,
        messages_batch: List[List[ChatMessage]],
        max_tokens: int = 4096,
        temperature: float = 0.7,
        thinking: bool = False,
        cancel_event: Optional[threading.Event] = None,
    ) -> List[str]:
        """Generate several prompts, with a sequential fallback for backends."""
        return [
            self.generate(
                messages=messages,
                max_tokens=max_tokens,
                temperature=temperature,
                thinking=thinking,
                cancel_event=cancel_event,
            )
            for messages in messages_batch
        ]

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
    def count_tokens(self, text: str) -> int:
        ...

    @abstractmethod
    def cleanup(self):
        ...
