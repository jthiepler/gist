use crate::audio::core_audio_tap::CoreAudioTapHandle;
use crate::audio::devices;
use crate::audio::mic_capture::MicCapture;
use crate::audio::mixer::Mixer;
use crate::audio::resampler::LinearResampler;
use crate::audio::wav_writer::StreamingWavWriter;
use anyhow::Result;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

static IS_RECORDING: AtomicBool = AtomicBool::new(false);

struct RecordingState {
    job_id: String,
    mic: Option<MicCapture>,
    tap: Option<CoreAudioTapHandle>,
    mic_resampler: LinearResampler,
    tap_resampler: LinearResampler,
    mixer: Mixer,
    wav_writer: Option<StreamingWavWriter>,
    active_started_at: Option<Instant>,
    accumulated_elapsed: Duration,
    is_paused: bool,
    file_path: String,
    tick_handle: Option<tauri::async_runtime::JoinHandle<()>>,
    _sleep_assertion: SleepAssertion,
}

unsafe impl Send for RecordingState {}

static RECORDER: Mutex<Option<RecordingState>> = Mutex::new(None);

const TARGET_SAMPLE_RATE: u32 = 48000;
const MIN_RECORDING_SECONDS: f64 = 5.0;

/// Keeps macOS awake while audio is being captured. This is deliberately
/// process-scoped and released automatically when recording stops or fails.
pub struct SleepAssertion {
    #[cfg(target_os = "macos")]
    child: Option<std::process::Child>,
}

impl SleepAssertion {
    pub fn acquire(reason: &str) -> Self {
        #[cfg(target_os = "macos")]
        {
            let child = std::process::Command::new("/usr/bin/caffeinate")
                .args(["-i", "-m", "-s", "-w"])
                .arg(std::process::id().to_string())
                .env("GIST_ACTIVITY_REASON", reason)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok();
            return Self { child };
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = reason;
            Self {}
        }
    }
}

impl Drop for SleepAssertion {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Clone, Serialize)]
pub struct RecordingTickPayload {
    pub elapsed_seconds: f64,
    pub level: f32,
    pub is_paused: bool,
}

#[derive(Clone, Serialize)]
pub struct RecordingErrorPayload {
    pub message: String,
}

#[derive(Clone, Serialize)]
pub struct RecordingStatePayload {
    pub is_recording: bool,
    pub is_paused: bool,
    pub elapsed_seconds: f64,
    pub has_recording: bool,
    pub file_path: Option<String>,
    pub job_id: Option<String>,
}

pub fn is_recording() -> bool {
    IS_RECORDING.load(Ordering::Acquire)
}

pub fn get_recording_state() -> RecordingStatePayload {
    let recorder = RECORDER.lock().unwrap();
    let elapsed = if let Some(ref state) = *recorder {
        recorded_elapsed(state).as_secs_f64()
    } else {
        0.0
    };
    let is_paused = if let Some(ref state) = *recorder {
        state.is_paused
    } else {
        false
    };
    let file_path = if let Some(ref state) = *recorder {
        Some(state.file_path.clone())
    } else {
        None
    };
    let job_id = if let Some(ref state) = *recorder {
        Some(state.job_id.clone())
    } else {
        None
    };
    RecordingStatePayload {
        is_recording: IS_RECORDING.load(Ordering::Acquire),
        is_paused,
        elapsed_seconds: elapsed,
        has_recording: recorder.is_some(),
        file_path,
        job_id,
    }
}

pub fn start_recording(
    app: AppHandle,
    mic_device: Option<String>,
    system_device: Option<String>,
    job_id: String,
    file_path: std::path::PathBuf,
) -> Result<()> {
    if IS_RECORDING.load(Ordering::Acquire)
        || RECORDER
            .lock()
            .map_err(|_| anyhow::anyhow!("Recording state is unavailable"))?
            .is_some()
    {
        anyhow::bail!("Recording is already in progress");
    }

    let wav_writer = StreamingWavWriter::create(&file_path, TARGET_SAMPLE_RATE)?;

    let mixer = Mixer::new(TARGET_SAMPLE_RATE);

    // Start mic capture
    let mic = match devices::resolve_input_device(mic_device.as_deref()) {
        Ok(device) => match MicCapture::create(&device, TARGET_SAMPLE_RATE) {
            Ok(mic) => Some(mic),
            Err(_) => None,
        },
        Err(_) => None,
    };

    if mic.is_none() {
        wav_writer.finalize()?;
        anyhow::bail!("Failed to start microphone capture — cannot record without a mic");
    }

    // Start system audio tap (macOS)
    let tap = match CoreAudioTapHandle::create(system_device.as_deref()) {
        Ok(tap) => Some(tap),
        Err(_) => None,
    };

    let file_path_str = file_path.to_string_lossy().to_string();

    let state = RecordingState {
        job_id: job_id.clone(),
        mic,
        tap,
        mic_resampler: LinearResampler::new(TARGET_SAMPLE_RATE, TARGET_SAMPLE_RATE),
        tap_resampler: LinearResampler::new(TARGET_SAMPLE_RATE, TARGET_SAMPLE_RATE),
        mixer,
        wav_writer: Some(wav_writer),
        active_started_at: Some(Instant::now()),
        accumulated_elapsed: Duration::ZERO,
        is_paused: false,
        file_path: file_path_str.clone(),
        tick_handle: None,
        _sleep_assertion: SleepAssertion::acquire("Gist is recording session audio"),
    };

    {
        let mut recorder = RECORDER.lock().unwrap();
        *recorder = Some(state);
    }

    IS_RECORDING.store(true, Ordering::Release);

    // Emit recording-started event
    let _ = app.emit("recording-started", serde_json::json!({}));

    // Spawn tick task
    let app_handle = app.clone();
    let tick_handle = tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(250));
        loop {
            interval.tick().await;
            if !IS_RECORDING.load(Ordering::Acquire) {
                break;
            }

            let payload = {
                let mut recorder = RECORDER.lock().unwrap();
                if let Some(ref mut state) = *recorder {
                    let level = if state.is_paused {
                        drain_and_discard(state);
                        0.0
                    } else {
                        let mixed = match drain_and_write(state) {
                            Ok(mixed) => mixed,
                            Err(error) => {
                                IS_RECORDING.store(false, Ordering::Release);
                                let _ = app_handle.emit(
                                    "recording-error",
                                    RecordingErrorPayload {
                                        message: error.to_string(),
                                    },
                                );
                                break;
                            }
                        };
                        compute_level(&mixed)
                    };

                    let elapsed = recorded_elapsed(state).as_secs_f64();
                    RecordingTickPayload {
                        elapsed_seconds: elapsed,
                        level,
                        is_paused: state.is_paused,
                    }
                } else {
                    break;
                }
            };

            let _ = app_handle.emit("recording-tick", payload);
        }
    });

    {
        let mut recorder = RECORDER.lock().unwrap();
        if let Some(ref mut state) = *recorder {
            state.tick_handle = Some(tick_handle);
        }
    }

    Ok(())
}

fn recorded_elapsed(state: &RecordingState) -> Duration {
    state.accumulated_elapsed
        + state
            .active_started_at
            .map(|started_at| started_at.elapsed())
            .unwrap_or(Duration::ZERO)
}

fn drain_inputs(state: &mut RecordingState) -> Result<()> {
    if let Some(ref mut mic) = state.mic {
        if let Some(error) = mic.error() {
            anyhow::bail!(error);
        }
        let mic_samples = mic.drain();
        if !mic_samples.is_empty() {
            let samples = state
                .mic_resampler
                .resample(&mic_samples, mic.sample_rate());
            state.mixer.add_mic(&samples);
        }
    }

    if let Some(ref mut tap) = state.tap {
        if let Some(error) = tap.error() {
            anyhow::bail!(error);
        }
        let sys_samples = tap.pop_batch(TARGET_SAMPLE_RATE as usize);
        if !sys_samples.is_empty() {
            let samples = state
                .tap_resampler
                .resample(&sys_samples, tap.sample_rate());
            state.mixer.add_sys(&samples);
        }
    }
    Ok(())
}

fn drain_and_discard(state: &mut RecordingState) {
    let _ = drain_inputs(state);
    let _ = state.mixer.drain_mixed();
}

fn drain_and_write(state: &mut RecordingState) -> Result<Vec<f32>> {
    drain_inputs(state)?;

    let mixed = state.mixer.drain_mixed();
    if !mixed.is_empty() {
        let should_flush = recorded_elapsed(state).as_secs() % 5 == 0;
        if let Some(ref mut writer) = state.wav_writer {
            writer.write_samples(&mixed)?;
            if should_flush {
                writer.flush()?;
            }
        }
    }
    Ok(mixed)
}

fn compute_level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    let rms = (sum_sq / samples.len() as f32).sqrt();
    // Apply a nonlinear curve (sqrt) so quiet signals are more visible.
    // Normal speech at a comfortable distance produces ~0.01–0.05 RMS;
    // sqrt maps that to ~0.1–0.22, which is clearly visible on the bar.
    (rms.sqrt() * 3.0).min(1.0)
}

pub fn pause_recording(app: AppHandle) -> Result<()> {
    if !IS_RECORDING.load(Ordering::Acquire) {
        anyhow::bail!("No recording in progress");
    }

    let payload = {
        let mut recorder = RECORDER.lock().unwrap();
        let state = recorder
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No recording state found"))?;

        if !state.is_paused {
            drain_and_write(state)?;
            if let Some(started_at) = state.active_started_at.take() {
                state.accumulated_elapsed += started_at.elapsed();
            }
            state.is_paused = true;
        }

        RecordingTickPayload {
            elapsed_seconds: recorded_elapsed(state).as_secs_f64(),
            level: 0.0,
            is_paused: true,
        }
    };

    let _ = app.emit("recording-paused", serde_json::json!({}));
    let _ = app.emit("recording-tick", payload);
    Ok(())
}

pub fn resume_recording(app: AppHandle) -> Result<()> {
    if !IS_RECORDING.load(Ordering::Acquire) {
        anyhow::bail!("No recording in progress");
    }

    let payload = {
        let mut recorder = RECORDER.lock().unwrap();
        let state = recorder
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("No recording state found"))?;

        if state.is_paused {
            drain_and_discard(state);
            state.active_started_at = Some(Instant::now());
            state.is_paused = false;
        }

        RecordingTickPayload {
            elapsed_seconds: recorded_elapsed(state).as_secs_f64(),
            level: 0.0,
            is_paused: false,
        }
    };

    let _ = app.emit("recording-resumed", serde_json::json!({}));
    let _ = app.emit("recording-tick", payload);
    Ok(())
}

#[derive(Clone, Serialize)]
pub struct StopRecordingResult {
    pub job_id: String,
    pub file_path: String,
    pub duration_seconds: f64,
    pub is_short_recording: bool,
}

pub fn stop_recording(_app: AppHandle) -> Result<StopRecordingResult> {
    IS_RECORDING.store(false, Ordering::Release);

    let (job_id, file_path, duration) = {
        let mut recorder = RECORDER.lock().unwrap();
        let state = recorder
            .take()
            .ok_or_else(|| anyhow::anyhow!("No recording state found"))?;

        if let Some(handle) = &state.tick_handle {
            handle.abort();
        }

        // Final drain
        let mut state = state;
        if state.is_paused {
            drain_and_discard(&mut state);
        } else {
            drain_and_write(&mut state)?;
            if let Some(started_at) = state.active_started_at.take() {
                state.accumulated_elapsed += started_at.elapsed();
            }
        }

        if let Some(writer) = state.wav_writer.take() {
            writer.finalize()?;
        }

        if let Some(ref mic) = state.mic {
            mic.stop();
        }

        let duration = state.accumulated_elapsed.as_secs_f64();
        (state.job_id.clone(), state.file_path.clone(), duration)
    };

    Ok(StopRecordingResult {
        job_id,
        file_path,
        duration_seconds: duration,
        is_short_recording: duration < MIN_RECORDING_SECONDS,
    })
}
