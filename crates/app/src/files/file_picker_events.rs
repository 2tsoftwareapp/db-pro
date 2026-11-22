//! Native file-picker reducers over the owning feature state.

use super::{ConnectionDialogState, FeedbackState, OverlayState};

pub(super) fn on_file_picked(
    dialog: &mut ConnectionDialogState,
    overlay: &mut OverlayState,
    feedback: &mut FeedbackState,
    kind: &str,
    path: Option<String>,
) -> Option<std::path::PathBuf> {
    if let Some(path) = path {
        if kind == "sqlite" {
            dialog.draft_mut().database = path;
        } else if kind == "ssh-key" {
            dialog.draft_mut().ssh_private_key = path;
        } else if kind == "backup" {
            overlay.backup_output_path = path;
        } else if kind == "restore" {
            overlay.restore_input_path = path;
        } else if kind == "export-results" {
            overlay.export_path = path;
            // #621: a browsed destination is explicit, so it is not re-generated.
            overlay.export_path_is_default = false;
        } else if kind == "workspace-folder" {
            let folder = std::path::PathBuf::from(path);
            dialog.clear_error();
            dialog.set_test_valid(false);
            return Some(folder);
        }
        dialog.clear_error();
        dialog.set_test_valid(false);
    } else if kind == "sqlite" || kind == "ssh-key" {
        dialog.set_error("File selection was cancelled");
        dialog.set_test_valid(false);
    } else if kind == "workspace-folder" {
        feedback.set_runtime_message("Workspace folder selection was cancelled");
    }
    None
}
