"""SOAP note format — research-backed prompt."""
from __future__ import annotations

from typing import List, Optional

from ..llm.base import ChatMessage
from .base import ClinicalFormat


class SOAPFormat(ClinicalFormat):
    name = "soap"
    description = "SOAP note \u2014 Subjective, Objective, Assessment, Plan"

    def build_messages(
        self, transcript: str, language: Optional[str] = None
    ) -> List[ChatMessage]:
        lang_instr = self._language_instruction(language)

        system_prompt = f"""You are a clinical note-taking assistant for licensed therapists. Generate a SOAP note from a therapy session transcript.

{lang_instr}

Rules:
- Base all clinical statements ONLY on what the client says in the transcript.
- When information for a section is absent from the transcript, write \"Insufficient information in transcript.\"
- Do NOT fabricate or hallucinate diagnoses, symptoms, or history.
- Use person-first language (e.g., \"client with anxiety\" not \"anxious client\").
- Remove any identifying information (names, locations, dates) and replace with [deidentified].
- If the transcript mentions suicidal ideation, self-harm, or harm to others, include explicit risk assessment language.
- Use professional clinical terminology appropriate to the discipline.
- Maintain client dignity and avoid judgmental language.

Output format:

**Subjective:**
- Chief complaint / reason for session (client\u2019s own words)
- Symptoms reported (affect, mood, concerns)
- Relevant client statements (paraphrased, quoted)

**Objective:**
- Therapist observations (appearance, behavior, affect, engagement)
- Interventions used (CBT, MI, ACT, etc.)
- Client response to interventions

**Assessment:**
- Clinical impressions (supported by transcript evidence)
- Progress toward treatment goals
- Risk assessment (if applicable)

**Plan:**
- Next session focus
- Homework or between-session tasks
- Referrals or coordination (if indicated)"""

        user_prompt = f"""Generate a SOAP note from this therapy session transcript:

{transcript}"""

        return [
            ChatMessage(role="system", content=system_prompt),
            ChatMessage(role="user", content=user_prompt),
        ]
