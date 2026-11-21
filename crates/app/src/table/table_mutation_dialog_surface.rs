// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
//! Staged-table-change dialogs and typed user intents.

use super::change_set::ChangeCounts;
use super::*;
use crate::components::button::{Button, ButtonVariant};
use crate::components::dialog::Dialog;
use crate::components::dialog::config::DIALOG_FORM_BODY_HEIGHT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DiscardChangesAction {
    Apply,
    Discard,
    Cancel,
}

pub(super) struct DiscardChangesContext {
    pub(super) theme: DbProTheme,
    pub(super) counts: ChangeCounts,
}

impl DiscardChangesContext {
    pub(super) fn draw(&self, ui: &mut egui::Ui) -> Option<DiscardChangesAction> {
        let mut open = true;
        let mut action = None;
        let description = format!(
            // cc-scan:allow LINE_TOO_LONG — literal must not wrap
            "You have {} unapplied staged change(s) (+{} inserts, {} updates, {} deletes). Apply changes to database, discard them, or cancel navigation?",
            self.counts.total(),
            self.counts.inserts,
            self.counts.updates,
            self.counts.deletes
        );
        Dialog::new(&mut open, "Unapplied Changes", self.theme)
            .description(&description)
            .id_salt("discard-table-changes")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if Button::new(self.theme)
                        .text("Apply Changes")
                        .variant(ButtonVariant::Default)
                        .show(ui)
                        .clicked()
                    {
                        action = Some(DiscardChangesAction::Apply);
                    }
                    if Button::new(self.theme)
                        .text("Discard Changes")
                        .variant(ButtonVariant::Destructive)
                        .show(ui)
                        .clicked()
                    {
                        action = Some(DiscardChangesAction::Discard);
                    }
                    if Button::new(self.theme)
                        .text("Cancel")
                        .variant(ButtonVariant::Ghost)
                        .show(ui)
                        .clicked()
                    {
                        action = Some(DiscardChangesAction::Cancel);
                    }
                });
            });
        action.or_else(|| (!open).then_some(DiscardChangesAction::Cancel))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PendingChangesAction {
    RevertCell(RowIdentity, usize),
    RevertRow(RowIdentity),
    RemoveInsert(u64),
    RequestDiscardAll,
    Close,
}

pub(super) struct PendingChangesContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) changes: &'a [StagedChange],
}

impl PendingChangesContext<'_> {
    pub(super) fn draw(&self, ui: &mut egui::Ui) -> Vec<PendingChangesAction> {
        let mut actions = Vec::new();
        let groups = group_changes(self.changes);
        let mut open = true;
        Dialog::new(&mut open, "Pending changes", self.theme)
            .width(680.0)
            .body_height(DIALOG_FORM_BODY_HEIGHT)
            .id_salt("pending_table_changes_dialog")
            .show_framed_ctx(ui.ctx(), |frame| {
                frame.body(|ui| {
                    ui.label(
                        RichText::new("Review staged changes before Apply")
                            .small()
                            .color(self.theme.text_muted),
                    );
                    Separator::horizontal(self.theme).show(ui);
                    ui.horizontal(|ui| {
                        ui.label(format!("{} row group(s)", groups.len()));
                        if Button::new(self.theme)
                            .text("Discard All")
                            .variant(ButtonVariant::Secondary)
                            .size(ButtonSize::Sm)
                            .show(ui)
                            .clicked()
                        {
                            actions.push(PendingChangesAction::RequestDiscardAll);
                        }
                    });
                    ScrollArea::new(self.theme).max_height(360.0).show(ui, |ui| {
                        for group in &groups {
                            self.draw_group(ui, group, &mut actions);
                        }
                    });
                });
                frame.footer(|ui| {
                    if Button::new(self.theme)
                        .text("Close")
                        .variant(ButtonVariant::Ghost)
                        .show(ui)
                        .clicked()
                    {
                        actions.push(PendingChangesAction::Close);
                    }
                });
            });
        if !open {
            actions.push(PendingChangesAction::Close);
        }
        actions
    }

    fn draw_group(&self, ui: &mut egui::Ui, group: &ChangeGroup, actions: &mut Vec<PendingChangesAction>) {
        self.draw_group_header(ui, group, actions);
        self.draw_group_entries(ui, group, actions);
        Separator::horizontal(self.theme).show(ui);
    }

    fn draw_group_header(&self, ui: &mut egui::Ui, group: &ChangeGroup, actions: &mut Vec<PendingChangesAction>) {
        let group_label = group.label();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(group_label).strong());
            if let Some(identity) = &group.identity {
                if Button::new(self.theme)
                    .text("Revert Row")
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .show(ui)
                    .clicked()
                {
                    actions.push(PendingChangesAction::RevertRow(identity.clone()));
                }
            } else if let Some(local_id) = group.local_id {
                if Button::new(self.theme)
                    .text("Remove Insert")
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .show(ui)
                    .clicked()
                {
                    actions.push(PendingChangesAction::RemoveInsert(local_id));
                }
            }
        });
    }

    fn draw_group_entries(&self, ui: &mut egui::Ui, group: &ChangeGroup, actions: &mut Vec<PendingChangesAction>) {
        for change in &group.changes {
            match change {
                StagedChange::Update {
                    identity,
                    column_index,
                    column,
                    original,
                    value,
                    ..
                } => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "{column}: {} → {}",
                            crate::cell_text(original),
                            crate::cell_text(value)
                        ));
                        if Button::new(self.theme)
                            .text("Revert Cell")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .show(ui)
                            .clicked()
                        {
                            actions.push(PendingChangesAction::RevertCell(identity.clone(), *column_index));
                        }
                    });
                }
                StagedChange::Delete { .. } => {
                    ui.label(RichText::new("Delete row").color(self.theme.warning));
                }
                StagedChange::Insert { columns, .. } => {
                    ui.label(format!("Insert ({})", columns.join(", ")));
                }
            }
        }
    }
}

struct ChangeGroup {
    identity: Option<RowIdentity>,
    local_id: Option<u64>,
    changes: Vec<StagedChange>,
}

impl ChangeGroup {
    fn label(&self) -> String {
        if let Some(identity) = &self.identity {
            return format!(
                "Row PK: [{}]",
                identity
                    .original_pk_values
                    .iter()
                    .map(crate::cell_text)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        match self.local_id {
            Some(local_id) => format!("New row · temporary identity #{local_id}"),
            None => "New row".to_owned(),
        }
    }
}

fn group_changes(changes: &[StagedChange]) -> Vec<ChangeGroup> {
    let mut groups = Vec::new();
    for entry in changes {
        let (identity, local_id) = match entry {
            StagedChange::Update { identity, .. } | StagedChange::Delete { identity, .. } => {
                (Some(identity.clone()), None)
            }
            StagedChange::Insert { local_id, .. } => (None, Some(*local_id)),
        };
        if let Some(group) = groups
            .iter_mut()
            .find(|group: &&mut ChangeGroup| {
                let same_row = match (&group.identity, &identity) {
                    (Some(group_identity), Some(entry_identity)) => group_identity.same_row(entry_identity),
                    // Inserts carry no row identity; `local_id` groups them.
                    (None, None) => true,
                    _ => false,
                };
                same_row && group.local_id == local_id
            })
        {
            group.changes.push(entry.clone());
        } else {
            groups.push(ChangeGroup {
                identity,
                local_id,
                changes: vec![entry.clone()],
            });
        }
    }
    groups
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StagedApplyConfirmAction {
    Apply,
    Cancel,
}

/// Mandatory gate when a staged apply contains deletes — the breakdown is
/// shown before `ApplyTableChanges` is dispatched.
pub(super) struct StagedApplyConfirmContext {
    pub(super) theme: DbProTheme,
    pub(super) counts: ChangeCounts,
    pub(super) table: String,
}

impl StagedApplyConfirmContext {
    pub(super) fn draw(&self, ui: &mut egui::Ui) -> Option<StagedApplyConfirmAction> {
        let mut open = true;
        let mut action = None;
        let description = format!(
            "Apply {} staged change(s) to `{}` in one transaction (+{} inserts, ~{} updates, -{} deletes)?",
            self.counts.total(),
            self.table,
            self.counts.inserts,
            self.counts.updates,
            self.counts.deletes,
        );
        Dialog::new(&mut open, "Apply staged changes?", self.theme)
            .description(&description)
            .id_salt("staged-apply-confirm")
            .show(ui, |ui| {
                if self.counts.deletes > 0 {
                    ui.colored_label(
                        self.theme.danger,
                        format!("{} row(s) will be permanently deleted.", self.counts.deletes),
                    );
                }
                ui.horizontal(|ui| {
                    if Button::new(self.theme)
                        .text("Apply Changes")
                        .variant(ButtonVariant::Default)
                        .show(ui)
                        .clicked()
                    {
                        action = Some(StagedApplyConfirmAction::Apply);
                    }
                    if Button::new(self.theme)
                        .text("Cancel")
                        .variant(ButtonVariant::Ghost)
                        .show(ui)
                        .clicked()
                    {
                        action = Some(StagedApplyConfirmAction::Cancel);
                    }
                });
            });
        action.or_else(|| (!open).then_some(StagedApplyConfirmAction::Cancel))
    }
}
