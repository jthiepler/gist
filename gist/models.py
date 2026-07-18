"""Model catalog for note-generation models."""
from __future__ import annotations

from dataclasses import dataclass
from typing import Dict


@dataclass(frozen=True)
class ModelSpec:
    name: str
    display: str
    backend: str
    hf_repo: str
    revision: str
    mtp_hf_repo: str = ""
    mtp_revision: str = ""
    size_gb: float = 0.0
    default: bool = False
    description: str = ""


LLM_MODELS: Dict[str, ModelSpec] = {
    "qwen-3.5-4b": ModelSpec(
        name="qwen-3.5-4b",
        display="Qwen 3.5 4B",
        backend="mlx",
        hf_repo="mlx-community/Qwen3.5-4B-OptiQ-4bit",
        revision="e88dc3c6e4c7eff684895598d14c1b149c67af2d",
        mtp_hf_repo="mlx-community/Qwen3.5-4B-MTP-4bit",
        mtp_revision="ab6f59bc6627196c611ab8851638651078170485",
        size_gb=2.6,
        default=True,
        description="Fast and reliable — 4B params",
    ),
    "qwen-3.5-9b": ModelSpec(
        name="qwen-3.5-9b",
        display="Qwen 3.5 9B",
        backend="mlx",
        hf_repo="mlx-community/Qwen3.5-9B-OptiQ-4bit",
        revision="1f7c283df48075ff4e50c24251b7d29d603bdc02",
        mtp_hf_repo="mlx-community/Qwen3.5-9B-MTP-4bit",
        mtp_revision="222dfd2c23fc9518d7b817e4f8e0cb0571787489",
        size_gb=5.7,
        description="Superior quality, slower — 9B params",
    ),
}

DEFAULT_LLM = "qwen-3.5-4b"
EVIDENCE_LLM = DEFAULT_LLM


def resolve_model(name: str, kind: str) -> ModelSpec:
    if kind == "llm":
        catalog = LLM_MODELS
    else:
        raise ValueError(f"Unknown model kind '{kind}'. Expected 'llm'.")
    spec = catalog.get(name)
    if not spec:
        available = ", ".join(catalog.keys())
        raise KeyError(f"Unknown {kind} model '{name}'. Available: {available}")
    return spec
