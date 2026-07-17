"""SQLite-backed, per-document evidence extraction cache."""
from __future__ import annotations

import hashlib
import json
import logging
import os
import sqlite3
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable

from .sources import render_unit
from .types import (
    EvidenceType,
    NoteGenerationSource,
    ParsedEvidence,
    SourceDocument,
    TranscriptBlock,
)


log = logging.getLogger(__name__)
GIST_DATABASE_PATH_ENV = "GIST_DATABASE_PATH"


@dataclass(frozen=True)
class CachedDocumentEvidence:
    extracted: tuple[tuple[TranscriptBlock, tuple[ParsedEvidence, ...]], ...]
    retry_count: int


def build_document_evidence_fingerprint(
    source: NoteGenerationSource,
    model_identity: str,
    cache_version: str,
) -> str:
    digest = hashlib.sha256()
    for value in (
        cache_version,
        model_identity,
        source.id,
        source.kind,
        source.origin,
        source.title,
        source.text,
    ):
        encoded = value.encode("utf-8")
        digest.update(len(encoded).to_bytes(8, "big"))
        digest.update(encoded)
    return digest.hexdigest()


def _connect() -> sqlite3.Connection | None:
    configured_path = os.environ.get(GIST_DATABASE_PATH_ENV)
    if not configured_path:
        return None
    path = Path(configured_path)
    if not path.is_file():
        return None
    connection = sqlite3.connect(path, timeout=5)
    connection.execute("PRAGMA foreign_keys = ON")
    connection.execute("PRAGMA busy_timeout = 5000")
    return connection


def _block_from_payload(
    document: SourceDocument,
    payload: dict[str, Any],
) -> tuple[TranscriptBlock, tuple[ParsedEvidence, ...]]:
    ordinal = payload.get("ordinal")
    unit_ordinals = payload.get("unit_ordinals")
    records_payload = payload.get("records")
    if not isinstance(ordinal, int) or ordinal < 1:
        raise ValueError("Cached block ordinal is invalid.")
    if not isinstance(unit_ordinals, list) or not unit_ordinals:
        raise ValueError("Cached block units are invalid.")
    if not all(isinstance(value, int) for value in unit_ordinals):
        raise ValueError("Cached block units are invalid.")
    if not isinstance(records_payload, list):
        raise ValueError("Cached evidence records are invalid.")

    unit_by_ordinal = {unit.ordinal: unit for unit in document.units}
    units = tuple(unit_by_ordinal[value] for value in unit_ordinals)
    block = TranscriptBlock(
        document_id=document.document_id,
        source_kind=document.source.kind,
        source_origin=document.source.origin,
        source_title=document.source.title,
        ordinal=ordinal,
        units=units,
        text="\n".join(render_unit(unit) for unit in units),
    )
    unit_ids = tuple(unit.unit_id for unit in units)
    records: list[ParsedEvidence] = []
    for record_payload in records_payload:
        if not isinstance(record_payload, dict):
            raise ValueError("Cached evidence record is invalid.")
        claim = record_payload.get("claim")
        if not isinstance(claim, str) or not claim.strip():
            raise ValueError("Cached evidence claim is invalid.")
        records.append(
            ParsedEvidence(
                unit_ids=unit_ids,
                evidence_type=EvidenceType(record_payload.get("evidence_type")),
                claim=claim,
            )
        )
    return block, tuple(records)


def load_document_evidence(
    document: SourceDocument,
    model_identity: str,
    cache_version: str,
) -> CachedDocumentEvidence | None:
    expected_fingerprint = build_document_evidence_fingerprint(
        document.source,
        model_identity,
        cache_version,
    )
    try:
        connection = _connect()
        if connection is None:
            return None
        try:
            row = connection.execute(
                """SELECT source_fingerprint, model_identity, pipeline_version,
                          payload_json, retry_count
                   FROM evidence_ledger_cache
                   WHERE source_id = ?1""",
                (document.source.id,),
            ).fetchone()
        finally:
            connection.close()
        if row is None:
            return None
        fingerprint, stored_model, stored_version, payload_json, retry_count = row
        if (
            fingerprint != expected_fingerprint
            or stored_model != model_identity
            or stored_version != cache_version
        ):
            return None
        if not isinstance(retry_count, int) or retry_count < 0:
            raise ValueError("Cached retry count is invalid.")
        blocks_payload = json.loads(payload_json)
        if not isinstance(blocks_payload, list) or not blocks_payload:
            raise ValueError("Cached blocks are invalid.")
        extracted = tuple(
            _block_from_payload(document, block_payload)
            for block_payload in blocks_payload
            if isinstance(block_payload, dict)
        )
        if len(extracted) != len(blocks_payload):
            raise ValueError("Cached blocks are invalid.")
        return CachedDocumentEvidence(extracted=extracted, retry_count=retry_count)
    except (OSError, sqlite3.Error, KeyError, TypeError, ValueError):
        # Cache failures must never prevent note generation or place clinical
        # data in logs. A later successful extraction replaces invalid rows.
        log.warning("event=evidence_document_cache_read_failed")
        return None


def save_document_evidence(
    document: SourceDocument,
    extracted: Iterable[tuple[TranscriptBlock, tuple[ParsedEvidence, ...]]],
    retry_count: int,
    model_identity: str,
    cache_version: str,
) -> bool:
    blocks_payload = [
        {
            "ordinal": block.ordinal,
            "unit_ordinals": [unit.ordinal for unit in block.units],
            "records": [
                {
                    "evidence_type": record.evidence_type.value,
                    "claim": record.claim,
                }
                for record in records
            ],
        }
        for block, records in extracted
    ]
    try:
        connection = _connect()
        if connection is None:
            return False
        try:
            connection.execute(
                """INSERT INTO evidence_ledger_cache (
                       source_id, source_fingerprint, model_identity,
                       pipeline_version, payload_json, retry_count, updated_at
                   ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                   ON CONFLICT(source_id) DO UPDATE SET
                       source_fingerprint = excluded.source_fingerprint,
                       model_identity = excluded.model_identity,
                       pipeline_version = excluded.pipeline_version,
                       payload_json = excluded.payload_json,
                       retry_count = excluded.retry_count,
                       updated_at = excluded.updated_at""",
                (
                    document.source.id,
                    build_document_evidence_fingerprint(
                        document.source,
                        model_identity,
                        cache_version,
                    ),
                    model_identity,
                    cache_version,
                    json.dumps(blocks_payload, ensure_ascii=False, separators=(",", ":")),
                    retry_count,
                    datetime.now(timezone.utc).isoformat(),
                ),
            )
            connection.commit()
        finally:
            connection.close()
        return True
    except sqlite3.IntegrityError:
        # Direct CLI/legacy sources are not rows in session_inputs and are
        # intentionally excluded from the durable application cache.
        return False
    except (OSError, sqlite3.Error):
        log.warning("event=evidence_document_cache_write_failed")
        return False
