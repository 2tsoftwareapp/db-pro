use super::ide_workspace::TaskRunResult;
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FilesTasksAction {
    RunTask,
    /// Stop the running shell task via `child.kill()` (#579).
    CancelTask,
    ConfirmShell,
    CancelShell,
    RunBenchmark,
}

pub(super) struct FilesTasksContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) command: &'a mut String,
    pub(super) last_task: Option<&'a TaskRunResult>,
    pub(super) pending_shell: bool,
    /// Live output of the task currently running on the worker thread (#579).
    pub(super) task_output: &'a [String],
    /// True while a task is running; flips the Run button into Cancel.
    pub(super) task_running: bool,
}

impl FilesTasksContext<'_> {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui) -> Vec<FilesTasksAction> {
        let mut actions = self.draw_command_form(ui);
        self.draw_task_output(ui);
        self.draw_last_task(ui);
        actions.extend(self.draw_benchmark(ui));
        actions
    }

    fn draw_command_form(&mut self, ui: &mut egui::Ui) -> Vec<FilesTasksAction> {
        let mut actions = Vec::new();
        Input::new(self.command, "shell command in workspace root…", self.theme)
            .width(ui.available_width())
            .show(ui);
        ui.add_space(4.0);
        if self.task_running {
            self.draw_cancel_button(ui, &mut actions);
        } else if Button::new(self.theme)
            .text("Run")
            .variant(ButtonVariant::Secondary)
            .size(ButtonSize::Sm)
            .show(ui)
            .clicked()
        {
            actions.push(FilesTasksAction::RunTask);
        }
        if self.pending_shell {
            ui.label("Run this command with sh -lc?");
            if Button::new(self.theme)
                .text("Confirm shell")
                .variant(ButtonVariant::Destructive)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                actions.push(FilesTasksAction::ConfirmShell);
            }
            if Button::new(self.theme)
                .text("Do not run")
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                actions.push(FilesTasksAction::CancelShell);
            }
        }
        actions
    }

    /// Streamed output of the in-flight task (#579), newest lines last.
    fn draw_task_output(&self, ui: &mut egui::Ui) {
        if self.task_output.is_empty() {
            return;
        }
        ui.add_space(6.0);
        ScrollArea::new(self.theme)
            .id_salt("files-task-output")
            .max_height(160.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in self.task_output {
                    ui.label(
                        RichText::new(line)
                            .small()
                            .monospace()
                            .color(self.theme.text_muted),
                    );
                }
            });
    }

    /// Cancel button + status while a task streams (#579).
    fn draw_cancel_button(&self, ui: &mut egui::Ui, actions: &mut Vec<FilesTasksAction>) {
        if Button::new(self.theme)
            .text("Cancel")
            .variant(ButtonVariant::Destructive)
            .size(ButtonSize::Sm)
            .show(ui)
            .clicked()
        {
            actions.push(FilesTasksAction::CancelTask);
        }
        ui.label(
            RichText::new("Task running…")
                .small()
                .color(self.theme.text_secondary),
        );
    }

    fn draw_last_task(&self, ui: &mut egui::Ui) {
        let Some(result) = self.last_task else {
            return;
        };
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "$ {} · exit {} · {}",
                result.command,
                fmt_opt(result.exit_code),
                format_duration_millis(result.duration_ms)
            ))
            .small()
            .monospace()
            .color(self.theme.text_secondary),
        );
        if !result.stdout.is_empty() {
            ui.label(
                RichText::new(result.stdout.chars().take(800).collect::<String>())
                    .small()
                    .monospace()
                    .color(self.theme.text_muted),
            );
        }
        if !result.stderr.is_empty() {
            ui.label(
                RichText::new(result.stderr.chars().take(400).collect::<String>())
                    .small()
                    .monospace()
                    .color(self.theme.danger),
            );
        }
    }

    fn draw_benchmark(&self, ui: &mut egui::Ui) -> Vec<FilesTasksAction> {
        let mut actions = Vec::new();
        ui.add_space(8.0);
        ui.label(
            RichText::new("BENCHMARK (local timing)")
                .font(font_caption())
                .strong()
                .color(self.theme.text_secondary),
        );
        if Button::new(self.theme)
            .text("Run sample suite")
            .variant(ButtonVariant::Secondary)
            .size(ButtonSize::Sm)
            .show(ui)
            .clicked()
        {
            actions.push(FilesTasksAction::RunBenchmark);
        }
        actions
    }
}
