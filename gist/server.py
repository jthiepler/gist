"""JSON-RPC server over stdin/stdout."""
from __future__ import annotations

import json
import logging
import sys
from typing import Any, Dict, Optional

from .downloader import download_model
from .formats.registry import get_format, list_formats
from .models import (
    DEFAULT_LLM,
    DEFAULT_TRANSCRIPTION,
    LLM_MODELS,
    TRANSCRIPTION_MODELS,
    resolve_model,
)
from .pipeline import generate_note, transcribe_audio

log = logging.getLogger(__name__)


def _send(obj: Dict[str, Any]):
    line = json.dumps(obj, ensure_ascii=False)
    sys.stdout.write(line + "\n")
    sys.stdout.flush()


def _send_progress(
    percent: int,
    stage: str,
    eta_seconds: Optional[float] = None,
    audio_duration: Optional[float] = None,
):
    msg = {"type": "progress", "percent": percent, "stage": stage}
    if eta_seconds is not None:
        msg["eta_seconds"] = round(eta_seconds, 1)
    if audio_duration is not None and audio_duration > 0:
        msg["audio_duration"] = round(audio_duration, 1)
    _send(msg)


def _handle_transcribe(params: Dict[str, Any]):
    audio_file = params.get("audio_file", "")
    model = params.get("model", DEFAULT_TRANSCRIPTION)
    language = params.get("language")

    # audio_duration is static for the whole transcription — send it once
    # on the first progress tick instead of repeating it on every message.
    duration_sent = False

    def progress(pct, stage, eta_seconds=None, audio_duration=None, **_kw):
        nonlocal duration_sent
        include_duration = audio_duration if not duration_sent else None
        _send_progress(pct, stage, eta_seconds=eta_seconds, audio_duration=include_duration)
        if audio_duration is not None:
            duration_sent = True

    try:
        result = transcribe_audio(
            audio_file,
            model_name=model,
            language=language,
            progress_callback=progress,
        )
        _send({
            "type": "result",
            "transcript": result.text,
            "segments": [
                {"start": s.start, "end": s.end, "text": s.text}
                for s in result.segments
            ],
            "duration": result.duration,
            "language": result.language,
        })
    except Exception as e:
        log.exception("Transcription failed")
        _send({"type": "error", "message": str(e)})


def _handle_generate_note(params: Dict[str, Any]):
    transcript = params.get("transcript", "")
    format_name = params.get("format", "soap")
    llm_model = params.get("model", DEFAULT_LLM)
    backend_type = params.get("backend")
    max_tokens = params.get("max_tokens", 16384)
    thinking = params.get("thinking", True)
    language = params.get("language")

    def progress(pct, stage):
        _send_progress(pct, stage)

    try:
        note = generate_note(
            transcript=transcript,
            format_name=format_name,
            llm_model=llm_model,
            backend_type=backend_type,
            max_tokens=max_tokens,
            thinking=thinking,
            language=language,
            progress_callback=progress,
        )
        _send({"type": "result", "note": note, "format": format_name})
    except Exception as e:
        log.exception("Note generation failed")
        _send({"type": "error", "message": str(e)})


def _handle_download_model(params: Dict[str, Any]):
    model_name = params.get("model", DEFAULT_LLM)
    kind = params.get("kind", "llm")

    def progress(pct, stage):
        _send_progress(pct, stage)

    try:
        _send_progress(0, f"starting download of {model_name}")
        download_model(model_name, kind=kind)
        _send({"type": "result", "ok": True, "model": model_name})
    except Exception as e:
        log.exception("Download failed")
        _send({"type": "error", "message": str(e)})


def run_server():
    log.info("JSON-RPC server started (stdin/stdout)")

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        try:
            msg = json.loads(line)
        except json.JSONDecodeError as e:
            _send({"type": "error", "message": f"Invalid JSON: {e}"})
            continue

        if not isinstance(msg, dict):
            _send({"type": "error", "message": f"Expected JSON object, got {type(msg).__name__}"})
            continue

        msg_type = msg.get("type", "")
        params = msg.get("params", msg)

        if msg_type == "ping":
            _send({"type": "pong"})
        elif msg_type == "exit":
            _send({"type": "ok"})
            break
        elif msg_type == "list_models":
            llm_info = {
                name: {
                    "display": spec.display,
                    "backend": spec.backend,
                    "size_gb": spec.size_gb,
                }
                for name, spec in LLM_MODELS.items()
            }
            tr_info = {
                name: {
                    "display": spec.display,
                    "backend": spec.backend,
                    "size_gb": spec.size_gb,
                }
                for name, spec in TRANSCRIPTION_MODELS.items()
            }
            _send({"type": "result", "llm": llm_info, "transcription": tr_info})
        elif msg_type == "list_formats":
            _send({"type": "result", "formats": list_formats()})
        elif msg_type == "transcribe":
            _handle_transcribe(params)
        elif msg_type == "generate_note":
            _handle_generate_note(params)
        elif msg_type == "download_model":
            _handle_download_model(params)
        else:
            _send({"type": "error", "message": f"Unknown message type: {msg_type}"})

    log.info("JSON-RPC server exiting (stdin closed)")
