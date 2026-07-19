use anyhow::Result;
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(target_os = "macos")]
use cidre::{arc, av, cat, cf, core_audio as ca, os};

#[cfg(target_os = "macos")]
use cidre::ns;

struct WakerState {
    waker: Option<std::task::Waker>,
    has_data: bool,
}

#[cfg(target_os = "macos")]
struct AudioContext {
    format: arc::R<av::AudioFormat>,
    producer: HeapProd<f32>,
    waker_state: Arc<Mutex<WakerState>>,
    current_sample_rate: Arc<AtomicU32>,
    consecutive_drops: Arc<AtomicU32>,
    should_terminate: Arc<AtomicBool>,
}

pub struct CoreAudioTapHandle {
    consumer: HeapCons<f32>,
    #[cfg(target_os = "macos")]
    _device: ca::hardware::StartedDevice<ca::AggregateDevice>,
    #[cfg(target_os = "macos")]
    _ctx: Box<AudioContext>,
    #[cfg(target_os = "macos")]
    _tap: ca::TapGuard,
}

impl CoreAudioTapHandle {
    pub fn pop_batch(&mut self, max: usize) -> Vec<f32> {
        let mut batch = Vec::with_capacity(max);
        for _ in 0..max {
            if let Some(s) = self.consumer.try_pop() {
                batch.push(s);
            } else {
                break;
            }
        }
        batch
    }

    pub fn error(&self) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            if self._ctx.should_terminate.load(Ordering::Acquire) {
                return Some("Computer-audio capture fell behind and was stopped to avoid an incomplete recording.".into());
            }
        }
        None
    }

    pub fn sample_rate(&self) -> u32 {
        #[cfg(target_os = "macos")]
        {
            self._ctx.current_sample_rate.load(Ordering::Acquire)
        }
        #[cfg(not(target_os = "macos"))]
        {
            48_000
        }
    }
}

#[cfg(target_os = "macos")]
impl CoreAudioTapHandle {
    pub fn create(selected_device_id: Option<&str>) -> Result<Self> {
        let output_device = match selected_device_id {
            Some(id) => ca::System::devices()?
                .into_iter()
                .find(|device| {
                    device
                        .uid()
                        .map(|device_uid| device_uid.to_string() == id)
                        .unwrap_or(false)
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("Selected computer-audio device is no longer available")
                })?,
            None => ca::System::default_output_device()
                .map_err(|e| anyhow::anyhow!("Failed to get default output device: {:?}", e))?,
        };

        let output_uid = output_device
            .uid()
            .map_err(|e| anyhow::anyhow!("Failed to get device UID: {:?}", e))?;

        let tap_desc = ca::TapDesc::with_mono_global_tap_excluding_processes(&ns::Array::new());
        let tap = tap_desc
            .create_process_tap()
            .map_err(|e| anyhow::anyhow!("Failed to create process tap: {:?}", e))?;

        let tap_asbd = tap.asbd();
        let tap_sample_rate = tap_asbd
            .as_ref()
            .map(|a| a.sample_rate as u32)
            .unwrap_or(48000);

        let tap_uid = tap
            .uid()
            .map_err(|e| anyhow::anyhow!("Failed to get audio tap UID: {:?}", e))?;
        let sub_tap = cf::DictionaryOf::with_keys_values(
            &[ca::sub_device_keys::uid()],
            &[tap_uid.as_type_ref()],
        );

        let agg_desc = cf::DictionaryOf::with_keys_values(
            &[
                ca::aggregate_device_keys::is_private(),
                ca::aggregate_device_keys::is_stacked(),
                ca::aggregate_device_keys::tap_auto_start(),
                ca::aggregate_device_keys::name(),
                ca::aggregate_device_keys::main_sub_device(),
                ca::aggregate_device_keys::uid(),
                ca::aggregate_device_keys::tap_list(),
            ],
            &[
                cf::Boolean::value_true().as_type_ref(),
                cf::Boolean::value_false(),
                cf::Boolean::value_true(),
                cf::str!(c"gist-audio-tap").as_type_ref(),
                &output_uid,
                &cf::Uuid::new().to_cf_string(),
                &cf::ArrayOf::from_slice(&[sub_tap.as_ref()]),
            ],
        );

        let asbd = tap
            .asbd()
            .map_err(|e| anyhow::anyhow!("Failed to get tap ASBD: {:?}", e))?;
        let format = av::AudioFormat::with_asbd(&asbd)
            .ok_or_else(|| anyhow::anyhow!("Failed to create audio format"))?;

        let buffer_size = 1024 * 128;
        let rb = HeapRb::<f32>::new(buffer_size);
        let (producer, consumer) = rb.split();

        let waker_state = Arc::new(Mutex::new(WakerState {
            waker: None,
            has_data: false,
        }));

        let current_sample_rate = Arc::new(AtomicU32::new(tap_sample_rate));

        let mut ctx = Box::new(AudioContext {
            format,
            producer,
            waker_state: waker_state.clone(),
            current_sample_rate: current_sample_rate.clone(),
            consecutive_drops: Arc::new(AtomicU32::new(0)),
            should_terminate: Arc::new(AtomicBool::new(false)),
        });

        let agg_device = ca::AggregateDevice::with_desc(&agg_desc)
            .map_err(|e| anyhow::anyhow!("Failed to create aggregate device: {:?}", e))?;

        let proc_id = agg_device
            .create_io_proc_id(audio_proc, Some(&mut ctx))
            .map_err(|e| anyhow::anyhow!("Failed to create IO proc: {:?}", e))?;

        let started_device = ca::device_start(agg_device, Some(proc_id))
            .map_err(|e| anyhow::anyhow!("Failed to start device: {:?}", e))?;

        Ok(CoreAudioTapHandle {
            consumer,
            _device: started_device,
            _ctx: ctx,
            _tap: tap,
        })
    }
}

#[cfg(target_os = "macos")]
extern "C" fn audio_proc(
    device: ca::Device,
    _now: &cat::AudioTimeStamp,
    input_data: &cat::AudioBufList<1>,
    _input_time: &cat::AudioTimeStamp,
    _output_data: &mut cat::AudioBufList<1>,
    _output_time: &cat::AudioTimeStamp,
    ctx: Option<&mut AudioContext>,
) -> os::Status {
    let ctx = match ctx {
        Some(c) => c,
        None => return os::Status::NO_ERR,
    };

    let after = device
        .nominal_sample_rate()
        .unwrap_or(ctx.format.absd().sample_rate) as u32;
    let before = ctx.current_sample_rate.load(Ordering::Acquire);
    if before != after {
        ctx.current_sample_rate.store(after, Ordering::Release);
    }

    if let Some(view) = av::AudioPcmBuf::with_buf_list_no_copy(&ctx.format, input_data, None) {
        if let Some(data) = view.data_f32_at(0) {
            push_samples(ctx, data);
        }
    } else if ctx.format.common_format() == av::audio::CommonFormat::PcmF32 {
        let first_buffer = &input_data.buffers[0];
        let byte_count = first_buffer.data_bytes_size as usize;
        let float_count = byte_count / std::mem::size_of::<f32>();

        if float_count > 0 && !first_buffer.data.is_null() {
            let data =
                unsafe { std::slice::from_raw_parts(first_buffer.data as *const f32, float_count) };
            push_samples(ctx, data);
        }
    }

    os::Status::NO_ERR
}

#[cfg(target_os = "macos")]
fn push_samples(ctx: &mut AudioContext, data: &[f32]) {
    let pushed = ctx.producer.push_slice(data);

    if pushed < data.len() {
        let consecutive = ctx.consecutive_drops.fetch_add(1, Ordering::AcqRel) + 1;
        if consecutive > 10 {
            ctx.should_terminate.store(true, Ordering::Release);
            return;
        }
    } else {
        ctx.consecutive_drops.store(0, Ordering::Release);
    }

    if pushed > 0 {
        let should_wake = {
            let mut waker_state = ctx.waker_state.lock().unwrap();
            if !waker_state.has_data {
                waker_state.has_data = true;
                waker_state.waker.take()
            } else {
                None
            }
        };

        if let Some(waker) = should_wake {
            waker.wake();
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl CoreAudioTapHandle {
    pub fn create(_selected_device_name: Option<&str>) -> Result<Self> {
        anyhow::bail!("System audio capture is only supported on macOS");
    }
}

#[cfg(target_os = "macos")]
impl Drop for CoreAudioTapHandle {
    fn drop(&mut self) {
        self._ctx.should_terminate.store(true, Ordering::Release);
    }
}
