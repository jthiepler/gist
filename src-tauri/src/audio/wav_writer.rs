use anyhow::Result;
use hound::{SampleFormat, WavSpec, WavWriter};
use std::fs::{File, OpenOptions};
use std::io::BufWriter;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub struct StreamingWavWriter {
    writer: WavWriter<BufWriter<File>>,
}

impl StreamingWavWriter {
    pub fn create(path: &Path, sample_rate: u32) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        options.mode(0o600);
        let file = options.open(path)?;
        let writer = WavWriter::new(BufWriter::new(file), spec)?;
        Ok(Self { writer })
    }

    pub fn write_samples(&mut self, samples: &[f32]) -> Result<()> {
        for &sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            let i16_sample = (clamped * i16::MAX as f32) as i16;
            self.writer.write_sample(i16_sample)?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }

    pub fn finalize(self) -> Result<()> {
        self.writer.finalize()?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn recordings_are_created_with_private_permissions() {
        let directory = tempfile::TempDir::new().expect("temporary directory");
        let path = directory.path().join("recording.wav");
        let writer = StreamingWavWriter::create(&path, 48_000).expect("WAV writer");
        writer.finalize().expect("finalize WAV");
        let mode = std::fs::metadata(path)
            .expect("recording metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
