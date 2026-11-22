// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
//! Query SQL editor surface and floating completion popup.
use super::*;
use crate::editor::{CompletionItem, SqlDialect};
use crate::query::{HoverColumn, RichHoverHelp, SqlSignatureHelp, SqlSymbolHelp};
use egui::RichText;

fn draw_symbol_help(ui: &mut egui::Ui, help: &SqlSymbolHelp, theme: &DbProTheme) {
    ui.label(
        RichText::new(&help.title)
            .font(FontId::monospace(12.5))
            .strong()
            .color(theme.text_primary),
    );
    ui.horizontal(|ui| {
        ui.label(RichText::new(&help.kind).small().strong().color(theme.accent));
        ui.label(RichText::new(&help.detail).small().color(theme.text_secondary));
    });
    ui.add(egui::Label::new(RichText::new(&help.documentation).small().color(theme.text_muted)).wrap());
}

/// Render a column summary row inside a table hover card.
fn draw_hover_column_row(ui: &mut egui::Ui, col: &HoverColumn, theme: &DbProTheme) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);
        // PK badge
        if col.is_primary_key {
            Badge::new("PK", *theme)
                .tint(theme.warning)
                .font(FontId::monospace(8.5))
                .show(ui);
        } else if col.is_foreign_key {
            Badge::new("FK", *theme)
                .tint(theme.info)
                .font(FontId::monospace(8.5))
                .show(ui);
        } else {
            ui.add_space(24.0);
        }
        // Column name
        ui.label(
            RichText::new(&col.name)
                .font(FontId::monospace(12.0))
                .color(theme.text_primary),
        );
        // Type
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let null_text = if col.nullable { "null" } else { "not null" };
            ui.label(
                RichText::new(null_text)
                    .font(FontId::monospace(10.0))
                    .color(theme.text_muted),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(&col.data_type)
                    .font(FontId::monospace(10.5))
                    .color(theme.code_type),
            );
        });
    });
}

// The hover popup is not scrollable: the pointer leaving the editor token closes it, so a
// scroll region inside can never be interacted with. Cap the rows instead so the card stays
// within a sane height budget, and — critically — so `content_ui.min_size()` always reports
// the full content height to the enclosing `Area`. A `ScrollArea` reports only the visible
// viewport height; once the Area stores a short size the clip locks and rows disappear.
const MAX_HOVER_COLUMN_ROWS: usize = 10;
const MAX_HOVER_FK_ROWS: usize = 3;
const HOVER_POPUP_MAX_HEIGHT: f32 = 400.0;

/// Draw the rich hover popup for any `RichHoverHelp` variant.
pub(super) fn draw_rich_hover_popup(
    ctx: &egui::Context,
    anchor: egui::Rect,
    token_range: (usize, usize),
    help: &RichHoverHelp,
    theme: &DbProTheme,
) {
    let popup_width: f32 = match help {
        RichHoverHelp::Table { columns, .. } => {
            if columns.len() > 8 {
                520.0
            } else {
                420.0
            }
        }
        RichHoverHelp::Keyword { example: Some(_), .. } => 500.0,
        _ => 380.0,
    };
    let position = crate::components::clamp_popup_to_screen(
        anchor.left_bottom() + egui::vec2(0.0, 6.0),
        egui::vec2(popup_width, HOVER_POPUP_MAX_HEIGHT),
        ctx.content_rect(),
        10.0,
    );

    FloatingPopup::new(egui::Id::new(("sql_rich_hover", token_range)), *theme, position)
        .order(egui::Order::Tooltip)
        .interactable(true) // Allow text selection / copy
        .surface_elevated()
        .radius(8.0)
        .margin(egui::Margin::same(10.0 as i8))
        .show(ctx, |ui| {
            ui.set_max_width(popup_width);
            draw_rich_hover_content(ui, help, theme);
        });
}

/// Inner content renderer (called inside the scrollable area).
fn draw_rich_hover_content(ui: &mut egui::Ui, help: &RichHoverHelp, theme: &DbProTheme) {
    match help {
        RichHoverHelp::Table {
            qualified_name,
            kind,
            row_count,
            columns,
            foreign_keys,
        } => {
            // Header
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);
                let (badge_text, badge_color) = if kind == "View" {
                    ("VIEW", theme.info)
                } else {
                    ("TABLE", theme.accent)
                };
                Badge::new(badge_text, *theme)
                    .tint(badge_color)
                    .font(FontId::monospace(9.0))
                    .show(ui);
                ui.label(
                    RichText::new(qualified_name)
                        .font(FontId::monospace(13.0))
                        .strong()
                        .color(theme.text_primary),
                );
            });

            if let Some(rows) = row_count {
                ui.label(
                    RichText::new(format!("~{rows} rows"))
                        .font(FontId::proportional(11.0))
                        .color(theme.text_muted),
                );
            }

            if !columns.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("{} columns", columns.len()))
                        .small()
                        .strong()
                        .color(theme.text_muted),
                );
                ui.add_space(2.0);
                Separator::horizontal(*theme).show(ui);
                for col in columns.iter().take(MAX_HOVER_COLUMN_ROWS) {
                    draw_hover_column_row(ui, col, theme);
                }
                if columns.len() > MAX_HOVER_COLUMN_ROWS {
                    ui.label(
                        RichText::new(format!(
                            "… and {} more columns",
                            columns.len() - MAX_HOVER_COLUMN_ROWS
                        ))
                        .small()
                        .color(theme.text_muted),
                    );
                }
            }

            if !foreign_keys.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new("Foreign keys").small().strong().color(theme.text_muted));
                ui.add_space(2.0);
                for fk in foreign_keys.iter().take(MAX_HOVER_FK_ROWS) {
                    ui.label(
                        RichText::new(format!(
                            "({}) → {}({})",
                            fk.from_columns.join(", "),
                            fk.to_table,
                            fk.to_columns.join(", "),
                        ))
                        .font(FontId::monospace(11.0))
                        .color(theme.text_secondary),
                    );
                }
                if foreign_keys.len() > MAX_HOVER_FK_ROWS {
                    ui.label(
                        RichText::new(format!("… and {} more", foreign_keys.len() - MAX_HOVER_FK_ROWS))
                            .small()
                            .color(theme.text_muted),
                    );
                }
            }
        }

        RichHoverHelp::Column {
            qualified_name,
            data_type,
            nullable,
            is_primary_key,
            parent_table,
            foreign_key_target,
        } => {
            // Type badge + name
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);
                Badge::new("COL", *theme)
                    .tint(theme.code_type)
                    .font(FontId::monospace(9.0))
                    .show(ui);
                ui.label(
                    RichText::new(qualified_name)
                        .font(FontId::monospace(13.0))
                        .strong()
                        .color(theme.text_primary),
                );
            });

            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                // Type chip
                Badge::new(data_type.as_str(), *theme)
                    .tint(theme.code_type)
                    .font(FontId::monospace(10.5))
                    .show(ui);
                // Null chip
                let null_color = if *nullable { theme.text_muted } else { theme.warning };
                let null_text = if *nullable { "nullable" } else { "not null" };
                Badge::new(null_text, *theme)
                    .tint(null_color)
                    .font(FontId::monospace(10.0))
                    .show(ui);
                // PK chip
                if *is_primary_key {
                    Badge::new("primary key", *theme)
                        .tint(theme.warning)
                        .font(FontId::monospace(10.0))
                        .show(ui);
                }
            });

            ui.add_space(4.0);
            ui.label(
                RichText::new(format!("Column of {parent_table}"))
                    .small()
                    .color(theme.text_muted),
            );

            if let Some(target) = foreign_key_target {
                ui.label(
                    RichText::new(format!("References {target}"))
                        .font(FontId::monospace(11.0))
                        .color(theme.info),
                );
            }
        }

        RichHoverHelp::Function {
            label,
            parameters,
            active_parameter,
            return_type,
            documentation,
        } => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);
                Badge::new("FN", *theme)
                    .tint(theme.code_function)
                    .font(FontId::monospace(9.0))
                    .show(ui);
                ui.label(
                    RichText::new(label)
                        .font(FontId::monospace(12.5))
                        .strong()
                        .color(theme.text_primary),
                );
            });
            if !parameters.is_empty() {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for (idx, param) in parameters.iter().enumerate() {
                        let color = if idx == *active_parameter {
                            theme.accent
                        } else {
                            theme.text_muted
                        };
                        ui.label(RichText::new(param).font(FontId::monospace(11.0)).color(color));
                        if idx + 1 < parameters.len() {
                            ui.label(RichText::new(",").font(FontId::monospace(11.0)).color(theme.text_muted));
                        }
                    }
                });
            }
            if !return_type.is_empty() {
                ui.label(
                    RichText::new(format!("→ {return_type}"))
                        .font(FontId::monospace(11.0))
                        .color(theme.code_type),
                );
            }
            if !documentation.is_empty() {
                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new(documentation).small().color(theme.text_muted)).wrap());
            }
        }

        RichHoverHelp::Keyword {
            keyword,
            dialect_note,
            documentation,
            example,
        } => {
            // Header row
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);
                Badge::new("SQL", *theme)
                    .tint(theme.code_keyword)
                    .font(FontId::monospace(9.0))
                    .show(ui);
                ui.label(
                    RichText::new(keyword.as_str())
                        .font(FontId::monospace(13.0))
                        .strong()
                        .color(theme.code_keyword),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(dialect_note.as_str()).small().color(theme.text_muted));
                });
            });

            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(documentation.as_str()).small().color(theme.text_primary)).wrap());

            if let Some(ex) = example {
                ui.add_space(6.0);
                ui.label(RichText::new("Example").small().strong().color(theme.text_muted));
                ui.add_space(2.0);
                Card::new(*theme).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::Label::new(
                            RichText::new(ex.as_str())
                                .font(FontId::monospace(11.0))
                                .color(theme.text_secondary),
                        )
                        .wrap(),
                    );
                });
            }
        }

        RichHoverHelp::Symbol(help) => {
            draw_symbol_help(ui, help, theme);
        }
    }
}

pub(super) fn draw_signature_help(
    ctx: &egui::Context,
    anchor: egui::Pos2,
    signature: &SqlSignatureHelp,
    theme: &DbProTheme,
) {
    const POPUP_WIDTH: f32 = 440.0;
    const POPUP_HEIGHT: f32 = 112.0;
    let position = crate::components::clamp_popup_to_screen(
        anchor + egui::vec2(8.0, 22.0),
        egui::vec2(POPUP_WIDTH, POPUP_HEIGHT),
        ctx.content_rect(),
        10.0,
    );
    FloatingPopup::new(egui::Id::new("sql_signature_help"), *theme, position)
        .order(egui::Order::Foreground)
        .interactable(false)
        .surface_elevated()
        .radius(7.0)
        .margin(egui::Margin::same(8.0 as i8))
        .show(ctx, |ui| {
            ui.set_width(POPUP_WIDTH);
            ui.label(
                RichText::new(&signature.label)
                    .font(FontId::monospace(12.0))
                    .color(theme.text_primary),
            );
            if !signature.parameters.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for (index, parameter) in signature.parameters.iter().enumerate() {
                        let color = if index == signature.active_parameter {
                            theme.accent
                        } else {
                            theme.text_muted
                        };
                        ui.label(RichText::new(parameter).font(FontId::monospace(11.0)).color(color));
                    }
                });
            }
            ui.label(
                RichText::new(&signature.documentation)
                    .small()
                    .color(theme.text_secondary),
            );
        });
}

/// Single shared close path for the SQL completion popup (#607).
///
/// Every close site — run dispatch, apply, the surface paint pass, and the
/// floating popup — routes through this one implementation so the close
/// semantics cannot drift between call sites. Callers that also need to
/// re-arm the auto-open timer do so explicitly after closing.
pub(super) fn close_completion(doc: &mut QueryDocument) {
    doc.completion.close();
}

pub(super) fn apply_completion_item(doc: &mut QueryDocument, item: &CompletionItem, dialect: SqlDialect) -> bool {
    if !doc.completion.can_apply_to_version(doc.buffer.version()) {
        close_completion(doc);
        return false;
    }

    let (start, end) = item.replacement_range;
    let source = doc.buffer.text();
    if start > end || end > source.len() || !source.is_char_boundary(start) || !source.is_char_boundary(end) {
        close_completion(doc);
        return false;
    }

    doc.buffer.replace(start, end, &item.insert_text);
    let cursor = start + item.insert_text.len();
    doc.cursor.set_offset(&doc.buffer, cursor);
    // Anchor AND active must land on the post-insert offset — collapse_to_active
    // alone keeps the stale pre-edit offset, so Shift+Arrow/Delete would operate
    // from the wrong position (#488).
    doc.selection = crate::editor::SelectionRange::point(cursor);
    doc.reanalyze(dialect);
    doc.dirty = true;
    doc.prediction = None;
    close_completion(doc);
    // The surface timer already opens completion once `completion_due_at` is due.
    if item.insert_text.ends_with('.') {
        doc.completion_due_at = Some(std::time::Instant::now());
    }
    true
}

/// Where a Ctrl/Cmd+click identifier should go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum IdentifierNavigation {
    Table(String),
    SchemaObject {
        selection: SchemaObjectSelection,
        schema: String,
        name: String,
        kind: String,
    },
    Missing(String),
}

pub(super) fn navigate_identifier(schema: &UiSchemaSummary, active_schema: &str, name: &str) -> IdentifierNavigation {
    if let Some(table) = find_named(
        &schema.table_details,
        active_schema,
        name,
        |table| &table.schema,
        |table| &table.name,
    ) {
        return IdentifierNavigation::Table(table.name.clone());
    }
    if let Some(table) = schema.table_details.iter().find(|table| {
        columns_match(table, name) && (active_schema.is_empty() || table.schema.eq_ignore_ascii_case(active_schema))
    }) {
        return IdentifierNavigation::Table(table.name.clone());
    }
    if let Some(table) = schema.table_details.iter().find(|table| columns_match(table, name)) {
        return IdentifierNavigation::Table(table.name.clone());
    }
    if let Some(view) = find_named(
        &schema.views,
        active_schema,
        name,
        |view| &view.schema,
        |view| &view.name,
    ) {
        return IdentifierNavigation::SchemaObject {
            selection: SchemaObjectSelection::View(view.name.clone()),
            schema: view.schema.clone(),
            name: view.name.clone(),
            kind: "view".to_owned(),
        };
    }
    if let Some(function) = find_named(
        &schema.functions,
        active_schema,
        name,
        |function| &function.schema,
        |function| &function.name,
    ) {
        return IdentifierNavigation::SchemaObject {
            selection: SchemaObjectSelection::Function {
                name: function.name.clone(),
                identity_arguments: function.identity_arguments.clone(),
            },
            schema: function.schema.clone(),
            name: function.name.clone(),
            kind: "function".to_owned(),
        };
    }
    if let Some(trigger) = find_named(
        &schema.triggers,
        active_schema,
        name,
        |trigger| &trigger.schema,
        |trigger| &trigger.name,
    ) {
        return IdentifierNavigation::SchemaObject {
            selection: SchemaObjectSelection::Trigger(trigger.name.clone()),
            schema: trigger.schema.clone(),
            name: trigger.name.clone(),
            kind: "trigger".to_owned(),
        };
    }
    IdentifierNavigation::Missing(format!("Open {name}"))
}

fn columns_match(table: &UiTableSummary, name: &str) -> bool {
    table
        .columns
        .iter()
        .any(|column| column.name.eq_ignore_ascii_case(name))
}

fn find_named<'a, T>(
    items: &'a [T],
    active_schema: &str,
    name: &str,
    schema_of: impl Fn(&T) -> &str,
    name_of: impl Fn(&T) -> &str,
) -> Option<&'a T> {
    if !active_schema.is_empty() {
        if let Some(item) = items.iter().find(|item| {
            name_of(item).eq_ignore_ascii_case(name) && schema_of(item).eq_ignore_ascii_case(active_schema)
        }) {
            return Some(item);
        }
    }
    items.iter().find(|item| name_of(item).eq_ignore_ascii_case(name))
}

#[cfg(test)]
#[path = "tests/query_editor_support_tests.rs"]
mod tests;

