use super::app_types::Activity;
use crate::components::overlay::{ToastManager, ToastPosition};

/// Bound on the retained message log; oldest entries drop first.
const FEEDBACK_LOG_CAPACITY: usize = 50;

/// One retained status message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeedbackEntry {
    pub(super) message: String,
    /// Activity that produced the message; `None` = global.
    pub(super) activity: Option<Activity>,
    /// `true` for failures, so the shell can show them differently.
    pub(super) is_error: bool,
}

/// User-visible feedback shared by feature slices and the shell status surfaces.
///
/// #571: `runtime_message` alone is a single slot, so a routine "copied" message
/// overwrote the error that preceded it and nothing remained to look at. Every
/// message now also lands in `log`; the status line still shows the newest one.
#[derive(Debug, Default)]
pub(crate) struct FeedbackState {
    pub(super) runtime_message: String,
    /// When set, the status bar shows `runtime_message` only on this activity.
    pub(super) runtime_message_activity: Option<Activity>,
    pub(super) copy_status: String,
    pub(super) toasts: ToastManager,
    /// Recent messages, newest last (#571).
    log: Vec<FeedbackEntry>,
}

impl FeedbackState {
    pub(crate) fn set_runtime_message(&mut self, message: impl Into<String>) {
        self.runtime_message_activity = None;
        let message = message.into();
        self.remember(message.clone(), None, false);
        self.runtime_message = message;
    }

    pub(crate) fn set_activity_runtime_message(&mut self, activity: Activity, message: impl Into<String>) {
        self.runtime_message_activity = Some(activity);
        let message = message.into();
        self.remember(message.clone(), Some(activity), false);
        self.runtime_message = message;
    }

    /// Recent messages, newest last.
    pub(crate) fn log(&self) -> &[FeedbackEntry] {
        &self.log
    }

    /// Newest retained failure, if any — surfaced when the status line was
    /// overwritten by routine chatter.
    pub(crate) fn last_error(&self) -> Option<&FeedbackEntry> {
        self.log.iter().rev().find(|entry| entry.is_error)
    }

    fn remember(&mut self, message: String, activity: Option<Activity>, is_error: bool) {
        if message.is_empty() {
            return;
        }
        // Identical consecutive messages carry no new information.
        if self.log.last().is_some_and(|last| {
            last.message == message && last.activity == activity && last.is_error == is_error
        }) {
            return;
        }
        self.log.push(FeedbackEntry {
            message,
            activity,
            is_error,
        });
        if self.log.len() > FEEDBACK_LOG_CAPACITY {
            let overflow = self.log.len() - FEEDBACK_LOG_CAPACITY;
            self.log.drain(0..overflow);
        }
    }

    pub(crate) fn runtime_message_for_display(&self, current: Activity) -> &str {
        match self.runtime_message_activity {
            Some(scope) if scope != current => "",
            _ => self.runtime_message.as_str(),
        }
    }

    pub(crate) fn show_error_toast(&mut self, message: impl Into<String>) {
        // Keep the error in the log too: the toast is persistent now, but the
        // log survives dismissing it (#571).
        let message = message.into();
        self.remember(message.clone(), None, true);
        self.toasts.error(message, ToastPosition::BottomRight);
    }

    pub(crate) fn show_success_toast(&mut self, message: impl Into<String>) {
        self.toasts.success(message, ToastPosition::BottomRight);
    }

    pub(crate) fn show_info_toast(&mut self, message: impl Into<String>) {
        self.toasts.info(message, ToastPosition::BottomRight);
    }
}

#[cfg(test)]
#[path = "tests/feedback_state_tests.rs"]
mod tests;

