use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

#[derive(Clone, Serialize, Debug)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub device_type: String, // "input" or "output"
}

pub fn enumerate_devices() -> anyhow::Result<Vec<AudioDeviceInfo>> {
    let host = cpal::default_host();
    let mut devices = Vec::new();

    for device in host.input_devices()? {
        if let Ok(name) = device.name() {
            devices.push(AudioDeviceInfo {
                name,
                device_type: "input".to_string(),
            });
        }
    }

    for device in host.output_devices()? {
        if let Ok(name) = device.name() {
            devices.push(AudioDeviceInfo {
                name,
                device_type: "output".to_string(),
            });
        }
    }

    Ok(devices)
}

pub fn resolve_input_device(name: Option<&str>) -> anyhow::Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = name {
        for device in host.input_devices()? {
            if let Ok(dn) = device.name() {
                if dn == name {
                    return Ok(device);
                }
            }
        }
        anyhow::bail!("Input device not found: {}", name);
    }
    host.default_input_device()
        .ok_or_else(|| anyhow::anyhow!("No default input device available"))
}

pub fn default_output_device_name() -> Option<String> {
    let host = cpal::default_host();
    host.default_output_device().and_then(|d| d.name().ok())
}
