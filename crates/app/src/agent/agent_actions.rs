// cc-scan:allow-file HUGE_FUNCTION,LONG_FUNCTION,HARD_COMPLEXITY,COMPLEXITY,DEEP_NESTING
// cc-scan:allow-file HARD_PARAMS,TOO_MANY_PARAMS,DUPLICATE_BLOCK
// egui painter/view file: fns are linear layout code; branches are per-state paint variants.
//! Composition-root adapters for Agent commands and cross-feature effects.
//!
//! `AgentState` owns the Agent lifecycle and preparation rules. This module is
//! deliberately the outer adapter: it gathers query/schema/workspace inputs,
//! dispatches runtime commands, and applies effects that cross feature
//! aggregates.

use super::agent_state::{AgentRunPreparation, AgentRunPreparationError, PreparedAgentRun};
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentPromptPreparationError {
    NoDocument,
    ProviderNotConfigured,
    AlreadyActive,
    SessionUnavailable,
}

struct AgentRunInputs {
    document_id: String,
    document: db_pro_core::domain::agent::AgentDocumentSnapshot,
    connection_id: Option<String>,
    schema: Option<String>,
    context: db_pro_core::domain::agent_context::AgentContext,
}

impl DbProApp {
    pub(super) fn agent_context(&self) -> AgentContext {
        let connection_name = Some(self.active_connection_name().to_owned());
        let driver = self.active_driver().to_owned();
        let selected_columns = self
            .table
            .state
            .table_info
            .as_ref()
            .map(|info| info.columns.iter().map(|column| column.name.clone()).collect())
            .unwrap_or_default();
        let current_sql = if self.query.session.selected_text.trim().is_empty() {
            self.query.session.active_text().to_owned()
        } else {
            self.query.session.selected_text.clone()
        };
        let result_summary = self
            .query
            .session
            .active_result()
            .or(self.table.data_query.result_ref())
            .map(|result| format!("{} rows returned in {} ms", result.row_count, result.duration_ms));
        let last_error = self.has_runtime_error().then(|| self.feedback.runtime_message.clone());

        AgentContext {
            connection_name,
            driver,
            tables: self.active_schema_table_names(),
            columns: self.active_schema_column_names(),
            schema: Some(self.active_schema().to_owned()),
            selected_table: self.schema.explorer.selected_table.clone(),
            selected_columns,
            current_sql,
            result_summary,
            explain_plan: self.query.session.active_explain_plan().map(str::to_owned),
            last_error,
            workspace_files: self.workspace.files.workspace_context_items.clone(),
        }
    }

    pub(super) fn submit_agent_prompt(&mut self) {
        if !self.workspace.files.workspace_context_items.is_empty() && !self.workspace.files.ide_workspace.is_trusted()
        {
            self.feedback.runtime_message =
                "Trust the workspace before sending folder/file context to Agent".to_owned();
            return;
        }
        let prompt = self.agent.input.trim().to_owned();
        if prompt.is_empty() {
            return;
        }
        self.submit_typed_agent_prompt(prompt, false);
    }

    fn submit_typed_agent_prompt(&mut self, prompt: String, is_retry: bool) {
        let prepared = match self.prepare_agent_run(prompt, is_retry) {
            Ok(prepared) => prepared,
            Err(AgentPromptPreparationError::NoDocument) => {
                self.feedback.runtime_message = "No query document is available for Agent".to_owned();
                return;
            }
            Err(AgentPromptPreparationError::ProviderNotConfigured) => {
                self.feedback.runtime_message =
                    "AI provider is not configured. Enter an API key in Agent Settings.".to_owned();
                self.feedback
                    .show_error_toast("Configure an API key in Agent Settings to start.");
                return;
            }
            Err(AgentPromptPreparationError::AlreadyActive) => {
                self.feedback.runtime_message = "An Agent run is already active for this query".to_owned();
                return;
            }
            Err(AgentPromptPreparationError::SessionUnavailable) => {
                self.feedback.runtime_message = "Agent session could not be initialized".to_owned();
                return;
            }
        };
        self.feedback.runtime_message = format!("Sending request to {}…", self.agent.provider_label);
        if !self.dispatch_command(UiCommand::StartAgentRun {
            request_id: prepared.request_id,
            prompt: prepared.prompt,
            session: prepared.session,
            document: prepared.document,
            mode: prepared.mode,
            allow_read_only_auto_run: prepared.allow_read_only_auto_run,
            context: prepared.context,
        }) {
            self.agent.mark_run_failed(&prepared.document_id);
        }
    }

    fn prepare_agent_run(&mut self, prompt: String, is_retry: bool) -> Result<PreparedAgentRun, AgentPromptPreparationError> {
        let inputs = self.build_agent_run_inputs(&prompt)?;
        let request_id = self.next_request_id();
        let mode = self
            .agent
            .sessions
            .get(&inputs.document_id)
            .map(|session| session.mode)
            .unwrap_or(db_pro_core::domain::agent::AgentMode::Ask);
        self.agent
            .prepare_run(AgentRunPreparation {
                request_id,
                prompt,
                document: inputs.document,
                connection_id: inputs.connection_id,
                schema: inputs.schema,
                mode,
                context: inputs.context,
                is_retry,
            })
            .map_err(|error| match error {
                AgentRunPreparationError::AlreadyActive => AgentPromptPreparationError::AlreadyActive,
                AgentRunPreparationError::SessionUnavailable => AgentPromptPreparationError::SessionUnavailable,
            })
    }

    fn build_agent_run_inputs(&self, prompt: &str) -> Result<AgentRunInputs, AgentPromptPreparationError> {
        let Some(document) = self
            .query
            .session
            .documents
            .get(self.query.session.active_document_index)
        else {
            return Err(AgentPromptPreparationError::NoDocument);
        };
        if self.agent.provider_label == "Offline draft" {
            return Err(AgentPromptPreparationError::ProviderNotConfigured);
        }
        let document_id = document.id.clone();
        let connection_id = document
            .connection_id
            .clone()
            .or_else(|| self.connection.lifecycle.active_connection_id().map(str::to_owned));
        let schema = document
            .schema
            .clone()
            .or_else(|| Some(self.active_schema().to_owned()));
        let snapshot = agent_context::document_snapshot(document);
        let context = agent_context::build_context(
            prompt,
            document,
            connection_id.as_deref(),
            schema.as_deref(),
            &self.schema.explorer.schema.table_details,
        );

        Ok(AgentRunInputs {
            document_id,
            document: snapshot,
            connection_id,
            schema,
            context,
        })
    }

    pub(super) fn on_agent_workflow_event(&mut self, event: db_pro_core::domain::agent_workflow::AgentWorkflowEvent) {
        agent_events::on_agent_workflow_event(&mut self.agent, event);
    }

    pub(super) fn agent_confirmation_action(&mut self, approved: bool) {
        let Some(document_id) = self
            .query
            .session
            .documents
            .get(self.query.session.active_document_index)
            .map(|document| document.id.clone())
        else {
            return;
        };
        let Some(pending) = self
            .agent
            .sessions
            .get(&document_id)
            .and_then(|session| session.pending_confirmation.clone())
        else {
            return;
        };
        let (continuation_approved, prepared) = match self.prepare_agent_confirmation(&pending, approved) {
            Ok(prepared) => prepared,
            Err(agent_confirmation::AgentConfirmationError::PreviewUnavailable) => {
                self.feedback.runtime_message = "Agent patch preview is unavailable".to_owned();
                return;
            }
            Err(agent_confirmation::AgentConfirmationError::DocumentUnavailable) => return,
            Err(agent_confirmation::AgentConfirmationError::DocumentChanged)
            | Err(agent_confirmation::AgentConfirmationError::InvalidRange) => return,
        };
        let request_id = self.next_request_id();
        let continuation = self.agent.prepare_continuation(
            &pending,
            request_id,
            continuation_approved,
            prepared.current_document,
            prepared.applied_patch,
        );
        if !self.send_command_best_effort(UiCommand::ContinueAgentRun {
            request_id: continuation.request_id,
            run_id: continuation.run_id,
            approved: continuation.approved,
            current_document: continuation.current_document,
            applied_patch: continuation.applied_patch,
        }) {
            self.agent.mark_run_failed(&pending.document_id);
            // The patch mutated the buffer optimistically; a command that never
            // reached the worker must not leave the document changed (#409).
            if let Some(revert) = prepared.revert {
                revert.revert(&mut self.query.session.documents);
                self.feedback.runtime_message =
                    "Agent run could not be continued — your query was restored".to_owned();
            }
            self.feedback.show_error_toast("Agent continuation failed to dispatch");
        }
    }

    fn prepare_agent_confirmation(
        &mut self,
        pending: &agent_workflow_state::AgentUiConfirmation,
        approved: bool,
    ) -> Result<(bool, agent_confirmation::PreparedAgentConfirmation), agent_confirmation::AgentConfirmationError> {
        match agent_confirmation::prepare_confirmation(
            &mut self.query.session.documents,
            self.query.session.active_document_index,
            pending,
            approved,
        ) {
            Ok(prepared) => Ok((approved, prepared)),
            Err(error @ agent_confirmation::AgentConfirmationError::DocumentChanged)
            | Err(error @ agent_confirmation::AgentConfirmationError::InvalidRange) => {
                let (message, toast) = if error == agent_confirmation::AgentConfirmationError::DocumentChanged {
                    (
                        "This query changed since the suggestion was created.",
                        "The query changed since the suggestion was created.",
                    )
                } else {
                    (
                        "Agent patch range is no longer valid",
                        "Agent patch range is no longer valid",
                    )
                };
                self.feedback.runtime_message = message.to_owned();
                self.feedback.show_error_toast(toast);
                let prepared = agent_confirmation::prepare_confirmation(
                    &mut self.query.session.documents,
                    self.query.session.active_document_index,
                    pending,
                    false,
                )?;
                Ok((false, prepared))
            }
            Err(error) => Err(error),
        }
    }

    pub(super) fn open_agent_result_in_workspace(&mut self, call_id: &str) {
        let Some(document_id) = self
            .query
            .session
            .documents
            .get(self.query.session.active_document_index)
            .map(|document| document.id.clone())
        else {
            return;
        };
        let Some(tool_result) = self
            .agent
            .sessions
            .get(&document_id)
            .and_then(|session| session.tool_results.get(call_id))
            .cloned()
        else {
            return;
        };
        let active_document_index = self.query.session.active_document_index;
        agent_result_projection::AgentResultWorkspaceContext::new(
            &mut self.query.session,
            &mut self.query.output,
            &mut self.table.data,
            &mut self.feedback,
        )
        .open(active_document_index, &tool_result);
    }

    pub(super) fn retry_agent_run(&mut self) {
        let Some(document_id) = self
            .query
            .session
            .documents
            .get(self.query.session.active_document_index)
            .map(|document| document.id.clone())
        else {
            return;
        };
        // Retry binds to the failed turn: it resubmits the *same* prompt
        // without appending a duplicate user message, and `prepare_run`
        // refuses when the session is not actually in a failed state or a
        // newer prompt has superseded the one that failed (#397).
        let prompt = self.agent.sessions.get(&document_id).and_then(|session| {
            if session.state != db_pro_core::domain::agent::AgentSessionState::Failed {
                return None;
            }
            session
                .messages
                .iter()
                .rev()
                .find(|msg| msg.role == AgentRole::User)
                .map(|msg| msg.content.clone())
        });
        if let Some(prompt) = prompt {
            self.submit_typed_agent_prompt(prompt, true);
        }
    }

    pub(super) fn cancel_active_agent_run(&mut self) {
        let Some(document_id) = self
            .query
            .session
            .documents
            .get(self.query.session.active_document_index)
            .map(|document| document.id.clone())
        else {
            return;
        };
        let Some(run_id) = self
            .agent
            .sessions
            .get(&document_id)
            .and_then(|session| session.active_run_id)
        else {
            return;
        };
        let request_id = self.next_request_id();
        // Optimistic teardown: the user pressed Stop, so the session leaves its
        // in-flight state even if the command never reaches the worker (#360).
        self.agent.mark_run_cancelled(&document_id);
        if !self.dispatch_command(UiCommand::CancelAgentRun { request_id, run_id }) {
            self.feedback.runtime_message = "Agent run stopped locally; runtime could not be notified".to_owned();
        }
    }
}
