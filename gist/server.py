"""JSON-RPC server over stdin/stdout."""
from __future__ import annotations

import json
import logging
import queue
import sys
import threading
from typing import Any, Dict, Optional

from .downloader import download_model, is_model_downloaded, delete_model
from .formats.registry import list_formats
from .models import (
    DEFAULT_LLM,
    LLM_MODELS,
)
from .pipeline import generate_note, transcribe_audio

log = logging.getLogger(__name__)

# Cancellation event — set by "cancel" messages, cleared before each operation
_cancel_event = threading.Event()
# Request queue — stdin reader thread puts messages here, main thread processes
_request_queue: "queue.Queue[Optional[dict]]" = queue.Queue()


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


def _stdin_reader():
    """Background thread: reads stdin, puts messages in queue, handles cancel/exit."""
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
        if msg_type == "cancel":
            _cancel_event.set()
            # Don't queue — it's a control message
        else:
            _request_queue.put(msg)
    # stdin closed — signal main thread to exit
    _request_queue.put(None)


def _handle_transcribe(params: Dict[str, Any]):
    audio_file = params.get("audio_file", "")
    language = params.get("language")
    diarize = params.get("diarize", False)

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
            language=language,
            diarize=diarize,
            progress_callback=progress,
            cancel_event=_cancel_event,
        )
        _send({
            "type": "result",
            "transcript": result.text,
            "segments": [
                {"start": s.start, "end": s.end, "text": s.text, "speaker": s.speaker}
                for s in result.segments
            ],
            "duration": result.duration,
            "language": result.language,
        })
    except InterruptedError:
        _send({"type": "error", "message": "Transcription cancelled"})
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
    prompt = params.get("prompt")

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
            progress_callback=progress,
            prompt=prompt,
            cancel_event=_cancel_event,
        )
        _send({"type": "result", "note": note, "format": format_name})
    except InterruptedError:
        _send({"type": "error", "message": "Note generation cancelled"})
    except Exception as e:
        log.exception("Note generation failed")
        _send({"type": "error", "message": str(e)})


def _handle_download_model(params: Dict[str, Any]):
    model_name = params.get("model", DEFAULT_LLM)
    kind = params.get("kind", "llm")

    def progress(pct, stage):
        _send_progress(pct, stage)

    try:
        _send_progress(0, "Preparing model download...")
        download_model(model_name, kind=kind, progress_callback=progress, cancel_event=_cancel_event)
        _send({"type": "result", "ok": True, "model": model_name})
    except InterruptedError:
        _send({"type": "error", "message": "Download cancelled"})
    except Exception as e:
        log.exception("Download failed")
        _send({"type": "error", "message": str(e)})


def _handle_delete_model(params: Dict[str, Any]):
    model_name = params.get("model", DEFAULT_LLM)
    kind = params.get("kind", "llm")

    try:
        delete_model(model_name, kind=kind)
        _send({"type": "result", "ok": True, "model": model_name})
    except Exception as e:
        log.exception("Delete failed")
        _send({"type": "error", "message": str(e)})


def run_server():
    log.info("JSON-RPC server started (stdin/stdout)")

    reader = threading.Thread(target=_stdin_reader, daemon=True)
    reader.start()

    while True:
        msg = _request_queue.get()  # blocks until a message arrives
        if msg is None:
            # stdin closed
            break

        msg_type = msg.get("type", "")
        params = msg.get("params", msg)

        # Clear cancel event before each operation
        _cancel_event.clear()

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
                    "description": spec.description,
                    "downloaded": is_model_downloaded(name, "llm"),
                }
                for name, spec in LLM_MODELS.items()
            }
            _send({"type": "result", "llm": llm_info})
        elif msg_type == "list_formats":
            _send({"type": "result", "formats": list_formats()})
        elif msg_type == "transcribe":
            _handle_transcribe(params)
        elif msg_type == "generate_note":
            _handle_generate_note(params)
        elif msg_type == "download_model":
            _handle_download_model(params)
        elif msg_type == "delete_model":
            _handle_delete_model(params)
        else:
            _send({"type": "error", "message": f"Unknown message type: {msg_type}"})

    log.info("JSON-RPC server exiting (stdin closed)")
