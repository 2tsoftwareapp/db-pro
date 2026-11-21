//! Tests for `../query_documents.rs` (moved out of the source file).
    use super::*;

    #[test]
    fn new_document_context_binds_explicit_connection_and_schema() {
        let mut query_session = QuerySessionState::default();
        let mut query_editor = QueryEditorState::default();
        let mut workspace = WorkspaceFeatureState::default();
        let mut agent = AgentState::default();
        let mut query_output = QueryOutputState::default();
        let mut schema_explorer = SchemaExplorerState::default();
        let mut task_bridge = TaskBridge::default();
        let mut feedback = FeedbackState::default();
        let mut execution = QueryExecutionPolicyState::default();

        QueryDocumentContext {
            query_session: &mut query_session,
            query_editor: &mut query_editor,
            workspace: &mut workspace,
            agent: &mut agent,
            query_output: &mut query_output,
            schema_explorer: &mut schema_explorer,
            command_dispatcher: command_dispatch::RuntimeCommandDispatcher::new(&mut task_bridge),
            feedback: &mut feedback,
            execution: &mut execution,
            active_connection_id: Some("connection-1".to_owned()),
            active_schema: "analytics".to_owned(),
        }
        .new_document(false);

        let document = query_session.active_document().expect("new document");
        assert_eq!(document.connection_id.as_deref(), Some("connection-1"));
        assert_eq!(document.schema.as_deref(), Some("analytics"));
        assert_eq!(workspace.active_tab, WorkspaceTab::Query);
        assert!(query_editor.query_focus_editor_on_open);
    }

    #[test]
    fn closing_document_context_removes_document_owned_output_state() {
        let mut query_session = QuerySessionState::default();
        query_session.add_document(QueryDocument::new("query-1", "Query 1", "select 1"));
        query_session.add_document(QueryDocument::new("query-2", "Query 2", "select 2"));
        query_session.select_document(0);
        let mut query_editor = QueryEditorState::default();
        let mut workspace = WorkspaceFeatureState::default();
        let mut agent = AgentState::default();
        let mut query_output = QueryOutputState::default();
        query_output
            .tabs_by_document
            .insert("query-1".to_owned(), OutputTab::Messages);
        let mut schema_explorer = SchemaExplorerState::default();
        let mut task_bridge = TaskBridge::default();
        let mut feedback = FeedbackState::default();
        let mut execution = QueryExecutionPolicyState::default();

        QueryDocumentContext {
            query_session: &mut query_session,
            query_editor: &mut query_editor,
            workspace: &mut workspace,
            agent: &mut agent,
            query_output: &mut query_output,
            schema_explorer: &mut schema_explorer,
            command_dispatcher: command_dispatch::RuntimeCommandDispatcher::new(&mut task_bridge),
            feedback: &mut feedback,
            execution: &mut execution,
            active_connection_id: None,
            active_schema: "public".to_owned(),
        }
        .close_document(0);

        assert_eq!(query_session.documents.len(), 1);
        assert_eq!(
            query_session.active_document().map(|doc| doc.id.as_str()),
            Some("query-2")
        );
        assert!(!query_output.tabs_by_document.contains_key("query-1"));
        assert_eq!(workspace.active_tab, WorkspaceTab::Welcome);
    }

    #[test]
    fn closing_a_tab_with_an_open_transaction_is_refused() {
        let mut query_session = QuerySessionState::default();
        query_session.add_document(QueryDocument::new("query-1", "Query 1", "select 1"));
        let mut query_editor = QueryEditorState::default();
        let mut workspace = WorkspaceFeatureState::default();
        let mut agent = AgentState::default();
        let mut query_output = QueryOutputState::default();
        let mut schema_explorer = SchemaExplorerState::default();
        let mut task_bridge = TaskBridge::default();
        let mut feedback = FeedbackState::default();
        let mut execution = QueryExecutionPolicyState::default();
        execution.note_transaction_dispatch("query-1", crate::RequestId(1), "BEGIN");
        execution.settle_transaction_request(crate::RequestId(1), true);

        QueryDocumentContext {
            query_session: &mut query_session,
            query_editor: &mut query_editor,
            workspace: &mut workspace,
            agent: &mut agent,
            query_output: &mut query_output,
            schema_explorer: &mut schema_explorer,
            command_dispatcher: command_dispatch::RuntimeCommandDispatcher::new(&mut task_bridge),
            feedback: &mut feedback,
            execution: &mut execution,
            active_connection_id: None,
            active_schema: "public".to_owned(),
        }
        .close_document(0);

        assert_eq!(query_session.documents.len(), 1);
        assert!(execution.document_is_busy("query-1"));
        assert!(feedback.runtime_message.contains("Commit or rollback"));
    }

    #[test]
    fn bulk_close_with_dirty_tabs_parks_the_close_for_confirmation() {
        let mut query_session = QuerySessionState::default();
        // A tab is dirty once its text differs from the snapshot taken at
        // construction — simulate the user typing into the second tab.
        query_session.add_document(QueryDocument::new("query-1", "Query 1", "select 1"));
        let mut dirty = QueryDocument::new("query-2", "Query 2", "select 2");
        dirty.set_text("select 2 -- edited");
        query_session.add_document(dirty);
        let mut query_editor = QueryEditorState::default();
        let mut workspace = WorkspaceFeatureState::default();
        let mut agent = AgentState::default();
        let mut query_output = QueryOutputState::default();
        let mut schema_explorer = SchemaExplorerState::default();
        let mut task_bridge = TaskBridge::default();
        let mut feedback = FeedbackState::default();
        let mut execution = QueryExecutionPolicyState::default();

        QueryDocumentContext {
            query_session: &mut query_session,
            query_editor: &mut query_editor,
            workspace: &mut workspace,
            agent: &mut agent,
            query_output: &mut query_output,
            schema_explorer: &mut schema_explorer,
            command_dispatcher: command_dispatch::RuntimeCommandDispatcher::new(&mut task_bridge),
            feedback: &mut feedback,
            execution: &mut execution,
            active_connection_id: None,
            active_schema: "public".to_owned(),
        }
        .close_other_documents(0);

        assert_eq!(query_session.documents.len(), 2, "dirty tab must survive the close attempt");
        assert_eq!(
            query_session.pending_bulk_close,
            Some(PendingBulkClose::Others(0)),
            "close scope must be parked for the confirmation dialog"
        );
    }

    #[test]
    fn bulk_close_replays_once_the_pending_scope_is_confirmed() {
        let mut query_session = QuerySessionState::default();
        query_session.add_document(QueryDocument::new("query-1", "Query 1", "select 1"));
        let mut dirty = QueryDocument::new("query-2", "Query 2", "select 2");
        dirty.set_text("select 2 -- edited");
        query_session.add_document(dirty);
        // Simulate the confirmation answering: the parked scope replays and
        // clears itself, then the close proceeds with the dirty tab gone.
        query_session.pending_bulk_close = Some(PendingBulkClose::Others(0));
        let mut query_editor = QueryEditorState::default();
        let mut workspace = WorkspaceFeatureState::default();
        let mut agent = AgentState::default();
        let mut query_output = QueryOutputState::default();
        let mut schema_explorer = SchemaExplorerState::default();
        let mut task_bridge = TaskBridge::default();
        let mut feedback = FeedbackState::default();
        let mut execution = QueryExecutionPolicyState::default();

        QueryDocumentContext {
            query_session: &mut query_session,
            query_editor: &mut query_editor,
            workspace: &mut workspace,
            agent: &mut agent,
            query_output: &mut query_output,
            schema_explorer: &mut schema_explorer,
            command_dispatcher: command_dispatch::RuntimeCommandDispatcher::new(&mut task_bridge),
            feedback: &mut feedback,
            execution: &mut execution,
            active_connection_id: None,
            active_schema: "public".to_owned(),
        }
        .close_other_documents(0);

        assert!(query_session.pending_bulk_close.is_none());
        assert_eq!(query_session.documents.len(), 1);
        assert_eq!(query_session.documents[0].id, "query-1");
    }
    #[test]
    fn open_text_document_preselects_the_template_so_typing_replaces_it() {
        let mut query_session = QuerySessionState::default();
        let mut query_editor = QueryEditorState::default();
        let mut workspace = WorkspaceFeatureState::default();
        let mut agent = AgentState::default();
        let mut query_output = QueryOutputState::default();
        let mut schema_explorer = SchemaExplorerState::default();
        let mut task_bridge = TaskBridge::default();
        let mut feedback = FeedbackState::default();
        let mut execution = QueryExecutionPolicyState::default();
        let template = "SELECT *\nFROM \"shop\".\"customers\"\nLIMIT 100;".to_owned();

        QueryDocumentContext {
            query_session: &mut query_session,
            query_editor: &mut query_editor,
            workspace: &mut workspace,
            agent: &mut agent,
            query_output: &mut query_output,
            schema_explorer: &mut schema_explorer,
            command_dispatcher: command_dispatch::RuntimeCommandDispatcher::new(&mut task_bridge),
            feedback: &mut feedback,
            execution: &mut execution,
            active_connection_id: Some("connection-1".to_owned()),
            active_schema: String::new(),
        }
        .open_text_document(template.clone(), None);

        let document = query_session.active_document().expect("document opened");
        assert_eq!(document.selection.normalized(), (0, template.len()));
        assert_eq!(document.cursor.offset, template.len());
    }

    #[test]
    fn open_text_document_with_empty_template_places_the_caret_at_the_end() {
        let mut query_session = QuerySessionState::default();
        let mut query_editor = QueryEditorState::default();
        let mut workspace = WorkspaceFeatureState::default();
        let mut agent = AgentState::default();
        let mut query_output = QueryOutputState::default();
        let mut schema_explorer = SchemaExplorerState::default();
        let mut task_bridge = TaskBridge::default();
        let mut feedback = FeedbackState::default();
        let mut execution = QueryExecutionPolicyState::default();

        QueryDocumentContext {
            query_session: &mut query_session,
            query_editor: &mut query_editor,
            workspace: &mut workspace,
            agent: &mut agent,
            query_output: &mut query_output,
            schema_explorer: &mut schema_explorer,
            command_dispatcher: command_dispatch::RuntimeCommandDispatcher::new(&mut task_bridge),
            feedback: &mut feedback,
            execution: &mut execution,
            active_connection_id: None,
            active_schema: String::new(),
        }
        .open_text_document(String::new(), None);

        let document = query_session.active_document().expect("document opened");
        assert_eq!(document.selection.normalized(), (0, 0));
        assert_eq!(document.cursor.offset, 0);
    }
