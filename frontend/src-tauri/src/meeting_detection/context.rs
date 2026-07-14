use std::collections::HashSet;

use super::provider::Platform;

/// Local operating-system state shared by every meeting provider in one poll.
///
/// Process names are normalized once so adding providers does not add more
/// process scans or provider-specific normalization rules.
#[derive(Debug, Clone, Default)]
pub struct DetectionContext {
    platform: Platform,
    process_names: HashSet<String>,
}

impl DetectionContext {
    pub fn from_process_names<I, S>(platform: Platform, process_names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self {
            platform,
            process_names: process_names
                .into_iter()
                .map(|name| normalize_process_name(name.as_ref()))
                .filter(|name| !name.is_empty())
                .collect(),
        }
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    pub fn contains_process(&self, process_name: &str) -> bool {
        self.process_names
            .contains(&normalize_process_name(process_name))
    }
}

fn normalize_process_name(process_name: &str) -> String {
    process_name.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_process_names_once() {
        let context =
            DetectionContext::from_process_names(Platform::Macos, [" CptHost ", "zoom.us", ""]);

        assert!(context.contains_process("cpthost"));
        assert!(context.contains_process("CPTHOST"));
        assert!(context.contains_process("zoom.us"));
        assert!(!context.contains_process("Microsoft Teams"));
    }
}
