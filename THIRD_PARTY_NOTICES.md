# Third-Party Notices

## Backup and encryption libraries

Gist uses the Rust implementations of
[`age`](https://github.com/str4d/rage) (MIT or Apache-2.0),
[`sha2`](https://github.com/RustCrypto/hashes) (MIT or Apache-2.0),
[`tempfile`](https://github.com/Stebalien/tempfile) (MIT or Apache-2.0), and
[`zip`](https://github.com/zip-rs/zip2) (MIT) to create, validate, encrypt, and
stage data exports. Their source code and license texts are available from the
linked upstream projects and the corresponding crates published on
[crates.io](https://crates.io/).

## pyannote Community-1

Gist can use the locally bundled `pyannote/speaker-diarization-community-1`
pipeline for speaker diarization. The pipeline and its model components are
provided by pyannote and are used under the terms shown by the model
repository, including Creative Commons Attribution 4.0 International
(CC BY 4.0):

https://creativecommons.org/licenses/by/4.0/

The pipeline includes the segmentation, speaker embedding, and PLDA components
described in its upstream README. Upstream documentation and citations are
available at:

https://huggingface.co/pyannote/speaker-diarization-community-1

The model files are kept outside Git and are supplied locally at build time.

## Parakeet TDT 0.6B v3 MLX 4-bit

Gist bundles the 4-bit MLX conversion of NVIDIA's Parakeet TDT 0.6B v3 speech
recognition model. It is provided by animaslabs under the Creative Commons
Attribution 4.0 International (CC BY 4.0) license:

https://creativecommons.org/licenses/by/4.0/

Upstream model card:

https://huggingface.co/animaslabs/parakeet-tdt-0.6b-v3-mlx-4bit

The model files are kept outside Git and are supplied locally at build time.
