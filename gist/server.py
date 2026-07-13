"""JSON-RPC server over stdin/stdout."""
from __future__ import annotations

import json
import logging
import queue
import sys
import threading
import time
from typing import Any, Dict, Optional

from .downloader import delete_model, download_model, is_model_downloaded
from .formats.registry import list_formats
from .models import (
    DEFAULT_LLM,
    LLM_MODELS,
)
from .pipeline import generate_note, release_cached_llm, transcribe_audio

log = logging.getLogger(__name__)

# Cancellation event — set by "cancel" messages, cleared before each operation
_cancel_event = threading.Event()
# Request queue — stdin reader thread puts messages here, main thread processes
_request_queue: "queue.Queue[Optional[dict]]" = queue.Queue()
_active_request_id: Optional[str] = None


def _log_event(operation: str, event: str, started_at: Optional[float] = None, **fields: Any) -> None:
    """Write privacy-safe structured diagnostics without source content or paths."""
    payload: Dict[str, Any] = {"operation": operation, "event": event}
    if started_at is not None:
        payload["elapsed_ms"] = round((time.monotonic() - started_at) * 1000)
    payload.update(fields)
    log.info("sidecar_event=%s", json.dumps(payload, sort_keys=True))


def _params_for(msg: Dict[str, Any], msg_type: str) -> Dict[str, Any]:
    params = msg.get("params", msg)
    if not isinstance(params, dict):
        raise ValueError("Request params must be a JSON object")

    def optional_string(name: str) -> None:
        value = params.get(name)
        if value is not None and not isinstance(value, str):
            raise ValueError(f"'{name}' must be a string")

    if msg_type == "transcribe":
        if not isinstance(params.get("audio_file"), str) or not params["audio_file"].strip():
            raise ValueError("'audio_file' must be a non-empty string")
        if not isinstance(params.get("diarize", False), bool):
            raise ValueError("'diarize' must be true or false")
    elif msg_type == "generate_note":
        if not isinstance(params.get("transcript"), str) or not params["transcript"].strip():
            raise ValueError("'transcript' must be a non-empty string")
        for name in ("format", "model", "prompt"):
            optional_string(name)
        max_tokens = params.get("max_tokens", 4096)
        if isinstance(max_tokens, bool) or not isinstance(max_tokens, int) or not 1 <= max_tokens <= 4096:
            raise ValueError("'max_tokens' must be an integer between 1 and 4096")
        if not isinstance(params.get("thinking", False), bool):
            raise ValueError("'thinking' must be true or false")
    elif msg_type in {"download_model", "delete_model"}:
        optional_string("model")
        if params.get("kind", "llm") != "llm":
            raise ValueError("'kind' must be 'llm'")
    return params


def _user_facing_error(operation: str, error: Exception) -> str:
    """Return helpful, non-technical errors while preserving diagnostics in logs."""
    detail = str(error).lower()
    if isinstance(error, PermissionError) or "permission denied" in detail:
        return "Gist cannot access this file or model. Check its permissions, then try again."
    if any(term in detail for term in ("no space", "disk full", "not enough space", "errno 28")):
        return "Your Mac is out of storage. Free some space, then try again."
    if any(term in detail for term in ("network", "connection", "timeout", "timed out", "http", "download")):
        return "Gist could not reach the model download service. Check your internet connection and try again."
    if any(term in detail for term in ("model", "mlx", "huggingface", "weights", "tokenizer")):
        return "The selected model could not be prepared. Check that it is fully downloaded, then try again."
    if operation == "transcription":
        return "Gist could not transcribe this audio. Confirm the recording can be played, then try again."
    if operation == "note_generation":
        return "Gist could not generate this note. Try again, or choose a different downloaded model."
    if operation == "model_download":
        return "Gist could not download this model. Check your internet connection and available storage, then try again."
    return "Gist could not complete that request. Please try again."


def _send(obj: Dict[str, Any]):
    if _active_request_id and obj.get("type") in {"progress", "result", "error", "pong"}:
        obj = {**obj, "request_id": _active_request_id}
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
    diarize = params.get("diarize", False)
    started_at = time.monotonic()
    _log_event("transcription", "started", diarize=diarize)

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
        })
        _log_event("transcription", "completed", started_at, diarize=diarize, segments=len(result.segments))
    except InterruptedError:
        _log_event("transcription", "cancelled", started_at, diarize=diarize)
        _send({"type": "error", "message": "Transcription cancelled"})
    except Exception as e:
        _log_event("transcription", "failed", started_at, error_type=type(e).__name__)
        log.exception("Transcription failed")
        _send({"type": "error", "message": _user_facing_error("transcription", e)})


def _handle_generate_note(params: Dict[str, Any]):
    transcript = params.get("transcript", "")
    format_name = params.get("format", "soap")
    llm_model = params.get("model", DEFAULT_LLM)
    max_tokens = params.get("max_tokens", 4096)
    thinking = params.get("thinking", False)
    prompt = params.get("prompt")
    started_at = time.monotonic()
    _log_event("note_generation", "started", model=llm_model, format=format_name, thinking=thinking)

    def progress(pct, stage):
        _send_progress(pct, stage)

    try:
        note = generate_note(
            transcript=transcript,
            format_name=format_name,
            llm_model=llm_model,
            max_tokens=max_tokens,
            thinking=thinking,
            progress_callback=progress,
            prompt=prompt,
            cancel_event=_cancel_event,
        )
        _send({"type": "result", "note": note, "format": format_name})
        _log_event("note_generation", "completed", started_at, model=llm_model, format=format_name)
    except InterruptedError:
        _log_event("note_generation", "cancelled", started_at, model=llm_model, format=format_name)
        _send({"type": "error", "message": "Note generation cancelled"})
    except Exception as e:
        _log_event("note_generation", "failed", started_at, model=llm_model, format=format_name, error_type=type(e).__name__)
        log.exception("Note generation failed")
        _send({"type": "error", "message": _user_facing_error("note_generation", e)})


def _handle_download_model(params: Dict[str, Any]):
    model_name = params.get("model", DEFAULT_LLM)
    kind = params.get("kind", "llm")
    started_at = time.monotonic()
    _log_event("model_download", "started", model=model_name)

    def progress(pct, stage):
        _send_progress(pct, stage)

    try:
        _send_progress(0, "Preparing model download...")
        download_model(model_name, kind=kind, progress_callback=progress, cancel_event=_cancel_event)
        _send({"type": "result", "ok": True, "model": model_name})
        _log_event("model_download", "completed", started_at, model=model_name)
    except InterruptedError:
        _log_event("model_download", "cancelled", started_at, model=model_name)
        _send({"type": "error", "message": "Download cancelled"})
    except Exception as e:
        _log_event("model_download", "failed", started_at, model=model_name, error_type=type(e).__name__)
        log.exception("Download failed")
        _send({"type": "error", "message": _user_facing_error("model_download", e)})


def _handle_delete_model(params: Dict[str, Any]):
    model_name = params.get("model", DEFAULT_LLM)
    kind = params.get("kind", "llm")

    try:
        release_cached_llm(model_name)
        delete_model(model_name, kind=kind)
        _send({"type": "result", "ok": True, "model": model_name})
    except Exception as e:
        log.exception("Delete failed")
        _send({"type": "error", "message": _user_facing_error("model_download", e)})


def run_server():
    global _active_request_id
    logging.basicConfig(
        level=logging.INFO,
        stream=sys.stderr,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )
    log.info("JSON-RPC server started")

    reader = threading.Thread(target=_stdin_reader, daemon=True)
    reader.start()

    while True:
        msg = _request_queue.get()  # blocks until a message arrives
        if msg is None:
            # stdin closed
            break

        msg_type = msg.get("type", "")
        _active_request_id = msg.get("request_id")

        # Clear cancel event before each operation
        _cancel_event.clear()

        try:
            params = _params_for(msg, msg_type)
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
        except Exception as e:
            log.warning("Rejected sidecar request: %s", e)
            _send({"type": "error", "message": f"Invalid request: {e}"})

        _active_request_id = None

    log.info("JSON-RPC server exiting (stdin closed)")
