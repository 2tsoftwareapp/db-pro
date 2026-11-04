#![cfg_attr(not(any(target_os = "macos", feature = "capture")), allow(dead_code))]

//! Route macOS window close through `-[NSApplication terminate:]` (R-01 M2).
//!
//! Closing the eframe window tears down `WinitView` while AppKit's Touch Bar
//! finder still holds a KVO observer on `nextResponder`. That is the
//! `NSRangeException` abort. `-[NSApplication terminate:]` is what Cmd+Q
//! already does via `PredefinedMenuItem::quit`; winit's
//! `applicationWillTerminate:` then runs eframe's `App::save` + storage flush
//! before the process ends, so persisted theme/history survive.
//!
//! Never abort the process from Rust on this path: that skips `App::save`.

use eframe::egui::{Context, ViewportCommand, ViewportId};

/// Call at the end of `eframe::App::logic`. When the root window was asked to
/// close and `quit` takes the platform path, cancel eframe teardown so the
/// window stays until AppKit terminates.
pub(crate) fn take_over_macos_close(ctx: &Context, quit: impl FnOnce() -> bool) -> bool {
    if ctx.viewport_id() != ViewportId::ROOT || !ctx.input(|i| i.viewport().close_requested()) {
        return false;
    }
    if ctx.viewport(|v| v.commands.contains(&ViewportCommand::CancelClose)) {
        return false;
    }
    if !quit() {
        return false;
    }
    ctx.send_viewport_cmd(ViewportCommand::CancelClose);
    true
}

/// Intercept a macOS close request (red button, Cmd+W, in-app control, capture
/// `ViewportCommand::Close`) and terminate through NSApp after eframe save.
pub(crate) fn on_macos_window_close(ctx: &Context) {
    let _ = take_over_macos_close(ctx, || {
        #[cfg(target_os = "macos")]
        {
            schedule_nsapp_terminate();
            true
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    });
}

#[cfg(target_os = "macos")]
fn schedule_nsapp_terminate() {
    use dispatch2::DispatchQueue;
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    // `terminate:` from inside winit's event handler cannot deliver the exit
    // event until the current event finishes. The main queue runs after that.
    DispatchQueue::main().exec_async(|| {
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm).terminate(None);
        }
    });
}
