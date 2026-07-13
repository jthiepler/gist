# -*- mode: python ; coding: utf-8 -*-
from pathlib import Path

import torchcodec
from PyInstaller.utils.hooks import collect_all, collect_submodules


datas = [("gist/formats/defaults.json", "gist/formats")]
binaries = []
hiddenimports = ["torch", "torchaudio", "torchcodec"]


def add_collection(package):
    package_datas, package_binaries, package_hiddenimports = collect_all(package)
    datas.extend(package_datas)
    binaries.extend(package_binaries)
    hiddenimports.extend(package_hiddenimports)


hiddenimports.extend(collect_submodules("mlx_lm.models"))
hiddenimports.extend(collect_submodules("torchcodec"))

for package in (
    "mlx_lm",
    "transformers",
    "huggingface_hub",
    "tokenizers",
    "numpy",
    "mlx",
    "mlx_audio",
    "pyannote.audio",
    "scipy",
    "miniaudio",
    "sentencepiece",
):
    add_collection(package)

torchcodec_dir = Path(torchcodec.__file__).parent
for name in (
    "libtorchcodec_core8.dylib",
    "libtorchcodec_custom_ops8.dylib",
    "libtorchcodec_pybind_ops8.so",
):
    binaries.append((str(torchcodec_dir / name), "torchcodec"))
binaries.append(
    (str(torchcodec_dir / ".dylibs" / "libc++.1.0.dylib"), "torchcodec/.dylibs")
)


a = Analysis(
    ["run_sidecar.py"],
    pathex=["."],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=["gist/_runtime_hook.py"],
    excludes=[
        "torchvision",
        "onnx",
        "onnxruntime",
        "tensorflow",
        "keras",
        "tf2onnx",
        "flax",
        "jax",
        "librosa",
    ],
    noarchive=False,
    optimize=0,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="gist-sidecar",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)
coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=False,
    upx_exclude=[],
    name="gist-sidecar",
)
