"""Source-audio normalization shared by transcription and diarization."""
from __future__ import annotations

import logging
import os
import subprocess
import tempfile
import wave
from pathlib import Path
from typing import Any, Optional

log = logging.getLogger(__name__)

NORMALIZED_SAMPLE_RATE = 16000
NORMALIZED_CHANNELS = 1
NORMALIZED_SAMPLE_WIDTH = 2  # signed 16-bit PCM


def _stream_to_wav(source_path: str, output_path: str, cancel_event: Any = None) -> None:
    import miniaudio

    with wave.open(output_path, "wb") as output:
        output.setnchannels(NORMALIZED_CHANNELS)
        output.setsampwidth(NORMALIZED_SAMPLE_WIDTH)
        output.setframerate(NORMALIZED_SAMPLE_RATE)
        for chunk in miniaudio.stream_file(
            source_path,
            output_format=miniaudio.SampleFormat.SIGNED16,
            nchannels=NORMALIZED_CHANNELS,
            sample_rate=NORMALIZED_SAMPLE_RATE,
        ):
            if cancel_event and cancel_event.is_set():
                raise InterruptedError("Transcription cancelled")
            output.writeframes(chunk.tobytes())


def _convert_with_system_decoder(
    source_path: str,
    output_path: str,
    cancel_event: Any = None,
) -> None:
    """Use macOS's built-in decoder for formats miniaudio cannot read."""
    if cancel_event and cancel_event.is_set():
        raise InterruptedError("Transcription cancelled")

    decoder = "/usr/bin/afconvert"
    if not Path(decoder).is_file():
        raise RuntimeError("The uploaded audio format is not supported on this Mac.")

    try:
        subprocess.run(
            [
                decoder,
                "-f",
                "WAVE",
                "-d",
                "LEI16@16000",
                "-c",
                "1",
                source_path,
                output_path,
            ],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
    except subprocess.CalledProcessError as error:
        raise RuntimeError("The uploaded audio could not be converted to WAV.") from error


def normalize_audio_for_pipeline(
    audio_path: str,
    cancel_event: Any = None,
) -> tuple[str, Optional[Path]]:
    """Return one pipeline path and an optional temporary file to clean up.

    WAV inputs are already in the pipeline's stable container format and are
    used directly. Every other input is converted to 16 kHz mono PCM WAV once,
    before transcription starts, so every downstream stage reads identical
    samples.
    """
    if Path(audio_path).suffix.lower() == ".wav":
        return audio_path, None

    file_descriptor, normalized_path = tempfile.mkstemp(
        prefix="gist-source-",
        suffix=".wav",
    )
    os.close(file_descriptor)

    try:
        try:
            _stream_to_wav(audio_path, normalized_path, cancel_event=cancel_event)
        except InterruptedError:
            raise
        except Exception:
            # miniaudio covers MP3/FLAC/Ogg and similar formats. Fall back to
            # macOS's native decoder for AAC/M4A and other unsupported inputs.
            with open(normalized_path, "wb"):
                pass
            _convert_with_system_decoder(
                audio_path,
                normalized_path,
                cancel_event=cancel_event,
            )
    except BaseException:
        try:
            os.unlink(normalized_path)
        except OSError:
            pass
        raise

    log.info("event=source_audio_normalized source_format=%s", Path(audio_path).suffix.lower())
    return normalized_path, Path(normalized_path)


def cleanup_normalized_audio(normalized_path: Optional[Path]) -> None:
    if normalized_path is None:
        return
    try:
        normalized_path.unlink(missing_ok=True)
    except OSError:
        log.warning("event=source_audio_cleanup_failed")
