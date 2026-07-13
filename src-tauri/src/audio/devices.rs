use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

#[cfg(target_os = "macos")]
use cidre::core_audio as ca;

#[derive(Clone, Serialize, Debug)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub device_type: String, // "input" or "output"
}

pub fn enumerate_devices() -> anyhow::Result<Vec<AudioDeviceInfo>> {
    let host = cpal::default_host();
    let mut devices = Vec::new();

    for (index, device) in host.input_devices()?.enumerate() {
        if let Ok(name) = device.name() {
            devices.push(AudioDeviceInfo {
                id: format!("input:{index}"),
                name,
                device_type: "input".to_string(),
            });
        }
    }

    enumerate_output_devices(&mut devices)?;

    Ok(devices)
}

#[cfg(target_os = "macos")]
fn enumerate_output_devices(devices: &mut Vec<AudioDeviceInfo>) -> anyhow::Result<()> {
    for device in ca::System::devices()? {
        let has_output = device
            .output_stream_cfg()
            .map(|config| {
                config
                    .buffers()
                    .iter()
                    .take(config.number_buffers())
                    .any(|buffer| buffer.number_channels > 0)
            })
            .unwrap_or(false);
        if !has_output {
            continue;
        }
        let Ok(name) = device.name() else { continue };
        let Ok(uid) = device.uid() else { continue };
        devices.push(AudioDeviceInfo {
            id: uid.to_string(),
            name: name.to_string(),
            device_type: "output".to_string(),
        });
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn enumerate_output_devices(devices: &mut Vec<AudioDeviceInfo>) -> anyhow::Result<()> {
    let host = cpal::default_host();
    for (index, device) in host.output_devices()?.enumerate() {
        if let Ok(name) = device.name() {
            devices.push(AudioDeviceInfo {
                id: format!("output:{index}"),
                name,
                device_type: "output".to_string(),
            });
        }
    }
    Ok(())
}

pub fn resolve_input_device(id: Option<&str>) -> anyhow::Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(id) = id {
        if let Some(index) = id
            .strip_prefix("input:")
            .and_then(|value| value.parse::<usize>().ok())
        {
            return host
                .input_devices()?
                .nth(index)
                .ok_or_else(|| anyhow::anyhow!("Selected input device is no longer available"));
        }
        anyhow::bail!("Invalid input device identifier");
    }
    host.default_input_device()
        .ok_or_else(|| anyhow::anyhow!("No default input device available"))
}
