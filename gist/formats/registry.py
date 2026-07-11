"""Format registry backed by the shared shipped-template resource."""
from __future__ import annotations

from typing import Dict

from .base import ClinicalFormat
from .defaults import load_templates
from .template import TemplateFormat

_registry: Dict[str, TemplateFormat] = {
    name: TemplateFormat(name, template["description"], template["prompt"])
    for name, template in load_templates().items()
}
_ALIASES = {"darp": "dart"}


def register(template: TemplateFormat) -> None:
    _registry[template.name] = template


def get_format(name: str) -> ClinicalFormat:
    canonical_name = _ALIASES.get(name.lower(), name.lower())
    template = _registry.get(canonical_name)
    if not template:
        available = ", ".join(_registry.keys())
        raise KeyError(f"Unknown format '{name}'. Available: {available}")
    return template


def list_formats() -> list[dict[str, str]]:
    return [
        {"name": name, "description": template.description}
        for name, template in _registry.items()
    ]
