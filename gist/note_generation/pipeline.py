"""End-to-end format-neutral evidence-ledger note generation."""
from __future__ import annotations

import hashlib
import logging
import threading
from collections.abc import Callable, Iterable
from typing import Any, Optional

from .chunking import build_blocks
from .diagnostics import DiagnosticCapture
from .evidence_cache import load_document_evidence, save_document_evidence
from .extraction import extract_evidence
from .ledger import build_evidence_bundle, build_ledger
from .rendering import render_note
from .sources import normalize_sources
from .types import (
    FormatFailure,
    EvidenceLedger,
    GenerateNotesResult,
    GeneratedNote,
    NoteFormatRequest,
    NoteGenerationSource,
    ParsedEvidence,
    SourceDocument,
    TranscriptBlock,
    VerificationSummary,
)
from .verification import verify_note


log = logging.getLogger(__name__)
ProgressCallback = Callable[[int, str], None]
_EVIDENCE_CACHE_VERSION = "evidence-free-text-v1"
_cached_evidence_key: str | None = None
_cached_evidence_ledger: EvidenceLedger | None = None
_cached_evidence_trace: dict[str, Any] | None = None


def build_evidence_cache_key(
    sources: Iterable[NoteGenerationSource],
    model_identity: str,
) -> str:
    digest = hashlib.sha256()
    digest.update(_EVIDENCE_CACHE_VERSION.encode("utf-8"))
    digest.update(b"\0")
    digest.update(model_identity.encode("utf-8"))
    for source in sources:
        for value in (source.id, source.kind, source.origin, source.title, source.text):
            encoded = value.encode("utf-8")
            digest.update(len(encoded).to_bytes(8, "big"))
            digest.update(encoded)
    return digest.hexdigest()


def clear_evidence_cache() -> None:
    global _cached_evidence_key, _cached_evidence_ledger, _cached_evidence_trace
    _cached_evidence_key = None
    _cached_evidence_ledger = None
    _cached_evidence_trace = None


def get_cached_evidence_ledger(
    evidence_cache_key: str | None,
    *,
    progress_callback: Optional[ProgressCallback] = None,
    diagnostic_capture: DiagnosticCapture | None = None,
) -> EvidenceLedger | None:
    """Return a reusable ledger without requiring an evidence model to load."""
    diagnostic_trace_available = (
        diagnostic_capture is None or _cached_evidence_trace is not None
    )
    cache_hit = (
        evidence_cache_key is not None
        and evidence_cache_key == _cached_evidence_key
        and _cached_evidence_ledger is not None
        and diagnostic_trace_available
    )
    if not cache_hit:
        return None

    ledger = _cached_evidence_ledger
    if diagnostic_capture and _cached_evidence_trace:
        diagnostic_capture.reuse_evidence_trace(_cached_evidence_trace)
    if diagnostic_capture:
        diagnostic_capture.set_stage(
            "evidence_cache",
            {
                "hit": True,
                "cache_key": evidence_cache_key,
                "evidence_trace_reused": _cached_evidence_trace is not None,
            },
        )
    if progress_callback:
        progress_callback(57, "Reusing extracted encounter evidence...")
    log.info("event=evidence_ledger_cache_hit records=%d", len(ledger.records))
    return ledger


def prepare_evidence_with_backend(
    llm: Any | None,
    sources: Iterable[NoteGenerationSource],
    *,
    evidence_cache_key: str | None = None,
    evidence_model_identity: str | None = None,
    evidence_backend_factory: Callable[[], Any] | None = None,
    progress_callback: Optional[ProgressCallback] = None,
    cancel_event: Optional[threading.Event] = None,
    diagnostic_capture: DiagnosticCapture | None = None,
) -> EvidenceLedger:
    source_list = tuple(sources)
    global _cached_evidence_key, _cached_evidence_ledger, _cached_evidence_trace
    cached = get_cached_evidence_ledger(
        evidence_cache_key,
        progress_callback=progress_callback,
        diagnostic_capture=diagnostic_capture,
    )
    if cached is not None:
        return cached

    if progress_callback:
        progress_callback(0, "Preparing source material...")
    documents = normalize_sources(source_list)
    if diagnostic_capture:
        diagnostic_capture.set_stage(
            "normalized_sources",
            {"input": source_list, "output": documents},
        )
    extracted_by_document: dict[
        str,
        tuple[tuple[TranscriptBlock, tuple[ParsedEvidence, ...]], ...],
    ] = {}
    retries_by_document: dict[str, int] = {}
    missing_documents: list[SourceDocument] = []
    cache_hit_count = 0
    if evidence_model_identity is not None:
        for document in documents:
            cached_document = load_document_evidence(
                document,
                evidence_model_identity,
                _EVIDENCE_CACHE_VERSION,
            )
            if cached_document is None:
                missing_documents.append(document)
                continue
            extracted_by_document[document.document_id] = cached_document.extracted
            retries_by_document[document.document_id] = cached_document.retry_count
            cache_hit_count += 1
    else:
        missing_documents.extend(documents)

    if missing_documents and llm is None:
        if evidence_backend_factory is None:
            raise RuntimeError("Evidence extraction requires a local model backend.")
        llm = evidence_backend_factory()

    missing_blocks = (
        build_blocks(missing_documents, llm.count_tokens)
        if missing_documents and llm is not None
        else ()
    )
    cached_blocks = tuple(
        block
        for extracted in extracted_by_document.values()
        for block, _records in extracted
    )
    blocks = tuple(
        sorted(
            (*cached_blocks, *missing_blocks),
            key=lambda block: (int(block.document_id[1:]), block.ordinal),
        )
    )
    if diagnostic_capture:
        diagnostic_capture.set_stage(
            "chunking",
            {
                "input": {
                    "documents": documents,
                    "target_tokens": 450,
                    "overlap_units": 1,
                },
                "output": blocks,
            },
        )
    log.info(
        "event=evidence_sources_prepared documents=%d units=%d blocks=%d cache_hits=%d cache_misses=%d",
        len(documents),
        sum(len(document.units) for document in documents),
        len(blocks),
        cache_hit_count,
        len(missing_documents),
    )

    def extraction_progress(completed: int, total: int) -> None:
        if progress_callback:
            progress_callback(
                5 + round((completed / max(total, 1)) * 50),
                "Extracting clinical evidence...",
            )

    if missing_blocks:
        extracted_missing, new_retries_by_document = extract_evidence(
            llm,
            missing_blocks,
            cancel_event=cancel_event,
            progress_callback=extraction_progress,
            diagnostic_capture=diagnostic_capture,
        )
        retries_by_document.update(new_retries_by_document)
        for document in missing_documents:
            document_extracted = tuple(
                item
                for item in extracted_missing
                if item[0].document_id == document.document_id
            )
            extracted_by_document[document.document_id] = document_extracted
            if evidence_model_identity is not None:
                save_document_evidence(
                    document,
                    document_extracted,
                    retries_by_document.get(document.document_id, 0),
                    evidence_model_identity,
                    _EVIDENCE_CACHE_VERSION,
                )
    else:
        if progress_callback:
            progress_callback(57, "Reusing extracted encounter evidence...")

    extracted = tuple(
        item
        for document in documents
        for item in extracted_by_document[document.document_id]
    )
    retry_count = sum(retries_by_document.values())
    if progress_callback:
        progress_callback(57, "Organizing encounter evidence...")
    ledger = build_ledger(documents, extracted, retry_count)
    if diagnostic_capture:
        diagnostic_capture.set_stage(
            "ledger",
            {
                "input": {
                    "documents": documents,
                    "parsed_evidence_by_block": extracted,
                    "retry_count": retry_count,
                },
                "output": ledger,
            },
        )
    if evidence_cache_key is not None:
        _cached_evidence_key = evidence_cache_key
        _cached_evidence_ledger = ledger
        _cached_evidence_trace = (
            diagnostic_capture.evidence_trace() if diagnostic_capture else None
        )
    if diagnostic_capture:
        diagnostic_capture.set_stage(
            "evidence_cache",
            {
                "hit": False,
                "cache_key": evidence_cache_key,
                "cached": evidence_cache_key is not None,
                "persistent_document_hits": cache_hit_count,
                "persistent_document_misses": len(missing_documents),
            },
        )
    log.info(
        "event=evidence_ledger_cache_miss records=%d cached=%s document_cache_hits=%d document_cache_misses=%d",
        len(ledger.records),
        evidence_cache_key is not None,
        cache_hit_count,
        len(missing_documents),
    )
    return ledger


def generate_notes_from_ledger_with_backend(
    llm: Any,
    ledger: EvidenceLedger,
    formats: Iterable[NoteFormatRequest],
    *,
    max_tokens: int = 4096,
    thinking: bool = False,
    verification_mode: str = "off",
    progress_callback: Optional[ProgressCallback] = None,
    cancel_event: Optional[threading.Event] = None,
    diagnostic_capture: DiagnosticCapture | None = None,
) -> GenerateNotesResult:
    format_list = tuple(formats)
    if not format_list:
        raise ValueError("At least one note format is required.")

    evidence_bundle, evidence_tokens = build_evidence_bundle(
        ledger,
        llm.count_tokens,
    )
    log.info(
        "event=evidence_ledger_ready records=%d retries=%d evidence_tokens=%d",
        len(ledger.records),
        ledger.retry_count,
        evidence_tokens,
    )
    if diagnostic_capture:
        diagnostic_capture.set_stage(
            "evidence_bundle",
            {
                "input": {
                    "ledger": ledger,
                },
                "output": {
                    "bundle": evidence_bundle,
                    "tokens": evidence_tokens,
                    "complete_ledger": True,
                },
            },
        )

    notes: list[GeneratedNote] = []
    failures: list[FormatFailure] = []
    total_formats = len(format_list)
    for index, format_request in enumerate(format_list):
        if cancel_event and cancel_event.is_set():
            raise InterruptedError("Note generation cancelled")
        render_percent = 60 + round((index / total_formats) * 38)
        if progress_callback:
            progress_callback(render_percent, f"Drafting {format_request.name.upper()} note...")
        try:
            draft = render_note(
                llm,
                evidence_bundle,
                format_request,
                max_tokens=max_tokens,
                thinking=thinking,
                cancel_event=cancel_event,
                diagnostic_capture=diagnostic_capture,
            )
            if progress_callback and verification_mode != "off":
                progress_callback(
                    60 + round(((index + 0.6) / total_formats) * 38),
                    f"Checking {format_request.name.upper()} source support...",
                )
            try:
                verified_note, verification = verify_note(
                    llm,
                    draft,
                    ledger,
                    mode=verification_mode,
                    cancel_event=cancel_event,
                    diagnostic_capture=diagnostic_capture,
                    format_name=format_request.name,
                )
            except InterruptedError:
                raise
            except Exception as verification_error:
                if verification_mode != "shadow":
                    raise
                log.warning(
                    "event=note_verification_shadow_failed format=%s error_type=%s",
                    format_request.name,
                    type(verification_error).__name__,
                )
                verified_note = draft
                verification = VerificationSummary()
            log.info(
                "event=note_verification_completed format=%s mode=%s claims=%d supported=%d partly_supported=%d unsupported=%d contradicted=%d",
                format_request.name,
                verification_mode,
                verification.claims_checked,
                verification.supported,
                verification.partly_supported,
                verification.unsupported,
                verification.contradicted,
            )
            notes.append(
                GeneratedNote(
                    format=format_request.name,
                    note=verified_note,
                    verification=verification,
                )
            )
        except InterruptedError:
            raise
        except Exception as error:
            log.warning(
                "event=note_format_generation_failed format=%s error_type=%s",
                format_request.name,
                type(error).__name__,
            )
            failures.append(
                FormatFailure(
                    format=format_request.name,
                    message="The note could not be generated from the available evidence.",
                )
            )

    if progress_callback:
        progress_callback(100, "Finalizing notes...")
    result = GenerateNotesResult(
        notes=tuple(notes),
        failures=tuple(failures),
        ledger_stats={
            "documents": ledger.document_count,
            "units": ledger.unit_count,
            "blocks": ledger.block_count,
            "evidence_records": len(ledger.records),
            "retry_count": ledger.retry_count,
            "evidence_tokens": evidence_tokens,
        },
    )
    if diagnostic_capture:
        diagnostic_capture.set_stage(
            "result",
            {
                "output": result,
            },
        )
    return result


def generate_notes_with_backend(
    llm: Any,
    sources: Iterable[NoteGenerationSource],
    formats: Iterable[NoteFormatRequest],
    *,
    max_tokens: int = 4096,
    thinking: bool = False,
    verification_mode: str = "off",
    evidence_cache_key: str | None = None,
    progress_callback: Optional[ProgressCallback] = None,
    cancel_event: Optional[threading.Event] = None,
    diagnostic_capture: DiagnosticCapture | None = None,
) -> GenerateNotesResult:
    """Compatibility wrapper for callers that use one model for every stage."""
    ledger = prepare_evidence_with_backend(
        llm,
        sources,
        evidence_cache_key=evidence_cache_key,
        progress_callback=progress_callback,
        cancel_event=cancel_event,
        diagnostic_capture=diagnostic_capture,
    )
    return generate_notes_from_ledger_with_backend(
        llm,
        ledger,
        formats,
        max_tokens=max_tokens,
        thinking=thinking,
        verification_mode=verification_mode,
        progress_callback=progress_callback,
        cancel_event=cancel_event,
        diagnostic_capture=diagnostic_capture,
    )
