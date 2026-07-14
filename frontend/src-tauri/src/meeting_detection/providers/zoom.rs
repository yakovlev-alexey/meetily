use crate::meeting_detection::{
    context::DetectionContext,
    provider::{DetectionEvidence, EvidenceKind, MeetingProvider, Platform},
};

const ZOOM_ACTIVE_MEETING_PROCESS: &str = "cpthost";
const ZOOM_SUPPORTED_PLATFORMS: &[Platform] = &[Platform::Macos];

#[derive(Debug, Default)]
pub struct ZoomProvider;

impl MeetingProvider for ZoomProvider {
    fn id(&self) -> &'static str {
        "zoom"
    }

    fn display_name(&self) -> &'static str {
        "Zoom"
    }

    fn supported_platforms(&self) -> &'static [Platform] {
        ZOOM_SUPPORTED_PLATFORMS
    }

    fn detect(&self, context: &DetectionContext) -> Option<DetectionEvidence> {
        context
            .contains_process(ZOOM_ACTIVE_MEETING_PROCESS)
            .then(|| DetectionEvidence {
                kind: EvidenceKind::Process,
                detail: "active_call_helper".to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_active_zoom_meeting_from_cpthost_on_macos() {
        let provider = ZoomProvider;
        let context = DetectionContext::from_process_names(Platform::Macos, ["zoom.us", "CptHost"]);

        let observation = provider.observe(&context).expect("Zoom should be detected");

        assert_eq!(observation.provider_id, "zoom");
        assert_eq!(observation.provider_name, "Zoom");
        assert_eq!(observation.evidence.kind, EvidenceKind::Process);
        assert_eq!(observation.evidence.detail, "active_call_helper");
    }

    #[test]
    fn does_not_treat_idle_zoom_app_as_a_meeting() {
        let provider = ZoomProvider;
        let context =
            DetectionContext::from_process_names(Platform::Macos, ["zoom.us", "ZoomClips"]);

        assert!(provider.observe(&context).is_none());
    }

    #[test]
    fn does_not_report_zoom_on_unsupported_platform() {
        let provider = ZoomProvider;
        let context = DetectionContext::from_process_names(Platform::Windows, ["CptHost"]);

        assert!(provider.observe(&context).is_none());
        assert!(!provider.descriptor(Platform::Windows).supported);
        assert!(provider.descriptor(Platform::Macos).supported);
    }
}
