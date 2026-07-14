use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::provider::MeetingObservation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinatorConfig {
    pub start_confirmations: u8,
    pub stop_grace_period: Duration,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            start_confirmations: 2,
            stop_grace_period: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinatorPhase {
    Disabled,
    Idle,
    Confirming,
    AutoStarting,
    AutoRecording,
    GracePeriod,
    AutoStopping,
    Suppressed,
    StopFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuppressionReason {
    ManualRecording,
    ManualStop,
    StartFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinatorStatus {
    pub phase: CoordinatorPhase,
    pub provider_id: Option<String>,
    pub detector_owns_recording: bool,
    pub suppression_reason: Option<SuppressionReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorAction {
    Start {
        provider_id: String,
        provider_name: String,
    },
    Stop {
        provider_id: String,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct CoordinatorInput<'a> {
    pub enabled: bool,
    pub auto_start: bool,
    pub auto_stop: bool,
    pub observation: Option<&'a MeetingObservation>,
    pub recording_active: bool,
    pub now: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Disabled,
    Idle,
    Confirming {
        provider_id: String,
        provider_name: String,
        confirmations: u8,
    },
    AutoStarting {
        provider_id: String,
        provider_name: String,
    },
    AutoRecording {
        provider_id: String,
    },
    GracePeriod {
        provider_id: String,
        lost_at: Duration,
    },
    AutoStopping {
        provider_id: String,
    },
    Suppressed {
        provider_id: String,
        reason: SuppressionReason,
    },
    StopFailed {
        provider_id: String,
    },
}

pub struct MeetingCoordinator {
    config: CoordinatorConfig,
    state: State,
}

impl MeetingCoordinator {
    pub fn new(config: CoordinatorConfig) -> Self {
        Self {
            config,
            state: State::Disabled,
        }
    }

    pub fn tick(&mut self, input: CoordinatorInput<'_>) -> Option<CoordinatorAction> {
        if !input.enabled {
            self.state = State::Disabled;
            return None;
        }

        match self.state.clone() {
            State::Disabled | State::Idle => self.handle_idle(input),
            State::Confirming {
                provider_id,
                provider_name,
                confirmations,
            } => self.handle_confirming(input, provider_id, provider_name, confirmations),
            State::AutoStarting { .. } | State::AutoStopping { .. } => None,
            State::AutoRecording { provider_id } => self.handle_auto_recording(input, provider_id),
            State::GracePeriod {
                provider_id,
                lost_at,
            } => self.handle_grace_period(input, provider_id, lost_at),
            State::Suppressed {
                provider_id,
                reason,
            } => self.handle_suppressed(input, provider_id, reason),
            State::StopFailed { provider_id } => self.handle_stop_failed(input, provider_id),
        }
    }

    pub fn resolve_start(&mut self, succeeded: bool) -> Result<(), &'static str> {
        let State::AutoStarting { provider_id, .. } = self.state.clone() else {
            return Err("no automatic start is pending");
        };

        self.state = if succeeded {
            State::AutoRecording { provider_id }
        } else {
            State::Suppressed {
                provider_id,
                reason: SuppressionReason::StartFailed,
            }
        };

        Ok(())
    }

    pub fn resolve_stop(&mut self, succeeded: bool) -> Result<(), &'static str> {
        let State::AutoStopping { provider_id } = self.state.clone() else {
            return Err("no automatic stop is pending");
        };

        self.state = if succeeded {
            State::Idle
        } else {
            State::StopFailed { provider_id }
        };

        Ok(())
    }

    pub fn status(&self) -> CoordinatorStatus {
        let (phase, provider_id, detector_owns_recording, suppression_reason) = match &self.state {
            State::Disabled => (CoordinatorPhase::Disabled, None, false, None),
            State::Idle => (CoordinatorPhase::Idle, None, false, None),
            State::Confirming { provider_id, .. } => (
                CoordinatorPhase::Confirming,
                Some(provider_id.clone()),
                false,
                None,
            ),
            State::AutoStarting { provider_id, .. } => (
                CoordinatorPhase::AutoStarting,
                Some(provider_id.clone()),
                false,
                None,
            ),
            State::AutoRecording { provider_id } => (
                CoordinatorPhase::AutoRecording,
                Some(provider_id.clone()),
                true,
                None,
            ),
            State::GracePeriod { provider_id, .. } => (
                CoordinatorPhase::GracePeriod,
                Some(provider_id.clone()),
                true,
                None,
            ),
            State::AutoStopping { provider_id } => (
                CoordinatorPhase::AutoStopping,
                Some(provider_id.clone()),
                true,
                None,
            ),
            State::Suppressed {
                provider_id,
                reason,
            } => (
                CoordinatorPhase::Suppressed,
                Some(provider_id.clone()),
                false,
                Some(reason.clone()),
            ),
            State::StopFailed { provider_id } => (
                CoordinatorPhase::StopFailed,
                Some(provider_id.clone()),
                true,
                None,
            ),
        };

        CoordinatorStatus {
            phase,
            provider_id,
            detector_owns_recording,
            suppression_reason,
        }
    }

    fn handle_idle(&mut self, input: CoordinatorInput<'_>) -> Option<CoordinatorAction> {
        self.state = State::Idle;

        let observation = input.observation?;

        if input.recording_active {
            self.state = State::Suppressed {
                provider_id: observation.provider_id.clone(),
                reason: SuppressionReason::ManualRecording,
            };
            return None;
        }

        if !input.auto_start {
            return None;
        }

        if self.config.start_confirmations <= 1 {
            return self.begin_start(observation);
        }

        self.state = State::Confirming {
            provider_id: observation.provider_id.clone(),
            provider_name: observation.provider_name.clone(),
            confirmations: 1,
        };
        None
    }

    fn handle_confirming(
        &mut self,
        input: CoordinatorInput<'_>,
        provider_id: String,
        provider_name: String,
        confirmations: u8,
    ) -> Option<CoordinatorAction> {
        if input.recording_active {
            if let Some(observation) = input.observation {
                self.state = State::Suppressed {
                    provider_id: observation.provider_id.clone(),
                    reason: SuppressionReason::ManualRecording,
                };
            } else {
                self.state = State::Idle;
            }
            return None;
        }

        if !input.auto_start {
            self.state = State::Idle;
            return None;
        }

        let Some(observation) = input.observation else {
            self.state = State::Idle;
            return None;
        };

        if observation.provider_id != provider_id {
            self.state = State::Confirming {
                provider_id: observation.provider_id.clone(),
                provider_name: observation.provider_name.clone(),
                confirmations: 1,
            };
            return None;
        }

        let confirmations = confirmations.saturating_add(1);
        if confirmations >= self.config.start_confirmations {
            return self.begin_start(observation);
        }

        self.state = State::Confirming {
            provider_id,
            provider_name,
            confirmations,
        };
        None
    }

    fn handle_auto_recording(
        &mut self,
        input: CoordinatorInput<'_>,
        provider_id: String,
    ) -> Option<CoordinatorAction> {
        if !input.recording_active {
            if observation_matches(input.observation, &provider_id) {
                self.state = State::Suppressed {
                    provider_id,
                    reason: SuppressionReason::ManualStop,
                };
            } else {
                self.state = State::Idle;
            }
            return None;
        }

        if observation_matches(input.observation, &provider_id) {
            self.state = State::AutoRecording { provider_id };
            return None;
        }

        if !input.auto_stop {
            self.state = State::Idle;
            return None;
        }

        self.state = State::GracePeriod {
            provider_id,
            lost_at: input.now,
        };
        None
    }

    fn handle_grace_period(
        &mut self,
        input: CoordinatorInput<'_>,
        provider_id: String,
        lost_at: Duration,
    ) -> Option<CoordinatorAction> {
        if !input.recording_active {
            if observation_matches(input.observation, &provider_id) {
                self.state = State::Suppressed {
                    provider_id,
                    reason: SuppressionReason::ManualStop,
                };
            } else {
                self.state = State::Idle;
            }
            return None;
        }

        if observation_matches(input.observation, &provider_id) {
            self.state = State::AutoRecording { provider_id };
            return None;
        }

        if !input.auto_stop {
            self.state = State::Idle;
            return None;
        }

        if input.now.saturating_sub(lost_at) >= self.config.stop_grace_period {
            self.state = State::AutoStopping {
                provider_id: provider_id.clone(),
            };
            return Some(CoordinatorAction::Stop { provider_id });
        }

        self.state = State::GracePeriod {
            provider_id,
            lost_at,
        };
        None
    }

    fn handle_suppressed(
        &mut self,
        input: CoordinatorInput<'_>,
        provider_id: String,
        reason: SuppressionReason,
    ) -> Option<CoordinatorAction> {
        if observation_matches(input.observation, &provider_id) {
            self.state = State::Suppressed {
                provider_id,
                reason,
            };
            return None;
        }

        self.state = State::Idle;
        self.handle_idle(input)
    }

    fn handle_stop_failed(
        &mut self,
        input: CoordinatorInput<'_>,
        provider_id: String,
    ) -> Option<CoordinatorAction> {
        if !input.recording_active {
            self.state = State::Idle;
        } else if observation_matches(input.observation, &provider_id) {
            self.state = State::AutoRecording { provider_id };
        } else {
            self.state = State::StopFailed { provider_id };
        }

        None
    }

    fn begin_start(&mut self, observation: &MeetingObservation) -> Option<CoordinatorAction> {
        self.state = State::AutoStarting {
            provider_id: observation.provider_id.clone(),
            provider_name: observation.provider_name.clone(),
        };

        Some(CoordinatorAction::Start {
            provider_id: observation.provider_id.clone(),
            provider_name: observation.provider_name.clone(),
        })
    }
}

impl Default for MeetingCoordinator {
    fn default() -> Self {
        Self::new(CoordinatorConfig::default())
    }
}

fn observation_matches(observation: Option<&MeetingObservation>, provider_id: &str) -> bool {
    observation
        .map(|observation| observation.provider_id == provider_id)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting_detection::provider::{DetectionEvidence, EvidenceKind};

    fn zoom_observation() -> MeetingObservation {
        MeetingObservation {
            provider_id: "zoom".to_string(),
            provider_name: "Zoom".to_string(),
            evidence: DetectionEvidence {
                kind: EvidenceKind::Process,
                detail: "active_call_helper".to_string(),
            },
        }
    }

    fn input<'a>(
        observation: Option<&'a MeetingObservation>,
        recording_active: bool,
        seconds: u64,
    ) -> CoordinatorInput<'a> {
        CoordinatorInput {
            enabled: true,
            auto_start: true,
            auto_stop: true,
            observation,
            recording_active,
            now: Duration::from_secs(seconds),
        }
    }

    fn start_owned_recording(
        coordinator: &mut MeetingCoordinator,
        observation: &MeetingObservation,
    ) {
        assert_eq!(coordinator.tick(input(Some(observation), false, 0)), None);
        assert_eq!(
            coordinator.tick(input(Some(observation), false, 2)),
            Some(CoordinatorAction::Start {
                provider_id: "zoom".to_string(),
                provider_name: "Zoom".to_string(),
            })
        );
        coordinator.resolve_start(true).unwrap();
    }

    #[test]
    fn requires_stable_detection_before_starting() {
        let observation = zoom_observation();
        let mut coordinator = MeetingCoordinator::default();

        assert_eq!(coordinator.tick(input(Some(&observation), false, 0)), None);
        assert_eq!(coordinator.status().phase, CoordinatorPhase::Confirming);

        assert_eq!(coordinator.tick(input(None, false, 2)), None);
        assert_eq!(coordinator.status().phase, CoordinatorPhase::Idle);

        assert_eq!(coordinator.tick(input(Some(&observation), false, 4)), None);
        assert!(matches!(
            coordinator.tick(input(Some(&observation), false, 6)),
            Some(CoordinatorAction::Start { .. })
        ));
    }

    #[test]
    fn cancels_stop_when_signal_returns_during_grace_period() {
        let observation = zoom_observation();
        let mut coordinator = MeetingCoordinator::default();
        start_owned_recording(&mut coordinator, &observation);

        assert_eq!(coordinator.tick(input(None, true, 10)), None);
        assert_eq!(coordinator.status().phase, CoordinatorPhase::GracePeriod);
        assert!(coordinator.status().detector_owns_recording);

        assert_eq!(coordinator.tick(input(Some(&observation), true, 14)), None);
        assert_eq!(coordinator.status().phase, CoordinatorPhase::AutoRecording);
    }

    #[test]
    fn stops_only_after_grace_period_expires() {
        let observation = zoom_observation();
        let mut coordinator = MeetingCoordinator::default();
        start_owned_recording(&mut coordinator, &observation);

        assert_eq!(coordinator.tick(input(None, true, 10)), None);
        assert_eq!(coordinator.tick(input(None, true, 14)), None);
        assert_eq!(
            coordinator.tick(input(None, true, 15)),
            Some(CoordinatorAction::Stop {
                provider_id: "zoom".to_string(),
            })
        );
        assert_eq!(coordinator.status().phase, CoordinatorPhase::AutoStopping);
    }

    #[test]
    fn never_claims_or_stops_a_manual_recording() {
        let observation = zoom_observation();
        let mut coordinator = MeetingCoordinator::default();

        assert_eq!(coordinator.tick(input(Some(&observation), true, 0)), None);
        let status = coordinator.status();
        assert_eq!(status.phase, CoordinatorPhase::Suppressed);
        assert_eq!(
            status.suppression_reason,
            Some(SuppressionReason::ManualRecording)
        );
        assert!(!status.detector_owns_recording);

        assert_eq!(coordinator.tick(input(Some(&observation), false, 10)), None);
        assert_eq!(coordinator.status().phase, CoordinatorPhase::Suppressed);
    }

    #[test]
    fn manual_stop_suppresses_restart_until_meeting_ends() {
        let observation = zoom_observation();
        let mut coordinator = MeetingCoordinator::default();
        start_owned_recording(&mut coordinator, &observation);

        assert_eq!(coordinator.tick(input(Some(&observation), false, 10)), None);
        assert_eq!(
            coordinator.status().suppression_reason,
            Some(SuppressionReason::ManualStop)
        );

        assert_eq!(coordinator.tick(input(Some(&observation), false, 12)), None);
        assert_eq!(coordinator.tick(input(None, false, 14)), None);
        assert_eq!(coordinator.status().phase, CoordinatorPhase::Idle);
    }

    #[test]
    fn failed_start_is_not_retried_during_same_meeting() {
        let observation = zoom_observation();
        let mut coordinator = MeetingCoordinator::default();

        coordinator.tick(input(Some(&observation), false, 0));
        assert!(matches!(
            coordinator.tick(input(Some(&observation), false, 2)),
            Some(CoordinatorAction::Start { .. })
        ));
        coordinator.resolve_start(false).unwrap();

        assert_eq!(coordinator.tick(input(Some(&observation), false, 4)), None);
        assert_eq!(
            coordinator.status().suppression_reason,
            Some(SuppressionReason::StartFailed)
        );

        coordinator.tick(input(None, false, 6));
        assert_eq!(coordinator.status().phase, CoordinatorPhase::Idle);
    }

    #[test]
    fn disabling_detection_relinquishes_owned_recording_without_stopping() {
        let observation = zoom_observation();
        let mut coordinator = MeetingCoordinator::default();
        start_owned_recording(&mut coordinator, &observation);

        let mut disabled_input = input(Some(&observation), true, 10);
        disabled_input.enabled = false;

        assert_eq!(coordinator.tick(disabled_input), None);
        assert_eq!(coordinator.status().phase, CoordinatorPhase::Disabled);
        assert!(!coordinator.status().detector_owns_recording);
    }

    #[test]
    fn failed_stop_keeps_ownership_without_retry_loop() {
        let observation = zoom_observation();
        let mut coordinator = MeetingCoordinator::default();
        start_owned_recording(&mut coordinator, &observation);

        coordinator.tick(input(None, true, 10));
        assert!(matches!(
            coordinator.tick(input(None, true, 15)),
            Some(CoordinatorAction::Stop { .. })
        ));
        coordinator.resolve_stop(false).unwrap();

        assert_eq!(coordinator.status().phase, CoordinatorPhase::StopFailed);
        assert!(coordinator.status().detector_owns_recording);
        assert_eq!(coordinator.tick(input(None, true, 20)), None);
        assert_eq!(coordinator.status().phase, CoordinatorPhase::StopFailed);
    }
}
