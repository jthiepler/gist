use std::collections::VecDeque;

pub struct Mixer {
    mic_buffer: VecDeque<f32>,
    sys_buffer: VecDeque<f32>,
    max_buffer_size: usize,
}

impl Mixer {
    pub fn new(sample_rate: u32) -> Self {
        let max_buffer_size = (sample_rate as usize) * 2; // 2 seconds max
        Self {
            mic_buffer: VecDeque::with_capacity(max_buffer_size),
            sys_buffer: VecDeque::with_capacity(max_buffer_size),
            max_buffer_size,
        }
    }

    pub fn add_mic(&mut self, samples: &[f32]) {
        self.mic_buffer.extend(samples);
        while self.mic_buffer.len() > self.max_buffer_size {
            self.mic_buffer.pop_front();
        }
    }

    pub fn add_sys(&mut self, samples: &[f32]) {
        self.sys_buffer.extend(samples);
        while self.sys_buffer.len() > self.max_buffer_size {
            self.sys_buffer.pop_front();
        }
    }

    pub fn drain_mixed(&mut self) -> Vec<f32> {
        if self.mic_buffer.is_empty() && !self.sys_buffer.is_empty() {
            self.sys_buffer.drain(..).collect()
        } else if !self.mic_buffer.is_empty() && self.sys_buffer.is_empty() {
            self.mic_buffer.drain(..).collect()
        } else {
            let count = self.mic_buffer.len().min(self.sys_buffer.len());
            let mut mixed = Vec::with_capacity(count);
            for _ in 0..count {
                let m = self.mic_buffer.pop_front().unwrap_or(0.0);
                let s = self.sys_buffer.pop_front().unwrap_or(0.0);
                mixed.push((m + s) * 0.5);
            }
            mixed
        }
    }

    pub fn has_data(&self) -> bool {
        !self.mic_buffer.is_empty() || !self.sys_buffer.is_empty()
    }
}
