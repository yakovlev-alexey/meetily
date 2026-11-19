use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    context::DetectionContext, providers::default_providers, settings::MeetingDetectionSettings,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
    #[default]
    Unknown,
}

impl Platform {
    pub const fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::Macos
        }

        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }

        #[cfg(target_os = "linux")]
        {
            Self::Linux
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Self::Unknown
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Process,
    Accessibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectionEvidence {
    pub kind: EvidenceKind,
    /// A non-sensitive semantic label, never a meeting title or process list.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeetingObservation {
    pub provider_id: String,
    pub provider_name: String,
    pub evidence: DetectionEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    pub id: String,
    pub display_name: String,
    pub supported: bool,
    pub supported_platforms: Vec<Platform>,
}

pub trait MeetingProvider: Send + Sync {
    fn id(&self) -> &'static str;

    fn display_name(&self) -> &'static str;

    fn supported_platforms(&self) -> &'static [Platform];

    fn detect(&self, context: &DetectionContext) -> Option<DetectionEvidence>;

    fn supports(&self, platform: Platform) -> bool {
        self.supported_platforms().contains(&platform)
    }

    fn descriptor(&self, platform: Platform) -> ProviderDescriptor {
        ProviderDescriptor {
            id: self.id().to_string(),
            display_name: self.display_name().to_string(),
            supported: self.supports(platform),
            supported_platforms: self.supported_platforms().to_vec(),
        }
    }

    fn observe(&self, context: &DetectionContext) -> Option<MeetingObservation> {
        if !self.supports(context.platform()) {
            return None;
        }

        self.detect(context).map(|evidence| MeetingObservation {
            provider_id: self.id().to_string(),
            provider_name: self.display_name().to_string(),
            evidence,
        })
    }
}

pub struct ProviderRegistry {
    providers: Vec<Arc<dyn MeetingProvider>>,
}

impl ProviderRegistry {
    pub fn new(providers: Vec<Arc<dyn MeetingProvider>>) -> Self {
        Self { providers }
    }

    pub fn descriptors(&self, platform: Platform) -> Vec<ProviderDescriptor> {
        self.providers
            .iter()
            .map(|provider| provider.descriptor(platform))
            .collect()
    }

    pub fn detect(
        &self,
        context: &DetectionContext,
        settings: &MeetingDetectionSettings,
        tracked_provider: Option<&str>,
    ) -> Option<MeetingObservation> {
        self.providers
            .iter()
            .filter(|provider| settings.is_provider_enabled(provider.id()))
            .filter(|provider| tracked_provider.map_or(true, |id| provider.id() == id))
            .find_map(|provider| provider.observe(context))
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new(default_providers())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_exposes_backend_provider_descriptors() {
        let descriptors = ProviderRegistry::default().descriptors(Platform::Macos);

        assert!(descriptors
            .iter()
            .any(|provider| provider.id == "zoom" && provider.supported));
    }

    #[test]
    fn telemost_is_available_on_macos_and_requires_explicit_opt_in() {
        let registry = ProviderRegistry::default();
        let descriptors = registry.descriptors(Platform::Macos);
        let telemost = descriptors
            .iter()
            .find(|provider| provider.id == "telemost")
            .expect("the Telemost provider should be registered");
        assert!(telemost.supported);
        assert_eq!(telemost.supported_platforms, vec![Platform::Macos]);
        assert!(!MeetingDetectionSettings::default().is_provider_enabled("telemost"));
    }

    #[test]
    fn registry_respects_provider_toggle() {
        let registry = ProviderRegistry::default();
        let context = DetectionContext::from_process_names(Platform::Macos, ["CptHost"]);
        let mut settings = MeetingDetectionSettings::default();
        settings.enabled_providers.insert("zoom".to_string(), false);

        assert!(registry.detect(&context, &settings, None).is_none());
    }

    #[test]
    fn parallel_zoom_call_does_not_mask_the_tracked_telemost_signal() {
        use crate::meeting_detection::context::TelemostSignal;

        let registry = ProviderRegistry::default();
        let mut settings = MeetingDetectionSettings::default();
        settings.enabled_providers.insert("telemost".into(), true);
        let mut context = DetectionContext::from_process_names(Platform::Macos, ["CptHost"]);
        context.telemost_signal = TelemostSignal::Active;

        assert_eq!(
            registry
                .detect(&context, &settings, None)
                .unwrap()
                .provider_id,
            "zoom"
        );
        assert_eq!(
            registry
                .detect(&context, &settings, Some("telemost"))
                .unwrap()
                .provider_id,
            "telemost"
        );

        context.telemost_signal = TelemostSignal::Inactive;
        assert!(registry
            .detect(&context, &settings, Some("telemost"))
            .is_none());
    }
}
