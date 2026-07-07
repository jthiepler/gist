"""OpenAI-compatible backend for debugging with LM Studio / Ollama."""
from __future__ import annotations

import json
import logging
import threading
from typing import List, Optional

import urllib.request

from ..config import DEFAULT_OPENAI_ENDPOINT, DEFAULT_OPENAI_MODEL
from .base import ChatMessage, LLMBackend

log = logging.getLogger(__name__)


class OpenAICompatBackend(LLMBackend):
    def __init__(
        self,
        endpoint: str = DEFAULT_OPENAI_ENDPOINT,
        model: str = DEFAULT_OPENAI_MODEL,
    ):
        self.endpoint = endpoint.rstrip("/")
        self.model = model

    def load(self, model_path: str):
        self.model = model_path or DEFAULT_OPENAI_MODEL

    def generate(
        self,
        messages: List[ChatMessage],
        max_tokens: int = 4096,
        temperature: float = 0.7,
        thinking: bool = False,
        cancel_event: Optional[threading.Event] = None,
    ) -> str:
        body = {
            "model": self.model,
            "messages": [{"role": m.role, "content": m.content} for m in messages],
            "max_tokens": max_tokens,
            "temperature": temperature,
            "stream": False,
        }

        req = urllib.request.Request(
            f"{self.endpoint}/chat/completions",
            data=json.dumps(body).encode(),
            headers={"Content-Type": "application/json"},
            method="POST",
        )

        log.info("Calling %s with model %s", self.endpoint, self.model)

        with urllib.request.urlopen(req, timeout=300) as resp:
            result = json.loads(resp.read().decode())

        return result["choices"][0]["message"]["content"].strip()

    def cleanup(self):
        pass
