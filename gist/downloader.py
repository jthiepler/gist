"""Model downloader."""
from __future__ import annotations

import logging
import json
import shutil
import threading
from pathlib import Path
from typing import Callable, Optional

from huggingface_hub import HfApi, hf_hub_download, snapshot_download
from huggingface_hub.constants import HF_HUB_CACHE
from tqdm import tqdm

from .models import resolve_model

log = logging.getLogger(__name__)


def _model_cache_dir(model_name: str, kind: str = "llm", cache_dir: Optional[Path] = None) -> Path:
    """Return the repository cache path even when its snapshot is incomplete."""
    spec = resolve_model(model_name, kind)
    cache_root = Path(cache_dir) if cache_dir else Path(HF_HUB_CACHE)
    return cache_root / f"models--{spec.hf_repo.replace('/', '--')}"


def _is_usable_mlx_snapshot(path: Path) -> bool:
    """Check the minimum local files needed to load an MLX language model."""
    if not path.is_dir() or not (path / "config.json").is_file():
        return False
    if not any((path / name).is_file() for name in ("tokenizer.json", "tokenizer.model")):
        return False

    index_path = path / "model.safetensors.index.json"
    if index_path.is_file():
        try:
            weight_map = json.loads(index_path.read_text(encoding="utf-8")).get("weight_map", {})
            shards = set(weight_map.values())
        except (OSError, ValueError, AttributeError):
            return False
        return bool(shards) and all((path / shard).is_file() for shard in shards)

    return any(path.glob("*.safetensors"))


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
                progress_callback(pct, f"Downloading model files ({file_idx}/{total_files})...")
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

    try:
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

            progress_callback(100, "Model download complete")
        else:
            snapshot_download(
                repo_id=spec.hf_repo,
                local_files_only=False,
                cache_dir=cache_dir_str,
            )
    except InterruptedError:
        # A cancelled repository must not look installed or retain gigabytes of
        # partial weights. This also removes Hugging Face's `.incomplete` blobs.
        delete_model(model_name, kind=kind, cache_dir=cache_dir)
        raise

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
        path = Path(snapshot_download(repo_id=spec.hf_repo, local_files_only=True))
        return _is_usable_mlx_snapshot(path)
    except Exception:
        return False


def delete_model(model_name: str, kind: str = "llm", cache_dir: Optional[Path] = None) -> None:
    spec = resolve_model(model_name, kind)
    log.info("Deleting %s model '%s' from cache...", kind, spec.name)
    model_cache = _model_cache_dir(model_name, kind=kind, cache_dir=cache_dir)
    if not model_cache.exists():
        log.info("Model '%s' not in cache, nothing to delete", spec.name)
        return
    shutil.rmtree(model_cache)
    if model_cache.exists():
        raise OSError(f"Model cache still exists after deletion: {model_cache}")
    log.info("Deleted cache directory: %s", model_cache)
