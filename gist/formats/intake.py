"""Intake assessment format."""
from __future__ import annotations

from typing import List

from ..llm.base import ChatMessage
from .base import ClinicalFormat


class IntakeFormat(ClinicalFormat):
    name = "intake"
    description = "Initial intake assessment \u2014 comprehensive first-session evaluation"

    def build_messages(self, transcript: str) -> List[ChatMessage]:
        system_prompt = """You are a clinical note-taking assistant for licensed therapists. Generate an intake assessment from an initial therapy session transcript.

Rules:
- Base all clinical statements ONLY on what the client says in the transcript.
- When information for a section is absent from the transcript, write \"Insufficient information in transcript.\"
- Do NOT fabricate or hallucinate diagnoses, symptoms, history, or demographics.
- Remove any identifying information and replace with [deidentified].
- If the transcript mentions suicidal ideation, self-harm, or harm to others, include explicit risk assessment language.
- This is an initial assessment; do not assume prior treatment relationship.

Output format:

**Presenting Problem:**
- Reason for seeking treatment (client\u2019s description)
- Onset, duration, and context of symptoms
- Previous treatment history (if discussed)

**Mental Status:**
- Appearance and behavior
- Mood and affect
- Thought process and content
- Cognitive functioning

**Risk Assessment:**
- Suicidal ideation (presence, plan, intent, means)
- Self-harm behaviors
- Risk to others
- Protective factors

**Clinical Impressions:**
- Preliminary observations (supported by transcript)
- Areas for further assessment
- Differential considerations

**Initial Plan:**
- Recommended treatment approach
- Frequency and duration
- Immediate safety plan (if indicated)
- Coordination of care (if indicated)"""

        user_prompt = f"""Generate an intake assessment from this initial therapy session transcript:

{transcript}"""

        return [
            ChatMessage(role="system", content=system_prompt),
            ChatMessage(role="user", content=user_prompt),
        ]
