"""Parse and validate the small-model-friendly evidence line protocol."""
from __future__ import annotations

import re
from collections.abc import Iterable

from .types import EvidenceType, ParsedEvidence, TranscriptBlock


_UNIT_ID = re.compile(r"^(D[1-9][0-9]*U[0-9]{4})(?:-(D[1-9][0-9]*U[0-9]{4}))?$")
_MAX_CLAIM_CHARS = 600
_CONTEXT_FREE_ACKNOWLEDGEMENT = re.compile(
    r"^(?:the\s+)?(?:patient|client)\s+"
    r"(?:(?:said|responded)(?:\s+with)?\s+(?:yes|yeah|affirmatively)|"
    r"(?:agreed|endorsed|affirmed|acknowledged|confirmed)"
    r"(?:\s+with)?(?:\s+(?:the\s+)?(?:practitioner|clinician|therapist)"
    r"(?:['’]s)?(?:\s+(?:formulation|interpretation|statement|suggestion|"
    r"assessment|reflection|point))?)?)\.?$",
    re.IGNORECASE,
)


class EvidenceProtocolError(ValueError):
    pass


def _expand_range(reference: str, block: TranscriptBlock) -> tuple[str, ...]:
    match = _UNIT_ID.fullmatch(reference.strip())
    if not match:
        raise EvidenceProtocolError(f"Invalid source-unit reference: {reference!r}")
    start_id, end_id = match.group(1), match.group(2) or match.group(1)
    block_ids = [unit.unit_id for unit in block.units]
    try:
        start = block_ids.index(start_id)
        end = block_ids.index(end_id)
    except ValueError as error:
        raise EvidenceProtocolError("Evidence referenced a unit outside the supplied block.") from error
    if end < start:
        raise EvidenceProtocolError("Evidence source-unit ranges must be chronological.")
    return tuple(block_ids[start : end + 1])


def parse_evidence_output(output: str, block: TranscriptBlock) -> tuple[ParsedEvidence, ...]:
    text = output.strip()
    if not text:
        raise EvidenceProtocolError("The evidence extractor returned an empty response.")
    if text == "NONE":
        return ()
    if "```" in text:
        raise EvidenceProtocolError("The evidence extractor returned a fenced response.")

    parsed: list[ParsedEvidence] = []
    for line_number, raw_line in enumerate(text.splitlines(), start=1):
        line = raw_line.strip()
        if not line:
            continue
        if line == "NONE":
            raise EvidenceProtocolError("NONE cannot be combined with evidence records.")
        parts = [part.strip() for part in line.split("|", 2)]
        if len(parts) != 3:
            raise EvidenceProtocolError(f"Evidence line {line_number} did not have three fields.")
        reference, label, claim = parts
        try:
            evidence_type = EvidenceType(label)
        except ValueError as error:
            raise EvidenceProtocolError(f"Evidence line {line_number} used an unknown label.") from error
        claim = " ".join(claim.split())
        if not claim:
            raise EvidenceProtocolError(f"Evidence line {line_number} had an empty claim.")
        if len(claim) > _MAX_CLAIM_CHARS:
            raise EvidenceProtocolError(f"Evidence line {line_number} exceeded the claim limit.")
        if _CONTEXT_FREE_ACKNOWLEDGEMENT.fullmatch(claim):
            continue
        parsed.append(
            ParsedEvidence(
                unit_ids=_expand_range(reference, block),
                evidence_type=evidence_type,
                claim=claim,
            )
        )
    if not parsed:
        raise EvidenceProtocolError("The evidence extractor returned no usable records.")
    return tuple(parsed)


def render_unit_reference(unit_ids: Iterable[str]) -> str:
    values = tuple(unit_ids)
    if not values:
        raise ValueError("An evidence record must reference at least one unit.")
    if len(values) == 1:
        return values[0]
    return f"{values[0]}-{values[-1]}"
