"""Developer-only capture of sensitive note-generation pipeline artifacts."""
from __future__ import annotations

import copy
import json
import os
from dataclasses import asdict, dataclass, field, is_dataclass
from datetime import datetime, timezone
from enum import Enum
from pathlib import Path
from typing import Any
from uuid import UUID, uuid4


DIAGNOSTIC_SCHEMA_VERSION = 1
DIAGNOSTICS_DIRECTORY_ENV = "GIST_DIAGNOSTICS_DIR"


def _json_value(value: Any) -> Any:
    if is_dataclass(value) and not isinstance(value, type):
        return _json_value(asdict(value))
    if isinstance(value, Enum):
        return value.value
    if isinstance(value, dict):
        return {str(key): _json_value(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [_json_value(item) for item in value]
    return value


def messages_to_dict(messages: list[Any]) -> list[dict[str, Any]]:
    return [
        {
            "role": message.role,
            "content": message.content,
            "cache_prefix_length": message.cache_prefix_length,
        }
        for message in messages
    ]


@dataclass
class DiagnosticCapture:
    """Accumulates one complete debug run without writing clinical data to logs."""

    session_id: str
    run_id: str = field(default_factory=lambda: str(uuid4()))
    created_at: str = field(
        default_factory=lambda: datetime.now(timezone.utc).isoformat()
    )
    request: dict[str, Any] = field(default_factory=dict)
    stages: dict[str, Any] = field(default_factory=dict)
    extraction_attempts: list[dict[str, Any]] = field(default_factory=list)
    rendering_attempts: list[dict[str, Any]] = field(default_factory=list)
    verification_attempts: list[dict[str, Any]] = field(default_factory=list)
    status: str = "running"
    completed_at: str | None = None
    error: dict[str, str] | None = None

    def __post_init__(self) -> None:
        # Directory traversal is never permitted, even though the Tauri caller
        # already supplies UUID session identifiers.
        self.session_id = str(UUID(self.session_id))

    def set_request(self, value: dict[str, Any]) -> None:
        self.request = _json_value(value)

    def set_stage(self, name: str, value: Any) -> None:
        self.stages[name] = _json_value(value)

    def append_extraction(self, value: dict[str, Any]) -> None:
        self.extraction_attempts.append(_json_value(value))

    def append_rendering(self, value: dict[str, Any]) -> None:
        self.rendering_attempts.append(_json_value(value))

    def append_verification(self, value: dict[str, Any]) -> None:
        self.verification_attempts.append(_json_value(value))

    def evidence_trace(self) -> dict[str, Any]:
        return copy.deepcopy(
            {
                "origin_run_id": self.run_id,
                "normalized_sources": self.stages.get("normalized_sources"),
                "chunking": self.stages.get("chunking"),
                "extraction_attempts": self.extraction_attempts,
                "ledger": self.stages.get("ledger"),
            }
        )

    def reuse_evidence_trace(self, value: dict[str, Any]) -> None:
        trace = copy.deepcopy(value)
        self.stages["reused_evidence_trace"] = {
            "origin_run_id": trace.get("origin_run_id")
        }
        for name in ("normalized_sources", "chunking", "ledger"):
            if trace.get(name) is not None:
                self.stages[name] = trace[name]
        self.extraction_attempts = trace.get("extraction_attempts", [])

    def finish(self, status: str, error: BaseException | None = None) -> None:
        self.status = status
        self.completed_at = datetime.now(timezone.utc).isoformat()
        if error is not None:
            self.error = {
                "type": type(error).__name__,
                "message": str(error),
            }

    def to_dict(self) -> dict[str, Any]:
        return _json_value(
            {
                "schema_version": DIAGNOSTIC_SCHEMA_VERSION,
                "run_id": self.run_id,
                "session_id": self.session_id,
                "created_at": self.created_at,
                "completed_at": self.completed_at,
                "status": self.status,
                "request": self.request,
                "stages": self.stages,
                "extraction_attempts": self.extraction_attempts,
                "rendering_attempts": self.rendering_attempts,
                "verification_attempts": self.verification_attempts,
                "error": self.error,
            }
        )

    def save(self) -> Path | None:
        root = os.environ.get(DIAGNOSTICS_DIRECTORY_ENV)
        if not root:
            return None
        root_directory = Path(root)
        root_directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        os.chmod(root_directory, 0o700)
        session_directory = root_directory / self.session_id
        session_directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        os.chmod(session_directory, 0o700)
        path = session_directory / f"{self.created_at.replace(':', '-')}_{self.run_id}.json"
        temporary = path.with_suffix(".tmp")
        with temporary.open("w", encoding="utf-8") as handle:
            json.dump(self.to_dict(), handle, ensure_ascii=False, indent=2)
            handle.write("\n")
        os.chmod(temporary, 0o600)
        temporary.replace(path)
        return path


def create_diagnostic_capture(session_id: str | None) -> DiagnosticCapture | None:
    if not session_id or not os.environ.get(DIAGNOSTICS_DIRECTORY_ENV):
        return None
    return DiagnosticCapture(session_id=session_id)
