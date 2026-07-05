"""LLM backend factory."""
from __future__ import annotations

import sys

from .base import LLMBackend


def create_backend(backend_type: str, endpoint: str | None = None) -> LLMBackend:
    if backend_type == "mlx":
        from .mlx_backend import MLXBackend

        return MLXBackend()
    elif backend_type == "openai":
        from .openai_compat_backend import OpenAICompatBackend

        if endpoint:
            return OpenAICompatBackend(endpoint=endpoint)
        return OpenAICompatBackend()
    else:
        raise ValueError(f"Unknown LLM backend: {backend_type}")


def auto_detect_backend() -> str:
    if sys.platform == "darwin":
        try:
            import mlx.core

            return "mlx"
        except ImportError:
            pass
    return "openai"
