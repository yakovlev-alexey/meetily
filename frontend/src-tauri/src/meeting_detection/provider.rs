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

    pub fn detect_first(
        &self,
        context: &DetectionContext,
        settings: &MeetingDetectionSettings,
    ) -> Option<MeetingObservation> {
        self.providers
            .iter()
            .filter(|provider| settings.is_provider_enabled(provider.id()))
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

        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].id, "zoom");
        assert!(descriptors[0].supported);
    }

    #[test]
    fn registry_respects_provider_toggle() {
        let registry = ProviderRegistry::default();
        let context = DetectionContext::from_process_names(Platform::Macos, ["CptHost"]);
        let mut settings = MeetingDetectionSettings::default();
        settings.enabled_providers.insert("zoom".to_string(), false);

        assert!(registry.detect_first(&context, &settings).is_none());
    }
}
