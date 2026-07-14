use tauri::{AppHandle, State, Wry};

use super::{
    monitor::{MeetingDetectionMonitorState, MeetingDetectionStatus},
    provider::ProviderDescriptor,
    settings::{self, MeetingDetectionSettings},
};

#[tauri::command]
pub async fn get_meeting_detection_settings(
    app: AppHandle<Wry>,
    state: State<'_, MeetingDetectionMonitorState>,
) -> Result<MeetingDetectionSettings, String> {
    let persisted_settings = settings::load_settings(&app)
        .await
        .map_err(|error| format!("Failed to load meeting detection settings: {error}"))?;
    state.update_settings(persisted_settings.clone()).await;
    Ok(persisted_settings)
}

#[tauri::command]
pub async fn set_meeting_detection_settings(
    app: AppHandle<Wry>,
    state: State<'_, MeetingDetectionMonitorState>,
    settings: MeetingDetectionSettings,
) -> Result<(), String> {
    settings::save_settings(&app, &settings)
        .await
        .map_err(|error| format!("Failed to save meeting detection settings: {error}"))?;
    state.update_settings(settings).await;
    Ok(())
}

#[tauri::command]
pub async fn get_meeting_detection_status(
    state: State<'_, MeetingDetectionMonitorState>,
) -> Result<MeetingDetectionStatus, String> {
    Ok(state.status().await)
}

#[tauri::command]
pub async fn list_meeting_detection_providers(
    state: State<'_, MeetingDetectionMonitorState>,
) -> Result<Vec<ProviderDescriptor>, String> {
    Ok(state.provider_descriptors())
}
