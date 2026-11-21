//! Result-pane shell presentation and typed user intents.
use super::*;
use egui::RichText;
use lucide_icons::Icon;

pub(super) struct QueryResultsSurfaceContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) result_count: usize,
    pub(super) active_result_index: usize,
    pub(super) result_names: &'a std::collections::HashMap<usize, String>,
    pub(super) pinned_results: &'a std::collections::BTreeSet<usize>,
    pub(super) result: Option<&'a UiQueryResult>,
    /// Applied query cap, displayed as a limit indicator without asserting truncation.
    pub(super) row_limit: Option<u64>,
    /// The grid still shows rows from an earlier run (#607).
    pub(super) results_stale: bool,
    /// The latest run is in flight while the previous results stay visible.
    pub(super) results_running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum QueryResultsSurfaceAction {
    SelectResult(usize),
    TogglePin(usize),
    CloseResult(usize),
    CloseOtherResults(usize),
    OpenExport,
    OpenInTab,
}

pub(super) fn draw_results<F>(
    context: &QueryResultsSurfaceContext<'_>,
    ui: &mut egui::Ui,
    mut draw_grid: F,
) -> Option<QueryResultsSurfaceAction>
where
    F: FnMut(&mut egui::Ui, &UiQueryResult),
{
    let results_width = ui.max_rect().width();
    let mut action = None;
    grid_frame(context.theme).show(ui, |ui| {
        ui.set_min_width(results_width.max(0.0));
        draw_result_selector(context, ui, &mut action);
        if context.results_stale && context.result.is_some() {
            draw_stale_results_banner(context, ui);
        }
        if let Some(result) = context.result {
            ui.horizontal(|ui| {
                let is_pinned = context.pinned_results.contains(&context.active_result_index);
                if Button::new(context.theme)
                    .icon(if is_pinned { Icon::PinOff } else { Icon::Pin })
                    .variant(if is_pinned { ButtonVariant::Secondary } else { ButtonVariant::Ghost })
                    .size(ButtonSize::IconSm)
                    .access_label(if is_pinned { "Unpin result tab" } else { "Pin result tab" })
                    .tooltip(if is_pinned { "Unpin result tab" } else { "Pin result tab" })
                    .show(ui)
                    .clicked()
                {
                    action = Some(QueryResultsSurfaceAction::TogglePin(context.active_result_index));
                }

                let loaded = result.rows.len();
                let limit_note = query_row_limit::row_limit_note(context.row_limit);
                let row_label = if loaded < result.row_count as usize {
                    // Sampled/truncated result — disclose the load count so
                    // Export/Copy-as on the visible subset isn't mistaken for
                    // the full result.
                    format!("{loaded} of {} rows{limit_note} · {} ms", result.row_count, result.duration_ms)
                } else {
                    format!("{} rows{limit_note} · {} ms", result.row_count, result.duration_ms)
                };
                ui.label(
                    RichText::new(row_label).small().color(context.theme.text_muted),
                );
                if Button::new(context.theme)
                    .text(t!("common.export"))
                    .variant(ButtonVariant::Secondary)
                    .size(ButtonSize::Sm)
                    .show(ui)
                    .clicked()
                {
                    action = Some(QueryResultsSurfaceAction::OpenExport);
                }
                if Button::new(context.theme)
                    .icon(Icon::ExternalLink)
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::IconSm)
                    .tooltip("Open results in a workspace tab")
                    .access_label("Open results in a workspace tab")
                    .show(ui)
                    .clicked()
                {
                    action = Some(QueryResultsSurfaceAction::OpenInTab);
                }
            });
            draw_grid(ui, result);
        } else {
            ui.centered_and_justified(|ui| {
                EmptyState::new(
                    Icon::Table2,
                    &t!("query.no_results"),
                    &t!("query.no_results_desc"),
                    context.theme,
                )
                .show(ui);
            });
        }
    });
    action
}

/// Flag the grid while it shows output of an earlier run so the user cannot
/// mistake it for the in-flight or just-cancelled query's result (#607).
fn draw_stale_results_banner(context: &QueryResultsSurfaceContext<'_>, ui: &mut egui::Ui) {
    let text = if context.results_running {
        "Running — grid still shows output from the previous run"
    } else {
        "Showing output from the previous run — the last query did not complete"
    };
    egui::Frame::NONE
        .fill(context.theme.warning_soft())
        .stroke(egui::Stroke::new(1.0, context.theme.warning.linear_multiply(0.4)))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(char::from(Icon::TriangleAlert).to_string())
                        .font(egui::FontId::new(
                            12.0,
                            egui::FontFamily::Name("lucide".into()),
                        ))
                        .color(context.theme.warning),
                );
                ui.label(RichText::new(text).small().color(context.theme.warning));
            });
        });
    ui.add_space(2.0);
}

fn draw_result_selector(
    context: &QueryResultsSurfaceContext<'_>,
    ui: &mut egui::Ui,
    action: &mut Option<QueryResultsSurfaceAction>,
) {
    if context.result_count <= 1 {
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);
        for index in 0..context.result_count {
            let is_active = context.active_result_index == index;
            let is_pinned = context.pinned_results.contains(&index);
            let default_name = format!("Result {}", index + 1);
            let name = context.result_names.get(&index).unwrap_or(&default_name);
            let label = if is_pinned {
                format!("📌 {name}")
            } else {
                name.clone()
            };

            let btn = Button::new(context.theme)
                .text(label)
                .variant(if is_active { ButtonVariant::Secondary } else { ButtonVariant::Ghost })
                .size(ButtonSize::Sm)
                .show(ui);

            if btn.clicked() {
                *action = Some(QueryResultsSurfaceAction::SelectResult(index));
            }

            btn.context_menu(|ui| {
                if Button::new(context.theme)
                    .text(if is_pinned { "Unpin Tab" } else { "Pin Tab" })
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .full_width(true)
                    .left_aligned()
                    .show(ui)
                    .clicked()
                {
                    *action = Some(QueryResultsSurfaceAction::TogglePin(index));
                    ui.close();
                }
                if Button::new(context.theme)
                    .text(t!("common.close"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .full_width(true)
                    .left_aligned()
                    .show(ui)
                    .clicked()
                {
                    *action = Some(QueryResultsSurfaceAction::CloseResult(index));
                    ui.close();
                }
                if context.result_count > 1
                    && Button::new(context.theme)
                        .text("Close Other Tabs")
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .full_width(true)
                        .left_aligned()
                        .show(ui)
                        .clicked()
                {
                    *action = Some(QueryResultsSurfaceAction::CloseOtherResults(index));
                    ui.close();
                }
            });
        }
    });
}
