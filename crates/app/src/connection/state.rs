use crate::UiConnectionDraft;

/// UI-owned state for the connection editor dialog.
///
/// This aggregate deliberately contains draft/edit/test state only. The saved
/// connection collection and lifecycle are siblings in `ConnectionFeatureState`;
/// they share the feature boundary without sharing mutable storage.
#[derive(Debug, Clone, Default)]
pub(crate) struct ConnectionDialogState {
    pub(super) open: bool,
    pub(super) focus_name_on_open: bool,
    pub(super) editing_connection_id: Option<String>,
    pub(super) draft: UiConnectionDraft,
    /// Snapshot of the draft as seeded by the last Open* action; the dirty
    /// check compares against this so closing a half-filled form asks first.
    pub(super) seeded_draft: Option<UiConnectionDraft>,
    /// Set when Esc/backdrop tried to close a dirty form — the confirmation
    /// dialog renders while this is true.
    pub(super) discard_draft_confirmation: bool,
    pub(super) show_password: bool,
    pub(super) error: String,
    pub(super) error_toast_pending: bool,
    pub(super) test_valid: bool,
    pub(super) test_draft: Option<UiConnectionDraft>,
    pub(super) diagnostics: Option<db_pro_core::domain::connection_diagnostics::ConnectionDiagnosticsReport>,
    pub(super) ssh_profiles: Vec<db_pro_core::domain::connection::SshProfile>,
    /// Collapsed state of the optional panels; `Collapsible` owns the toggle.
    pub(super) cloud_presets_open: bool,
    pub(super) tags_panel_open: bool,
    /// Bumped on every open so the form body scroll area gets a fresh Id and
    /// reopens scrolled to the top (#609).
    pub(super) open_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConnectionDialogAction {
    OpenNew,
    OpenEdit {
        connection_id: String,
        draft: UiConnectionDraft,
    },
    OpenDuplicate {
        draft: UiConnectionDraft,
    },
    DraftChanged,
    TestStarted {
        draft: UiConnectionDraft,
    },
    TestCancelled,
    Close,
}

impl ConnectionDialogState {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// The form differs from its seeded baseline — closing must confirm.
    pub(crate) fn draft_is_dirty(&self) -> bool {
        self.seeded_draft
            .as_ref()
            .is_some_and(|seeded| seeded != &self.draft)
    }

    pub(crate) fn draft(&self) -> &UiConnectionDraft {
        &self.draft
    }

    pub(crate) fn draft_mut(&mut self) -> &mut UiConnectionDraft {
        &mut self.draft
    }

    pub(crate) fn set_draft(&mut self, draft: UiConnectionDraft) {
        self.draft = draft;
    }

    pub(crate) fn set_editing_connection_id(&mut self, connection_id: Option<String>) {
        self.editing_connection_id = connection_id;
    }

    pub(crate) fn editing_connection_id(&self) -> Option<&str> {
        self.editing_connection_id.as_deref()
    }

    pub(crate) fn set_error(&mut self, error: impl Into<String>) {
        self.error = error.into();
        self.error_toast_pending = true;
    }

    #[cfg(test)]
    pub(crate) fn error(&self) -> &str {
        &self.error
    }

    pub(crate) fn clear_error(&mut self) {
        self.error.clear();
        self.error_toast_pending = false;
    }

    /// Consume a freshly-set error once so the dialog can surface it as a
    /// toast instead of an inline alert.
    pub(crate) fn take_error_toast(&mut self) -> Option<String> {
        if self.error_toast_pending && !self.error.is_empty() {
            self.error_toast_pending = false;
            Some(self.error.clone())
        } else {
            None
        }
    }

    pub(crate) fn set_test_valid(&mut self, valid: bool) {
        self.test_valid = valid;
    }

    #[cfg(test)]
    pub(crate) fn test_valid(&self) -> bool {
        self.test_valid
    }

    pub(crate) fn test_draft(&self) -> Option<&UiConnectionDraft> {
        self.test_draft.as_ref()
    }

    pub(crate) fn clear_test(&mut self) {
        self.invalidate_test();
    }

    pub(crate) fn diagnostics(
        &self,
    ) -> Option<&db_pro_core::domain::connection_diagnostics::ConnectionDiagnosticsReport> {
        self.diagnostics.as_ref()
    }

    pub(crate) fn ssh_profiles(&self) -> &[db_pro_core::domain::connection::SshProfile] {
        &self.ssh_profiles
    }

    pub(crate) fn set_ssh_profiles(&mut self, profiles: Vec<db_pro_core::domain::connection::SshProfile>) {
        self.ssh_profiles = profiles;
    }

    pub(crate) fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    pub(crate) fn set_focus_name_on_open(&mut self, focus: bool) {
        self.focus_name_on_open = focus;
    }

    pub(crate) fn transition(&mut self, action: ConnectionDialogAction) {
        match action {
            ConnectionDialogAction::OpenNew => {
                self.editing_connection_id = None;
                self.draft = UiConnectionDraft::default();
                self.seeded_draft = Some(self.draft.clone());
                self.diagnostics = None;
                self.show_password = false;
                self.focus_name_on_open = true;
                self.discard_draft_confirmation = false;
                self.open_generation += 1;
                self.open = true;
                self.invalidate_test();
            }
            ConnectionDialogAction::OpenEdit { connection_id, draft } => {
                self.editing_connection_id = Some(connection_id);
                self.seeded_draft = Some(draft.clone());
                self.draft = draft;
                self.focus_name_on_open = true;
                self.discard_draft_confirmation = false;
                self.open_generation += 1;
                self.open = true;
                self.invalidate_test();
            }
            ConnectionDialogAction::OpenDuplicate { draft } => {
                self.editing_connection_id = None;
                self.seeded_draft = Some(draft.clone());
                self.draft = draft;
                self.focus_name_on_open = true;
                self.discard_draft_confirmation = false;
                self.open_generation += 1;
                self.open = true;
                self.invalidate_test();
            }
            ConnectionDialogAction::DraftChanged => self.invalidate_test(),
            ConnectionDialogAction::TestStarted { draft } => {
                self.test_valid = false;
                self.test_draft = Some(draft);
                self.clear_error();
            }
            ConnectionDialogAction::TestCancelled => {
                self.invalidate_test();
                self.diagnostics = None;
            }
            ConnectionDialogAction::Close => {
                self.open = false;
                self.invalidate_test();
            }
        }
    }

    fn invalidate_test(&mut self) {
        self.test_valid = false;
        self.test_draft = None;
        // Diagnostics describe one tested draft; any new draft or close makes
        // them stale — never show them against untested input (#609).
        self.diagnostics = None;
        self.clear_error();
    }
}

#[cfg(test)]
#[path = "tests/state_tests.rs"]
mod tests;

