"""MLX backend for macOS Apple Silicon."""
from __future__ import annotations

import copy
import logging
import threading
from dataclasses import dataclass
from typing import Any, List, Optional, Tuple

import mlx.core as mx
from mlx_lm import load, stream_generate
from mlx_lm.generate import BatchGenerator, generate_step
from mlx_lm.models.cache import make_prompt_cache

from .base import ChatMessage, GenerationIncompleteError, LLMBackend

log = logging.getLogger(__name__)

_CACHE_BOUNDARY_MARKER = "\ue000GIST_CACHE_BOUNDARY\ue001"


@dataclass
class _PromptCacheEntry:
    prefix_tokens: Tuple[int, ...]
    prompt_cache: List[Any]


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
        self._prompt_cache_entry: Optional[_PromptCacheEntry] = None

    def load(self, model_path: str, revision: Optional[str] = None):
        self._prompt_cache_entry = None
        log.info("event=mlx_model_load_started revision=%s", revision or "default")
        self.model, self.tokenizer = load(model_path, revision=revision)
        log.info("event=mlx_model_loaded revision=%s", revision or "default")

    def _render_cacheable_prompt(
        self,
        messages: List[ChatMessage],
        thinking: bool,
    ) -> Optional[Tuple[Tuple[int, ...], List[int]]]:
        """Render one cache-marked message into exact prefix and suffix tokens."""
        boundaries = [
            (index, message.cache_prefix_length)
            for index, message in enumerate(messages)
            if message.cache_prefix_length is not None
        ]
        if not boundaries:
            return None
        if len(boundaries) != 1:
            raise ValueError("Exactly one prompt cache boundary is supported.")

        message_index, boundary = boundaries[0]
        assert boundary is not None
        content = messages[message_index].content
        if not 0 < boundary < len(content):
            raise ValueError("Prompt cache boundary must fall inside the message content.")

        marker = _CACHE_BOUNDARY_MARKER
        while any(marker in message.content for message in messages):
            marker += "_"

        rendered_messages = []
        for index, message in enumerate(messages):
            rendered_content = message.content
            if index == message_index:
                rendered_content = content[:boundary] + marker + content[boundary:]
            rendered_messages.append({"role": message.role, "content": rendered_content})

        rendered = self.tokenizer.apply_chat_template(
            rendered_messages,
            add_generation_prompt=True,
            enable_thinking=thinking,
            tokenize=False,
        )
        if not isinstance(rendered, str) or rendered.count(marker) != 1:
            raise RuntimeError("The model chat template could not preserve the prompt cache boundary.")

        prefix_text, suffix_text = rendered.split(marker, 1)
        full_text = prefix_text + suffix_text
        prefix_tokens: Tuple[int, ...]
        suffix_tokens: List[int]

        # Fast Hugging Face tokenizers expose character offsets. Use them to
        # split the normally tokenized full prompt without changing tokenization
        # at the cache boundary. The fallback supports tokenizer implementations
        # that do not provide offsets.
        try:
            raw_tokenizer = getattr(self.tokenizer, "_tokenizer", self.tokenizer)
            encoding = raw_tokenizer(
                full_text,
                add_special_tokens=False,
                return_offsets_mapping=True,
            )
            input_ids = encoding["input_ids"]
            offsets = encoding["offset_mapping"]
            if len(input_ids) != len(offsets):
                raise ValueError("Tokenizer returned mismatched token offsets.")

            boundary_char = len(prefix_text)
            split_index = 0
            for index, (start, end) in enumerate(offsets):
                if start >= boundary_char or end > boundary_char:
                    break
                split_index = index + 1
            if split_index == 0 or split_index == len(input_ids):
                raise ValueError("Tokenizer offsets did not locate the cache boundary.")
            prefix_tokens = tuple(input_ids[:split_index])
            suffix_tokens = list(input_ids[split_index:])
        except (KeyError, TypeError, ValueError, NotImplementedError):
            prefix_tokens = tuple(
                self.tokenizer.encode(prefix_text, add_special_tokens=False)
            )
            suffix_tokens = list(
                self.tokenizer.encode(suffix_text, add_special_tokens=False)
            )
        if not prefix_tokens or not suffix_tokens:
            raise RuntimeError("The model chat template produced an empty cache prefix or suffix.")
        return prefix_tokens, suffix_tokens

    def _prompt_cache_for(
        self,
        prefix_tokens: Tuple[int, ...],
        cancel_event: Optional[threading.Event],
    ) -> Tuple[List[Any], bool]:
        """Return an isolated working cache, prefilling and retaining one base cache."""
        cache_hit = (
            self._prompt_cache_entry is not None
            and self._prompt_cache_entry.prefix_tokens == prefix_tokens
        )
        if not cache_hit:
            if cancel_event and cancel_event.is_set():
                raise InterruptedError("Generation cancelled")

            prompt_cache = make_prompt_cache(self.model)

            def check_cancelled(_processed: int, _total: int) -> None:
                if cancel_event and cancel_event.is_set():
                    raise InterruptedError("Generation cancelled")

            for _ in generate_step(
                mx.array(prefix_tokens),
                self.model,
                max_tokens=0,
                prompt_cache=prompt_cache,
                prompt_progress_callback=check_cancelled,
            ):
                pass
            self._prompt_cache_entry = _PromptCacheEntry(
                prefix_tokens=prefix_tokens,
                prompt_cache=prompt_cache,
            )
            log.info(
                "event=mlx_prompt_cache_miss cached_prefix_tokens=%d",
                len(prefix_tokens),
            )
        else:
            log.info(
                "event=mlx_prompt_cache_hit cached_prefix_tokens=%d",
                len(prefix_tokens),
            )

        assert self._prompt_cache_entry is not None
        return copy.deepcopy(self._prompt_cache_entry.prompt_cache), cache_hit

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

        cacheable_prompt = self._render_cacheable_prompt(messages, thinking)
        prompt_cache = None
        cache_hit = False
        cached_prefix_tokens = 0
        if cacheable_prompt is not None:
            prefix_tokens, prompt = cacheable_prompt
            cached_prefix_tokens = len(prefix_tokens)
            prompt_cache, cache_hit = self._prompt_cache_for(prefix_tokens, cancel_event)
        else:
            prompt = self.tokenizer.apply_chat_template(
                [{"role": m.role, "content": m.content} for m in messages],
                add_generation_prompt=True,
                enable_thinking=thinking,
            )

        log.info(
            "event=mlx_generation_started max_tokens=%d thinking=%s message_count=%d prompt_cache=%s cached_prefix_tokens=%d",
            max_tokens,
            thinking,
            len(messages),
            "hit" if cache_hit else "miss" if cacheable_prompt is not None else "disabled",
            cached_prefix_tokens,
        )

        sampler = _make_sampler(temperature)

        text_parts: list[str] = []
        finish_reason: Optional[str] = None
        if cancel_event and cancel_event.is_set():
            raise InterruptedError("Generation cancelled")
        generation_args = {
            "prompt": prompt,
            "max_tokens": max_tokens,
            "sampler": sampler,
        }
        if prompt_cache is not None:
            generation_args["prompt_cache"] = prompt_cache
        for response in stream_generate(
            self.model,
            self.tokenizer,
            **generation_args,
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
            raise GenerationIncompleteError(
                "The note reached the generation limit and may be incomplete. "
                "Use shorter source material or a more concise template and try again.",
                partial_output=text,
            )
        if finish_reason != "stop":
            raise GenerationIncompleteError(
                "The model stopped without completing the note. Please try again.",
                partial_output=text,
            )
        log.info(
            "event=mlx_generation_completed finish_reason=%s output_chars=%d",
            finish_reason,
            len(text),
        )
        return text

    def generate_batch(
        self,
        messages_batch: List[List[ChatMessage]],
        max_tokens: int = 4096,
        temperature: float = 0.7,
        thinking: bool = False,
        cancel_event: Optional[threading.Event] = None,
    ) -> List[str]:
        """Generate a native MLX batch while preserving per-prompt completion."""
        if self.model is None or self.tokenizer is None:
            raise RuntimeError("Model not loaded. Call load() first.")
        if not messages_batch:
            return []

        prompts: list[list[int]] = []
        prompt_caches: list[Optional[List[Any]]] = []
        cache_hits = 0
        cached_prefix_tokens = 0
        for messages in messages_batch:
            cacheable_prompt = self._render_cacheable_prompt(messages, thinking)
            if cacheable_prompt is not None:
                prefix_tokens, prompt = cacheable_prompt
                prompt_cache, cache_hit = self._prompt_cache_for(prefix_tokens, cancel_event)
                cache_hits += int(cache_hit)
                cached_prefix_tokens = len(prefix_tokens)
                prompts.append(prompt)
                prompt_caches.append(prompt_cache)
            else:
                prompt = self.tokenizer.apply_chat_template(
                    [{"role": message.role, "content": message.content} for message in messages],
                    add_generation_prompt=True,
                    enable_thinking=thinking,
                )
                prompts.append(list(prompt))
                prompt_caches.append(None)

        log.info(
            "event=mlx_batch_generation_started batch_size=%d max_tokens=%d thinking=%s prompt_cache_hits=%d cached_prefix_tokens=%d",
            len(prompts),
            max_tokens,
            thinking,
            cache_hits,
            cached_prefix_tokens,
        )
        if cancel_event and cancel_event.is_set():
            raise InterruptedError("Generation cancelled")

        generator = BatchGenerator(
            self.model,
            stop_tokens=[[token] for token in self.tokenizer.eos_token_ids],
            sampler=_make_sampler(temperature),
            completion_batch_size=len(prompts),
            prefill_batch_size=len(prompts),
        )
        uids = generator.insert(
            prompts,
            [max_tokens] * len(prompts),
            caches=prompt_caches,
        )
        tokens_by_uid: dict[int, list[int]] = {uid: [] for uid in uids}
        finish_reasons: dict[int, str] = {}
        try:
            with generator.stats() as stats:
                while len(finish_reasons) < len(uids):
                    if cancel_event and cancel_event.is_set():
                        raise InterruptedError("Generation cancelled")
                    responses = generator.next_generated()
                    if not responses:
                        break
                    for response in responses:
                        if response.finish_reason != "stop":
                            tokens_by_uid[response.uid].append(response.token)
                        if response.finish_reason is not None:
                            finish_reasons[response.uid] = response.finish_reason
        finally:
            generator.close()

        texts = [self.tokenizer.decode(tokens_by_uid[uid]).strip() for uid in uids]
        for uid, text in zip(uids, texts):
            finish_reason = finish_reasons.get(uid)
            if finish_reason == "length":
                raise GenerationIncompleteError(
                    "An evidence response reached the generation limit and may be incomplete.",
                    partial_output=text,
                )
            if finish_reason != "stop":
                raise GenerationIncompleteError(
                    "An evidence response stopped without completing.",
                    partial_output=text,
                )
        log.info(
            "event=mlx_batch_generation_completed batch_size=%d prompt_tokens=%d generation_tokens=%d peak_memory_gb=%.3f",
            len(texts),
            stats.prompt_tokens,
            stats.generation_tokens,
            stats.peak_memory,
        )
        return texts

    def generate_choice(
        self,
        messages: List[ChatMessage],
        choices: List[str],
        max_tokens: int = 16,
        cancel_event: Optional[threading.Event] = None,
    ) -> str:
        """Generate exactly one allowed value using Outlines constraints."""
        if self.model is None or self.tokenizer is None:
            raise RuntimeError("Model not loaded. Call load() first.")
        if not choices or len(set(choices)) != len(choices):
            raise ValueError("Constrained choices must be non-empty and unique.")

        prompt = self.tokenizer.apply_chat_template(
            [{"role": m.role, "content": m.content} for m in messages],
            add_generation_prompt=True,
            enable_thinking=False,
        )
        from outlines import Generator, from_mlxlm
        from outlines.types import Choice

        generator = Generator(from_mlxlm(self.model, self.tokenizer), Choice(choices))
        logits_processor = generator.logits_processor
        if logits_processor is None:
            raise RuntimeError("Could not create the constrained speaker-label generator.")
        logits_processor.reset()

        log.info(
            "event=mlx_choice_generation_started max_tokens=%d choice_count=%d",
            max_tokens,
            len(choices),
        )
        if cancel_event and cancel_event.is_set():
            raise InterruptedError("Generation cancelled")

        text_parts: list[str] = []
        finish_reason: Optional[str] = None
        for response in stream_generate(
            self.model,
            self.tokenizer,
            prompt=prompt,
            max_tokens=max_tokens,
            sampler=_make_sampler(0.0),
            logits_processors=[logits_processor],
        ):
            if cancel_event and cancel_event.is_set():
                raise InterruptedError("Generation cancelled")
            text_parts.append(response.text)
            if response.finish_reason is not None:
                finish_reason = response.finish_reason

        text = "".join(text_parts).strip()
        if finish_reason == "length":
            raise RuntimeError("Constrained generation reached its token limit.")
        if text not in choices:
            raise RuntimeError("Constrained generation returned an unexpected value.")
        log.info(
            "event=mlx_choice_generation_completed finish_reason=%s",
            finish_reason,
        )
        return text

    def count_tokens(self, text: str) -> int:
        if self.tokenizer is None:
            raise RuntimeError("Model not loaded. Call load() first.")
        return len(self.tokenizer.encode(text, add_special_tokens=False))

    def cleanup(self):
        self._prompt_cache_entry = None
        self.model = None
        self.tokenizer = None
        log.info("event=mlx_model_released")
