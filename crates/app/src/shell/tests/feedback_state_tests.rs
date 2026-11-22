//! Tests for `../feedback_state.rs` (moved out of the source file).
use super::{FeedbackState, FEEDBACK_LOG_CAPACITY};

#[test]
fn default_feedback_is_quiet() {
    let state = FeedbackState::default();

    assert!(state.runtime_message.is_empty());
}

#[test]
fn activity_scoped_message_hides_on_other_activities() {
    use super::super::app_types::Activity;
    let mut state = FeedbackState::default();
    state.set_activity_runtime_message(Activity::Security, "Security · 3 role(s)");
    assert_eq!(
        state.runtime_message_for_display(Activity::Security),
        "Security · 3 role(s)"
    );
    assert!(state.runtime_message_for_display(Activity::Explorer).is_empty());
    assert!(state.copy_status.is_empty());
    assert!(state.toasts.is_empty());
}

#[test]
fn routine_messages_no_longer_erase_the_last_error() {
    // #571: a single `runtime_message` slot meant "copied" wiped the failure.
    use super::super::app_types::Activity;
    let mut state = FeedbackState::default();
    state.show_error_toast("connection refused");
    state.set_runtime_message("Copied `id` to clipboard");

    assert_eq!(state.runtime_message, "Copied `id` to clipboard");
    let error = state.last_error().expect("the failure is retained");
    assert_eq!(error.message, "connection refused");
    assert!(error.is_error);
    assert!(state.log().iter().any(|entry| entry.message == "Copied `id` to clipboard"));
    let _ = Activity::Explorer;
}

#[test]
fn repeated_identical_messages_are_collapsed() {
    let mut state = FeedbackState::default();
    for _ in 0..5 {
        state.set_runtime_message("Saved");
    }
    assert_eq!(state.log().len(), 1);
    assert_eq!(state.log()[0].message, "Saved");
}

#[test]
fn feedback_log_is_bounded() {
    let mut state = FeedbackState::default();
    for index in 0..(FEEDBACK_LOG_CAPACITY + 20) {
        state.set_runtime_message(format!("message {index}"));
    }
    assert_eq!(state.log().len(), FEEDBACK_LOG_CAPACITY);
    let newest = format!("message {}", FEEDBACK_LOG_CAPACITY + 19);
    assert_eq!(state.log().last().map(|entry| entry.message.as_str()), Some(newest.as_str()));
    assert_eq!(state.log()[0].message, "message 20", "oldest entries drop first");
}
