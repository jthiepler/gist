pub mod core_audio_tap;
pub mod devices;
pub mod mic_capture;
pub mod mixer;
pub mod recorder;
pub mod wav_writer;

pub use devices::{enumerate_devices as list_audio_devices, AudioDeviceInfo};
pub use recorder::{
    get_recording_state, is_recording, start_recording, stop_recording, RecordingStatePayload,
    StopRecordingResult,
};
