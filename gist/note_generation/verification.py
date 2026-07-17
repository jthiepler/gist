"""Format-neutral support verification for generated note claims."""
from __future__ import annotations

import re
import threading
from collections import Counter
from dataclasses import dataclass
from typing import Any, Optional

from ..llm.base import ChatMessage
from .diagnostics import DiagnosticCapture, messages_to_dict
from .types import (
    EvidenceLedger,
    EvidenceRecord,
    EvidenceType,
    VerificationSummary,
    VerificationVerdict,
)


_WORD = re.compile(r"[A-Za-z0-9][A-Za-z0-9'/-]*")
_HEADING = re.compile(r"^(?:#{1,6}\s+.+|\*\*[^*]+:\*\*|[^.!?]{1,80}:)$")
_BULLET = re.compile(r"^(\s*(?:[-*+]\s+|\d+[.)]\s+))(.*)$")
_SEPARATOR = re.compile(r"^\s*(?:---+|___+|\*\*\*+|\|?\s*:?-{3,})")
_CRITICAL_CLAIM = re.compile(
    r"\b(suicid|self[- ]?harm|homicid|risk|safety|diagnos|medicat|prescrib|dose|mg\b|"
    r"referr|appointment|scheduled|assigned|homework|agreed|age\b|\d)\w*",
    re.IGNORECASE,
)
_RISK_ABSENCE = re.compile(r"risk assessment.*not documented|current .*risk.*cannot be determined", re.I)
_PLAN_ABSENCE = re.compile(r"no (?:formal )?(?:plan|follow-up).*documented|no .*was established", re.I)


class VerificationError(RuntimeError):
    pass


@dataclass(frozen=True)
class _ClaimLine:
    index: int
    prefix: str
    claim: str


def _claim_lines(note: str) -> tuple[list[str], tuple[_ClaimLine, ...]]:
    lines = note.splitlines()
    claims: list[_ClaimLine] = []
    for index, raw_line in enumerate(lines):
        stripped = raw_line.strip()
        if not stripped or _HEADING.fullmatch(stripped) or _SEPARATOR.match(stripped):
            continue
        bullet = _BULLET.match(raw_line)
        if bullet:
            prefix, claim = bullet.group(1), bullet.group(2).strip()
        else:
            prefix, claim = "", stripped
        if claim:
            claims.append(_ClaimLine(index=index, prefix=prefix, claim=claim))
    return lines, tuple(claims)


def _tokens(text: str) -> Counter[str]:
    return Counter(token.casefold() for token in _WORD.findall(text) if len(token) > 1)


def _candidate_evidence(
    claim: str,
    records: tuple[EvidenceRecord, ...],
    limit: int = 8,
) -> tuple[EvidenceRecord, ...]:
    claim_tokens = _tokens(claim)
    claim_numbers = {token for token in claim_tokens if any(char.isdigit() for char in token)}
    scored: list[tuple[float, int, EvidenceRecord]] = []
    for index, record in enumerate(records):
        evidence_tokens = _tokens(f"{record.claim} {record.source_excerpt}")
        overlap = set(claim_tokens) & set(evidence_tokens)
        score = sum(2.0 if len(token) >= 7 else 1.0 for token in overlap)
        evidence_numbers = {
            token for token in evidence_tokens if any(char.isdigit() for char in token)
        }
        if claim_numbers & evidence_numbers:
            score += 8.0
        scored.append((score, -index, record))
    scored.sort(key=lambda item: (item[0], item[1]), reverse=True)
    return tuple(item[2] for item in scored[:limit])


def _render_candidates(records: tuple[EvidenceRecord, ...]) -> str:
    return "\n\n".join(
        f"[{record.evidence_id}] {record.evidence_type.value}\n"
        f"Claim: {record.claim}\nEvidence: {record.source_excerpt}"
        for record in records
    )


def _absence_verdict(claim: str, ledger: EvidenceLedger) -> VerificationVerdict | None:
    if _RISK_ABSENCE.search(claim):
        has_risk = any(
            record.evidence_type == EvidenceType.RISK_OR_SAFETY
            for record in ledger.records
        )
        return VerificationVerdict.CONTRADICTED if has_risk else VerificationVerdict.SUPPORTED
    if _PLAN_ABSENCE.search(claim):
        has_plan = any(
            record.evidence_type == EvidenceType.ACTION_OR_PLAN
            for record in ledger.records
        )
        return VerificationVerdict.CONTRADICTED if has_plan else VerificationVerdict.SUPPORTED
    return None


def _verify_claim(
    llm: Any,
    claim: str,
    ledger: EvidenceLedger,
    cancel_event: Optional[threading.Event],
    diagnostic_capture: DiagnosticCapture | None,
    format_name: str | None,
) -> VerificationVerdict:
    absence = _absence_verdict(claim, ledger)
    if absence is not None:
        if diagnostic_capture:
            diagnostic_capture.append_verification(
                {
                    "format": format_name,
                    "kind": "deterministic_absence_check",
                    "input": {"claim": claim},
                    "output": {"verdict": absence.value},
                }
            )
        return absence
    candidates = _candidate_evidence(claim, ledger.records)
    evidence_text = _render_candidates(candidates) or "No candidate evidence was found."
    messages = [
        ChatMessage(
            role="system",
            content=(
                "Classify whether one clinical-note claim is supported by the supplied source-grounded evidence. "
                "SUPPORTED means every material detail and attribution is supported. PARTLY_SUPPORTED means only "
                "part is supported or wording is stronger than the evidence. UNSUPPORTED means the evidence does "
                "not establish it. CONTRADICTED means the evidence conflicts with it. Return exactly one label."
            ),
        ),
        ChatMessage(
            role="user",
            content=(
                f"<note_claim>\n{claim}\n</note_claim>\n\n"
                f"<candidate_evidence>\n{evidence_text}\n</candidate_evidence>"
            ),
        ),
    ]
    attempt: dict[str, Any] = {
        "format": format_name,
        "kind": "model_claim_check",
        "input": {
            "claim": claim,
            "candidate_records": candidates,
            "messages": messages_to_dict(messages),
            "choices": [verdict.value for verdict in VerificationVerdict],
            "max_tokens": 16,
        },
    }
    try:
        output = llm.generate_choice(
            messages=messages,
            choices=[verdict.value for verdict in VerificationVerdict],
            max_tokens=16,
            cancel_event=cancel_event,
        )
        verdict = VerificationVerdict(output)
        attempt["output"] = {
            "raw_model_output": output,
            "verdict": verdict.value,
        }
        return verdict
    except Exception as error:
        attempt["error"] = {
            "type": type(error).__name__,
            "message": str(error),
        }
        raise
    finally:
        if diagnostic_capture:
            diagnostic_capture.append_verification(attempt)


def verify_note(
    llm: Any,
    note: str,
    ledger: EvidenceLedger,
    *,
    mode: str = "off",
    cancel_event: Optional[threading.Event] = None,
    max_removed_ratio: float = 0.2,
    diagnostic_capture: DiagnosticCapture | None = None,
    format_name: str | None = None,
) -> tuple[str, VerificationSummary]:
    if mode not in {"off", "shadow", "enforce"}:
        raise ValueError("Verification mode must be 'off', 'shadow', or 'enforce'.")
    if mode == "off":
        if diagnostic_capture:
            diagnostic_capture.append_verification(
                {
                    "format": format_name,
                    "kind": "verification_skipped",
                    "input": {"mode": mode, "draft": note},
                    "output": {"note": note, "summary": VerificationSummary()},
                }
            )
        return note, VerificationSummary()

    lines, claims = _claim_lines(note)
    counts = Counter()
    remove_indexes: set[int] = set()
    for claim_line in claims:
        if cancel_event and cancel_event.is_set():
            raise InterruptedError("Note generation cancelled")
        verdict = _verify_claim(
            llm,
            claim_line.claim,
            ledger,
            cancel_event,
            diagnostic_capture,
            format_name,
        )
        counts[verdict] += 1
        if mode != "enforce" or verdict == VerificationVerdict.SUPPORTED:
            continue
        is_critical = _CRITICAL_CLAIM.search(claim_line.claim) is not None
        if verdict == VerificationVerdict.CONTRADICTED or is_critical:
            raise VerificationError(
                "The generated note contained a critical or contradicted claim that could not be safely retained."
            )
        remove_indexes.add(claim_line.index)

    if mode == "enforce" and claims:
        if len(remove_indexes) / len(claims) > max_removed_ratio:
            raise VerificationError(
                "Too much of the generated note lacked adequate source support."
            )
        for index in remove_indexes:
            lines[index] = ""
        verified_note = "\n".join(lines).strip()
        if not verified_note or not _claim_lines(verified_note)[1]:
            raise VerificationError("No supported clinical note content remained after verification.")
    else:
        verified_note = note

    summary = VerificationSummary(
        claims_checked=len(claims),
        supported=counts[VerificationVerdict.SUPPORTED],
        partly_supported=counts[VerificationVerdict.PARTLY_SUPPORTED],
        unsupported=counts[VerificationVerdict.UNSUPPORTED],
        contradicted=counts[VerificationVerdict.CONTRADICTED],
        claims_removed=len(remove_indexes),
    )
    if diagnostic_capture:
        diagnostic_capture.set_stage(
            f"verification_result:{format_name or 'unknown'}",
            {
                "input": {"mode": mode, "draft": note},
                "output": {"note": verified_note, "summary": summary},
            },
        )
    return verified_note, summary
