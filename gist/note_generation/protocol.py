"""Normalize free-text evidence summaries while retaining optional labels."""
from __future__ import annotations

import re

from .types import EvidenceType, ParsedEvidence, TranscriptBlock


_CONTEXT_FREE_ACKNOWLEDGEMENT = re.compile(
    r"^(?:the\s+)?(?:patient|client)\s+"
    r"(?:(?:said|responded)(?:\s+with)?\s+(?:yes|yeah|affirmatively)|"
    r"(?:agreed|endorsed|affirmed|acknowledged|confirmed)"
    r"(?:\s+with)?(?:\s+(?:the\s+)?(?:practitioner|clinician|therapist)"
    r"(?:['’]s)?(?:\s+(?:formulation|interpretation|statement|suggestion|"
    r"assessment|reflection|point))?)?)\.?$",
    re.IGNORECASE,
)
_NO_EVIDENCE = re.compile(
    r"^(?:none|no\s+(?:(?:note[- ]?worthy|relevant)(?:\s+clinical)?|clinically relevant)\s+"
    r"(?:evidence|information))\.?$",
    re.IGNORECASE,
)


class EmptyEvidenceOutputError(ValueError):
    """Raised only when the extractor returned no usable text at all."""


def _strip_wrapping(text: str) -> str:
    text = text.strip()
    if text.startswith("```") and text.endswith("```"):
        lines = text.splitlines()
        if len(lines) >= 2:
            text = "\n".join(lines[1:-1]).strip()
    return text


def _clean_line(line: str) -> str:
    line = line.strip()
    line = re.sub(r"^(?:[-*]|\d+[.)])\s+", "", line)
    return line.strip()


def parse_evidence_output(output: str, block: TranscriptBlock) -> tuple[ParsedEvidence, ...]:
    text = _strip_wrapping(output)
    if not text:
        raise EmptyEvidenceOutputError("The evidence extractor returned an empty response.")
    if _NO_EVIDENCE.fullmatch(text):
        return ()

    unit_ids = tuple(unit.unit_id for unit in block.units)
    lines = []
    for raw_line in text.splitlines():
        line = _clean_line(raw_line)
        if line:
            lines.append(line)
    if not lines:
        raise EmptyEvidenceOutputError("The evidence extractor returned no usable text.")

    # Preserve labels when the model happens to follow the old compact format,
    # but never require them. If every line is labelled, retain separate episodes.
    labelled: list[ParsedEvidence] = []
    all_labelled = bool(lines)
    for line in lines:
        parts = [part.strip() for part in line.split("|", 1)]
        if len(parts) != 2:
            all_labelled = False
            break
        label, claim = parts
        try:
            evidence_type = EvidenceType(label.strip("*_` "))
        except ValueError:
            all_labelled = False
            break
        claim = " ".join(claim.split())
        if not claim:
            all_labelled = False
            break
        if _CONTEXT_FREE_ACKNOWLEDGEMENT.fullmatch(claim):
            continue
        labelled.append(
            ParsedEvidence(
                unit_ids=unit_ids,
                evidence_type=evidence_type,
                claim=claim,
            )
        )
    if all_labelled:
        return tuple(labelled)

    summary = " ".join(lines)
    if _NO_EVIDENCE.fullmatch(summary) or _CONTEXT_FREE_ACKNOWLEDGEMENT.fullmatch(summary):
        return ()
    return (
        ParsedEvidence(
            unit_ids=unit_ids,
            evidence_type=EvidenceType.OTHER_RELEVANT_FACT,
            claim=summary,
        ),
    )
