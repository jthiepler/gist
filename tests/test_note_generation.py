from __future__ import annotations

import json
import os
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch
from uuid import uuid4

from gist.note_generation.chunking import build_blocks
from gist.note_generation.diagnostics import DiagnosticCapture, DIAGNOSTICS_DIRECTORY_ENV
from gist.note_generation.evidence_cache import GIST_DATABASE_PATH_ENV
from gist.note_generation.ledger import build_evidence_bundle
from gist.note_generation.pipeline import (
    build_evidence_cache_key,
    clear_evidence_cache,
    generate_notes_with_backend,
    prepare_evidence_with_backend,
)
from gist.note_generation.protocol import EmptyEvidenceOutputError, parse_evidence_output
from gist.note_generation.sources import normalize_sources
from gist.note_generation.types import (
    EvidenceLedger,
    EvidenceRecord,
    EvidenceType,
    NoteFormatRequest,
    NoteGenerationSource,
)
from gist.note_generation.verification import verify_note
from gist.note_generation.verification import VerificationError
from gist.server import _params_for
from gist.llm.base import GenerationIncompleteError


class FakeLLM:
    def __init__(self, generations=None, choices=None):
        self.generations = list(generations or [])
        self.choices = list(choices or [])
        self.generate_calls = []
        self.batch_calls = []
        self.choice_calls = []

    def count_tokens(self, text: str) -> int:
        return len(text.split())

    def generate(self, **kwargs):
        self.generate_calls.append(kwargs)
        if not self.generations:
            raise AssertionError("Unexpected generate call")
        result = self.generations.pop(0)
        if isinstance(result, BaseException):
            raise result
        return result

    def generate_batch(self, *, messages_batch, **kwargs):
        self.batch_calls.append(
            {
                "messages_batch": messages_batch,
                **kwargs,
            }
        )
        return [
            self.generate(messages=messages, **kwargs)
            for messages in messages_batch
        ]

    def generate_choice(self, **kwargs):
        self.choice_calls.append(kwargs)
        if not self.choices:
            raise AssertionError("Unexpected choice call")
        return self.choices.pop(0)


def source(text: str) -> NoteGenerationSource:
    return NoteGenerationSource(
        id="source-1",
        kind="session_transcript",
        origin="typed",
        title="Session transcript",
        text=text,
    )


def identified_source(source_id: str, text: str) -> NoteGenerationSource:
    return NoteGenerationSource(
        id=source_id,
        kind="session_transcript",
        origin="typed",
        title="Session transcript",
        text=text,
    )


def create_evidence_cache_database(
    directory: str,
    source_ids: tuple[str, ...],
) -> Path:
    path = Path(directory) / "gist.db"
    connection = sqlite3.connect(path)
    connection.executescript(
        """PRAGMA foreign_keys = ON;
        CREATE TABLE session_inputs (id TEXT PRIMARY KEY);
        CREATE TABLE evidence_ledger_cache (
            source_id TEXT PRIMARY KEY REFERENCES session_inputs(id) ON DELETE CASCADE,
            source_fingerprint TEXT NOT NULL,
            model_identity TEXT NOT NULL,
            pipeline_version TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            retry_count INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL
        );"""
    )
    connection.executemany(
        "INSERT INTO session_inputs (id) VALUES (?1)",
        ((source_id,) for source_id in source_ids),
    )
    connection.commit()
    connection.close()
    return path


class SourceAndProtocolTests(unittest.TestCase):
    def test_diarized_transcript_becomes_stable_units(self):
        documents = normalize_sources(
            [source("Practitioner: How have things been?\nPatient 1: I have felt anxious.\nStill most evenings.")]
        )
        self.assertEqual([unit.unit_id for unit in documents[0].units], ["D1U0001", "D1U0002"])
        self.assertEqual(documents[0].units[1].speaker, "Patient 1")
        self.assertIn("Still most evenings", documents[0].units[1].text)

    def test_canonical_markdown_speaker_labels_become_stable_units(self):
        documents = normalize_sources(
            [
                source(
                    "**Practitioner:** How have things been?\n"
                    "**Patient 1:** I have felt anxious.\n"
                    "**Patient 2**: I noticed that too."
                )
            ]
        )
        self.assertEqual(
            [unit.speaker for unit in documents[0].units],
            ["Practitioner", "Patient 1", "Patient 2"],
        )

    def test_chunking_preserves_units_and_overlap(self):
        documents = normalize_sources(
            [source("Patient 1: One two three four.\nPractitioner: Five six seven eight.\nPatient 1: Nine ten eleven.")]
        )
        blocks = build_blocks(documents, len, target_tokens=100, overlap_units=1)
        self.assertGreaterEqual(len(blocks), 2)
        self.assertEqual(blocks[0].units[-1].unit_id, blocks[1].units[0].unit_id)

    def test_protocol_accepts_unstructured_free_text(self):
        documents = normalize_sources([source("Patient 1: I feel anxious.")])
        block = build_blocks(documents, lambda text: len(text.split()))[0]
        records = parse_evidence_output(
            "The patient described feeling anxious and having difficulty settling at night.",
            block,
        )
        self.assertEqual(records[0].evidence_type, EvidenceType.OTHER_RELEVANT_FACT)
        self.assertIn("difficulty settling", records[0].claim)

    def test_protocol_retries_only_an_empty_response(self):
        documents = normalize_sources([source("Patient 1: I feel anxious.")])
        block = build_blocks(documents, lambda text: len(text.split()))[0]
        with self.assertRaises(EmptyEvidenceOutputError):
            parse_evidence_output("", block)

    def test_protocol_keeps_pipes_inside_claim(self):
        documents = normalize_sources([source("Patient 1: I feel anxious.")])
        block = build_blocks(documents, lambda text: len(text.split()))[0]
        records = parse_evidence_output(
            "CLIENT_REPORT | Patient described work | home conflict.",
            block,
        )
        self.assertEqual(records[0].claim, "Patient described work | home conflict.")

    def test_protocol_discards_context_free_acknowledgement_as_empty_evidence(self):
        documents = normalize_sources(
            [
                source(
                    "Practitioner: Could school have felt unsafe?\n"
                    "Patient 1: Yeah, that feels more like it."
                )
            ]
        )
        block = build_blocks(documents, lambda text: len(text.split()))[0]
        records = parse_evidence_output(
            "CLIENT_RESPONSE | Patient agreed with the practitioner's formulation.",
            block,
        )
        self.assertEqual(records, ())

    def test_protocol_recognizes_free_text_no_evidence_response(self):
        documents = normalize_sources([source("Patient 1: Yeah.")])
        block = build_blocks(documents, lambda text: len(text.split()))[0]
        records = parse_evidence_output(
            "No note-worthy clinical information.",
            block,
        )
        self.assertEqual(records, ())

    def test_protocol_accepts_harmless_markdown_wrapping(self):
        documents = normalize_sources([source("Patient 1: I feel anxious.")])
        block = build_blocks(documents, lambda text: len(text.split()))[0]
        records = parse_evidence_output(
            "```text\n- **CLIENT_REPORT** | Patient reported anxiety.\n```",
            block,
        )
        self.assertEqual(records[0].evidence_type, EvidenceType.CLIENT_REPORT)
        self.assertEqual(records[0].claim, "Patient reported anxiety.")

    def test_protocol_accepts_formulation_with_meaningful_response(self):
        documents = normalize_sources(
            [
                source(
                    "Practitioner: Could school have felt unsafe?\n"
                    "Patient 1: Yeah, that feels more like it."
                )
            ]
        )
        block = build_blocks(documents, lambda text: len(text.split()))[0]
        records = parse_evidence_output(
            "CLINICIAN_FORMULATION | "
            "Practitioner tentatively contrasted safe home life with unsafe school life; "
            "the patient said this felt more accurate.",
            block,
        )
        self.assertEqual(records[0].unit_ids, ("D1U0001", "D1U0002"))


class PipelineTests(unittest.TestCase):
    def tearDown(self):
        clear_evidence_cache()

    def test_extraction_runs_once_for_multiple_formats(self):
        llm = FakeLLM(
            generations=[
                "CLIENT_REPORT | Patient reported feeling anxious.",
                "First custom note.",
                "Second custom note.",
            ]
        )
        result = generate_notes_with_backend(
            llm,
            [source("Patient: I feel anxious.")],
            [
                NoteFormatRequest(name="first", prompt="Write the first format."),
                NoteFormatRequest(name="second", prompt="Write the second format."),
            ],
            verification_mode="off",
        )
        self.assertEqual([note.note for note in result.notes], ["First custom note.", "Second custom note."])
        self.assertEqual(result.ledger_stats["evidence_records"], 1)
        self.assertEqual(len(llm.generate_calls), 3)
        extraction_message = llm.generate_calls[0]["messages"][-1]
        extraction_prompt = extraction_message.content
        self.assertIn("concise, self-contained free text", extraction_prompt)
        self.assertIn("one short paragraph", extraction_prompt)
        self.assertIn("Patient: I feel anxious.", extraction_prompt)
        self.assertNotIn("D1U0001", extraction_prompt)
        self.assertNotIn("<source_metadata>", extraction_prompt)
        self.assertNotIn("Examples:", extraction_prompt)
        self.assertNotIn("taking on too many commitments", extraction_prompt)
        self.assertIn("preserve who reported, observed, suggested", extraction_prompt)
        self.assertIn("discussed, proposed, agreed", extraction_prompt)
        self.assertNotIn("Allowed labels", extraction_prompt)
        self.assertLess(extraction_message.cache_prefix_length, 1800)
        self.assertEqual(llm.generate_calls[0]["temperature"], 0.2)

    def test_extraction_does_not_run_a_keyword_triggered_critical_review(self):
        llm = FakeLLM(
            generations=[
                "CLIENT_REPORT | Patient discussed possibly missing an appointment.",
                "A supported custom note.",
            ]
        )
        result = generate_notes_with_backend(
            llm,
            [source("Patient 1: I might miss an appointment next week.")],
            [NoteFormatRequest(name="custom", prompt="Write a clinical note.")],
            verification_mode="off",
        )

        self.assertEqual(result.notes[0].note, "A supported custom note.")
        self.assertEqual(result.ledger_stats["retry_count"], 0)
        self.assertEqual(len(llm.generate_calls), 2)

    def test_evidence_extraction_batches_four_blocks_at_a_time(self):
        long_turns = "\n".join(
            f"Patient 1: detail-{index} " + "word " * 500
            for index in range(6)
        )
        llm = FakeLLM(
            generations=[
                *[
                    f"CLIENT_REPORT | Patient described detail {index}."
                    for index in range(6)
                ],
                "A supported custom note.",
            ]
        )
        result = generate_notes_with_backend(
            llm,
            [source(long_turns)],
            [NoteFormatRequest(name="custom", prompt="Write a clinical note.")],
            verification_mode="off",
        )

        self.assertEqual(result.ledger_stats["blocks"], 6)
        self.assertEqual(result.ledger_stats["evidence_records"], 6)
        self.assertEqual(
            [len(call["messages_batch"]) for call in llm.batch_calls],
            [4, 2],
        )

    def test_empty_extraction_retries_once(self):
        llm = FakeLLM(
            generations=[
                "",
                "The patient reported feeling anxious.",
                "A supported custom note.",
            ]
        )
        result = generate_notes_with_backend(
            llm,
            [source("Patient: I feel anxious.")],
            [NoteFormatRequest(name="custom", prompt="Write a note.")],
            verification_mode="off",
        )
        self.assertEqual(result.ledger_stats["retry_count"], 1)
        self.assertEqual(len(result.notes), 1)

    def test_repeated_empty_extraction_falls_back_to_source_block(self):
        llm = FakeLLM(
            generations=[
                "",
                "",
                "A supported custom note.",
            ]
        )
        result = generate_notes_with_backend(
            llm,
            [source("Patient: I feel anxious.")],
            [NoteFormatRequest(name="custom", prompt="Write a note.")],
            verification_mode="off",
        )

        self.assertEqual(result.ledger_stats["retry_count"], 1)
        self.assertEqual(result.notes[0].note, "A supported custom note.")
        self.assertEqual(result.ledger_stats["evidence_records"], 1)
        self.assertEqual(llm.generate_calls[1]["temperature"], 0.2)

    def test_incomplete_render_retries_with_tighter_concision(self):
        llm = FakeLLM(
            generations=[
                "CLIENT_REPORT | Patient reported feeling anxious.",
                GenerationIncompleteError("generation limit"),
                "A concise completed note.",
            ]
        )
        result = generate_notes_with_backend(
            llm,
            [source("Patient: I feel anxious.")],
            [NoteFormatRequest(name="custom", prompt="Write a clinical note.")],
            verification_mode="off",
        )
        self.assertEqual(result.notes[0].note, "A concise completed note.")
        retry_messages = llm.generate_calls[-1]["messages"]
        self.assertIn("This is a retry", retry_messages[-1].content)

    def test_developer_diagnostics_capture_and_save_every_pipeline_stage(self):
        llm = FakeLLM(
            generations=[
                "CLIENT_REPORT | Patient reported feeling anxious.",
                "A concise completed note.",
            ]
        )
        capture = DiagnosticCapture(session_id=str(uuid4()))
        result = generate_notes_with_backend(
            llm,
            [source("Patient: I feel anxious.")],
            [NoteFormatRequest(name="custom", prompt="Write a clinical note.")],
            verification_mode="off",
            diagnostic_capture=capture,
        )
        capture.set_request({"sources": [source("Patient: I feel anxious.")]})
        capture.finish("completed")

        self.assertEqual(result.notes[0].note, "A concise completed note.")
        self.assertIn("normalized_sources", capture.stages)
        self.assertIn("chunking", capture.stages)
        self.assertIn("ledger", capture.stages)
        self.assertIn("evidence_bundle", capture.stages)
        self.assertIn("result", capture.stages)
        self.assertEqual(len(capture.extraction_attempts), 1)
        self.assertEqual(len(capture.rendering_attempts), 1)
        self.assertEqual(capture.verification_attempts[0]["kind"], "verification_skipped")

        with tempfile.TemporaryDirectory() as directory:
            with patch.dict(os.environ, {DIAGNOSTICS_DIRECTORY_ENV: directory}):
                path = capture.save()
            self.assertIsNotNone(path)
            saved = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(saved["status"], "completed")
        self.assertEqual(saved["session_id"], capture.session_id)
        self.assertEqual(
            saved["extraction_attempts"][0]["output"]["raw_model_output"],
            "CLIENT_REPORT | Patient reported feeling anxious.",
        )

    def test_reuses_cached_ledger_for_a_later_format_request(self):
        llm = FakeLLM(
            generations=[
                "CLIENT_REPORT | Patient reported feeling anxious.",
                "First note.",
                "Second note.",
            ]
        )
        first = generate_notes_with_backend(
            llm,
            [source("Patient: I feel anxious.")],
            [NoteFormatRequest(name="first", prompt="Write the first note.")],
            verification_mode="off",
            evidence_cache_key="stable-source-and-model",
        )
        second = generate_notes_with_backend(
            llm,
            [source("Patient: I feel anxious.")],
            [NoteFormatRequest(name="second", prompt="Write the second note.")],
            verification_mode="off",
            evidence_cache_key="stable-source-and-model",
        )
        self.assertEqual(first.notes[0].note, "First note.")
        self.assertEqual(second.notes[0].note, "Second note.")
        self.assertEqual(len(llm.generate_calls), 3)

    def test_source_edit_changes_evidence_cache_key(self):
        original = build_evidence_cache_key([source("Patient: I feel anxious.")], "model@1")
        edited = build_evidence_cache_key([source("Patient: I feel calmer.")], "model@1")
        self.assertNotEqual(original, edited)

    def test_persistent_document_cache_survives_memory_cache_clear(self):
        source_material = source("Patient 1: I feel anxious most evenings.")
        llm = FakeLLM(
            generations=["CLIENT_REPORT | Patient reported anxiety most evenings."]
        )
        backend_factory = Mock()

        with tempfile.TemporaryDirectory() as directory:
            database_path = create_evidence_cache_database(directory, ("source-1",))
            with patch.dict(
                os.environ,
                {GIST_DATABASE_PATH_ENV: str(database_path)},
            ):
                first = prepare_evidence_with_backend(
                    llm,
                    [source_material],
                    evidence_cache_key="first-process-memory-key",
                    evidence_model_identity="evidence-model@revision-1",
                )
                clear_evidence_cache()
                second = prepare_evidence_with_backend(
                    None,
                    [source_material],
                    evidence_cache_key="second-process-memory-key",
                    evidence_model_identity="evidence-model@revision-1",
                    evidence_backend_factory=backend_factory,
                )

                connection = sqlite3.connect(database_path)
                cached_rows = connection.execute(
                    "SELECT COUNT(*) FROM evidence_ledger_cache"
                ).fetchone()[0]
                connection.close()

        self.assertEqual(second, first)
        self.assertEqual(cached_rows, 1)
        backend_factory.assert_not_called()
        self.assertEqual(len(llm.generate_calls), 1)

    def test_document_edit_reextracts_only_changed_document(self):
        first_source = identified_source(
            "source-a",
            "Patient 1: I feel anxious most evenings.",
        )
        second_source = identified_source(
            "source-b",
            "Patient 1: I have been sleeping poorly.",
        )
        initial_llm = FakeLLM(
            generations=[
                "CLIENT_REPORT | Patient reported anxiety most evenings.",
                "CLIENT_REPORT | Patient reported poor sleep.",
            ]
        )

        with tempfile.TemporaryDirectory() as directory:
            database_path = create_evidence_cache_database(
                directory,
                ("source-a", "source-b"),
            )
            with patch.dict(
                os.environ,
                {GIST_DATABASE_PATH_ENV: str(database_path)},
            ):
                prepare_evidence_with_backend(
                    initial_llm,
                    [first_source, second_source],
                    evidence_cache_key="initial-source-set",
                    evidence_model_identity="evidence-model@revision-1",
                )
                clear_evidence_cache()

                edited_llm = FakeLLM(
                    generations=[
                        "CLIENT_REPORT | Patient reported improved sleep after changing routines."
                    ]
                )
                ledger = prepare_evidence_with_backend(
                    edited_llm,
                    [
                        first_source,
                        identified_source(
                            "source-b",
                            "Patient 1: I sleep better after changing my evening routine.",
                        ),
                    ],
                    evidence_cache_key="edited-source-set",
                    evidence_model_identity="evidence-model@revision-1",
                )

        self.assertEqual(len(initial_llm.generate_calls), 2)
        self.assertEqual(len(edited_llm.generate_calls), 1)
        self.assertEqual(
            [record.claim for record in ledger.records],
            [
                "Patient reported anxiety most evenings.",
                "Patient reported improved sleep after changing routines.",
            ],
        )

    def test_deleting_source_cascades_to_evidence_cache(self):
        source_material = source("Patient 1: I feel anxious most evenings.")
        llm = FakeLLM(
            generations=["CLIENT_REPORT | Patient reported anxiety most evenings."]
        )
        with tempfile.TemporaryDirectory() as directory:
            database_path = create_evidence_cache_database(directory, ("source-1",))
            with patch.dict(
                os.environ,
                {GIST_DATABASE_PATH_ENV: str(database_path)},
            ):
                prepare_evidence_with_backend(
                    llm,
                    [source_material],
                    evidence_cache_key="source-set",
                    evidence_model_identity="evidence-model@revision-1",
                )

            connection = sqlite3.connect(database_path)
            connection.execute("PRAGMA foreign_keys = ON")
            connection.execute("DELETE FROM session_inputs WHERE id = ?1", ("source-1",))
            connection.commit()
            cached_rows = connection.execute(
                "SELECT COUNT(*) FROM evidence_ledger_cache"
            ).fetchone()[0]
            connection.close()

        self.assertEqual(cached_rows, 0)

    def test_invalid_cached_unit_reference_degrades_to_cache_miss(self):
        source_material = source("Patient 1: I feel anxious most evenings.")
        initial_llm = FakeLLM(
            generations=["The patient reported anxiety most evenings."]
        )
        with tempfile.TemporaryDirectory() as directory:
            database_path = create_evidence_cache_database(directory, ("source-1",))
            with patch.dict(
                os.environ,
                {GIST_DATABASE_PATH_ENV: str(database_path)},
            ):
                prepare_evidence_with_backend(
                    initial_llm,
                    [source_material],
                    evidence_cache_key="initial-source-set",
                    evidence_model_identity="evidence-model@revision-1",
                )
                clear_evidence_cache()

                connection = sqlite3.connect(database_path)
                connection.execute(
                    "UPDATE evidence_ledger_cache SET payload_json = ?1",
                    ('[{"ordinal":1,"unit_ordinals":[999],"records":[]}]',),
                )
                connection.commit()
                connection.close()

                replacement_llm = FakeLLM(
                    generations=["The patient again reported anxiety most evenings."]
                )
                ledger = prepare_evidence_with_backend(
                    replacement_llm,
                    [source_material],
                    evidence_cache_key="new-process-source-set",
                    evidence_model_identity="evidence-model@revision-1",
                )

        self.assertEqual(len(replacement_llm.generate_calls), 1)
        self.assertEqual(
            ledger.records[0].claim,
            "The patient again reported anxiety most evenings.",
        )

    def test_complete_bundle_keeps_large_source_excerpt(self):
        ledger = EvidenceLedger(
            records=(
                EvidenceRecord(
                    evidence_id="E0001",
                    document_id="D1",
                    unit_ids=("D1U0001",),
                    evidence_type=EvidenceType.CLIENT_REPORT,
                    claim="Patient described details.",
                    source_excerpt=" ".join(["verbatim"] * 9000),
                    ordinal=1,
                ),
            ),
            document_count=1,
            unit_count=1,
            block_count=1,
            retry_count=0,
        )
        bundle, tokens = build_evidence_bundle(
            ledger,
            lambda text: len(text.split()),
        )
        self.assertIn("Evidence: verbatim verbatim", bundle)
        self.assertNotIn("D1U0001", bundle)
        self.assertGreater(tokens, 8000)

    def test_shadow_verification_reports_without_removing(self):
        ledger = EvidenceLedger(
            records=(
                EvidenceRecord(
                    evidence_id="E0001",
                    document_id="D1",
                    unit_ids=("D1U0001",),
                    evidence_type=EvidenceType.CLIENT_REPORT,
                    claim="Patient reported feeling anxious.",
                    source_excerpt="Patient: I feel anxious.",
                    ordinal=1,
                ),
            ),
            document_count=1,
            unit_count=1,
            block_count=1,
            retry_count=0,
        )
        llm = FakeLLM(choices=["SUPPORTED"])
        note, summary = verify_note(
            llm,
            "Patient reported feeling anxious.",
            ledger,
            mode="shadow",
        )
        self.assertEqual(note, "Patient reported feeling anxious.")
        self.assertEqual(summary.supported, 1)

    def test_enforced_verification_rejects_unsupported_critical_claim(self):
        ledger = EvidenceLedger(
            records=(),
            document_count=1,
            unit_count=1,
            block_count=1,
            retry_count=0,
        )
        llm = FakeLLM(choices=["UNSUPPORTED"])
        with self.assertRaises(VerificationError):
            verify_note(
                llm,
                "The patient was prescribed 20 mg of medication.",
                ledger,
                mode="enforce",
            )


class BatchRequestValidationTests(unittest.TestCase):
    def test_accepts_structured_batch_request(self):
        params = _params_for(
            {
                "type": "generate_notes",
                "sources": [
                    {
                        "id": "input-1",
                        "kind": "session_transcript",
                        "origin": "typed",
                        "title": "Session transcript",
                        "text": "Patient: I feel anxious.",
                    }
                ],
                "formats": [{"name": "soap"}],
                "verification_mode": "shadow",
            },
            "generate_notes",
        )
        self.assertEqual(params["formats"][0]["name"], "soap")

    def test_rejects_duplicate_source_ids(self):
        repeated = {
            "id": "input-1",
            "kind": "session_transcript",
            "origin": "typed",
            "title": "Session transcript",
            "text": "Patient: I feel anxious.",
        }
        with self.assertRaisesRegex(ValueError, "unique"):
            _params_for(
                {
                    "type": "generate_notes",
                    "sources": [repeated, repeated],
                    "formats": [{"name": "soap"}],
                },
                "generate_notes",
            )

    def test_diagnostic_capture_requires_a_session_uuid(self):
        with self.assertRaisesRegex(ValueError, "diagnostic_session_id"):
            _params_for(
                {
                    "type": "generate_notes",
                    "sources": [
                        {
                            "id": "input-1",
                            "kind": "session_transcript",
                            "origin": "typed",
                            "title": "Session transcript",
                            "text": "Patient: I feel anxious.",
                        }
                    ],
                    "formats": [{"name": "soap"}],
                    "capture_diagnostics": True,
                },
                "generate_notes",
            )


if __name__ == "__main__":
    unittest.main()
