use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use log::{error, info};
use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tokio::sync::{Mutex, RwLock};

use crate::notifications::commands::NotificationManagerState;

use super::{
    context::DetectionContext,
    coordinator::{CoordinatorAction, CoordinatorInput, CoordinatorStatus, MeetingCoordinator},
    provider::{MeetingObservation, Platform, ProviderDescriptor, ProviderRegistry},
    settings::{self, MeetingDetectionSettings},
};

const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeetingDetectionStatus {
    pub enabled: bool,
    pub detected_provider_id: Option<String>,
    pub detected_provider_name: Option<String>,
    pub coordinator: CoordinatorStatus,
}

#[derive(Debug, Clone, Serialize)]
struct MeetingDetectionErrorPayload {
    operation: String,
    provider_id: String,
    message: String,
}

pub struct MeetingDetectionMonitor {
    settings: RwLock<MeetingDetectionSettings>,
    registry: ProviderRegistry,
    coordinator: Mutex<MeetingCoordinator>,
    status: RwLock<MeetingDetectionStatus>,
    task_started: AtomicBool,
}

pub type MeetingDetectionMonitorState = Arc<MeetingDetectionMonitor>;

impl MeetingDetectionMonitor {
    pub fn new() -> Self {
        let settings = MeetingDetectionSettings::default();
        let coordinator = MeetingCoordinator::default();
        let status = MeetingDetectionStatus {
            enabled: settings.enabled,
            detected_provider_id: None,
            detected_provider_name: None,
            coordinator: coordinator.status(),
        };

        Self {
            settings: RwLock::new(settings),
            registry: ProviderRegistry::default(),
            coordinator: Mutex::new(coordinator),
            status: RwLock::new(status),
            task_started: AtomicBool::new(false),
        }
    }

    pub async fn initialize(self: &Arc<Self>, app: AppHandle<Wry>) {
        match settings::load_settings(&app).await {
            Ok(settings) => {
                info!(
                    "Meeting detection initialized: enabled={}",
                    settings.enabled
                );
                *self.settings.write().await = settings;
            }
            Err(error) => {
                error!("Failed to initialize meeting detection settings: {error}");
            }
        }

        self.start(app);
    }

    pub async fn settings(&self) -> MeetingDetectionSettings {
        self.settings.read().await.clone()
    }

    pub async fn update_settings(&self, settings: MeetingDetectionSettings) {
        *self.settings.write().await = settings;
    }

    pub async fn status(&self) -> MeetingDetectionStatus {
        self.status.read().await.clone()
    }

    pub fn provider_descriptors(&self) -> Vec<ProviderDescriptor> {
        self.registry.descriptors(Platform::current())
    }

    fn start(self: &Arc<Self>, app: AppHandle<Wry>) {
        if self.task_started.swap(true, Ordering::SeqCst) {
            info!("Meeting detection monitor is already running");
            return;
        }

        let monitor = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            monitor.run(app).await;
        });
    }

    async fn run(self: Arc<Self>, app: AppHandle<Wry>) {
        info!("Meeting detection monitor started");
        let started_at = Instant::now();
        let mut system = System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
        );

        loop {
            let current_settings = self.settings().await;
            let observation = if current_settings.enabled {
                system.refresh_processes(ProcessesToUpdate::All, true);
                let context = DetectionContext::from_process_names(
                    Platform::current(),
                    system
                        .processes()
                        .values()
                        .map(|process| process.name().to_string_lossy().into_owned()),
                );
                self.registry.detect_first(&context, &current_settings)
            } else {
                None
            };

            let recording_active = crate::audio::recording_commands::is_recording().await;
            let action = {
                let mut coordinator = self.coordinator.lock().await;
                coordinator.tick(CoordinatorInput {
                    enabled: current_settings.enabled,
                    auto_start: current_settings.auto_start,
                    auto_stop: current_settings.auto_stop,
                    observation: observation.as_ref(),
                    recording_active,
                    now: started_at.elapsed(),
                })
            };

            if let Some(action) = action {
                self.execute_action(&app, action).await;
            }

            self.publish_status(&app, &current_settings, observation.as_ref())
                .await;
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    async fn execute_action(&self, app: &AppHandle<Wry>, action: CoordinatorAction) {
        match action {
            CoordinatorAction::Start {
                provider_id,
                provider_name,
            } => {
                info!("Auto-starting recording for provider {provider_id}");
                let result = start_automatic_recording(app, &provider_name).await;
                let succeeded = result.is_ok();

                if let Err(error) = result {
                    self.report_error(app, "start", &provider_id, error).await;
                }

                if let Err(error) = self.coordinator.lock().await.resolve_start(succeeded) {
                    error!("Failed to resolve meeting detection start action: {error}");
                }
            }
            CoordinatorAction::Stop { provider_id } => {
                info!("Auto-stopping recording for provider {provider_id}");
                let result = stop_automatic_recording(app).await;
                let succeeded = result.is_ok();

                if let Err(error) = result {
                    self.report_error(app, "stop", &provider_id, error).await;
                }

                if let Err(error) = self.coordinator.lock().await.resolve_stop(succeeded) {
                    error!("Failed to resolve meeting detection stop action: {error}");
                }
            }
        }
    }

    async fn publish_status(
        &self,
        app: &AppHandle<Wry>,
        settings: &MeetingDetectionSettings,
        observation: Option<&MeetingObservation>,
    ) {
        let status = MeetingDetectionStatus {
            enabled: settings.enabled,
            detected_provider_id: observation.map(|value| value.provider_id.clone()),
            detected_provider_name: observation.map(|value| value.provider_name.clone()),
            coordinator: self.coordinator.lock().await.status(),
        };

        let mut previous_status = self.status.write().await;
        if *previous_status == status {
            return;
        }

        info!(
            "Meeting detection state changed: {:?} -> {:?}",
            previous_status.coordinator.phase, status.coordinator.phase
        );
        *previous_status = status.clone();

        if let Err(error) = app.emit("meeting-detection-status-changed", status) {
            error!("Failed to emit meeting detection status: {error}");
        }
    }

    async fn report_error(
        &self,
        app: &AppHandle<Wry>,
        operation: &str,
        provider_id: &str,
        message: String,
    ) {
        error!("Meeting detection {operation} failed for provider {provider_id}: {message}");
        let payload = MeetingDetectionErrorPayload {
            operation: operation.to_string(),
            provider_id: provider_id.to_string(),
            message: message.clone(),
        };

        if let Err(error) = app.emit("meeting-detection-error", payload) {
            error!("Failed to emit meeting detection error: {error}");
        }

        let notification_state = app.state::<NotificationManagerState<Wry>>();
        if let Err(error) = crate::notifications::commands::show_system_error_notification(
            &notification_state,
            format!("Automatic meeting recording failed: {message}"),
        )
        .await
        {
            error!("Failed to show meeting detection error notification: {error}");
        }
    }
}

impl Default for MeetingDetectionMonitor {
    fn default() -> Self {
        Self::new()
    }
}

pub fn new_state() -> MeetingDetectionMonitorState {
    Arc::new(MeetingDetectionMonitor::new())
}

async fn start_automatic_recording(
    app: &AppHandle<Wry>,
    provider_name: &str,
) -> Result<(), String> {
    let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let meeting_name = format!("{provider_name} Meeting {timestamp}");

    crate::start_recording_with_devices_and_meeting(app.clone(), None, None, Some(meeting_name))
        .await
}

async fn stop_automatic_recording(app: &AppHandle<Wry>) -> Result<(), String> {
    let save_path = crate::default_recording_save_path(app)?;
    crate::stop_recording(app.clone(), crate::RecordingArgs { save_path }).await?;
    app.emit("recording-stop-complete", true)
        .map_err(|error| format!("Failed to trigger recording post-processing: {error}"))?;
    Ok(())
}
