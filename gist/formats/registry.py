"""Format registry."""
from __future__ import annotations

from typing import Dict, Type

from .base import ClinicalFormat

_registry: Dict[str, Type[ClinicalFormat]] = {}


def register(cls: Type[ClinicalFormat]):
    _registry[cls.name] = cls


def get_format(name: str) -> ClinicalFormat:
    cls = _registry.get(name)
    if not cls:
        available = ", ".join(_registry.keys())
        raise KeyError(f"Unknown format '{name}'. Available: {available}")
    return cls()


def list_formats() -> list[dict[str, str]]:
    return [{"name": name, "description": cls.description} for name, cls in _registry.items()]


from .cbt import CBTFormat
from .intake import IntakeFormat
from .soap import SOAPFormat

register(SOAPFormat)
register(CBTFormat)
register(IntakeFormat)
