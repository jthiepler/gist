#[cfg(target_os = "macos")]
static MICROPHONE_PERMISSION_REQUEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(target_os = "macos")]
pub async fn ensure_microphone_access() -> anyhow::Result<()> {
    use cidre::av::{AudioApp, AudioAppRecordPermission};

    // Device loading can be triggered by more than one reactive UI path.
    // Serialize the native request so macOS never receives overlapping prompts.
    let _request_guard = MICROPHONE_PERMISSION_REQUEST.lock().await;

    match AudioApp::shared().record_permission() {
        AudioAppRecordPermission::Granted => Ok(()),
        AudioAppRecordPermission::Denied => anyhow::bail!(
            "Microphone access is denied. Allow Gist in System Settings > Privacy & Security > Microphone, then try again."
        ),
        AudioAppRecordPermission::Undetermined => {
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
