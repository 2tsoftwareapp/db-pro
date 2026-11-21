//! Feature-owned lifecycle state for query documents and their shell requests.

use super::query_editor_support::close_completion;
use super::{PendingBulkClose, QueryDocument, QueryExecutionState};
use crate::runtime::UiQueryResult;
use crate::RequestId;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

/// Soft cap on open query tabs (#401): palette/saved-query/agent opens each
/// used to push a new document forever, growing memory and the tab bar.
/// Adding beyond the cap closes the oldest evictable tab (clean, idle) or
/// refuses with feedback when every tab is dirty or busy.
pub(crate) const MAX_QUERY_DOCUMENTS: usize = 50;

#[derive(Debug, Default)]
pub(crate) struct QuerySessionState {
    pub(super) documents: Vec<QueryDocument>,
    pub(super) active_document_index: usize,
    pub(super) selected_text: String,
    pub(super) document_requests: HashMap<RequestId, String>,
    pub(super) save_requests: HashMap<RequestId, (String, u64)>,
    pub(super) pending_dirty_close: Option<usize>,
    /// Bulk tab closes ("Close Other/Right/All") staged here while the dirty
    /// confirmation is up — closing must never silently drop unsaved SQL.
    pub(super) pending_bulk_close: Option<PendingBulkClose>,
    pub(super) pending_close_after_save: Option<usize>,
    pub(super) save_as_name: String,
    pub(super) save_as_name_error: bool,
    pub(super) save_as_open: bool,
}

impl QuerySessionState {
    pub(crate) fn active_document(&self) -> Option<&QueryDocument> {
        self.documents.get(self.active_document_index)
    }

    pub(crate) fn active_document_mut(&mut self) -> Option<&mut QueryDocument> {
        self.documents.get_mut(self.active_document_index)
    }

    pub(crate) fn active_text(&self) -> &str {
        self.active_document().map(QueryDocument::text).unwrap_or("")
    }

    pub(crate) fn active_buffer_version(&self) -> u64 {
        self.active_document()
            .map(|document| document.buffer.version())
            .unwrap_or(0)
    }

    pub(crate) fn set_active_text(&mut self, text: impl Into<String>) -> bool {
        let Some(document) = self.active_document_mut() else {
            return false;
        };
        document.set_text(text);
        true
    }

    pub(crate) fn append_active_text(&mut self, text: &str) -> bool {
        let Some(document) = self.active_document_mut() else {
            return false;
        };
        let mut current = document.text().to_owned();
        if !current.trim().is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(text);
        document.set_text(current);
        true
    }

    pub(crate) fn active_explain_plan(&self) -> Option<&str> {
        self.active_document()
            .and_then(|document| document.explain_plan.as_deref())
    }

    pub(crate) fn active_explain_request(&self) -> Option<RequestId> {
        self.active_document().and_then(|document| document.explain_request)
    }

    pub(crate) fn active_running_request(&self) -> Option<RequestId> {
        self.active_document()
            .and_then(|document| match document.execution_state {
                QueryExecutionState::Running(request_id) => Some(request_id),
                _ => None,
            })
    }

    pub(crate) fn active_result(&self) -> Option<&UiQueryResult> {
        self.active_result_handle_ref().map(Rc::as_ref)
    }

    /// Cheap owned handle to the active result (#541): a refcount bump, safe to take
    /// once per frame in the draw path. Never deep-clone the grid to satisfy the
    /// borrow checker — `Option<&UiQueryResult>` consumers take `handle.as_deref()`.
    pub(crate) fn active_result_handle(&self) -> Option<Rc<UiQueryResult>> {
        self.active_result_handle_ref().cloned()
    }

    fn active_result_handle_ref(&self) -> Option<&Rc<UiQueryResult>> {
        self.active_document().and_then(|document| {
            document
                .query_results
                .get(document.active_result_index)
                .or(document.query_result.as_ref())
        })
    }

    pub(crate) fn active_result_count(&self) -> usize {
        self.active_document().map_or(0, |document| {
            document
                .query_results
                .len()
                .max(usize::from(document.query_result.is_some()))
        })
    }

    pub(crate) fn set_active_result(&mut self, index: usize) -> bool {
        let Some(document) = self.active_document_mut() else {
            return false;
        };
        if index >= document.query_results.len() || document.active_result_index == index {
            return false;
        }
        document.active_result_index = index;
        true
    }

    pub(crate) fn active_messages(&self) -> &[String] {
        self.active_document()
            .map(|document| document.query_messages.as_slice())
            .unwrap_or(&[])
    }

    pub(crate) fn active_connection_id(&self) -> Option<&str> {
        self.active_document()
            .and_then(|document| document.connection_id.as_deref())
    }

    pub(crate) fn active_schema(&self) -> Option<&str> {
        self.active_document().and_then(|document| document.schema.as_deref())
    }

    pub(crate) fn set_document_connection(&mut self, index: usize, connection_id: Option<String>) -> bool {
        let Some(document) = self.documents.get_mut(index) else {
            return false;
        };
        if document.connection_id == connection_id {
            return false;
        }
        document.connection_id = connection_id;
        close_completion(document);
        // UAT-8: a schema belongs to the connection it was picked from — keep a
        // stale "main" only until the driver's own default resolves it.
        document.schema = None;
        true
    }

    pub(crate) fn set_document_schema(&mut self, index: usize, schema: Option<String>) -> bool {
        let Some(document) = self.documents.get_mut(index) else {
            return false;
        };
        document.schema = schema;
        close_completion(document);
        true
    }

    pub(crate) fn invalidate_prediction(&mut self, index: usize) -> Option<RequestId> {
        let document = self.documents.get_mut(index)?;
        let request_id = document.pending_prediction_request;
        if request_id.is_some() {
            document.prediction_requests_cancelled = document.prediction_requests_cancelled.saturating_add(1);
        }
        document.invalidate_prediction();
        request_id
    }

    pub(crate) fn commit_prediction_request(
        &mut self,
        document_id: &str,
        request_id: RequestId,
        fingerprint: u64,
    ) -> bool {
        let Some(document) = self.documents.iter_mut().find(|document| document.id == document_id) else {
            return false;
        };
        if document.pending_prediction_request.is_some() {
            return false;
        }
        let now = Instant::now();
        document.prediction_context_fingerprint = Some(fingerprint);
        document.prediction_last_request_fingerprint = Some(fingerprint);
        document.prediction_last_request_at = Some(now);
        document.pending_prediction_request = Some(request_id);
        document.prediction_request_started_at = Some(now);
        document.prediction_requests_sent = document.prediction_requests_sent.saturating_add(1);
        true
    }

    pub(crate) fn add_document(&mut self, document: QueryDocument) -> usize {
        self.documents.push(document);
        self.active_document_index = self.documents.len() - 1;
        self.active_document_index
    }

    pub(crate) fn select_document(&mut self, index: usize) -> bool {
        if index >= self.documents.len() {
            return false;
        }
        self.active_document_index = index;
        true
    }

    pub(crate) fn remove_document(&mut self, index: usize) -> Option<QueryDocument> {
        if index >= self.documents.len() {
            return None;
        }

        let document = self.documents.remove(index);
        if self.documents.is_empty() {
            self.active_document_index = 0;
        } else if self.active_document_index > index {
            self.active_document_index -= 1;
        } else if self.active_document_index == index {
            self.active_document_index = self.active_document_index.min(self.documents.len() - 1);
        }
        Some(document)
    }

    pub(crate) fn keep_document(&mut self, index: usize) -> bool {
        let Some(document) = self.documents.get(index).cloned() else {
            return false;
        };
        self.documents = vec![document];
        self.active_document_index = 0;
        true
    }

    pub(crate) fn close_documents_to_right(&mut self, index: usize) -> bool {
        if index >= self.documents.len() {
            return false;
        }
        self.documents.truncate(index + 1);
        if self.active_document_index > index {
            self.active_document_index = index;
        }
        true
    }

    pub(crate) fn replace_with_document(&mut self, document: QueryDocument) {
        self.documents = vec![document];
        self.active_document_index = 0;
    }
}

#[cfg(test)]
#[path = "tests/query_state_tests.rs"]
mod tests;

