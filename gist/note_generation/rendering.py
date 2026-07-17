"""Render arbitrary clinical note templates from one generic evidence bundle."""
from __future__ import annotations

import logging
import threading
from typing import Any, Optional

from ..formats.defaults import build_messages
from ..formats.registry import get_format
from ..llm.base import ChatMessage, GenerationIncompleteError
from .diagnostics import DiagnosticCapture, messages_to_dict
from .types import NoteFormatRequest


log = logging.getLogger(__name__)


def _add_concision_constraints(
    messages: list[ChatMessage],
    target_words: int,
    *,
    retry: bool,
) -> list[ChatMessage]:
    retry_rule = (
        "This is a retry because the previous draft did not stop within its generation limit. "
        if retry
        else ""
    )
    constraints = (
        "\n\n<rendering_constraints>\n"
        f"{retry_rule}Complete the entire requested note in no more than {target_words} words. "
        "The evidence ledger is a support pool, not a checklist: do not restate every evidence "
        "record or create one bullet per record. Select, combine, and synthesize the most clinically "
        "salient supported information; omit lower-priority repetition. Preserve attribution, "
        "negation, uncertainty, timing, quantities, and action status. Use the requested headings, "
        "then stop immediately after the completed note.\n"
        "</rendering_constraints>"
    )
    return [
        ChatMessage(
            role=message.role,
            content=(
                message.content + constraints
                if index == len(messages) - 1
                else message.content
            ),
            cache_prefix_length=message.cache_prefix_length,
        )
        for index, message in enumerate(messages)
    ]


def render_note(
    llm: Any,
    evidence_bundle: str,
    format_request: NoteFormatRequest,
    *,
    max_tokens: int,
    thinking: bool,
    cancel_event: Optional[threading.Event] = None,
    diagnostic_capture: DiagnosticCapture | None = None,
) -> str:
    if cancel_event and cancel_event.is_set():
        raise InterruptedError("Note generation cancelled")
    if format_request.prompt:
        base_messages = build_messages({"prompt": format_request.prompt}, evidence_bundle)
    else:
        base_messages = get_format(format_request.name).build_messages(evidence_bundle)
    target_words = max(50, min(600, max_tokens // 3))
    messages = _add_concision_constraints(base_messages, target_words, retry=False)

    def generate_attempt(attempt_kind: str, attempt_messages: list[ChatMessage]) -> str:
        attempt: dict[str, Any] = {
            "format": format_request.name,
            "kind": attempt_kind,
            "input": {
                "messages": messages_to_dict(attempt_messages),
                "max_tokens": max_tokens,
                "temperature": 0.0,
                "thinking": thinking,
            },
        }
        try:
            output = llm.generate(
                messages=attempt_messages,
                max_tokens=max_tokens,
                temperature=0.0,
                thinking=thinking,
                cancel_event=cancel_event,
            )
            attempt["output"] = {"raw_model_output": output}
            return output
        except Exception as error:
            attempt["error"] = {
                "type": type(error).__name__,
                "message": str(error),
            }
            partial_output = getattr(error, "partial_output", None)
            if partial_output is not None:
                attempt["output"] = {"partial_model_output": partial_output}
            raise
        finally:
            if diagnostic_capture:
                diagnostic_capture.append_rendering(attempt)

    try:
        return generate_attempt("initial", messages)
    except GenerationIncompleteError:
        if cancel_event and cancel_event.is_set():
            raise InterruptedError("Note generation cancelled")
        retry_words = max(40, target_words // 2)
        log.warning(
            "event=note_rendering_concise_retry format=%s target_words=%d",
            format_request.name,
            retry_words,
        )
        retry_messages = _add_concision_constraints(
            base_messages,
            retry_words,
            retry=True,
        )
        return generate_attempt("concise_retry", retry_messages)
