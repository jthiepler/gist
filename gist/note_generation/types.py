"""Internal data contracts for evidence-ledger note generation."""
from __future__ import annotations

from dataclasses import asdict, dataclass, field
from enum import Enum
from typing import Any, Optional


class EvidenceType(str, Enum):
    CLIENT_REPORT = "CLIENT_REPORT"
    CLINICIAN_OBSERVATION = "CLINICIAN_OBSERVATION"
    CLINICIAN_INTERVENTION = "CLINICIAN_INTERVENTION"
    CLIENT_RESPONSE = "CLIENT_RESPONSE"
    CLINICIAN_FORMULATION = "CLINICIAN_FORMULATION"
    ACTION_OR_PLAN = "ACTION_OR_PLAN"
    RISK_OR_SAFETY = "RISK_OR_SAFETY"
    OTHER_RELEVANT_FACT = "OTHER_RELEVANT_FACT"


class VerificationVerdict(str, Enum):
    SUPPORTED = "SUPPORTED"
    PARTLY_SUPPORTED = "PARTLY_SUPPORTED"
    UNSUPPORTED = "UNSUPPORTED"
    CONTRADICTED = "CONTRADICTED"


@dataclass(frozen=True)
class NoteGenerationSource:
    id: str
    kind: str
    origin: str
    title: str
    text: str


@dataclass(frozen=True)
class SourceUnit:
    unit_id: str
    document_id: str
    ordinal: int
    speaker: Optional[str]
    text: str


@dataclass(frozen=True)
class SourceDocument:
    document_id: str
    source: NoteGenerationSource
    units: tuple[SourceUnit, ...]


@dataclass(frozen=True)
class TranscriptBlock:
    document_id: str
    source_kind: str
    source_origin: str
    source_title: str
    ordinal: int
    units: tuple[SourceUnit, ...]
    text: str


@dataclass(frozen=True)
class ParsedEvidence:
    unit_ids: tuple[str, ...]
    evidence_type: EvidenceType
    claim: str


@dataclass(frozen=True)
class EvidenceRecord:
    evidence_id: str
    document_id: str
    unit_ids: tuple[str, ...]
    evidence_type: EvidenceType
    claim: str
    source_excerpt: str
    ordinal: int


@dataclass(frozen=True)
class EvidenceLedger:
    records: tuple[EvidenceRecord, ...]
    document_count: int
    unit_count: int
    block_count: int
    retry_count: int


@dataclass(frozen=True)
class NoteFormatRequest:
    name: str
    prompt: Optional[str] = None


@dataclass(frozen=True)
class VerificationSummary:
    claims_checked: int = 0
    supported: int = 0
    partly_supported: int = 0
    unsupported: int = 0
    contradicted: int = 0
    claims_removed: int = 0


@dataclass(frozen=True)
class GeneratedNote:
    format: str
    note: str
    verification: VerificationSummary


@dataclass(frozen=True)
class FormatFailure:
    format: str
    message: str


@dataclass(frozen=True)
class GenerateNotesResult:
    notes: tuple[GeneratedNote, ...] = field(default_factory=tuple)
    failures: tuple[FormatFailure, ...] = field(default_factory=tuple)
    ledger_stats: dict[str, int] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)
