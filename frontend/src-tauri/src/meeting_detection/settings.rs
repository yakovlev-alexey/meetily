use std::collections::HashMap;

use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;
use tokio::sync::{RwLock, RwLockReadGuard};

const STORE_FILE: &str = "meeting_detection_settings.json";
const STORE_KEY: &str = "settings";
const ZOOM_PROVIDER_ID: &str = "zoom";

pub(crate) const AUTOMATIC_START_DISABLED: &str = "Automatic recording was disabled before capture.";
pub(crate) const AUTOMATIC_STOP_DISABLED: &str = "Automatic recording was disabled before stopping.";

pub(crate) enum AutomationAction {
    Start,
    Stop,
}

pub(crate) struct AutomationPermission<'a> {
    pub settings: &'a RwLock<MeetingDetectionSettings>,
    pub provider_id: &'a str,
}

impl<'a> AutomationPermission<'a> {
    pub async fn authorize(
        self,
        action: AutomationAction,
    ) -> Result<RwLockReadGuard<'a, MeetingDetectionSettings>, &'static str> {
        let settings = self.settings.read().await;
        let (enabled, denied) = match action {
            AutomationAction::Start => (settings.auto_start, AUTOMATIC_START_DISABLED),
            AutomationAction::Stop => (settings.auto_stop, AUTOMATIC_STOP_DISABLED),
        };
        if settings.enabled && enabled && settings.is_provider_enabled(self.provider_id) {
            Ok(settings)
        } else {
            Err(denied)
        }
    }
}

pub(crate) async fn authorize_automation(
    permission: Option<AutomationPermission<'_>>,
    action: AutomationAction,
) -> Result<Option<RwLockReadGuard<'_, MeetingDetectionSettings>>, String> {
    match permission {
        Some(permission) => permission.authorize(action).await.map(Some).map_err(str::to_owned),
        None => Ok(None),
    }
}

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
    let previous = store.get(STORE_KEY);
    store.set(STORE_KEY, serde_json::to_value(settings)?);
    if let Err(error) = store.save() {
        if let Some(previous) = previous {
            store.set(STORE_KEY, previous);
        } else {
            store.delete(STORE_KEY);
        }
        return Err(error.into());
    }
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

    #[tokio::test]
    async fn a_disable_during_startup_prevents_capture_authorization() {
        let settings = RwLock::new(MeetingDetectionSettings {
            enabled: true,
            enabled_providers: HashMap::from([("telemost".into(), true)]),
            ..MeetingDetectionSettings::default()
        });
        let start = AutomationPermission { settings: &settings, provider_id: "telemost" };
        settings.write().await.enabled_providers.insert("telemost".into(), false);
        assert!(matches!(start.authorize(AutomationAction::Start).await, Err(AUTOMATIC_START_DISABLED)));
    }

    #[tokio::test]
    async fn settings_cannot_finish_updating_until_authorized_capture_finishes_opening() {
        let settings = RwLock::new(MeetingDetectionSettings {
            enabled: true,
            ..MeetingDetectionSettings::default()
        });
        let capture = AutomationPermission { settings: &settings, provider_id: "zoom" };
        let authorized = capture.authorize(AutomationAction::Start).await.unwrap();
        assert!(settings.try_write().is_err());
        drop(authorized);
        let mut revision = settings.try_write().unwrap();
        revision.auto_start = false;
        drop(revision);
        assert!(matches!(
            (AutomationPermission { settings: &settings, provider_id: "zoom" }).authorize(AutomationAction::Start).await,
            Err(AUTOMATIC_START_DISABLED)
        ));
    }

    #[tokio::test]
    async fn a_disable_before_stopping_prevents_stop_authorization() {
        let settings = RwLock::new(MeetingDetectionSettings {
            enabled: true,
            ..MeetingDetectionSettings::default()
        });
        let stop = AutomationPermission { settings: &settings, provider_id: "zoom" };
        settings.write().await.auto_stop = false;
        assert!(matches!(stop.authorize(AutomationAction::Stop).await, Err(AUTOMATIC_STOP_DISABLED)));
    }

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
