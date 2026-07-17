"""Construct and render the format-neutral encounter evidence ledger."""
from __future__ import annotations

from collections.abc import Callable, Iterable

from .types import (
    EvidenceLedger,
    EvidenceRecord,
    EvidenceType,
    ParsedEvidence,
    SourceDocument,
    TranscriptBlock,
)


def _normalized_claim(value: str) -> str:
    return " ".join(value.casefold().split())


def build_ledger(
    documents: Iterable[SourceDocument],
    extracted: Iterable[tuple[TranscriptBlock, tuple[ParsedEvidence, ...]]],
    retry_count: int,
) -> EvidenceLedger:
    document_list = tuple(documents)
    extracted_list = tuple(extracted)
    unit_by_id = {
        unit.unit_id: unit
        for document in document_list
        for unit in document.units
    }
    records: list[EvidenceRecord] = []
    seen: set[tuple[str, EvidenceType, str, tuple[str, ...]]] = set()
    for block, parsed_records in extracted_list:
        for parsed in parsed_records:
            key = (
                block.document_id,
                parsed.evidence_type,
                _normalized_claim(parsed.claim),
                parsed.unit_ids,
            )
            if key in seen:
                continue
            seen.add(key)
            source_excerpt = "\n".join(
                (
                    f"{unit_by_id[unit_id].speaker}: {unit_by_id[unit_id].text}"
                    if unit_by_id[unit_id].speaker
                    else unit_by_id[unit_id].text
                )
                for unit_id in parsed.unit_ids
            )
            records.append(
                EvidenceRecord(
                    evidence_id="",
                    document_id=block.document_id,
                    unit_ids=parsed.unit_ids,
                    evidence_type=parsed.evidence_type,
                    claim=parsed.claim,
                    source_excerpt=source_excerpt,
                    ordinal=unit_by_id[parsed.unit_ids[0]].ordinal,
                )
            )

    records.sort(key=lambda record: (int(record.document_id[1:]), record.ordinal))
    assigned = tuple(
        EvidenceRecord(
            evidence_id=f"E{index:04d}",
            document_id=record.document_id,
            unit_ids=record.unit_ids,
            evidence_type=record.evidence_type,
            claim=record.claim,
            source_excerpt=record.source_excerpt,
            ordinal=record.ordinal,
        )
        for index, record in enumerate(records, start=1)
    )
    return EvidenceLedger(
        records=assigned,
        document_count=len(document_list),
        unit_count=sum(len(document.units) for document in document_list),
        block_count=len(extracted_list),
        retry_count=retry_count,
    )


def _render_record(record: EvidenceRecord) -> str:
    return "\n".join(
        [
            f"[{record.evidence_id}] {record.evidence_type.value}",
            f"Claim: {record.claim}",
            f"Evidence: {record.source_excerpt}",
        ]
    )


def _render_bundle(records: tuple[EvidenceRecord, ...]) -> str:
    rendered = [_render_record(record) for record in records]
    body = "\n\n".join(rendered) if rendered else "No note-worthy evidence was extracted."
    return f"<evidence_ledger>\n{body}\n</evidence_ledger>"


def build_evidence_bundle(
    ledger: EvidenceLedger,
    count_tokens: Callable[[str], int],
) -> tuple[str, int]:
    bundle = _render_bundle(ledger.records)
    return bundle, count_tokens(bundle)
