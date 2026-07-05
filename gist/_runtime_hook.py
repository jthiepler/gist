"""PyInstaller runtime hook: fix MLX metallib path resolution."""
import os
import sys

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
