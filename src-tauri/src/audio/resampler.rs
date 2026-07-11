/// A small stateful linear resampler for live mono audio streams.
///
/// Keeping the state between callback batches avoids clicks or time drift at
/// their boundaries. Speech transcription does not require a higher-order
/// filter, while this keeps the recording path allocation- and dependency-light.
pub struct LinearResampler {
    input_rate: u32,
    output_rate: u32,
    position: f64,
    previous_sample: Option<f32>,
}

impl LinearResampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Self {
        Self {
            input_rate,
            output_rate,
            position: 0.0,
            previous_sample: None,
        }
    }

    pub fn resample(&mut self, samples: &[f32], input_rate: u32) -> Vec<f32> {
        if samples.is_empty() {
            return Vec::new();
        }
        if input_rate != self.input_rate {
            self.input_rate = input_rate;
            self.position = 0.0;
            self.previous_sample = None;
        }
        if self.input_rate == self.output_rate {
            self.previous_sample = samples.last().copied();
            return samples.to_vec();
        }

        let mut input =
            Vec::with_capacity(samples.len() + usize::from(self.previous_sample.is_some()));
        if let Some(previous) = self.previous_sample {
            input.push(previous);
        }
        input.extend_from_slice(samples);
        if input.len() < 2 {
            self.previous_sample = input.last().copied();
            return Vec::new();
        }

        let step = self.input_rate as f64 / self.output_rate as f64;
        let mut output = Vec::with_capacity(
            ((samples.len() as f64 * self.output_rate as f64 / self.input_rate as f64).ceil())
                as usize,
        );
        while self.position + 1.0 < input.len() as f64 {
            let index = self.position.floor() as usize;
            let fraction = (self.position - index as f64) as f32;
            output.push(input[index] + (input[index + 1] - input[index]) * fraction);
            self.position += step;
        }
        self.position -= (input.len() - 1) as f64;
        self.previous_sample = input.last().copied();
        output
    }
}
