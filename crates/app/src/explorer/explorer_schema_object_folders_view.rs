// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
//! Presentation and intent mapping for schema-object folders in the Explorer.

use super::explorer_navigation::SchemaObjectActivation;
use super::explorer_schema_object_row_view::{SchemaObjectRowAction, SchemaObjectRowContext};
use super::explorer_tree::{draw_category_folder, CategoryFolder};
use super::{
    context_action_menu, ctx_menu_item, DbProTheme, SchemaExplorerState, SchemaObjectSelection, UiFunctionSummary,
    UiTriggerSummary, UiViewSummary,
};
use crate::components::Separator;
use eframe::egui;
// #583: row height for viewport culling of the object folders.
use crate::app::EXPLORER_ROW_HEIGHT;
use lucide_icons::Icon;

pub(super) enum SchemaObjectFolderAction {
    Open(SchemaObjectActivation),
    OpenQuery(String),
    /// Open a dialect-correct row-limited `SELECT *` over the qualified FROM target;
    /// the snippet itself is built where the active driver is known (#260).
    OpenSelectTop(String),
    ModifyView(UiViewSummary),
    DropObject { schema: String, name: String, kind: String },
    ToggleTrigger { schema: String, table: String, name: String, enable: bool },
    CopyName(String),
    RefreshSchema,
}

pub(super) struct SchemaObjectFoldersView<'a> {
    theme: DbProTheme,
    explorer: &'a SchemaExplorerState,
    trigger_toggle_unavailable_reason: Option<String>,
}

impl<'a> SchemaObjectFoldersView<'a> {
    pub(super) fn new(
        theme: DbProTheme,
        explorer: &'a SchemaExplorerState,
        trigger_toggle_unavailable_reason: Option<String>,
    ) -> Self {
        Self {
            theme,
            explorer,
            trigger_toggle_unavailable_reason,
        }
    }

    /// Shared schema/search predicate — mirrors the old `filter_by_schema`
    /// semantics without materialising a `Vec` before the folder-open check.
    fn object_in_scope(&self, schema: &str, item_schema: &str, name: &str, search_query: &str) -> bool {
        let schema_matches = self.explorer.schema.schemas.is_empty() || schema.is_empty() || item_schema == schema;
        schema_matches && (search_query.is_empty() || self.explorer.explorer_filter.name_matches(name, search_query))
    }

    /// Draw one culled row list inside an open folder (#583).
    ///
    /// Views/Functions/Triggers used to paint *every* matching item each frame:
    /// a PostGIS `public` schema carries 1.000+ functions, so the folder cost
    /// O(n) layout + paint per frame. Like the Tables folder, rows outside the
    /// clip rect collapse into a single spacer of the same height, so scroll
    /// extent and hit-testing stay identical.
    fn draw_culled_rows<'b, T: 'b>(
        &self,
        ui: &mut egui::Ui,
        items: impl Iterator<Item = &'b T>,
        mut draw_row: impl FnMut(&mut egui::Ui, &T) -> Vec<SchemaObjectFolderAction>,
    ) -> Vec<SchemaObjectFolderAction> {
        let clip = ui.clip_rect();
        let mut actions = Vec::new();
        for item in items {
            let row_top = ui.cursor().min.y;
            if crate::app::explorer_row_is_offscreen(row_top, EXPLORER_ROW_HEIGHT, clip) {
                ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), EXPLORER_ROW_HEIGHT),
                    egui::Sense::hover(),
                );
                continue;
            }
            actions.extend(draw_row(ui, item));
        }
        actions
    }

    pub(super) fn draw_views(
        &self,
        ui: &mut egui::Ui,
        schema: &str,
        search_query: &str,
    ) -> Vec<SchemaObjectFolderAction> {
        // #583: filter once per frame; the count is the filtered length.
        let in_scope = |view: &&UiViewSummary| self.object_in_scope(schema, &view.schema, &view.name, search_query);
        let visible: Vec<&UiViewSummary> = self.explorer.schema.views.iter().filter(in_scope).collect();
        let count = visible.len();
        let mut actions = Vec::new();
        let theme = self.theme;
        let views_label = t!("explorer.views");
        let empty_hint = if search_query.is_empty() {
            t!("explorer.no_views")
        } else {
            t!("explorer.no_matching_views")
        };
        let folder_id = ui.make_persistent_id(("codex_views_folder", schema));
        let folder_response = draw_category_folder(
            ui,
            &theme,
            CategoryFolder {
                depth: 3,
                id: folder_id,
                icon: Icon::Eye,
                icon_color: theme.success,
                label: &views_label,
                count,
                empty_label: Some(empty_hint.as_str()),
            },
            |ui| {
                let rows = self.draw_culled_rows(ui, visible.iter().copied(), |ui, view| {
                    self.draw_view_row(ui, view)
                });
                actions.extend(rows);
            },
        );
        self.draw_folder_menu(ui, &folder_response, schema, &mut actions);
        actions
    }

    pub(super) fn draw_functions(
        &self,
        ui: &mut egui::Ui,
        schema: &str,
        search_query: &str,
    ) -> Vec<SchemaObjectFolderAction> {
        let in_scope = |function: &&UiFunctionSummary| {
            self.object_in_scope(schema, &function.schema, &function.name, search_query)
        };
        let visible: Vec<&UiFunctionSummary> =
            self.explorer.schema.functions.iter().filter(in_scope).collect();
        let count = visible.len();
        let mut actions = Vec::new();
        let theme = self.theme;
        let functions_label = t!("explorer.functions");
        let empty_hint = if search_query.is_empty() {
            t!("explorer.no_functions")
        } else {
            t!("explorer.no_matching_functions")
        };
        let folder_id = ui.make_persistent_id(("codex_functions_folder", schema));
        let folder_response = draw_category_folder(
            ui,
            &theme,
            CategoryFolder {
                depth: 3,
                id: folder_id,
                icon: Icon::Code2,
                icon_color: theme.code_type,
                label: &functions_label,
                count,
                empty_label: Some(empty_hint.as_str()),
            },
            |ui| {
                let rows = self.draw_culled_rows(ui, visible.iter().copied(), |ui, function| {
                    self.draw_function_row(ui, function)
                });
                actions.extend(rows);
            },
        );
        self.draw_folder_menu(ui, &folder_response, schema, &mut actions);
        actions
    }

    pub(super) fn draw_triggers(
        &self,
        ui: &mut egui::Ui,
        schema: &str,
        search_query: &str,
    ) -> Vec<SchemaObjectFolderAction> {
        let in_scope =
            |trigger: &&UiTriggerSummary| self.object_in_scope(schema, &trigger.schema, &trigger.name, search_query);
        let visible: Vec<&UiTriggerSummary> =
            self.explorer.schema.triggers.iter().filter(in_scope).collect();
        let count = visible.len();
        let mut actions = Vec::new();
        let theme = self.theme;
        let triggers_label = t!("explorer.triggers");
        let empty_hint = if search_query.is_empty() {
            t!("explorer.no_triggers")
        } else {
            t!("explorer.no_matching_triggers")
        };
        let folder_id = ui.make_persistent_id(("codex_triggers_folder", schema));
        let folder_response = draw_category_folder(
            ui,
            &theme,
            CategoryFolder {
                depth: 3,
                id: folder_id,
                icon: Icon::Zap,
                icon_color: theme.warning,
                label: &triggers_label,
                count,
                empty_label: Some(empty_hint.as_str()),
            },
            |ui| {
                let rows = self.draw_culled_rows(ui, visible.iter().copied(), |ui, trigger| {
                    self.draw_trigger_row(ui, trigger)
                });
                actions.extend(rows);
            },
        );
        self.draw_folder_menu(ui, &folder_response, schema, &mut actions);
        actions
    }

    /// Folder rows share one context menu: schema refresh plus copying the
    /// parent schema name. `context_action_menu` owns trigger detection.
    fn draw_folder_menu(
        &self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        schema: &str,
        actions: &mut Vec<SchemaObjectFolderAction>,
    ) {
        let theme = self.theme;
        let schema = schema.to_owned();
        context_action_menu(ui, response, theme, |ui, close_menu| {
            if ctx_menu_item(
                ui,
                Some(Icon::RefreshCw),
                "Refresh Schema",
                None,
                theme.text_primary,
                theme,
            )
            .clicked()
            {
                actions.push(SchemaObjectFolderAction::RefreshSchema);
                *close_menu = true;
            }
            Separator::horizontal(theme).show(ui);
            if ctx_menu_item(
                ui,
                Some(Icon::Copy),
                "Copy Schema Name",
                None,
                theme.text_primary,
                theme,
            )
            .clicked()
            {
                actions.push(SchemaObjectFolderAction::CopyName(schema));
                *close_menu = true;
            }
        });
    }

    fn draw_view_row(&self, ui: &mut egui::Ui, view: &UiViewSummary) -> Vec<SchemaObjectFolderAction> {
        let is_selected = matches!(
            self.explorer.selected_schema_object.as_ref(),
            Some(SchemaObjectSelection::View(name)) if name == &view.name
        );
        let from = super::result_grid_export::qualified_sql_name(&view.schema, &view.name);
        let view_clone = view.clone();
        let schema = view.schema.clone();
        let name = view.name.clone();
        let query_label = t!("explorer.select_top_100");
        let modify_label = t!("explorer.modify_view");
        let drop_label = t!("explorer.drop_view");
        let actions = SchemaObjectRowContext {
            theme: self.theme,
            label: &view.name,
            icon: Icon::Eye,
            icon_color: self.theme.success,
            is_selected,
            query_label: Some(query_label.as_str()),
            query_icon: Icon::Play,
            open_label: &t!("explorer.open_view"),
            open_icon: Icon::Eye,
            modify_label: Some(modify_label.as_str()),
            drop_label: Some(drop_label.as_str()),
            copy_label: &t!("explorer.copy_view_name"),
            trigger_toggle_label: None,
            trigger_toggle_unavailable_reason: None,
        }
        .draw(ui);

        actions
            .into_iter()
            .filter_map(|action| match action {
                SchemaObjectRowAction::Open => Some(SchemaObjectFolderAction::Open(SchemaObjectActivation {
                    selection: SchemaObjectSelection::View(view.name.clone()),
                    schema: view.schema.clone(),
                    name: view.name.clone(),
                    kind: "view".to_owned(),
                })),
                SchemaObjectRowAction::OpenQuery => {
                    Some(SchemaObjectFolderAction::OpenSelectTop(from.clone()))
                }
                SchemaObjectRowAction::Modify => Some(SchemaObjectFolderAction::ModifyView(view_clone.clone())),
                SchemaObjectRowAction::Drop => Some(SchemaObjectFolderAction::DropObject {
                    schema: schema.clone(),
                    name: name.clone(),
                    kind: "VIEW".to_owned(),
                }),
                SchemaObjectRowAction::ToggleTrigger => None,
                SchemaObjectRowAction::CopyName => Some(SchemaObjectFolderAction::CopyName(view.name.clone())),
            })
            .collect()
    }

    fn draw_function_row(&self, ui: &mut egui::Ui, function: &UiFunctionSummary) -> Vec<SchemaObjectFolderAction> {
        let is_selected = matches!(
            self.explorer.selected_schema_object.as_ref(),
            Some(SchemaObjectSelection::Function { name, identity_arguments })
                if name == &function.name && identity_arguments == &function.identity_arguments
        );
        let icon = if function.routine_type.eq_ignore_ascii_case("procedure") {
            Icon::GitBranch
        } else {
            Icon::Code2
        };
        let label = if function.identity_arguments.is_empty() {
            format!("{} · {}", function.name, function.routine_type)
        } else {
            format!(
                "{}({}) · {}",
                function.name, function.identity_arguments, function.routine_type
            )
        };
        let call_label = t!("explorer.open_call_in_query");
        self.draw_row(
            ui,
            SchemaObjectRowContext {
                theme: self.theme,
                label: &label,
                icon,
                icon_color: self.theme.code_type,
                is_selected,
                query_label: Some(call_label.as_str()),
                query_icon: Icon::Play,
                open_label: &t!("explorer.open_routine"),
                open_icon: Icon::Code2,
                modify_label: None,
                drop_label: None,
                copy_label: &t!("explorer.copy_routine_name"),
                trigger_toggle_label: None,
                trigger_toggle_unavailable_reason: None,
            },
            SchemaObjectActivation {
                selection: SchemaObjectSelection::Function {
                    name: function.name.clone(),
                    identity_arguments: function.identity_arguments.clone(),
                },
                schema: function.schema.clone(),
                name: function.name.clone(),
                kind: "function".to_owned(),
            },
            Some(format!(
                "SELECT * FROM {}();",
                super::result_grid_export::qualified_sql_name(&function.schema, &function.name)
            )),
        )
    }

    fn draw_trigger_row(&self, ui: &mut egui::Ui, trigger: &UiTriggerSummary) -> Vec<SchemaObjectFolderAction> {
        let is_selected = matches!(
            self.explorer.selected_schema_object.as_ref(),
            Some(SchemaObjectSelection::Trigger(name)) if name == &trigger.name
        );
        let label = format!(
            "{} · {} · {}",
            trigger.name,
            trigger.event,
            if trigger.enabled {
                t!("explorer.trigger_enabled")
            } else {
                t!("explorer.trigger_disabled")
            }
        );
        let schema = trigger.schema.clone();
        let name = trigger.name.clone();
        let drop_trigger_label = t!("explorer.drop_trigger");
        let actions = SchemaObjectRowContext {
            theme: self.theme,
            label: &label,
            icon: Icon::Zap,
            icon_color: self.theme.warning,
            is_selected,
            query_label: None,
            query_icon: Icon::Play,
            open_label: &t!("explorer.view_trigger"),
            open_icon: Icon::Eye,
            modify_label: None,
            drop_label: Some(drop_trigger_label.as_str()),
            copy_label: &t!("explorer.copy_trigger_name"),
            trigger_toggle_label: trigger_toggle_menu_label(
                trigger.enabled,
                self.trigger_toggle_unavailable_reason.is_none(),
            ),
            trigger_toggle_unavailable_reason: self.trigger_toggle_unavailable_reason.as_deref(),
        }
        .draw(ui);

        actions
            .into_iter()
            .filter_map(|action| match action {
                SchemaObjectRowAction::Open => Some(SchemaObjectFolderAction::Open(SchemaObjectActivation {
                    selection: SchemaObjectSelection::Trigger(trigger.name.clone()),
                    schema: String::new(),
                    name: trigger.name.clone(),
                    kind: "trigger".to_owned(),
                })),
                SchemaObjectRowAction::OpenQuery | SchemaObjectRowAction::Modify => None,
                SchemaObjectRowAction::Drop => Some(SchemaObjectFolderAction::DropObject {
                    schema: schema.clone(),
                    name: name.clone(),
                    kind: "TRIGGER".to_owned(),
                }),
                SchemaObjectRowAction::ToggleTrigger => Some(SchemaObjectFolderAction::ToggleTrigger {
                    schema: trigger.schema.clone(),
                    table: trigger.table_name.clone(),
                    name: trigger.name.clone(),
                    enable: !trigger.enabled,
                }),
                SchemaObjectRowAction::CopyName => Some(SchemaObjectFolderAction::CopyName(trigger.name.clone())),
            })
            .collect()
    }

    fn draw_row(
        &self,
        ui: &mut egui::Ui,
        context: SchemaObjectRowContext<'_>,
        activation: SchemaObjectActivation,
        query: Option<String>,
    ) -> Vec<SchemaObjectFolderAction> {
        context
            .draw(ui)
            .into_iter()
            .filter_map(|action| match action {
                SchemaObjectRowAction::Open => Some(SchemaObjectFolderAction::Open(activation.clone())),
                SchemaObjectRowAction::OpenQuery => query.clone().map(SchemaObjectFolderAction::OpenQuery),
                SchemaObjectRowAction::Modify => None,
                SchemaObjectRowAction::ToggleTrigger => None,
                SchemaObjectRowAction::Drop => None,
                SchemaObjectRowAction::CopyName => Some(SchemaObjectFolderAction::CopyName(activation.name.clone())),
            })
            .collect()
    }
}
/// Label for the enable/disable item in a trigger's context menu (#568).
fn trigger_toggle_menu_label(enabled: bool, supported: bool) -> Option<String> {
    if !supported {
        None
    } else if enabled {
        Some(t!("explorer.disable_trigger").to_string())
    } else {
        Some(t!("explorer.enable_trigger").to_string())
    }
}

#[cfg(test)]
#[path = "tests/explorer_schema_object_folders_view_tests.rs"]
mod tests;

