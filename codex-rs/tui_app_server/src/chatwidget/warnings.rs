const RUNTIME_RECOVERY_PREFIX: &str = "runtime recovery: ";
const RUNTIME_POLICY_DRIFT_PREFIX: &str = "runtime policy drift detected: ";
const RUNTIME_MISMATCH_PREFIX: &str = "runtime mismatch detected: ";
const RUNTIME_FALLBACK_TO_HUMAN_PREFIX: &str = "runtime fallback to human: ";

pub(super) fn format_warning_message(message: String) -> String {
    let (title, detail) = if let Some(detail) = message.strip_prefix(RUNTIME_RECOVERY_PREFIX) {
        ("Runtime recovery", detail)
    } else if let Some(detail) = message.strip_prefix(RUNTIME_POLICY_DRIFT_PREFIX) {
        ("Runtime policy drift", detail)
    } else if let Some(detail) = message.strip_prefix(RUNTIME_MISMATCH_PREFIX) {
        ("Runtime mismatch", detail)
    } else if let Some(detail) = message.strip_prefix(RUNTIME_FALLBACK_TO_HUMAN_PREFIX) {
        ("Runtime fallback to human review", detail)
    } else {
        return message;
    };

    let detail = detail.trim();
    if detail.is_empty() {
        title.to_string()
    } else {
        format!("{title}\n{detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::format_warning_message;
    use pretty_assertions::assert_eq;

    #[test]
    fn rewrites_runtime_warning_variants() {
        assert_eq!(
            format_warning_message(
                "runtime recovery: switched to conservative enforcement".to_string()
            ),
            "Runtime recovery\nswitched to conservative enforcement".to_string()
        );
        assert_eq!(
            format_warning_message(
                "runtime policy drift detected: enforcement policy changed after approval"
                    .to_string()
            ),
            "Runtime policy drift\nenforcement policy changed after approval".to_string()
        );
        assert_eq!(
            format_warning_message(
                "runtime mismatch detected: observed delete exceeded the predicted permit scope"
                    .to_string()
            ),
            "Runtime mismatch\nobserved delete exceeded the predicted permit scope".to_string()
        );
        assert_eq!(
            format_warning_message(
                "runtime fallback to human: runtime lease unavailable for destructive patch"
                    .to_string()
            ),
            "Runtime fallback to human review\nruntime lease unavailable for destructive patch"
                .to_string()
        );
    }

    #[test]
    fn leaves_other_warnings_unchanged() {
        let message = "Ignored invalid terminal title item: foo.".to_string();
        assert_eq!(format_warning_message(message.clone()), message);
    }
}
