use anyhow::Result;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::SampleFormat;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct MicCapture {
    stream: cpal::Stream,
    buffer: Arc<Mutex<Vec<f32>>>,
    stop_flag: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
    sample_rate: u32,
}

// SAFETY: cpal::Stream is not Send by default, but we only access it from
// controlled contexts. The buffer is protected by a Mutex.
unsafe impl Send for MicCapture {}

impl MicCapture {
    pub fn create(device: &cpal::Device, preferred_sample_rate: u32) -> Result<Self> {
        let supported = device.default_input_config()?;
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        let config = cpal::StreamConfig {
            channels: supported.channels(),
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let _ = preferred_sample_rate;

        let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(48000)));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

        let buf_clone = buffer.clone();
        let stop_clone = stop_flag.clone();
        let sf = supported.sample_format();

        let stream =
            match sf {
                SampleFormat::F32 => device.build_input_stream(
                    &config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        if !stop_clone.load(Ordering::Relaxed) {
                            let mut b = buf_clone.lock().unwrap();
                            b.extend(data.chunks(channels).map(|frame| {
                                frame.iter().copied().sum::<f32>() / frame.len() as f32
                            }));
                        }
                    },
                    {
                        let error = error.clone();
                        move |err| {
                            let message = format!("Microphone capture stopped: {}", err);
                            if let Ok(mut slot) = error.lock() {
                                *slot = Some(message);
                            }
                        }
                    },
                    None,
                )?,
                SampleFormat::I16 => {
                    let buf_clone2 = buffer.clone();
                    let stop_clone2 = stop_flag.clone();
                    device.build_input_stream(
                        &config,
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            if !stop_clone2.load(Ordering::Relaxed) {
                                let mut b = buf_clone2.lock().unwrap();
                                b.extend(data.chunks(channels).map(|frame| {
                                    frame
                                        .iter()
                                        .map(|&s| s as f32 / i16::MAX as f32)
                                        .sum::<f32>()
                                        / frame.len() as f32
                                }));
                            }
                        },
                        {
                            let error = error.clone();
                            move |err| {
                                let message = format!("Microphone capture stopped: {}", err);
                                if let Ok(mut slot) = error.lock() {
                                    *slot = Some(message);
                                }
                            }
                        },
                        None,
                    )?
                }
                SampleFormat::I32 => {
                    let buf_clone3 = buffer.clone();
                    let stop_clone3 = stop_flag.clone();
                    device.build_input_stream(
                        &config,
                        move |data: &[i32], _: &cpal::InputCallbackInfo| {
                            if !stop_clone3.load(Ordering::Relaxed) {
                                let mut b = buf_clone3.lock().unwrap();
                                b.extend(data.chunks(channels).map(|frame| {
                                    frame
                                        .iter()
                                        .map(|&s| s as f32 / i32::MAX as f32)
                                        .sum::<f32>()
                                        / frame.len() as f32
                                }));
                            }
                        },
                        {
                            let error = error.clone();
                            move |err| {
                                let message = format!("Microphone capture stopped: {}", err);
                                if let Ok(mut slot) = error.lock() {
                                    *slot = Some(message);
                                }
                            }
                        },
                        None,
                    )?
                }
                _ => {
                    anyhow::bail!("Unsupported sample format: {:?}", sf);
                }
            };

        stream.play()?;

        Ok(MicCapture {
            stream,
            buffer,
            stop_flag,
            error,
            sample_rate,
        })
    }

    pub fn drain(&self) -> Vec<f32> {
        let mut b = self.buffer.lock().unwrap();
        let drained = b.drain(..).collect();
        drained
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Release);
        let _ = self.stream.pause();
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|error| error.clone())
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

impl Drop for MicCapture {
    fn drop(&mut self) {
        self.stop();
    }
}
