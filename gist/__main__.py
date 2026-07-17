"""CLI entry point: `python -m gist` or `gist`.

Stateless: no database, no persistence. Transcribe produces text,
note consumes text. The caller handles storage.
"""
from __future__ import annotations

import logging
import sys
from pathlib import Path

import click

from . import __version__
from .formats.registry import list_formats
from .models import (
    DEFAULT_LLM,
    LLM_MODELS,
)

log = logging.getLogger(__name__)


@click.group(invoke_without_command=True)
@click.version_option(__version__)
@click.option("-v", "--verbose", is_flag=True, help="Enable debug logging.")
@click.pass_context
def cli(ctx, verbose):
    logging.basicConfig(
        level=logging.DEBUG if verbose else logging.INFO,
        format="%(asctime)s %(levelname)-8s %(message)s",
        datefmt="%H:%M:%S",
    )
    if ctx.invoked_subcommand is None:
        click.echo(ctx.get_help())


@cli.command()
@click.argument("audio_file", type=click.Path(exists=True))
@click.option("-o", "--output", type=click.Path(), help="Write transcript to file (default: stdout).")
def transcribe(audio_file, output):
    """Transcribe an audio file to text."""
    from .pipeline import transcribe_audio

    click.echo("Transcribing...", err=True)
    result = transcribe_audio(audio_file)

    output_text = result.text
    if output:
        Path(output).write_text(output_text, encoding="utf-8")
        click.echo(f"Transcript written to {output}")
    else:
        click.echo(output_text)

    click.echo(
        f"\nDuration: {result.duration:.1f}s  |  Segments: {len(result.segments)}",
        err=True,
    )


@cli.command()
@click.option("-t", "--transcript", "transcript_file", type=click.Path(exists=True), help="Read transcript from file (default: read from stdin).")
@click.option("-f", "--format", "format_name", default="soap", help="Note format name (default: soap). Use 'gist formats' to list available formats.")
@click.option("-o", "--output", type=click.Path(), help="Write note to file (default: stdout).")
@click.option("--model", default=DEFAULT_LLM, help=f"LLM model (default: {DEFAULT_LLM}). Use 'gist models' to list.")
@click.option("--max-tokens", default=4096, help="Maximum tokens for LLM generation (default: 4096)")
@click.option("--thinking/--no-thinking", default=False, help="Enable or disable chain-of-thought reasoning (default: disabled)")
def note(transcript_file, format_name, output, model, max_tokens, thinking):
    """Generate a clinical note from a transcript."""
    from .formats.registry import get_format
    from .pipeline import generate_note as _generate_note

    if transcript_file:
        transcript = Path(transcript_file).read_text(encoding="utf-8")
    else:
        click.echo("Paste transcript (Ctrl+D to end):", err=True)
        transcript = sys.stdin.read()

    if not transcript.strip():
        click.echo("Error: empty transcript", err=True)
        sys.exit(1)

    result = _generate_note(
        transcript=transcript,
        format_name=format_name,
        llm_model=model,
        max_tokens=max_tokens,
        thinking=thinking,
    )

    if output:
        Path(output).write_text(result, encoding="utf-8")
        click.echo(f"Note written to {output}")
    else:
        click.echo(result)


@cli.command()
def formats():
    """List available clinical note formats."""
    for fmt in list_formats():
        click.echo(f"  {fmt['name']:<12} {fmt['description']}")


@cli.group()
def models():
    """List available models."""
    pass


@models.command(name="list")
def list_models():
    """Show all available local language models."""
    click.echo("\nLLM models:")
    for name, spec in LLM_MODELS.items():
        default = " (default)" if spec.default else ""
        click.echo(f"  {name:<20} {spec.display:<20} {spec.backend:<8} ~{spec.size_gb:.1f} GB{default}")

@cli.command()
@click.argument("model", required=False)
@click.option("--kind", type=click.Choice(["llm"]), help="Filter by model kind (default: download all LLM defaults).")
def download(model, kind):
    """Download models from HuggingFace Hub.

    If MODEL is specified, download that language model.
    If neither is specified, download the default model, which also performs
    evidence extraction.
    """
    from .downloader import download_model

    if model:
        # Determine kind from model
        if model in LLM_MODELS:
            models_to_download = [(model, "llm")]
        else:
            click.echo(f"Unknown model: {model}", err=True)
            sys.exit(1)
    elif kind == "llm":
        models_to_download = [(name, "llm") for name in LLM_MODELS]
    else:
        models_to_download = [(DEFAULT_LLM, "llm")]

    for name, kind in models_to_download:
        click.echo(f"Downloading {kind} model: {name}...")
        try:
            download_model(name, kind=kind)
            click.echo(f"  Done")
        except Exception as e:
            click.echo(f"  Failed: {e}", err=True)


@cli.command()
def serve():
    """Run as JSON-RPC sidecar (reads JSON from stdin, writes to stdout)."""
    from .server import run_server

    run_server()
