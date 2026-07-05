"""Model catalog: supported LLM and transcription models."""
from __future__ import annotations

from dataclasses import dataclass
from typing import Dict, Optional


@dataclass(frozen=True)
class ModelSpec:
    name: str
    display: str
    backend: str
    hf_repo: str
    size_gb: float = 0.0
    default: bool = False


LLM_MODELS: Dict[str, ModelSpec] = {
    "qwen-3.5-4b": ModelSpec(
        name="qwen-3.5-4b",
        display="Qwen 3.5 4B",
        backend="mlx",
        hf_repo="mlx-community/Qwen3.5-4B-OptiQ-4bit",
        size_gb=2.5,
        default=True,
    ),
    "qwen-2.5-7b": ModelSpec(
        name="qwen-2.5-7b",
        display="Qwen 2.5 7B",
        backend="mlx",
        hf_repo="mlx-community/Qwen2.5-7B-Instruct-4bit",
        size_gb=4.0,
    ),
    "llama-3.2-3b": ModelSpec(
        name="llama-3.2-3b",
        display="Llama 3.2 3B",
        backend="mlx",
        hf_repo="mlx-community/Llama-3.2-3B-Instruct-4bit",
        size_gb=1.8,
    ),
    "gemma-3-4b": ModelSpec(
        name="gemma-3-4b",
        display="Gemma 3 4B",
        backend="mlx",
        hf_repo="mlx-community/gemma-3-4b-it-4bit",
        size_gb=2.5,
    ),
}

TRANSCRIPTION_MODELS: Dict[str, ModelSpec] = {
    "whisper-base": ModelSpec(
        name="whisper-base",
        display="Whisper Base",
        backend="whisper",
        hf_repo="Systran/faster-whisper-base",
        size_gb=0.15,
        default=True,
    ),
    "whisper-large-v3": ModelSpec(
        name="whisper-large-v3",
        display="Whisper Large v3",
        backend="whisper",
        hf_repo="Systran/faster-whisper-large-v3",
        size_gb=3.0,
    ),
}

DEFAULT_LLM = "qwen-3.5-4b"
DEFAULT_TRANSCRIPTION = "whisper-base"


def resolve_model(name: str, kind: str) -> ModelSpec:
    catalog = LLM_MODELS if kind == "llm" else TRANSCRIPTION_MODELS
    spec = catalog.get(name)
    if not spec:
        available = ", ".join(catalog.keys())
        raise KeyError(f"Unknown {kind} model '{name}'. Available: {available}")
    return spec
