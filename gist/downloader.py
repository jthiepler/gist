"""Model downloader."""
from __future__ import annotations

import logging
from pathlib import Path
from typing import Optional

from huggingface_hub import snapshot_download

from .models import TRANSCRIPTION_MODELS, LLM_MODELS, resolve_model

log = logging.getLogger(__name__)


def download_model(
    model_name: str,
    kind: str = "llm",
    cache_dir: Optional[Path] = None,
    progress_callback=None,
) -> Path:
    spec = resolve_model(model_name, kind)
    log.info("Downloading %s model '%s' from %s...", kind, spec.name, spec.hf_repo)

    path = snapshot_download(
        repo_id=spec.hf_repo,
        local_files_only=False,
        cache_dir=str(cache_dir) if cache_dir else None,
    )

    log.info("Downloaded %s to %s", spec.name, path)
    return Path(path)
