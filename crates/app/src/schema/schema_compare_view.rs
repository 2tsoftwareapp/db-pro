use super::*;
use egui::{CornerRadius, Pos2, Rect, Vec2};

pub(super) struct SchemaCompareViewContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) compare: &'a mut SchemaCompareState,
    pub(super) schema: &'a UiSchemaSummary,
    pub(super) connection_name: &'a str,
    pub(super) active_schema: &'a str,
    pub(super) driver: &'a str,
    pub(super) feedback: &'a mut FeedbackState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchemaCompareAction {
    OpenWorkspace,
    /// Diff was requested; the handler force-refreshes the schema first so the
    /// diff runs against live introspection, not the cached summary.
    RefreshThenDiff,
    RequestDataDiff,
    ApplyMigration,
}

pub(super) fn draw_schema_compare_sidebar(
    context: &mut SchemaCompareViewContext<'_>,
    ui: &mut egui::Ui,
) -> Option<SchemaCompareAction> {
    ui.label(
        RichText::new("SCHEMA COMPARE")
            .font(font_caption())
            .strong()
            .color(context.theme.text_secondary),
    );
    ui.add_space(6.0);
    ui.label(
        RichText::new("Snapshot the loaded schema, then re-introspect and diff.")
            .small()
            .color(context.theme.text_muted),
    );
    ui.add_space(8.0);
    if Button::new(context.theme)
        .icon(Icon::Camera)
        .text(t!("compare.take_snapshot"))
        .variant(ButtonVariant::Secondary)
        .size(ButtonSize::Sm)
        .show(ui)
        .clicked()
    {
        context
            .compare
            .take_snapshot(context.schema, context.connection_name, context.feedback);
    }
    let mut action = None;
    if Button::new(context.theme)
        .icon(Icon::GitCompare)
        .text(t!("compare.diff_vs_snapshot"))
        .variant(ButtonVariant::Secondary)
        .size(ButtonSize::Sm)
        .show(ui)
        .clicked()
    {
        if context.compare.schema_snapshot.is_some() {
            context.compare.pending_diff_after_refresh = true;
            action = Some(SchemaCompareAction::RefreshThenDiff);
        } else {
            context.compare.diff_against_snapshot(context.schema, context.feedback);
            action = Some(SchemaCompareAction::OpenWorkspace);
        }
    }
    ui.add_space(8.0);
    if let Some(snap) = &context.compare.schema_snapshot {
        ui.label(
            RichText::new(format!("Snapshot: {}", snap.label))
                .small()
                .color(context.theme.text_secondary),
        );
        ui.label(
            RichText::new(format!("{} tables", snap.tables.len()))
                .small()
                .color(context.theme.text_muted),
        );
    } else {
        ui.label(
            RichText::new("No snapshot yet.")
                .small()
                .color(context.theme.text_muted),
        );
    }
    action
}

pub(super) fn draw_schema_compare(
    context: &mut SchemaCompareViewContext<'_>,
    ui: &mut egui::Ui,
) -> Option<SchemaCompareAction> {
    // The shell body runs flush to the window edge; give this page its own
    // gutter so the header buttons and form fields do not touch it.
    let mut action = None;
    ui.horizontal(|ui| {
        ui.add_space(SPACE_LG);
        ui.vertical(|ui| {
            ui.set_max_width((ui.available_width() - SPACE_LG).max(0.0));
            action = draw_schema_compare_body(context, ui);
        });
    });
    action
}

fn draw_schema_compare_body(
    context: &mut SchemaCompareViewContext<'_>,
    ui: &mut egui::Ui,
) -> Option<SchemaCompareAction> {
    ui.set_min_width(ui.available_width());
    ui.add_space(SPACE_SM);
    let mut action = None;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(t!("compare.title"))
                .font(font_subheading())
                .strong()
                .color(context.theme.text_primary),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // First allocation lands flush at the window edge — inset it.
            ui.add_space(SPACE_MD);
            ui.spacing_mut().item_spacing.x = SPACE_XS;
            if Button::new(context.theme)
                .icon(Icon::GitCompare)
                .text(t!("compare.diff_now"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                if context.compare.schema_snapshot.is_some() {
                    context.compare.pending_diff_after_refresh = true;
                    action = Some(SchemaCompareAction::RefreshThenDiff);
                } else {
                    context.compare.diff_against_snapshot(context.schema, context.feedback);
                }
            }
            if Button::new(context.theme)
                .icon(Icon::Camera)
                .text(t!("compare.snapshot"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                context
                    .compare
                    .take_snapshot(context.schema, context.connection_name, context.feedback);
            }
        });
    });
    ui.add_space(SPACE_MD);
    let Some(diff) = context.compare.schema_diff.clone() else {
        Card::new(context.theme).show(ui, |ui| {
            ui.set_min_width((ui.available_width() - 8.0).max(0.0));
            EmptyState::new(
                Icon::GitCompare,
                &t!("compare.no_diff_yet"),
                &t!("compare.no_diff_desc"),
                context.theme,
            )
            .show(ui);
        });
        return action;
    };
    ScrollArea::new(context.theme).show(ui, |ui| {
        // Summary header chips
        let total_diffs = diff.tables_only_in_source.len()
            + diff.tables_only_in_target.len()
            + diff.views_only_in_source.len()
            + diff.views_only_in_target.len()
            + diff.functions_only_in_source.len()
            + diff.functions_only_in_target.len()
            + diff.column_changes.len();

        ui.horizontal(|ui| {
            let added = diff.tables_only_in_target.len()
                + diff.views_only_in_target.len()
                + diff.functions_only_in_target.len();
            let removed = diff.tables_only_in_source.len()
                + diff.views_only_in_source.len()
                + diff.functions_only_in_source.len();
            let changed = diff.column_changes.len();
            ui.label(
                RichText::new(format!("{total_diffs} total difference(s)"))
                    .small()
                    .strong()
                    .color(if total_diffs > 0 {
                        context.theme.accent
                    } else {
                        context.theme.success
                    }),
            );
            ui.label(
                RichText::new(format!("{added} Added"))
                    .small()
                    .color(context.theme.success),
            );
            ui.label(
                RichText::new(format!("{changed} Changed"))
                    .small()
                    .color(context.theme.warning),
            );
            ui.label(
                RichText::new(format!("{removed} Removed"))
                    .small()
                    .color(context.theme.danger),
            );
        });
        ui.add_space(8.0);

        for (title, items, is_added, is_removed) in [
            (
                "Tables only in snapshot (Removed in current)",
                &diff.tables_only_in_source,
                false,
                true,
            ),
            (
                "Tables only in current (Added in current)",
                &diff.tables_only_in_target,
                true,
                false,
            ),
            ("Views only in snapshot", &diff.views_only_in_source, false, true),
            ("Views only in current", &diff.views_only_in_target, true, false),
            ("Routines only in snapshot", &diff.functions_only_in_source, false, true),
            ("Routines only in current", &diff.functions_only_in_target, true, false),
            ("Column / type changes", &diff.column_changes, false, false),
        ] {
            if items.is_empty() {
                continue;
            }
            Card::new(context.theme).frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = SPACE_XS;
                    let badge_text = if is_added {
                        format!("+{}", items.len())
                    } else if is_removed {
                        format!("-{}", items.len())
                    } else {
                        format!("~{}", items.len())
                    };
                    let badge_color = if is_added {
                        context.theme.success
                    } else if is_removed {
                        context.theme.danger
                    } else {
                        context.theme.warning
                    };
                    ui.label(RichText::new(badge_text).small().strong().color(badge_color));
                    ui.label(RichText::new(title).strong().color(context.theme.text_primary));
                });
                ui.add_space(4.0);
                for item in items {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = SPACE_XS;
                        ui.label(RichText::new("•").color(context.theme.text_muted));
                        ui.label(
                            RichText::new(item)
                                .small()
                                .monospace()
                                .color(context.theme.text_secondary),
                        );
                    });
                }
            });
            ui.add_space(6.0);
        }

        if total_diffs == 0 {
            ui.label(RichText::new("Schemas are identical. No drift detected.").color(context.theme.success));
            ui.add_space(8.0);
        }

        Separator::horizontal(context.theme).show(ui);
        ui.add_space(8.0);
        section_label(ui, "MIGRATION PLAN", context.theme);
        let current_target = format!("{} · {}", context.connection_name, context.active_schema);
        if Button::new(context.theme)
            .icon(Icon::FileCode2)
            .text("Generate migration plan")
            .show(ui)
            .clicked()
        {
            context
                .compare
                .plan_migration(context.driver, &current_target, context.feedback);
        }
        if let Some(plan_target) = context.compare.migration_plan_target.clone() {
            if plan_target != current_target {
                Card::new(context.theme).frame().show(ui, |ui| {
                    ui.label(
                        RichText::new("SAFETY LOCK — plan target mismatch")
                            .small()
                            .strong()
                            .color(context.theme.danger),
                    );
                    ui.label(
                        RichText::new(format!("Recorded plan target: {plan_target}"))
                            .small()
                            .color(context.theme.text_secondary),
                    );
                    ui.label(
                        RichText::new(format!("Active session target: {current_target}"))
                            .small()
                            .color(context.theme.text_secondary),
                    );
                    ui.label(
                        // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                        RichText::new(
                            "Plan cannot execute against a divergent target. Re-plan or switch the active session.",
                        )
                        .small()
                        .color(context.theme.warning),
                    );
                });
            }
        }
        if let Some(plan) = &context.compare.migration_plan {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "{} operation(s) · fingerprint {}",
                    plan.operations.len(),
                    plan.fingerprint,
                ))
                .small()
                .monospace()
                .color(context.theme.text_secondary),
            );
            for warning in &plan.warnings {
                ui.label(
                    RichText::new(format!("⚠ {warning}"))
                        .small()
                        .color(context.theme.warning),
                );
            }
            ui.add_space(4.0);
            for op in &plan.operations {
                let (risk_label, risk_color, risk_bg) = match op.risk {
                    db_pro_core::domain::migration::MigrationRisk::Destructive => {
                        ("DESTRUCTIVE", context.theme.danger, context.theme.danger_soft())
                    }
                    db_pro_core::domain::migration::MigrationRisk::Mutating => {
                        ("MUTATING", context.theme.warning, context.theme.warning_soft())
                    }
                    db_pro_core::domain::migration::MigrationRisk::Safe => {
                        ("SAFE", context.theme.success, context.theme.success_soft())
                    }
                };
                ui.horizontal(|ui| {
                    let galley = ui
                        .painter()
                        .layout_no_wrap(risk_label.to_owned(), font_caption(), risk_color);
                    // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                    let r = Rect::from_min_size(
                        Pos2::new(ui.cursor().min.x, ui.cursor().min.y + 1.0),
                        Vec2::new(galley.size().x + 6.0, 15.0),
                    );
                    ui.painter()
                        .rect_filled(r, CornerRadius::same(RADIUS_XS as u8), risk_bg);
                    ui.painter().galley(
                        Pos2::new(r.left() + 3.0, r.top() + 1.0),
                        galley,
                        egui::Color32::PLACEHOLDER,
                    );
                    ui.add_space(r.width() + 4.0);
                    ui.label(
                        RichText::new(format!("{} · {}", op.id, op.sql))
                            .small()
                            .monospace()
                            .color(context.theme.text_secondary),
                    );
                });
            }
        }
        if !context.compare.migration_preview_sql.is_empty() {
            ui.add_space(8.0);
            Card::new(context.theme).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Generated Migration DDL")
                            .strong()
                            .color(context.theme.text_primary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if Button::new(context.theme)
                            .icon(Icon::Copy)
                            .text("Copy SQL")
                            .variant(ButtonVariant::Secondary)
                            .size(ButtonSize::Sm)
                            .show(ui)
                            .clicked()
                        {
                            let sql = context.compare.migration_preview_sql.clone();
                            ui.output_mut(|o| o.commands.push(egui::OutputCommand::CopyText(sql)));
                            context
                                .feedback
                                .set_runtime_message(&t!("compare.copy_migration_sql"));
                        }
                    });
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new(&context.compare.migration_preview_sql)
                        .small()
                        .monospace()
                        .color(context.theme.text_primary),
                );
            });
            ui.add_space(6.0);
            if context
                .compare
                .migration_plan
                .as_ref()
                .is_some_and(|plan| plan.has_destructive)
            {
                Checkbox::new(
                    &mut context.compare.migration_confirm_destructive,
                    "Confirm destructive operations (never auto-applied)",
                    context.theme,
                )
                .show(ui);
            }
            let target_mismatch = context
                .compare
                .migration_plan_target
                .as_deref()
                .is_some_and(|target| target != current_target.as_str());
            if ui
                .add_enabled_ui(!target_mismatch, |ui| {
                    Button::new(context.theme)
                        .icon(Icon::Play)
                        .text("Apply migration SQL")
                        .variant(ButtonVariant::Default)
                        .size(ButtonSize::Sm)
                        .show(ui)
                })
                .inner
                .clicked()
            {
                action = Some(SchemaCompareAction::ApplyMigration);
            }
        }
        Separator::horizontal(context.theme).show(ui);
        ui.add_space(8.0);
        ui.label(
            RichText::new(t!("compare.data_compare"))
                .font(font_caption())
                .strong()
                .color(context.theme.text_secondary),
        );
        ui.label(
            RichText::new("Key-aware sample compare across two active connections. Sync SQL is preview-only.")
                .small()
                .color(context.theme.text_muted),
        );
        ui.add_space(SPACE_XS);
        Input::new(
            &mut context.compare.data_diff_target_id,
            "e.g. the connection to compare against",
            context.theme,
        )
        .label("Target connection id")
        .show(ui);
        // Labeled inputs need more air than the shell's row rhythm.
        ui.add_space(SPACE_SM);
        Input::new(&mut context.compare.data_diff_schema, "public", context.theme)
            .label("Schema")
            .show(ui);
        ui.add_space(SPACE_SM);
        Input::new(&mut context.compare.data_diff_table, "table name", context.theme)
            .label("Table")
            .show(ui);
        ui.add_space(SPACE_SM);
        Input::new(
            &mut context.compare.data_diff_keys,
            "comma-separated, e.g. id",
            context.theme,
        )
        .label("Key columns")
        .show(ui);
        ui.add_space(SPACE_XS);
        // Bottom margin so the pinned results panel never overlays the button.
        ui.add_space(SPACE_LG);
        if Button::new(context.theme)
            .icon(Icon::GitCompare)
            .text(t!("compare.compare_rows"))
            .variant(ButtonVariant::Default)
            .size(ButtonSize::Sm)
            .show(ui)
            .clicked()
        {
            action = Some(SchemaCompareAction::RequestDataDiff);
        }
        if let Some(diff) = &context.compare.data_diff_result {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "counts src={} tgt={} · +{} -{} ~{} ={} · truncated={}",
                    diff.source_row_count,
                    diff.target_row_count,
                    diff.added,
                    diff.removed,
                    diff.changed,
                    diff.equal,
                    diff.truncated
                ))
                .small()
                .monospace()
                .color(context.theme.text_secondary),
            );
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = SPACE_XS;
                for label in ["all", "added", "removed", "changed"] {
                    if Button::new(context.theme)
                        .text(label)
                        .variant(if context.compare.data_diff_filter == label {
                            ButtonVariant::Secondary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .size(ButtonSize::Sm)
                        .show(ui)
                        .clicked()
                    {
                        context.compare.data_diff_filter = label.to_owned();
                    }
                }
            });
            if let Some(window) = db_pro_core::application::describe_sample_window(diff.sample_limit, diff.truncated) {
                ui.label(RichText::new(window).small().color(context.theme.text_secondary));
            }
            for row in &diff.row_diffs {
                let include = match context.compare.data_diff_filter.as_str() {
                    "added" => row.state == db_pro_core::domain::cross_connection::DataRowState::Added,
                    "removed" => row.state == db_pro_core::domain::cross_connection::DataRowState::Removed,
                    "changed" => row.state == db_pro_core::domain::cross_connection::DataRowState::Changed,
                    _ => true,
                };
                if include {
                    let (state_text, state_color) = match row.state {
                        // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                        db_pro_core::domain::cross_connection::DataRowState::Added => {
                            ("+ ADDED", context.theme.success)
                        }
                        // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                        db_pro_core::domain::cross_connection::DataRowState::Removed => {
                            ("- REMOVED", context.theme.danger)
                        }
                        // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                        db_pro_core::domain::cross_connection::DataRowState::Changed => {
                            ("~ CHANGED", context.theme.warning)
                        }
                        // cc-scan:allow LINE_TOO_LONG — literal must not wrap
                        db_pro_core::domain::cross_connection::DataRowState::Equal => {
                            ("= EQUAL", context.theme.text_muted)
                        }
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(state_text).small().strong().color(state_color));
                        ui.label(
                            RichText::new(format!("key: {}", row.key.replace('\u{1f}', " | ")))
                                .small()
                                .monospace()
                                .color(context.theme.text_secondary),
                        );
                    });
                }
            }
            if !diff.sync_sql_preview.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("DATA SYNC SQL PREVIEW")
                        .font(font_caption())
                        .strong()
                        .color(context.theme.text_secondary),
                );
                for sql in &diff.sync_sql_preview {
                    ui.label(RichText::new(sql).small().monospace().color(context.theme.text_muted));
                }
            }
        }
    });
    action
}
