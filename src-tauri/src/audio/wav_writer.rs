use anyhow::Result;
use hound::{SampleFormat, WavSpec, WavWriter};
use std::fs::File;
use std::io::BufWriter;
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
        let writer = WavWriter::create(path, spec)?;
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
