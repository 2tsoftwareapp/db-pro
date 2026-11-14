//! Small workspace-files tabs with explicit presentation inputs.
use super::super::ide_workspace::{DependencyEdge, MigrationEntry};
use super::super::*;
use egui::RichText;
use lucide_icons::Icon;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FilesSecondaryTabAction {
    OpenFile(String),
}

pub(super) fn draw_migrations(
    ui: &mut egui::Ui,
    theme: DbProTheme,
    migrations: &[MigrationEntry],
) -> Vec<FilesSecondaryTabAction> {
    if migrations.is_empty() {
        ui.label(
            RichText::new("No migration SQL detected under migrations/ paths.")
                .small()
                .color(theme.text_muted),
        );
        return Vec::new();
    }
    let mut actions = Vec::new();
    for entry in migrations {
        let label = format!("{} · {}", entry.version, entry.status.label());
        if sidebar_item(ui, Icon::FileCode2, &label, false, theme)
            .on_hover_text(&entry.relative_path)
            .clicked()
        {
            actions.push(FilesSecondaryTabAction::OpenFile(entry.relative_path.clone()));
        }
    }
    actions
}

/// Maximum number of dependency rows painted by the Graph tab.
const GRAPH_MAX_ROWS: usize = 60;

pub(super) fn draw_graph(
    ui: &mut egui::Ui,
    theme: DbProTheme,
    edges: &[DependencyEdge],
) -> Vec<FilesSecondaryTabAction> {
    if edges.is_empty() {
        ui.label(
            RichText::new("No FROM/JOIN object references found yet.")
                .small()
                .color(theme.text_muted),
        );
        return Vec::new();
    }
    for edge in edges.iter().take(GRAPH_MAX_ROWS) {
        // #578: each row opens the referencing file. `from_file` is
        // `"<root-id>::<relative-path>"`, so strip the root prefix.
        let relative_path = edge
            .from_file
            .split_once("::")
            .map_or(edge.from_file.as_str(), |(_, path)| path);
        let label = ui.add(
            egui::Label::new(
                RichText::new(format!("{} → {}", edge.from_file, edge.object_name))
                    .small()
                    .monospace()
                    .color(theme.text_secondary),
            )
            .sense(egui::Sense::click()),
        );
        if label.clicked() {
            return vec![FilesSecondaryTabAction::OpenFile(relative_path.to_owned())];
        }
    }
    if edges.len() > GRAPH_MAX_ROWS {
        ui.label(
            RichText::new(format_shown_total(GRAPH_MAX_ROWS, edges.len()))
                .small()
                .color(theme.text_muted),
        );
    }
    Vec::new()
}
