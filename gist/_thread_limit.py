"""Limit CPU thread usage to keep macOS responsive during inference.

Sets BLAS / scipy / numpy thread env vars to ~50% of available cores.
Must be imported before any numerical library (numpy, scipy, mlx, ctranslate2).
Respects existing env vars if the user has already set them.
"""
import os

_MAX_THREADS = max(1, (os.cpu_count() or 4) // 2)

os.environ.setdefault("OMP_NUM_THREADS", str(_MAX_THREADS))
os.environ.setdefault("MKL_NUM_THREADS", str(_MAX_THREADS))
os.environ.setdefault("VECLIB_MAXIMUM_THREADS", str(_MAX_THREADS))
os.environ.setdefault("NUMEXPR_NUM_THREADS", str(_MAX_THREADS))
