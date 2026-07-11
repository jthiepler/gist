"""Clinical format implementation backed by the shared defaults resource."""
from __future__ import annotations

from typing import List

from ..llm.base import ChatMessage
from .base import ClinicalFormat


class TemplateFormat(ClinicalFormat):
    def __init__(self, name: str, description: str, prompt: str):
        self._name = name
        self._description = description
        self._prompt = prompt

    @property
    def name(self) -> str:
        return self._name

    @property
    def description(self) -> str:
        return self._description

    def build_messages(self, transcript: str) -> List[ChatMessage]:
        from .defaults import build_messages

        return build_messages({"prompt": self._prompt}, transcript)
