use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

#[cfg(target_os = "macos")]
use cidre::core_audio as ca;
#[cfg(target_os = "macos")]
use std::collections::HashMap;

#[derive(Clone, Serialize, Debug)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub device_type: String, // "input" or "output"
}

#[cfg(target_os = "macos")]
fn core_audio_input_devices() -> anyhow::Result<Vec<(String, String)>> {
    Ok(ca::System::devices()?
        .into_iter()
        .filter_map(|device| {
            let has_input = device
                .input_stream_cfg()
                .map(|config| {
                    config
                        .buffers()
                        .iter()
                        .take(config.number_buffers())
                        .any(|buffer| buffer.number_channels > 0)
                })
                .unwrap_or(false);
            if !has_input {
                return None;
            }
            let name = device.name().ok()?.to_string();
            let uid = device.uid().ok()?.to_string();
            Some((name, uid))
        })
        .collect())
}

pub fn enumerate_devices() -> anyhow::Result<Vec<AudioDeviceInfo>> {
    let host = cpal::default_host();
    let mut devices = Vec::new();

    #[cfg(target_os = "macos")]
    let core_input_devices = core_audio_input_devices()?;

    #[cfg(target_os = "macos")]
    let mut input_name_occurrences = HashMap::<String, usize>::new();

    #[cfg(not(target_os = "macos"))]
    let mut input_index = 0;

    for device in host.input_devices()? {
        #[cfg(not(target_os = "macos"))]
        let current_input_index = input_index;
        #[cfg(not(target_os = "macos"))]
        {
            input_index += 1;
        }

        if let Ok(name) = device.name() {
            #[cfg(target_os = "macos")]
            let id = {
                let occurrence = input_name_occurrences.entry(name.clone()).or_insert(0);
                let uid = core_input_devices
                    .iter()
                    .filter(|(candidate_name, _)| candidate_name == &name)
                    .nth(*occurrence)
                    .map(|(_, uid)| uid.clone());
                *occurrence += 1;
                uid.map(|uid| format!("input:uid:{uid}"))
            };

            #[cfg(not(target_os = "macos"))]
            let id = Some(format!("input:{current_input_index}"));

            let Some(id) = id else { continue };
            devices.push(AudioDeviceInfo {
                id,
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
        #[cfg(target_os = "macos")]
        if let Some(uid) = id.strip_prefix("input:uid:") {
            let core_input_devices = core_audio_input_devices()?;
            let selected_index = core_input_devices
                .iter()
                .position(|(_, candidate_uid)| candidate_uid == uid)
                .ok_or_else(|| anyhow::anyhow!("Selected input device is no longer available"))?;
            let selected_name = &core_input_devices[selected_index].0;
            let selected_occurrence = core_input_devices[..selected_index]
                .iter()
                .filter(|(candidate_name, _)| candidate_name == selected_name)
                .count();

            return host
                .input_devices()?
                .filter(|device| {
                    device
                        .name()
                        .map(|name| name == *selected_name)
                        .unwrap_or(false)
                })
                .nth(selected_occurrence)
                .ok_or_else(|| anyhow::anyhow!("Selected input device is no longer available"));
        }

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
