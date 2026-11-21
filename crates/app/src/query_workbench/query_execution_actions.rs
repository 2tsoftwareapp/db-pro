//! State transitions and command preparation for query execution.

use super::query_execution_state::{PendingDestructiveRun, PendingDestructiveRunMetadata, PendingDestructiveRunOrigin};
use super::{
    FeedbackState, QueryEditorState, QueryExecutionPolicyState, QueryExecutionState, QuerySessionState, RequestId,
};
use db_pro_core::domain::query::QueryParam;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedQueryRun {
    pub(crate) request_id: RequestId,
    pub(crate) connection_id: String,
    pub(crate) sql: String,
    pub(crate) params: Vec<QueryParam>,
    pub(crate) all_statements: bool,
    pub(crate) schema: Option<String>,
    /// Query tab that owns this run. Table-grid reads leave it empty so they
    /// do not join the editor transaction.
    pub(crate) session_id: Option<String>,
}

/// Coordinates query execution policy with the query document/session state.
///
/// This context deliberately separates command preparation from the local
/// execution transition. The app shell sends the command through `TaskBridge`
/// and commits the transition only after the runtime boundary accepts it.
pub(crate) struct QueryExecutionContext<'a> {
    session: &'a mut QuerySessionState,
    editor: &'a mut QueryEditorState,
    policy: &'a mut QueryExecutionPolicyState,
    feedback: &'a mut FeedbackState,
    driver: String,
}

impl<'a> QueryExecutionContext<'a> {
    pub(crate) fn new(
        session: &'a mut QuerySessionState,
        editor: &'a mut QueryEditorState,
        policy: &'a mut QueryExecutionPolicyState,
        feedback: &'a mut FeedbackState,
        driver: impl Into<String>,
    ) -> Self {
        Self {
            session,
            editor,
            policy,
            feedback,
            driver: driver.into(),
        }
    }

    pub(crate) fn hold_destructive_run(
        &mut self,
        sql: &str,
        origin: PendingDestructiveRunOrigin,
        target_label: String,
        metadata: PendingDestructiveRunMetadata,
        bound_params: Option<Vec<QueryParam>>,
    ) -> bool {
        // An open confirmation owns the slot: never overwrite the statement the user is
        // looking at — the new statement is held (not sent) with a clear reason.
        if self.policy.pending_destructive_run().is_some() {
            self.feedback
                .set_runtime_message("A destructive statement is already awaiting confirmation — confirm or cancel it first");
            return true;
        }
        if db_pro_core::domain::safety::classify_script_safety(sql)
            != Some(db_pro_core::domain::safety::StatementSafety::Destructive)
        {
            return false;
        }

        self.policy.set_pending_destructive_run(PendingDestructiveRun::new(
            sql.to_owned(),
            origin,
            target_label,
            metadata,
            bound_params,
        ));
        self.feedback
            .set_runtime_message("Destructive statement held for confirmation — nothing was sent to the database");
        true
    }

    pub(crate) fn take_pending_destructive_run(&mut self) -> Option<PendingDestructiveRun> {
        self.policy.take_pending_destructive_run()
    }

    pub(crate) fn cancel_pending_destructive_run(&mut self) {
        if self.policy.take_pending_destructive_run().is_some() {
            self.feedback
                .set_runtime_message("Destructive statement cancelled — nothing was sent to the database");
        }
    }

    pub(crate) fn prepare_query_run(
        &mut self,
        request_id: RequestId,
        connection_id: String,
        sql: String,
        all_statements: bool,
    ) -> Option<PreparedQueryRun> {
        let discovered = crate::query::discover_sql_parameters_for_driver(&sql, &self.driver);
        if all_statements && !discovered.is_empty() {
            self.feedback.set_runtime_message(
                "Parameterized scripts are not supported yet — run a single statement with bindings",
            );
            return None;
        }

        let style = crate::query::placeholder_style_for_driver(&self.driver);
        let values = self
            .session
            .active_document()
            .map(|document| document.parameter_values.clone())
            .unwrap_or_default();
        let (sql, params) = if discovered.is_empty() {
            (sql, Vec::new())
        } else {
            match crate::query::prepare_bound_sql(&sql, &values, style) {
                // The editor's parameter panel collects text; bind it as a text
                // parameter value, never splice it back into the SQL (#225).
                Ok(prepared) => (prepared.sql, prepared.values.into_iter().map(QueryParam::Text).collect()),
                Err(missing) => {
                    self.feedback
                        .set_runtime_message(format!("Fill parameter {missing} before running"));
                    return None;
                }
            }
        };

        let schema = self.session.active_schema().map(str::to_owned);
        Some(PreparedQueryRun {
            request_id,
            schema,
            connection_id,
            sql,
            params,
            all_statements,
            session_id: None,
        })
    }

    /// Prepare a run whose SQL already carries provider placeholders and whose
    /// arguments are already typed `QueryParam`s (routine workbench, #252).
    /// Skips editor parameter discovery — the bound vector *is* the binding, so
    /// no literal argument value can reach the server as SQL text.
    pub(crate) fn prepare_bound_run(
        &mut self,
        request_id: RequestId,
        connection_id: String,
        sql: String,
        params: Vec<QueryParam>,
        all_statements: bool,
    ) -> PreparedQueryRun {
        let schema = self.session.active_schema().map(str::to_owned);
        PreparedQueryRun {
            request_id,
            schema,
            connection_id,
            sql,
            params,
            all_statements,
            session_id: None,
        }
    }

    pub(crate) fn commit_dispatched(
        &mut self,
        prepared: &PreparedQueryRun,
        execution_range: (usize, usize),
        version: u64,
        applied_row_limit: Option<u64>,
    ) {
        self.record_query_history(&prepared.sql);
        self.mark_document_running(prepared.request_id, &prepared.sql, execution_range, version, applied_row_limit);
        self.feedback.set_runtime_message(if prepared.all_statements {
            "Sending full script to runtime…"
        } else {
            "Sending query to runtime…"
        });
    }

    pub(crate) fn commit_inline_dispatched(&mut self, prepared: &PreparedQueryRun) {
        self.record_query_history(&prepared.sql);
        self.feedback.set_runtime_message(if prepared.all_statements {
            "Sending full script to runtime…"
        } else {
            "Sending query to runtime…"
        });
    }

    fn record_query_history(&mut self, sql: &str) {
        if self.editor.query_history.iter().any(|query| query == sql) {
            return;
        }
        self.editor.query_history.push(sql.to_owned());
        if self.editor.query_history.len() > 20 {
            self.editor.query_history.remove(0);
        }
    }

    fn mark_document_running(
        &mut self,
        request_id: RequestId,
        sql: &str,
        execution_range: (usize, usize),
        version: u64,
        applied_row_limit: Option<u64>,
    ) {
        let Some(document) = self.session.active_document_mut() else {
            return;
        };
        document.execution_state = QueryExecutionState::Running(request_id);
        // Whatever the grid is showing now is output of an earlier run (#607).
        document.results_stale = true;
        document.execution_started_at = Some(Instant::now());
        document.execution_started_wall_time = Some(chrono::Utc::now().to_rfc3339());
        document.executing_range = Some(execution_range);
        document.executing_sql = Some(sql.to_owned());
        document.executing_row_limit = applied_row_limit;
        document.query_result_row_limit = None;
        document.executing_version = Some(version);
        document.last_executed_range = Some(execution_range);
        document.execution_diagnostic = None;
        let document_id = document.id.clone();
        self.session.document_requests.insert(request_id, document_id);
    }
}

#[cfg(test)]
#[path = "tests/query_execution_actions_tests.rs"]
mod tests;

