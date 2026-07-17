"""Normalize saved session inputs into stable, model-addressable source units."""
from __future__ import annotations

import re
from typing import Iterable

from .types import NoteGenerationSource, SourceDocument, SourceUnit


# Session transcripts are canonicalized before they reach note generation. Keep
# this deliberately narrow so an ordinary clinician note containing a label-like
# phrase is not accidentally treated as a diarized transcript. The two Markdown
# variants cover both ``**Practitioner:**`` and ``**Practitioner**:``.
_SPEAKER_LINE = re.compile(
    r"^\s*(?:\*\*)?(Practitioner|Patient\s+[1-9]\d*)"
    r"(?:(?:\*\*)\s*:\s*|:\s*(?:\*\*)?\s*)(.*)$"
)
_SENTENCE_BOUNDARY = re.compile(r"(?<=[.!?])\s+(?=[A-Z0-9])")
_MAX_UNLABELLED_UNIT_CHARS = 900


def _clean_text(text: str) -> str:
    return " ".join(text.split())


def _split_long_paragraph(paragraph: str) -> list[str]:
    paragraph = _clean_text(paragraph)
    if not paragraph:
        return []
    if len(paragraph) <= _MAX_UNLABELLED_UNIT_CHARS:
        return [paragraph]

    sentences = _SENTENCE_BOUNDARY.split(paragraph)
    groups: list[str] = []
    current: list[str] = []
    current_length = 0
    for sentence in sentences:
        sentence = sentence.strip()
        if not sentence:
            continue
        projected = current_length + (1 if current else 0) + len(sentence)
        if current and projected > _MAX_UNLABELLED_UNIT_CHARS:
            groups.append(" ".join(current))
            current = []
            current_length = 0
        current.append(sentence)
        current_length += (1 if current_length else 0) + len(sentence)
    if current:
        groups.append(" ".join(current))
    return groups or [paragraph]


def _speaker_turns(text: str) -> list[tuple[str | None, str]]:
    turns: list[tuple[str | None, str]] = []
    current_speaker: str | None = None
    current_parts: list[str] = []
    saw_speaker = False

    def flush() -> None:
        nonlocal current_parts
        content = _clean_text(" ".join(current_parts))
        if content:
            turns.append((current_speaker, content))
        current_parts = []

    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line:
            if current_parts and not saw_speaker:
                flush()
            continue
        match = _SPEAKER_LINE.match(line)
        if match:
            saw_speaker = True
            flush()
            current_speaker = match.group(1)
            current_parts = [match.group(2)] if match.group(2).strip() else []
        else:
            current_parts.append(line)
    flush()

    if saw_speaker:
        return turns
    return []


def _unlabelled_units(text: str) -> list[tuple[None, str]]:
    paragraphs = re.split(r"\n\s*\n", text.strip())
    units: list[tuple[None, str]] = []
    for paragraph in paragraphs:
        for part in _split_long_paragraph(paragraph):
            units.append((None, part))
    return units


def normalize_sources(sources: Iterable[NoteGenerationSource]) -> tuple[SourceDocument, ...]:
    documents: list[SourceDocument] = []
    seen_source_ids: set[str] = set()
    for document_index, source in enumerate(sources, start=1):
        source_id = source.id.strip()
        if not source_id:
            raise ValueError("Every note-generation source must have an id.")
        if source_id in seen_source_ids:
            raise ValueError("Note-generation source ids must be unique.")
        seen_source_ids.add(source_id)
        if not source.text.strip():
            raise ValueError("Every note-generation source must contain text.")

        document_id = f"D{document_index}"
        raw_units = _speaker_turns(source.text) or _unlabelled_units(source.text)
        units = tuple(
            SourceUnit(
                unit_id=f"{document_id}U{unit_index:04d}",
                document_id=document_id,
                ordinal=unit_index,
                speaker=speaker,
                text=content,
            )
            for unit_index, (speaker, content) in enumerate(raw_units, start=1)
            if content.strip()
        )
        if not units:
            raise ValueError("A note-generation source could not be divided into text units.")
        documents.append(SourceDocument(document_id=document_id, source=source, units=units))
    if not documents:
        raise ValueError("At least one note-generation source is required.")
    return tuple(documents)


def render_unit(unit: SourceUnit) -> str:
    speaker = f" [{unit.speaker}]" if unit.speaker else ""
    return f"{unit.unit_id}{speaker}: {unit.text}"
