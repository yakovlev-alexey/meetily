use std::sync::Arc;

use super::provider::MeetingProvider;

mod zoom;

pub use zoom::ZoomProvider;

pub fn default_providers() -> Vec<Arc<dyn MeetingProvider>> {
    vec![Arc::new(ZoomProvider)]
}
