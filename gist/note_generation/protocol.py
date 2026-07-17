"""Parse and validate the small-model-friendly evidence line protocol."""
from __future__ import annotations

import re

from .types import EvidenceType, ParsedEvidence, TranscriptBlock


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
        parts = [part.strip() for part in line.split("|", 1)]
        if len(parts) != 2:
            raise EvidenceProtocolError(f"Evidence line {line_number} did not have two fields.")
        label, claim = parts
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
                unit_ids=tuple(unit.unit_id for unit in block.units),
                evidence_type=evidence_type,
                claim=claim,
            )
        )
    if not parsed:
        raise EvidenceProtocolError("The evidence extractor returned no usable records.")
    return tuple(parsed)
