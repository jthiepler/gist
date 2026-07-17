"""Token-aware chunking that preserves complete source units."""
from __future__ import annotations

from collections.abc import Callable, Iterable

from .sources import render_unit
from .types import SourceDocument, SourceUnit, TranscriptBlock


TokenCounter = Callable[[str], int]


def _render_units(units: Iterable[SourceUnit]) -> str:
    return "\n".join(render_unit(unit) for unit in units)


def build_blocks(
    documents: Iterable[SourceDocument],
    count_tokens: TokenCounter,
    target_tokens: int = 450,
    overlap_units: int = 1,
) -> tuple[TranscriptBlock, ...]:
    if target_tokens < 64:
        raise ValueError("target_tokens must be at least 64.")
    if overlap_units < 0:
        raise ValueError("overlap_units cannot be negative.")

    blocks: list[TranscriptBlock] = []
    for document in documents:
        units = document.units
        start = 0
        block_ordinal = 1
        while start < len(units):
            end = start
            chosen: list[SourceUnit] = []
            while end < len(units):
                candidate = chosen + [units[end]]
                candidate_text = _render_units(candidate)
                if chosen and count_tokens(candidate_text) > target_tokens:
                    break
                chosen = candidate
                end += 1
            if not chosen:
                chosen = [units[start]]
                end = start + 1
            text = _render_units(chosen)
            blocks.append(
                TranscriptBlock(
                    document_id=document.document_id,
                    source_kind=document.source.kind,
                    source_origin=document.source.origin,
                    source_title=document.source.title,
                    ordinal=block_ordinal,
                    units=tuple(chosen),
                    text=text,
                )
            )
            block_ordinal += 1
            if end >= len(units):
                break
            start = max(start + 1, end - min(overlap_units, len(chosen) - 1))
    return tuple(blocks)
