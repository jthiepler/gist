"""Model downloader."""
from __future__ import annotations

import logging
import shutil
import threading
from pathlib import Path
from typing import Callable, Optional

from huggingface_hub import HfApi, hf_hub_download, snapshot_download
from tqdm import tqdm

from .models import resolve_model

log = logging.getLogger(__name__)


def _make_progress_tqdm(
    base: int,
    filename: str,
    file_idx: int,
    total_files: int,
    total_size: int,
    progress_callback: Callable[[int, str], None],
    cancel_event=None,
):
    """Create a tqdm subclass that reports overall download progress."""

    class _ProgressTqdm(tqdm):
        def update(self, n=1):
            if cancel_event and cancel_event.is_set():
                raise InterruptedError("Download cancelled")
            result = super().update(n)
            if total_size > 0:
                done = base + self.n
                pct = min(int(done / total_size * 100), 100)
                progress_callback(pct, f"downloading {filename} ({file_idx}/{total_files})")
            return result

    return _ProgressTqdm


def download_model(
    model_name: str,
    kind: str = "llm",
    cache_dir: Optional[Path] = None,
    progress_callback: Optional[Callable[[int, str], None]] = None,
    cancel_event: Optional[threading.Event] = None,
) -> Path:
    spec = resolve_model(model_name, kind)
    log.info("Downloading %s model '%s' from %s...", kind, spec.name, spec.hf_repo)

    cache_dir_str = str(cache_dir) if cache_dir else None

    if progress_callback:
        # Download files individually so we can report byte-level progress
        api = HfApi()
        info = api.repo_info(repo_id=spec.hf_repo, files_metadata=True)
        siblings = info.siblings
        total_files = len(siblings)
        file_sizes = {s.rfilename: (s.size or 0) for s in siblings}
        total_size = sum(file_sizes.values()) or 1
        accumulated = 0

        for i, sibling in enumerate(siblings):
            if cancel_event and cancel_event.is_set():
                raise InterruptedError("Download cancelled")
            filename = sibling.rfilename
            file_size = file_sizes[filename]
            base = accumulated

            tqdm_cls = _make_progress_tqdm(
                base, filename, i + 1, total_files, total_size, progress_callback,
                cancel_event=cancel_event,
            )

            hf_hub_download(
                repo_id=spec.hf_repo,
                filename=filename,
                cache_dir=cache_dir_str,
                tqdm_class=tqdm_cls,
            )
            accumulated += file_size

        progress_callback(100, "download complete")
    else:
        snapshot_download(
            repo_id=spec.hf_repo,
            local_files_only=False,
            cache_dir=cache_dir_str,
        )

    # Return the local snapshot path
    path = snapshot_download(
        repo_id=spec.hf_repo,
        local_files_only=True,
        cache_dir=cache_dir_str,
    )
    log.info("Downloaded %s to %s", spec.name, path)
    return Path(path)


def is_model_downloaded(model_name: str, kind: str = "llm") -> bool:
    spec = resolve_model(model_name, kind)
    try:
        snapshot_download(repo_id=spec.hf_repo, local_files_only=True)
        return True
    except Exception:
        return False


def delete_model(model_name: str, kind: str = "llm") -> None:
    spec = resolve_model(model_name, kind)
    log.info("Deleting %s model '%s' from cache...", kind, spec.name)
    try:
        path = snapshot_download(repo_id=spec.hf_repo, local_files_only=True)
    except Exception:
        log.info("Model '%s' not in cache, nothing to delete", spec.name)
        return

    p = Path(path)
    for parent in p.parents:
        if parent.name.startswith("models--"):
            shutil.rmtree(parent, ignore_errors=True)
            log.info("Deleted cache directory: %s", parent)
            return
    log.warning("Could not locate cache directory for '%s'", spec.name)
