//! Monitoring session presentation and user intents.

use super::*;

/// Idle-in-transaction rows painted per frame (#569).
const IDLE_XACT_CAP: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum MonitoringSessionsAction {
    OpenSql(String),
    /// The whole row travels so the confirm dialog can show user/database/
    /// query context instead of a bare pid (#386).
    Cancel(db_pro_core::domain::monitoring::MonitorSession),
    RequestTerminate(db_pro_core::domain::monitoring::MonitorSession),
}

pub(super) struct MonitoringSessionsContext<'a> {
    pub(super) theme: DbProTheme,
    pub(super) snapshot: &'a db_pro_core::domain::monitoring::MonitoringSnapshot,
    pub(super) filter_active_only: &'a mut bool,
}

impl MonitoringSessionsContext<'_> {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui) -> Vec<MonitoringSessionsAction> {
        let mut actions = Vec::new();
        self.draw_filter(ui);
        self.draw_idle_transactions(ui);
        let sessions = self.visible_sessions();
        if sessions.is_empty() {
            self.draw_empty(ui);
        } else {
            for session in sessions {
                actions.extend(self.draw_session(ui, session));
                ui.add_space(SPACE_XS);
            }
        }
        actions
    }

    fn draw_filter(&mut self, ui: &mut egui::Ui) {
        section_label(ui, "SESSIONS", self.theme);
        ui.add_space(SPACE_SM);
        Checkbox::new(self.filter_active_only, "Active queries only", self.theme).show(ui);
        ui.add_space(SPACE_SM);
    }

    fn draw_idle_transactions(&self, ui: &mut egui::Ui) {
        let idle_xacts = self.snapshot.idle_in_transaction_sessions();
        if idle_xacts.is_empty() {
            return;
        }
        section_label(ui, "IDLE IN TRANSACTION", self.theme);
        ui.add_space(SPACE_SM);
        // #569: idle-in-transaction sessions are a lock risk; never truncate silently.
        if let Some(hint) = shown_total_hint(IDLE_XACT_CAP, idle_xacts.len()) {
            ui.label(RichText::new(hint).small().color(self.theme.text_muted));
        }
        for session in idle_xacts.into_iter().take(IDLE_XACT_CAP) {
            ui.label(
                RichText::new(format!(
                    "pid {} · xact_age={} · backend_age={} · {}",
                    session.backend_id,
                    fmt_opt_duration_millis(session.xact_age_ms),
                    fmt_opt_duration_millis(session.backend_age_ms),
                    fmt_opt(session.username.as_deref())
                ))
                .small()
                .color(self.theme.warning),
            );
        }
        ui.add_space(SPACE_MD);
    }

    fn visible_sessions(&self) -> Vec<&db_pro_core::domain::monitoring::MonitorSession> {
        if *self.filter_active_only {
            self.snapshot.active_queries()
        } else {
            self.snapshot.sessions.iter().collect()
        }
    }

    fn draw_empty(&self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new(if self.snapshot.sessions.is_empty() {
                self.snapshot.message.as_str()
            } else {
                "No active queries right now."
            })
            .small()
            .color(self.theme.text_muted),
        );
    }

    fn draw_session(
        &self,
        ui: &mut egui::Ui,
        session: &db_pro_core::domain::monitoring::MonitorSession,
    ) -> Vec<MonitoringSessionsAction> {
        let mut actions = Vec::new();
        panel_frame(self.theme)
            .inner_margin(egui::Margin::symmetric(SPACE_SM as i8, SPACE_XS as i8))
            .show(ui, |ui| {
                self.draw_session_header(ui, session);
                ui.label(
                    RichText::new(format!(
                        "{} · {} · {}",
                        session.username.as_deref().unwrap_or("?"),
                        session.database.as_deref().unwrap_or("?"),
                        session.application_name.as_deref().unwrap_or("-")
                    ))
                    .small()
                    .color(self.theme.text_secondary),
                );
                if let Some(query) = &session.query_text {
                    self.draw_query(ui, session, query, &mut actions);
                }
            });
        actions
    }

    fn draw_session_header(&self, ui: &mut egui::Ui, session: &db_pro_core::domain::monitoring::MonitorSession) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SPACE_XS, SPACE_XXS);
            ui.label(
                RichText::new(format!("pid {}", session.backend_id))
                    .strong()
                    .monospace()
                    .color(self.theme.text_primary),
            );
            if session.is_current {
                Badge::new("current", self.theme)
                    .variant(BadgeVariant::Secondary)
                    .compact(true)
                    .show(ui);
            }
            ui.label(
                RichText::new(session.state.clone().unwrap_or_else(|| "—".into()))
                    .small()
                    .color(self.theme.text_secondary),
            );
            if let Some(ms) = session.query_duration_ms {
                ui.label(
                    RichText::new(format!("query {ms} ms"))
                        .small()
                        .color(self.theme.text_muted),
                );
            }
            if let Some(ms) = session.xact_age_ms {
                ui.label(
                    RichText::new(format!("xact {ms} ms"))
                        .small()
                        .color(self.theme.text_muted),
                );
            }
            if session.idle_in_transaction {
                Badge::new("idle-in-xact", self.theme)
                    .tint(self.theme.warning)
                    .compact(true)
                    .show(ui);
            }
        });
    }

    fn draw_query(
        &self,
        ui: &mut egui::Ui,
        session: &db_pro_core::domain::monitoring::MonitorSession,
        query: &str,
        actions: &mut Vec<MonitoringSessionsAction>,
    ) {
        let short = if query.len() > 120 {
            format!("{}…", query.chars().take(119).collect::<String>())
        } else {
            query.to_owned()
        };
        ui.label(RichText::new(short).monospace().small().color(self.theme.text_primary));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SPACE_XS, SPACE_XXS);
            if Button::new(self.theme)
                .icon(Icon::FileCode2)
                .text("Open SQL")
                .variant(ButtonVariant::Ghost)
                .show(ui)
                .clicked()
            {
                actions.push(MonitoringSessionsAction::OpenSql(query.to_owned()));
            }
            if !session.is_current
                && Button::new(self.theme)
                    .icon(Icon::Ban)
                    .text("Cancel")
                    .variant(ButtonVariant::Secondary)
                    .show(ui)
                    .clicked()
            {
                actions.push(MonitoringSessionsAction::Cancel(session.clone()));
            }
            if !session.is_current
                && Button::new(self.theme)
                    .text("Terminate")
                    .variant(ButtonVariant::Destructive)
                    .show(ui)
                    .clicked()
            {
                actions.push(MonitoringSessionsAction::RequestTerminate(session.clone()));
            }
        });
    }
}

#[cfg(test)]
#[path = "tests/monitoring_sessions_view_tests.rs"]
mod tests;
