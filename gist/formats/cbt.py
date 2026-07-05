"""CBT session note format."""
from __future__ import annotations

from typing import List, Optional

from ..llm.base import ChatMessage
from .base import ClinicalFormat


class CBTFormat(ClinicalFormat):
    name = "cbt"
    description = "CBT session note \u2014 Cognitive Behavioral Therapy format"

    def build_messages(
        self, transcript: str, language: Optional[str] = None
    ) -> List[ChatMessage]:
        lang_instr = self._language_instruction(language)

        system_prompt = f"""You are a clinical note-taking assistant for licensed therapists. Generate a CBT session note from a therapy session transcript.

{lang_instr}

Rules:
- Base all clinical statements ONLY on what the client says in the transcript.
- When information for a section is absent, write \"Insufficient information in transcript.\"
- Do NOT fabricate or hallucinate diagnoses, symptoms, or history.
- Remove any identifying information and replace with [deidentified].
- If the transcript mentions suicidal ideation, self-harm, or harm to others, include explicit risk assessment language.

Output format:

**Session Overview:**
- Session number / phase of treatment (if discernible)
- Presenting concerns and session focus

**Cognitive Conceptualization:**
- Automatic thoughts identified
- Cognitive distortions noted
- Core beliefs / schemas addressed

**Behavioral Interventions:**
- Behavioral activation tasks
- Exposure work (if applicable)
- Homework review

**Cognitive Interventions:**
- Socratic dialogue / guided discovery
- Cognitive restructuring
- Behavioral experiments discussed

**Progress and Plan:**
- Progress toward goals
- Homework assigned
- Next session focus"""

        user_prompt = f"""Generate a CBT session note from this therapy session transcript:

{transcript}"""

        return [
            ChatMessage(role="system", content=system_prompt),
            ChatMessage(role="user", content=user_prompt),
        ]
