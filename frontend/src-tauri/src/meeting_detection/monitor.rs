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

use crate::{
    audio::recording_commands::{active_recording_session, recording_start_ready, RECORDING_IS_SAVING},
    notifications::commands::NotificationManagerState,
};

use super::{
    context::{DetectionContext, TelemostSignal},
    coordinator::{
        CoordinatorAction, CoordinatorInput, CoordinatorPhase, CoordinatorStatus,
        MeetingCoordinator,
    },
    provider::{MeetingObservation, Platform, ProviderDescriptor, ProviderRegistry},
    providers::telemost,
    settings::{self, AutomationPermission, MeetingDetectionSettings, AUTOMATIC_START_DISABLED, AUTOMATIC_STOP_DISABLED},
};

const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeetingDetectionStatus {
    pub enabled: bool,
    pub detected_provider_id: Option<String>,
    pub detected_provider_name: Option<String>,
    pub telemost_signal: Option<TelemostSignal>,
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
            telemost_signal: None,
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
        let mut current_settings = self.settings.write().await;
        match settings::load_settings(&app).await {
            Ok(settings) => {
                info!(
                    "Meeting detection initialized: enabled={}",
                    settings.enabled
                );
                *current_settings = settings;
            }
            Err(error) => {
                error!("Failed to initialize meeting detection settings: {error}");
            }
        }

        drop(current_settings);
        self.start(app);
    }

    pub async fn settings(&self) -> MeetingDetectionSettings {
        self.settings.read().await.clone()
    }

    pub async fn update_settings(
        &self,
        app: &AppHandle<Wry>,
        settings: MeetingDetectionSettings,
    ) -> Result<(), String> {
        let mut current_settings = self.settings.write().await;
        settings::save_settings(app, &settings)
            .await
            .map_err(|error| format!("Failed to save meeting detection settings: {error}"))?;
        *current_settings = settings;
        Ok(())
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
        let mut telemost_suppressed_until_inactive = false;

        loop {
            let current_settings = self.settings().await;
            let coordinator_status = self.coordinator.lock().await.status();
            let tracked_provider = coordinator_status.provider_id.as_deref();
            let telemost_signal = read_telemost_signal(&current_settings).await;
            if self.settings().await != current_settings {
                continue;
            }

            // Keep a manually stopped Telemost call suppressed during an AX outage
            // without preventing Zoom from starting during that outage.
            let release_telemost = tracked_provider == Some(telemost::PROVIDER_ID)
                && coordinator_status.phase == CoordinatorPhase::Suppressed
                && telemost_signal.is_some_and(TelemostSignal::is_unavailable);
            if release_telemost {
                telemost_suppressed_until_inactive = true;
            } else if matches!(
                telemost_signal,
                None | Some(TelemostSignal::Inactive | TelemostSignal::InactivePermissionRequired)
            ) {
                telemost_suppressed_until_inactive = false;
            }
            let tracked_provider = tracked_provider.filter(|_| !release_telemost);
            let enabled = current_settings.enabled
                && tracked_provider.map_or(true, |id| current_settings.is_provider_enabled(id));

            let observation = if enabled {
                system.refresh_processes(ProcessesToUpdate::All, true);
                let mut context = DetectionContext::from_process_names(
                    Platform::current(),
                    system
                        .processes()
                        .values()
                        .map(|process| process.name().to_string_lossy().into_owned()),
                );
                if !telemost_suppressed_until_inactive {
                    context.telemost_signal = telemost_signal.unwrap_or_default();
                }
                self.registry
                    .detect(&context, &current_settings, tracked_provider)
            } else {
                None
            };

            let action = {
                let mut coordinator = self.coordinator.lock().await;
                let recording_session = active_recording_session();
                coordinator.tick(CoordinatorInput {
                    enabled,
                    auto_start: current_settings.auto_start && recording_start_ready(),
                    auto_stop: current_settings.auto_stop,
                    observation: observation.as_ref(),
                    recording_session,
                    signal_unavailable: tracked_provider == Some(telemost::PROVIDER_ID)
                        && telemost_signal.is_some_and(TelemostSignal::is_unavailable),
                    now: started_at.elapsed(),
                })
            };

            if let Some(action) = action {
                self.execute_action(&app, action).await;
            }

            self.publish_status(
                &app,
                &current_settings,
                observation.as_ref(),
                telemost_signal,
            )
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
                let permission = AutomationPermission { settings: &self.settings, provider_id: &provider_id };
                let result = start_automatic_recording(app, &provider_name, permission).await;
                if result.as_ref().is_err_and(|error| {
                    matches!(error.as_str(), RECORDING_IS_SAVING | AUTOMATIC_START_DISABLED)
                }) {
                    self.coordinator.lock().await.defer_start();
                    return;
                }
                let session_id = result.as_ref().ok().copied();

                if let Err(error) = result {
                    self.report_error(app, "start", &provider_id, error).await;
                }

                if let Err(error) = self.coordinator.lock().await.resolve_start(session_id) {
                    error!("Failed to resolve meeting detection start action: {error}");
                }
            }
            CoordinatorAction::Stop {
                provider_id,
                session_id,
            } => {
                info!("Auto-stopping recording for provider {provider_id}");
                let permission = AutomationPermission { settings: &self.settings, provider_id: &provider_id };
                let result = stop_automatic_recording(app, session_id, permission).await;
                if result.as_ref().is_err_and(|error| error == AUTOMATIC_STOP_DISABLED) {
                    self.coordinator.lock().await.defer_stop();
                    return;
                }
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
        telemost_signal: Option<TelemostSignal>,
    ) {
        let status = MeetingDetectionStatus {
            enabled: settings.enabled,
            detected_provider_id: observation.map(|value| value.provider_id.clone()),
            detected_provider_name: observation.map(|value| value.provider_name.clone()),
            telemost_signal,
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
    permission: AutomationPermission<'_>,
) -> Result<u64, String> {
    let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let meeting_name = format!("{provider_name} Meeting {timestamp}");

    crate::start_recording_with_devices_session(app.clone(), None, None, Some(meeting_name), Some(permission))
        .await
}

async fn stop_automatic_recording(
    app: &AppHandle<Wry>,
    session_id: u64,
    permission: AutomationPermission<'_>,
) -> Result<(), String> {
    let save_path = crate::default_recording_save_path(app)?;
    crate::stop_recording_with_session(
        app.clone(),
        crate::RecordingArgs { save_path },
        Some(session_id),
        Some(permission),
    )
    .await
}

async fn read_telemost_signal(_settings: &MeetingDetectionSettings) -> Option<TelemostSignal> {
    #[cfg(target_os = "macos")]
    if _settings.enabled && _settings.is_provider_enabled(telemost::PROVIDER_ID) {
        return Some(
            tokio::task::spawn_blocking(telemost::read_signal)
                .await
                .unwrap_or(TelemostSignal::Unavailable),
        );
    }
    None
}
