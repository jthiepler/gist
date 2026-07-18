#[cfg(target_os = "macos")]
static MICROPHONE_PERMISSION_REQUEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(target_os = "macos")]
pub async fn ensure_microphone_access() -> anyhow::Result<()> {
    use cidre::av::{AudioApp, AuthorizationStatus, CaptureDevice, MediaType};

    // Device loading can be triggered by more than one reactive UI path.
    // Serialize the native request so macOS never receives overlapping prompts.
    let _request_guard = MICROPHONE_PERMISSION_REQUEST.lock().await;

    let media_type = MediaType::audio();
    let status = CaptureDevice::authorization_status_for_media_type(media_type)
        .map_err(|error| anyhow::anyhow!("Could not check microphone access: {error:?}"))?;

    match status {
        AuthorizationStatus::Authorized => Ok(()),
        AuthorizationStatus::Denied => anyhow::bail!(
            "Microphone access is denied. Allow Gist in System Settings > Privacy & Security > Microphone, then try again."
        ),
        AuthorizationStatus::Restricted => anyhow::bail!(
            "Microphone access is restricted by macOS and cannot be enabled for Gist."
        ),
        AuthorizationStatus::NotDetermined => {
            // AVAudioApplication owns the async prompt API without holding a
            // non-Send Objective-C media type across the await boundary.
            if AudioApp::request_record_permission().await {
                Ok(())
            } else {
                anyhow::bail!(
                    "Microphone access was not allowed. Allow Gist in System Settings > Privacy & Security > Microphone, then try again."
                )
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub async fn ensure_microphone_access() -> anyhow::Result<()> {
    Ok(())
}
