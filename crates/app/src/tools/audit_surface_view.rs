//! Audit activity presentation and typed user intents.
use super::super::audit_state::AuditState;
use super::super::*;

/// Audit rows painted per frame (#569).
const AUDIT_ROW_CAP: usize = 80;


#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AuditSurfaceAction {
    Refresh,
    ExportSelected,
    SetSelected { event_id: String, selected: bool },
    ToggleBookmark(String),
    OpenQuery(String),
}

pub(super) struct AuditSurfaceContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) state: &'a mut AuditState,
}

impl AuditSurfaceContext<'_> {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui) -> Vec<AuditSurfaceAction> {
        let mut actions = Vec::new();
        self.draw_header(ui, &mut actions);
        self.draw_filters(ui);
        self.draw_error(ui);
        self.draw_page(ui, &mut actions);
        actions
    }

    fn draw_header(&self, ui: &mut egui::Ui, actions: &mut Vec<AuditSurfaceAction>) {
        ui.add_space(SPACE_MD);
        ui.label(
            RichText::new(t!("audit.title"))
                .font(font_caption())
                .strong()
                .color(self.theme.text_secondary),
        );
        ui.add_space(SPACE_SM);
        ui.label(
            RichText::new(
                "Database audit (server CSV log) — distinct from application Diagnostics. \
                 Configure tag audit:csvlog=/path/to/logfile.csv. Logging is never auto-enabled.",
            )
            .small()
            .color(self.theme.text_muted),
        );
        ui.horizontal(|ui| {
            if Button::new(self.theme)
                .icon(Icon::RefreshCw)
                .text(t!("audit.load_page"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                actions.push(AuditSurfaceAction::Refresh);
            }
            if Button::new(self.theme)
                .icon(Icon::Download)
                .text(t!("audit.export_selected"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                actions.push(AuditSurfaceAction::ExportSelected);
            }
        });
    }

    fn draw_filters(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Text").small().color(self.theme.text_muted));
            Input::new(&mut self.state.audit_filter_text, "message/query", self.theme).width(120.0).show(ui);
            ui.label(RichText::new("DB").small().color(self.theme.text_muted));
            Input::new(&mut self.state.audit_filter_database, "database", self.theme).width(80.0).show(ui);
            ui.label(RichText::new("User").small().color(self.theme.text_muted));
            Input::new(&mut self.state.audit_filter_username, "user", self.theme).width(80.0).show(ui);
            ui.label(RichText::new("Severity").small().color(self.theme.text_muted));
            Input::new(&mut self.state.audit_filter_severity, "level", self.theme).width(60.0).show(ui);
        });
    }

    fn draw_error(&self, ui: &mut egui::Ui) {
        if let Some(error) = &self.state.audit_error {
            ui.colored_label(self.theme.danger, error);
        }
    }

    fn draw_page(&mut self, ui: &mut egui::Ui, actions: &mut Vec<AuditSurfaceAction>) {
        let Some(page) = self.state.audit_page.clone() else {
            return;
        };
        ui.label(
            RichText::new(format!(
                "{} · scanned {} bytes · truncated={}",
                page.source.guidance, page.scanned_bytes, page.truncated
            ))
            .small()
            .color(self.theme.text_secondary),
        );
        if let Some(path) = &page.source.path {
            ui.label(RichText::new(format!("source: {path}")).small().monospace());
        }
        if page.source.kind == db_pro_core::domain::audit::AuditSourceKind::Unavailable {
            ui.colored_label(self.theme.warning, &page.source.guidance);
        }
        // #569: disclose the cap so a truncated audit list never reads as complete.
        if let Some(hint) = crate::components::common::shown_total_hint(AUDIT_ROW_CAP, page.events.len()) {
            ui.label(RichText::new(hint).small().color(self.theme.text_muted));
        }
        for event in page.events.iter().take(AUDIT_ROW_CAP) {
            self.draw_event(ui, event, actions);
        }
        if let Some(preview) = &self.state.audit_export_preview {
            ui.label(RichText::new(page.export_warning).small().color(self.theme.warning));
            ui.label(
                RichText::new(preview.chars().take(400).collect::<String>())
                    .monospace()
                    .small()
                    .color(self.theme.text_muted),
            );
        }
    }

    fn draw_event(
        &self,
        ui: &mut egui::Ui,
        event: &db_pro_core::domain::audit::AuditEvent,
        actions: &mut Vec<AuditSurfaceAction>,
    ) {
        let bookmarked = self.state.audit_bookmarks.contains(&event.id);
        let selected = self.state.audit_selected.contains(&event.id);
        Card::new(self.theme).show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut selected_now = selected;
                if Checkbox::new(&mut selected_now, "", self.theme).show(ui).changed() {
                    actions.push(AuditSurfaceAction::SetSelected {
                        event_id: event.id.clone(),
                        selected: selected_now,
                    });
                }
                ui.label(
                    RichText::new(format!(
                        "{} · {} · {} · {}",
                        event.timestamp.as_deref().unwrap_or("-"),
                        event.severity.as_deref().unwrap_or("-"),
                        event.username.as_deref().unwrap_or("-"),
                        event.database.as_deref().unwrap_or("-"),
                    ))
                    .small()
                    .strong(),
                );
                if event.redacted {
                    ui.colored_label(self.theme.warning, "redacted");
                }
                if bookmarked {
                    ui.colored_label(self.theme.accent, "★");
                }
            });
            ui.label(RichText::new(&event.message).small().color(self.theme.text_primary));
            if let Some(query) = &event.query {
                let preview = if query.len() > 160 {
                    format!("{}…", query.chars().take(159).collect::<String>())
                } else {
                    query.clone()
                };
                ui.label(RichText::new(preview).monospace().small().color(self.theme.text_muted));
                if Button::new(self.theme)
                    .icon(Icon::FileCode2)
                    .text(t!("audit.open_sql"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .show(ui)
                    .clicked()
                {
                    actions.push(AuditSurfaceAction::OpenQuery(query.clone()));
                }
            }
            ui.horizontal(|ui| {
                let label = if bookmarked { t!("audit.unbookmark") } else { t!("audit.bookmark") };
                if Button::new(self.theme)
                    .icon(Icon::Bookmark)
                    .text(label)
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .show(ui)
                    .clicked()
                {
                    actions.push(AuditSurfaceAction::ToggleBookmark(event.id.clone()));
                }
            });
        });
        ui.add_space(SPACE_XS);
    }
}
