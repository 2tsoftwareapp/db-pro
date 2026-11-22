//! Tests for `../toast.rs`.
//!
//! #571: an error toast expired after the same 5 s as a routine success message,
//! could not be paused by hovering, and had no way to copy its text — so the
//! only record of a failure vanished while the user was reading it.

use super::*;

fn manager_with_error() -> (ToastManager, u64) {
    let mut manager = ToastManager::new();
    let id = manager.error("connection refused", ToastPosition::BottomRight);
    (manager, id)
}

#[test]
fn error_toasts_do_not_expire_on_the_default_timer() {
    let (mut manager, _) = manager_with_error();
    manager.tick(DURATION_TOAST_DEFAULT_SECS * 10.0);
    assert_eq!(manager.len(), 1, "an error must survive the default duration");
}

#[test]
fn success_toasts_still_auto_dismiss() {
    let mut manager = ToastManager::new();
    manager.success("saved", ToastPosition::BottomRight);
    manager.tick(DURATION_TOAST_DEFAULT_SECS + 0.1);
    assert!(manager.is_empty(), "routine confirmations must not pile up");
}

#[test]
fn hover_pauses_the_countdown() {
    let mut manager = ToastManager::new();
    let id = manager.success("saved", ToastPosition::BottomRight);
    manager.set_hovered(id, true);
    manager.tick(DURATION_TOAST_DEFAULT_SECS * 2.0);
    assert_eq!(manager.len(), 1, "hovering keeps the toast on screen");
    manager.set_hovered(id, false);
    manager.tick(DURATION_TOAST_DEFAULT_SECS + 0.1);
    assert!(manager.is_empty(), "timing resumes once the pointer leaves");
}

#[test]
fn error_toasts_offer_a_copy_action() {
    let (manager, _) = manager_with_error();
    let item = manager.toasts.first().expect("the error is queued");
    assert_eq!(item.action_label.as_deref(), Some(config::TOAST_COPY_ACTION_LABEL));
    assert!(item.persistent);
}

#[test]
fn duplicate_errors_refresh_instead_of_stacking() {
    let mut manager = ToastManager::new();
    let first = manager.error("timeout", ToastPosition::BottomRight);
    let second = manager.error("timeout", ToastPosition::BottomRight);
    assert_eq!(first, second);
    assert_eq!(manager.len(), 1);
}
