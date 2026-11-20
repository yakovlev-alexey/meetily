use tauri::{AppHandle, State, Wry};

use super::{
    monitor::{MeetingDetectionMonitorState, MeetingDetectionStatus},
    provider::ProviderDescriptor,
    settings::{self, MeetingDetectionSettings},
};

#[tauri::command]
pub async fn get_meeting_detection_settings(
    app: AppHandle<Wry>,
) -> Result<MeetingDetectionSettings, String> {
    settings::load_settings(&app)
        .await
        .map_err(|error| format!("Failed to load meeting detection settings: {error}"))
}

#[tauri::command]
pub async fn set_meeting_detection_settings(
    app: AppHandle<Wry>,
    state: State<'_, MeetingDetectionMonitorState>,
    settings: MeetingDetectionSettings,
) -> Result<(), String> {
    state.update_settings(&app, settings).await
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
