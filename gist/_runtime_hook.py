"""PyInstaller runtime hook: fix MLX metallib path resolution + limit CPU threads."""
import os
import sys
import tempfile

# Limit CPU threads before any numerical library imports.
# Inline copy of gist/_thread_limit.py — runtime hooks execute before the
# gist package is fully importable in the bundled environment.
_max_threads = max(1, (os.cpu_count() or 4) // 2)
os.environ.setdefault("OMP_NUM_THREADS", str(_max_threads))
os.environ.setdefault("MKL_NUM_THREADS", str(_max_threads))
os.environ.setdefault("VECLIB_MAXIMUM_THREADS", str(_max_threads))
os.environ.setdefault("NUMEXPR_NUM_THREADS", str(_max_threads))

# Gist promises that clinical processing has no telemetry. pyannote.audio 4
# enables OpenTelemetry metrics by default unless this variable is already set,
# so force it off before PyInstaller imports any bundled pyannote module.
os.environ["PYANNOTE_METRICS_ENABLED"] = "false"

# pyannote pulls in matplotlib through its metrics dependencies. A persistent,
# writable cache prevents its first use from rebuilding the font cache every
# time the packaged sidecar launches.
_mpl_config_dir = os.path.join(tempfile.gettempdir(), "gist-matplotlib")
try:
    os.makedirs(_mpl_config_dir, exist_ok=True)
    os.environ.setdefault("MPLCONFIGDIR", _mpl_config_dir)
except OSError:
    pass

def _fix_mlx_path():
    # In PyInstaller bundle, _internal/mlx/lib/ contains libmlx.dylib and mlx.metallib
    base = getattr(sys, '_MEIPASS', os.path.dirname(os.path.abspath(sys.executable)))
    mlx_lib_dir = os.path.join(base, 'mlx', 'lib')
    
    if not os.path.exists(mlx_lib_dir):
        return
    
    metallib = os.path.join(mlx_lib_dir, 'mlx.metallib')
    libmlx = os.path.join(mlx_lib_dir, 'libmlx.dylib')
    
    if not os.path.exists(metallib) or not os.path.exists(libmlx):
        return
    
    # Pre-load libmlx.dylib with absolute path so dladdr resolves correctly
    import ctypes
    try:
        ctypes.CDLL(libmlx)
    except OSError:
        pass

_fix_mlx_path()
