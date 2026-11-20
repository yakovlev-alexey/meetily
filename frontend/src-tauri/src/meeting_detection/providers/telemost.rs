use crate::meeting_detection::{
    context::{DetectionContext, TelemostSignal},
    provider::{DetectionEvidence, EvidenceKind, MeetingProvider, Platform},
};

pub(crate) const PROVIDER_ID: &str = "telemost";

pub struct TelemostProvider;

impl MeetingProvider for TelemostProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn display_name(&self) -> &'static str {
        "Yandex Telemost"
    }

    fn supported_platforms(&self) -> &'static [Platform] {
        &[Platform::Macos]
    }

    fn detect(&self, context: &DetectionContext) -> Option<DetectionEvidence> {
        (context.telemost_signal == TelemostSignal::Active).then(|| DetectionEvidence {
            kind: EvidenceKind::Accessibility,
            detail: "active_call_controls".into(),
        })
    }
}

#[cfg(any(target_os = "macos", test))]
fn is_end_call_button(identifier: &str) -> bool {
    identifier.ends_with(".MainConferenceView.ControlPanelWidget.endCallButton")
}

#[cfg(target_os = "macos")]
pub fn read_signal() -> TelemostSignal {
    use cidre::{ax, ns};
    use objc::rc::autoreleasepool;

    autoreleasepool(|| {
        let bundle_id = ns::String::with_str("ru.yandex.desktop.telemost");
        let applications = ns::RunningApp::with_bundle_id(&bundle_id);
        let trusted = ax::is_process_trusted();
        if applications.is_empty() {
            return if trusted {
                TelemostSignal::Inactive
            } else {
                TelemostSignal::InactivePermissionRequired
            };
        }
        if !trusted {
            return TelemostSignal::PermissionRequired;
        }
        let mut unreadable = false;
        for application in applications.iter().filter(|app| !app.is_terminated()) {
            let element = ax::UiElement::with_app_pid(application.pid());
            match scan_app(&element) {
                Some(true) => return TelemostSignal::Active,
                Some(false) => {}
                None => unreadable = true,
            }
        }

        if unreadable {
            TelemostSignal::Unavailable
        } else {
            TelemostSignal::Inactive
        }
    })
}

#[cfg(target_os = "macos")]
fn scan_app(app: &cidre::ax::UiElement) -> Option<bool> {
    use cidre::{ax, cf};
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(1);
    app.set_messaging_timeout_secs(0.25).ok()?;
    let windows = app.attr_value(ax::attr::windows()).ok()?;
    if windows.get_type_id() != cf::Array::type_id() {
        return None;
    }
    // AXWindows is an array of AXUIElements owned by the copied attribute.
    let windows = unsafe {
        &*(windows.as_type_ref() as *const cf::Type as *const cf::ArrayOf<ax::UiElement>)
    };
    let mut unreadable = false;
    for window in windows.iter() {
        if Instant::now() >= deadline {
            return None;
        }
        match scan_window(window, deadline) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => unreadable = true,
        }
    }
    (!unreadable).then_some(false)
}

#[cfg(target_os = "macos")]
fn scan_window(window: &cidre::ax::UiElement, deadline: std::time::Instant) -> Option<bool> {
    use cidre::{ax, cf};
    use std::time::Instant;

    window.set_messaging_timeout_secs(0.25).ok()?;
    // The Qt client's call toolbar controls are direct window children even
    // when minimized. Skip unrelated nested UI such as chats and participants.
    let children = window.children().ok()?;
    let identifier_attr: &ax::Attr = cf::str!(c"AXIdentifier").as_ref();
    for child in children.iter() {
        if Instant::now() >= deadline {
            return None;
        }
        child.set_messaging_timeout_secs(0.25).ok()?;
        let role = child.attr_value(ax::attr::role()).ok()?;
        if role.try_as_string()? != "AXButton" {
            continue;
        }
        match child.attr_value(identifier_attr) {
            Ok(value) if is_end_call_button(&value.try_as_string()?.to_string()) => {
                return Some(true);
            }
            Ok(_) => {}
            Err(error) if matches!(error, ax::err::NO_VALUE | ax::err::ATTR_UNSUPPORTED) => {}
            Err(_) => return None,
        }
    }
    Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_only_the_connected_call_button() {
        assert!(is_end_call_button(
            "TApplication.TelemostWindowTelemes.centralwidget.Telemost::QWidgetRestrictedToDeleteLater.MainConferenceView.ControlPanelWidget.endCallButton"
        ));
        assert!(!is_end_call_button(
            "Telemost.PreJoinView.ControlPanelWidget.endCallButton"
        ));
        assert!(!is_end_call_button(
            "Telemost.MainConferenceView.ControlPanelWidget.cameraButton"
        ));
    }

    #[test]
    fn only_a_confirmed_call_produces_an_observation() {
        let provider = TelemostProvider;
        let mut context =
            DetectionContext::from_process_names(Platform::Macos, ["Yandex.Telemost"]);
        for signal in [
            TelemostSignal::Inactive,
            TelemostSignal::PermissionRequired,
            TelemostSignal::InactivePermissionRequired,
            TelemostSignal::Unavailable,
        ] {
            context.telemost_signal = signal;
            assert!(provider.observe(&context).is_none());
        }
        context.telemost_signal = TelemostSignal::Active;
        assert_eq!(
            provider.observe(&context).unwrap().evidence.kind,
            EvidenceKind::Accessibility
        );
    }
}
