"""MLX backend for macOS Apple Silicon."""
from __future__ import annotations

import logging
import threading
from typing import List, Optional

import mlx.core as mx
from mlx_lm import load, stream_generate

from .base import ChatMessage, LLMBackend

log = logging.getLogger(__name__)


def _make_sampler(temperature: float):
    """Create a temperature sampler for mlx_lm."""
    if temperature <= 0:
        return lambda x: mx.argmax(x, axis=-1)

    def sampler(logprobs):
        return mx.random.categorical(logprobs * (1 / temperature))

    return sampler


class MLXBackend(LLMBackend):
    def __init__(self):
        self.model = None
        self.tokenizer = None

    def load(self, model_path: str, revision: Optional[str] = None):
        log.info("event=mlx_model_load_started revision=%s", revision or "default")
        self.model, self.tokenizer = load(model_path, revision=revision)
        log.info("event=mlx_model_loaded revision=%s", revision or "default")

    def generate(
        self,
        messages: List[ChatMessage],
        max_tokens: int = 4096,
        temperature: float = 0.7,
        thinking: bool = False,
        cancel_event: Optional[threading.Event] = None,
    ) -> str:
        if self.model is None or self.tokenizer is None:
            raise RuntimeError("Model not loaded. Call load() first.")

        prompt = self.tokenizer.apply_chat_template(
            [{"role": m.role, "content": m.content} for m in messages],
            add_generation_prompt=True,
            enable_thinking=thinking,
        )

        if isinstance(prompt, list):
            prompt = self.tokenizer.decode(prompt)

        log.info(
            "event=mlx_generation_started max_tokens=%d thinking=%s message_count=%d",
            max_tokens,
            thinking,
            len(messages),
        )

        sampler = _make_sampler(temperature)

        text_parts: list[str] = []
        finish_reason: Optional[str] = None
        if cancel_event and cancel_event.is_set():
            raise InterruptedError("Generation cancelled")
        for response in stream_generate(
            self.model,
            self.tokenizer,
            prompt=prompt,
            max_tokens=max_tokens,
            sampler=sampler,
        ):
            if cancel_event and cancel_event.is_set():
                raise InterruptedError("Generation cancelled")
            text_parts.append(response.text)
            if response.finish_reason is not None:
                finish_reason = response.finish_reason

        text = "".join(text_parts).strip()
        if not text:
            raise RuntimeError("The model returned an empty note. Please try again.")
        if finish_reason == "length":
            raise RuntimeError(
                "The note reached the generation limit and may be incomplete. "
                "Use shorter source material or a more concise template and try again."
            )
        if finish_reason != "stop":
            raise RuntimeError("The model stopped without completing the note. Please try again.")
        log.info(
            "event=mlx_generation_completed finish_reason=%s output_chars=%d",
            finish_reason,
            len(text),
        )
        return text

    def cleanup(self):
        self.model = None
        self.tokenizer = None
        log.info("event=mlx_model_released")
