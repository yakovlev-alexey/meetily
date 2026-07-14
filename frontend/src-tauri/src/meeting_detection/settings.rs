use std::collections::HashMap;

use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "meeting_detection_settings.json";
const STORE_KEY: &str = "settings";
const ZOOM_PROVIDER_ID: &str = "zoom";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeetingDetectionSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub auto_start: bool,
    #[serde(default = "default_true")]
    pub auto_stop: bool,
    #[serde(default = "default_enabled_providers")]
    pub enabled_providers: HashMap<String, bool>,
}

impl MeetingDetectionSettings {
    pub fn is_provider_enabled(&self, provider_id: &str) -> bool {
        self.enabled_providers
            .get(provider_id)
            .copied()
            .unwrap_or(false)
    }
}

impl Default for MeetingDetectionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_start: true,
            auto_stop: true,
            enabled_providers: default_enabled_providers(),
        }
    }
}

pub async fn load_settings<R: Runtime>(app: &AppHandle<R>) -> Result<MeetingDetectionSettings> {
    let store = match app.store(STORE_FILE) {
        Ok(store) => store,
        Err(error) => {
            warn!(
                "Failed to access meeting detection settings store: {error}; using disabled defaults"
            );
            return Ok(MeetingDetectionSettings::default());
        }
    };

    let Some(value) = store.get(STORE_KEY) else {
        info!("No meeting detection settings found; using disabled defaults");
        return Ok(MeetingDetectionSettings::default());
    };

    match serde_json::from_value(value.clone()) {
        Ok(settings) => Ok(settings),
        Err(error) => {
            warn!(
                "Failed to deserialize meeting detection settings: {error}; using disabled defaults"
            );
            Ok(MeetingDetectionSettings::default())
        }
    }
}

pub async fn save_settings<R: Runtime>(
    app: &AppHandle<R>,
    settings: &MeetingDetectionSettings,
) -> Result<()> {
    let store = app.store(STORE_FILE)?;
    store.set(STORE_KEY, serde_json::to_value(settings)?);
    store.save()?;
    info!("Meeting detection settings persisted");
    Ok(())
}

fn default_true() -> bool {
    true
}

fn default_enabled_providers() -> HashMap<String, bool> {
    HashMap::from([(ZOOM_PROVIDER_ID.to_string(), true)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_opt_in_but_ready_after_enablement() {
        let settings = MeetingDetectionSettings::default();

        assert!(!settings.enabled);
        assert!(settings.auto_start);
        assert!(settings.auto_stop);
        assert!(settings.is_provider_enabled("zoom"));
        assert!(!settings.is_provider_enabled("unknown"));
    }

    #[test]
    fn missing_fields_deserialize_with_backward_compatible_defaults() {
        let settings: MeetingDetectionSettings =
            serde_json::from_str(r#"{"enabled":true}"#).unwrap();

        assert!(settings.enabled);
        assert!(settings.auto_start);
        assert!(settings.auto_stop);
        assert!(settings.is_provider_enabled("zoom"));
    }

    #[test]
    fn explicit_provider_preferences_are_preserved() {
        let settings: MeetingDetectionSettings = serde_json::from_str(
            r#"{
                "enabled": true,
                "auto_start": false,
                "auto_stop": false,
                "enabled_providers": {"zoom": false, "teams": true}
            }"#,
        )
        .unwrap();

        assert!(!settings.auto_start);
        assert!(!settings.auto_stop);
        assert!(!settings.is_provider_enabled("zoom"));
        assert!(settings.is_provider_enabled("teams"));
    }
}
