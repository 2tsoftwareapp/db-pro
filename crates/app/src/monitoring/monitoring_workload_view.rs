//! Monitoring workload presentation and user intents.

use super::*;

/// Statement rows painted per frame (#569).
const WORKLOAD_STATEMENT_CAP: usize = 40;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum MonitoringWorkloadAction {
    SortChanged(db_pro_core::domain::monitoring::StatStatementSort),
    ResetStatistics,
    OpenSql(String),
}

pub(super) struct MonitoringWorkloadContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) workload: Option<&'a db_pro_core::domain::monitoring::StatStatementsSnapshot>,
    pub(super) previous: Option<&'a db_pro_core::domain::monitoring::StatStatementsSnapshot>,
    pub(super) sort: db_pro_core::domain::monitoring::StatStatementSort,
    pub(super) filter: &'a mut String,
}

impl MonitoringWorkloadContext<'_> {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui) -> Vec<MonitoringWorkloadAction> {
        let Some(workload) = self.workload else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        ui.add_space(SPACE_MD);
        section_label(ui, "TOP QUERIES (pg_stat_statements)", self.theme);
        ui.add_space(SPACE_SM);
        if !workload.extension_present {
            ui.colored_label(self.theme.warning, &workload.message);
            return actions;
        }
        self.draw_summary(ui, workload);
        actions.extend(self.draw_toolbar(ui));
        let rows = self.capped_statements(ui, workload);
        let mut row_count = 0;
        let previous_by_id = previous_totals(self.previous);
        for statement in rows {
            row_count += 1;
            self.draw_statement(ui, statement, &previous_by_id, &mut actions);
            ui.add_space(SPACE_XS);
        }
        if row_count == 0 {
            ui.label(
                RichText::new("No statements match the current filter.")
                    .small()
                    .color(self.theme.text_muted),
            );
        }
        actions
    }

    /// Statements matching the filter, capped, with the "showing N of M" hint (#569).
    fn capped_statements<'w>(
        &self,
        ui: &mut egui::Ui,
        workload: &'w db_pro_core::domain::monitoring::StatStatementsSnapshot,
    ) -> Vec<&'w db_pro_core::domain::monitoring::StatStatement> {
        let filter = self.filter.to_ascii_lowercase();
        let matching: Vec<_> = workload
            .statements
            .iter()
            .filter(|statement| matches_filter(statement, &filter))
            .collect();
        if let Some(hint) = shown_total_hint(WORKLOAD_STATEMENT_CAP, matching.len()) {
            ui.label(RichText::new(hint).small().color(self.theme.text_muted));
        }
        matching.into_iter().take(WORKLOAD_STATEMENT_CAP).collect()
    }

    fn draw_summary(&self, ui: &mut egui::Ui, workload: &db_pro_core::domain::monitoring::StatStatementsSnapshot) {
        if let Some(version) = &workload.extension_version {
            ui.label(
                RichText::new(format!("extension v{version} · {}", workload.message))
                    .small()
                    .color(self.theme.text_muted),
            );
        }
    }

    fn draw_toolbar(&mut self, ui: &mut egui::Ui) -> Vec<MonitoringWorkloadAction> {
        let mut actions = Vec::new();
        ui.horizontal_wrapped(|ui| {
            use db_pro_core::domain::monitoring::StatStatementSort;
            for sort in [
                StatStatementSort::TotalTime,
                StatStatementSort::MeanTime,
                StatStatementSort::Calls,
                StatStatementSort::Rows,
            ] {
                if Button::new(self.theme)
                    .text(sort.as_label())
                    .variant(if self.sort == sort {
                        ButtonVariant::Secondary
                    } else {
                        ButtonVariant::Ghost
                    })
                    .size(ButtonSize::Sm)
                    .show(ui)
                    .clicked()
                {
                    actions.push(MonitoringWorkloadAction::SortChanged(sort));
                }
            }
            if Button::new(self.theme)
                .text("Reset stats…")
                .variant(ButtonVariant::Destructive)
                .show(ui)
                .clicked()
            {
                actions.push(MonitoringWorkloadAction::ResetStatistics);
            }
        });
        ui.add_space(SPACE_XS);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Filter").small().color(self.theme.text_muted));
            crate::components::input::SearchInput::new(self.filter, "Query text…", self.theme)
                .width(260.0)
                .show(ui);
        });
        actions
    }

    fn draw_statement(
        &self,
        ui: &mut egui::Ui,
        statement: &db_pro_core::domain::monitoring::StatStatement,
        previous_by_id: &std::collections::HashMap<Option<i64>, f64>,
        actions: &mut Vec<MonitoringWorkloadAction>,
    ) {
        panel_frame(self.theme)
            .inner_margin(egui::Margin::symmetric(SPACE_SM as i8, SPACE_XS as i8))
            .show(ui, |ui| {
                let delta = previous_by_id
                    .get(&statement.queryid)
                    .map(|previous| statement.total_time_ms - previous)
                    .filter(|delta| delta.abs() > 0.01);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "calls {} · total {:.1} ms · mean {:.1} ms · rows {}",
                            statement.calls, statement.total_time_ms, statement.mean_time_ms, statement.rows
                        ))
                        .small()
                        .strong()
                        .color(self.theme.text_primary),
                    );
                    if let Some(delta) = delta {
                        ui.label(
                            RichText::new(format!("Δ total {delta:+.1} ms"))
                                .small()
                                .color(self.theme.accent),
                        );
                    }
                });
                ui.label(
                    RichText::new(format!(
                        "{} · {} · shared hit/read {}/{} · temp r/w {}/{}",
                        statement.username.as_deref().unwrap_or("?"),
                        statement.database.as_deref().unwrap_or("?"),
                        statement.shared_blks_hit,
                        statement.shared_blks_read,
                        statement.temp_blks_read,
                        statement.temp_blks_written
                    ))
                    .small()
                    .color(self.theme.text_muted),
                );
                let short = if statement.query.len() > 160 {
                    format!("{}…", statement.query.chars().take(159).collect::<String>())
                } else {
                    statement.query.clone()
                };
                ui.label(RichText::new(short).monospace().small().color(self.theme.text_primary));
                if Button::new(self.theme)
                    .icon(Icon::FileCode2)
                    .text("Open SQL")
                    .variant(ButtonVariant::Ghost)
                    .show(ui)
                    .clicked()
                {
                    actions.push(MonitoringWorkloadAction::OpenSql(statement.query.clone()));
                }
            });
    }
}

fn matches_filter(statement: &db_pro_core::domain::monitoring::StatStatement, filter: &str) -> bool {
    filter.is_empty()
        || statement.query.to_ascii_lowercase().contains(filter)
        || statement
            .database
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase()
            .contains(filter)
        || statement
            .username
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase()
            .contains(filter)
}

fn previous_totals(
    previous: Option<&db_pro_core::domain::monitoring::StatStatementsSnapshot>,
) -> std::collections::HashMap<Option<i64>, f64> {
    previous
        .map(|snapshot| {
            snapshot
                .statements
                .iter()
                .map(|statement| (statement.queryid, statement.total_time_ms))
                .collect()
        })
        .unwrap_or_default()
}
