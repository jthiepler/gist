"""Compatibility wrappers for callers that imported individual formats."""
from __future__ import annotations

from typing import List

from ..llm.base import ChatMessage
from .base import ClinicalFormat
from .defaults import build_messages, load_templates


class _LegacyFormat(ClinicalFormat):
    template_name: str

    @property
    def name(self) -> str:
        return self.template_name

    @property
    def description(self) -> str:
        return load_templates()[self.template_name]["description"]

    def build_messages(self, transcript: str) -> List[ChatMessage]:
        return build_messages(load_templates()[self.template_name], transcript)


class SOAPFormat(_LegacyFormat):
    template_name = "soap"


class CBTFormat(_LegacyFormat):
    template_name = "cbt"


class IntakeFormat(_LegacyFormat):
    template_name = "intake"
